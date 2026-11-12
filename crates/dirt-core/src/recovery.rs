//! The recovery phrase: a way back into an account when no approved device
//! is left.
//!
//! The phrase is twenty-four words from the BIP 39 English list. It stands
//! for 256 random bits, and its last word carries a checksum, so a mistyped
//! word is caught before anything is tried with it.
//!
//! Two values are derived from the phrase, with HKDF, each bound to the
//! account:
//!
//! - a key that encrypts the account key. The server keeps that encrypted
//!   copy and cannot open it;
//! - a claim code. The server keeps only its hash. A device that presents
//!   the code has shown that it knows the phrase, and is approved.
//!
//! Knowing one of the two says nothing about the other, so what the server
//! holds does not help it open the key. And because the phrase is as strong
//! as a key itself, no slow password hash is needed: there is nothing to
//! guess.

use bip39::{Language, Mnemonic};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto::AccountKey;

/// How many words a phrase has.
pub const WORDS: usize = 24;

/// Why a phrase could not be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RecoveryError {
    /// The text is not a phrase: a word is not on the list, a word is
    /// missing, or the checksum does not fit.
    #[error("that is not a recovery phrase: check each word and their order")]
    NotAPhrase,
    /// The phrase is well formed but does not open this account's copy.
    #[error("that recovery phrase is not this account's")]
    WrongPhrase,
}

/// A recovery phrase.
pub struct RecoveryPhrase {
    /// The 256 bits the words stand for.
    entropy: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for RecoveryPhrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryPhrase([REDACTED])")
    }
}

/// What the server stores for a phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryCopy {
    /// The account key, encrypted under the phrase.
    pub wrapped: Vec<u8>,
    /// The hash of the claim code.
    pub claim_hash: Vec<u8>,
}

impl RecoveryPhrase {
    /// A new random phrase.
    #[must_use]
    pub fn generate() -> Self {
        use chacha20poly1305::aead::rand_core::RngCore as _;

        let mut entropy = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut *entropy);
        Self { entropy }
    }

    /// Reads a phrase the person typed. Case, extra spaces and line breaks
    /// do not matter.
    pub fn parse(text: &str) -> Result<Self, RecoveryError> {
        let normalised = Zeroizing::new(
            text.split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
                .join(" "),
        );
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, &normalised)
            .map_err(|_| RecoveryError::NotAPhrase)?;
        let bytes = Zeroizing::new(mnemonic.to_entropy());
        let entropy: [u8; 32] = bytes
            .as_slice()
            .try_into()
            // A valid phrase of another length, twelve words for example.
            .map_err(|_| RecoveryError::NotAPhrase)?;
        Ok(Self {
            entropy: Zeroizing::new(entropy),
        })
    }

    /// The words, separated by spaces, to show the person once.
    #[must_use]
    pub fn words(&self) -> Zeroizing<String> {
        // Thirty-two bytes always make a phrase.
        Zeroizing::new(
            Mnemonic::from_entropy_in(Language::English, &*self.entropy)
                .map(|mnemonic| mnemonic.to_string())
                .unwrap_or_default(),
        )
    }

    fn derive(&self, account: Uuid, purpose: &str) -> Zeroizing<[u8; 32]> {
        let mut output = Zeroizing::new([0u8; 32]);
        // Thirty-two bytes is within what HKDF-SHA256 can give.
        let _ = Hkdf::<Sha256>::new(Some(account.as_bytes()), &*self.entropy)
            .expand(purpose.as_bytes(), &mut *output);
        output
    }

    fn cipher(&self, account: Uuid) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&*self.derive(account, "dirt recovery key wrap v1")).into())
    }

    /// The code that proves knowledge of the phrase to the server.
    #[must_use]
    pub fn claim_code(&self, account: Uuid) -> Vec<u8> {
        self.derive(account, "dirt recovery claim v1").to_vec()
    }

    /// Encrypts the account key under the phrase, and makes the hash of the
    /// claim code, for the server to store.
    #[must_use]
    pub fn seal(&self, account: Uuid, key: &AccountKey) -> RecoveryCopy {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = self
            .cipher(account)
            .encrypt(
                &nonce,
                Payload {
                    msg: key.as_bytes(),
                    aad: account.as_bytes(),
                },
            )
            // Encrypting 32 bytes in memory cannot fail.
            .unwrap_or_default();
        RecoveryCopy {
            wrapped: [nonce.as_slice(), &sealed].concat(),
            claim_hash: Sha256::digest(self.claim_code(account)).to_vec(),
        }
    }

    /// Opens the account key from the server's copy. Fails if the phrase is
    /// not the one the copy was made with, or the copy is another account's.
    pub fn open(&self, account: Uuid, wrapped: &[u8]) -> Result<AccountKey, RecoveryError> {
        const NONCE: usize = 24;
        if wrapped.len() <= NONCE {
            return Err(RecoveryError::WrongPhrase);
        }
        let (nonce, ciphertext) = wrapped.split_at(NONCE);
        let opened = Zeroizing::new(
            self.cipher(account)
                .decrypt(
                    XNonce::from_slice(nonce),
                    Payload {
                        msg: ciphertext,
                        aad: account.as_bytes(),
                    },
                )
                .map_err(|_| RecoveryError::WrongPhrase)?,
        );
        AccountKey::from_bytes(&opened).map_err(|_| RecoveryError::WrongPhrase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: Uuid = Uuid::from_u128(0xACC0);
    const OTHER_ACCOUNT: Uuid = Uuid::from_u128(0x07E2);

    #[test]
    fn a_phrase_is_twenty_four_words_and_reads_back() {
        let phrase = RecoveryPhrase::generate();
        let words = phrase.words();
        assert_eq!(words.split(' ').count(), WORDS);

        let typed = RecoveryPhrase::parse(&words).unwrap();
        assert_eq!(typed.claim_code(ACCOUNT), phrase.claim_code(ACCOUNT));
        // Typed carelessly: capitals, extra spaces, line breaks.
        let careless = format!("  {}\n", words.to_uppercase().replace(' ', "  \n "));
        let typed = RecoveryPhrase::parse(&careless).unwrap();
        assert_eq!(typed.claim_code(ACCOUNT), phrase.claim_code(ACCOUNT));

        assert_ne!(
            RecoveryPhrase::generate().words().as_str(),
            words.as_str(),
            "each phrase is new"
        );
    }

    #[test]
    fn the_phrase_opens_the_copy_it_made() {
        let phrase = RecoveryPhrase::generate();
        let key = AccountKey::generate();

        let copy = phrase.seal(ACCOUNT, &key);

        let typed = RecoveryPhrase::parse(&phrase.words()).unwrap();
        assert_eq!(
            typed.open(ACCOUNT, &copy.wrapped).unwrap().as_bytes(),
            key.as_bytes()
        );
        // The server can check the claim code against what it stores.
        assert_eq!(
            Sha256::digest(typed.claim_code(ACCOUNT)).to_vec(),
            copy.claim_hash
        );
    }

    #[test]
    fn another_phrase_or_another_account_opens_nothing() {
        let phrase = RecoveryPhrase::generate();
        let copy = phrase.seal(ACCOUNT, &AccountKey::generate());

        let other = RecoveryPhrase::generate();
        assert_eq!(
            other.open(ACCOUNT, &copy.wrapped).unwrap_err(),
            RecoveryError::WrongPhrase
        );
        assert_ne!(other.claim_code(ACCOUNT), phrase.claim_code(ACCOUNT));
        // The right phrase against a copy moved to another account.
        assert_eq!(
            phrase.open(OTHER_ACCOUNT, &copy.wrapped).unwrap_err(),
            RecoveryError::WrongPhrase
        );
        assert_ne!(phrase.claim_code(OTHER_ACCOUNT), phrase.claim_code(ACCOUNT));
        // Damaged or truncated copies.
        let mut damaged = copy.wrapped;
        damaged[30] ^= 1;
        assert_eq!(
            phrase.open(ACCOUNT, &damaged).unwrap_err(),
            RecoveryError::WrongPhrase
        );
        assert_eq!(
            phrase.open(ACCOUNT, b"short").unwrap_err(),
            RecoveryError::WrongPhrase
        );
    }

    #[test]
    fn what_the_server_stores_does_not_contain_what_opens_the_key() {
        let phrase = RecoveryPhrase::generate();
        let key = AccountKey::generate();
        let copy = phrase.seal(ACCOUNT, &key);

        // The claim code and the wrapping key are different values, and
        // neither the account key nor the claim code is in what is stored.
        assert_ne!(
            phrase.claim_code(ACCOUNT),
            phrase.derive(ACCOUNT, "dirt recovery key wrap v1").to_vec()
        );
        let stored = [copy.wrapped.as_slice(), copy.claim_hash.as_slice()].concat();
        assert!(!stored.windows(32).any(|window| window == key.as_bytes()));
        assert!(!stored
            .windows(32)
            .any(|window| window == phrase.claim_code(ACCOUNT)));
        // Two copies made with one phrase differ, apart from the hash.
        let again = phrase.seal(ACCOUNT, &key);
        assert_ne!(again.wrapped, copy.wrapped);
        assert_eq!(again.claim_hash, copy.claim_hash);
    }

    #[test]
    fn a_mistyped_phrase_is_caught_before_it_is_tried() {
        let phrase = RecoveryPhrase::generate();
        let words = phrase.words();
        let list: Vec<&str> = words.split(' ').collect();

        // A word that is not on the list.
        let mut wrong_word = list.clone();
        wrong_word[3] = "dirtnotes";
        // A word missing.
        let missing = list[..WORDS - 1].to_vec();
        // Two words swapped: every word is valid, the checksum is not.
        // A fixed phrase is used for this, the standard's own example. The
        // checksum is eight bits, so two words swapped in a phrase made at
        // random pass it once in 256 times, and a test that swapped them
        // there failed that often.
        let mut swapped: Vec<&str> = "legal winner thank year wave sausage worth useful              legal winner thank year wave sausage worth useful              legal winner thank year wave sausage worth title"
            .split(' ')
            .collect();
        assert!(RecoveryPhrase::parse(&swapped.join(" ")).is_ok());
        swapped.swap(0, 1);

        for text in [
            wrong_word.join(" "),
            missing.join(" "),
            swapped.join(" "),
            String::new(),
            // A valid phrase of twelve words is not one of ours.
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
                .to_owned(),
        ] {
            assert_eq!(
                RecoveryPhrase::parse(&text).unwrap_err(),
                RecoveryError::NotAPhrase,
                "{text:?}"
            );
        }
    }

    /// A fixed phrase is used, the standard's own example. What is printed
    /// in a phrase's place is the name of the type, which holds the words
    /// "cover", "over" and "very". All three are on the word list, so a
    /// phrase made at random began with one of them once in about seven
    /// hundred runs, and the test failed.
    #[test]
    fn a_phrase_is_not_printed() {
        let words = "legal winner thank year wave sausage worth useful \
                     legal winner thank year wave sausage worth useful \
                     legal winner thank year wave sausage worth title";
        let phrase = RecoveryPhrase::parse(words).unwrap();
        let printed = format!("{phrase:?}");
        for word in words.split(' ') {
            assert!(!printed.contains(word), "{word} is in {printed}");
        }
    }
}
