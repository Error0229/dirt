//! Keys, and the encryption of note blobs.
//!
//! Every note leaves the device encrypted under the **account key**, a random
//! 256-bit key made on the account's first device. The server never sees it.
//! A blob is bound to the account, the note, the key version and the document
//! format it was made for, so the server cannot hand back one note's blob in
//! place of another's: such a blob fails to decrypt.
//!
//! The cipher is XChaCha20-Poly1305 from the `chacha20poly1305` crate, with a
//! random nonce per blob. Nothing here is hand-written cryptography: this
//! module only fixes what is encrypted, what is bound to it, and how the
//! result is laid out.

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use dirt_proto::NoteId;
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// The version of an account's first key. Each change of the key
/// (`key_change`) is the next version.
pub const KEY_VERSION: i32 = 1;

/// The document format inside a blob: a yrs document in the v2 encoding.
pub const FORMAT_VERSION: i32 = 1;

/// The first byte of every blob, naming this layout: nonce, then ciphertext.
const BLOB_LAYOUT: u8 = 1;
const NONCE_BYTES: usize = 24;
/// Mixed into the associated data, so a blob can never be confused with
/// anything else encrypted under the same key.
const DOMAIN: &[u8] = b"dirt/note-blob/v1";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    /// The blob is too short or starts with a layout this version does not
    /// know. A newer app wrote it, or it is not a blob at all.
    #[error("the note blob has an unknown layout")]
    UnknownLayout,
    /// The blob did not decrypt: the key is wrong, the blob was changed, or
    /// it belongs to another note, account, key version or format.
    #[error("the note blob could not be decrypted")]
    Rejected,
    /// A stored key has the wrong length.
    #[error("a stored key has the wrong length")]
    BadKeyLength,
}

/// The key that encrypts every note of one account.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct AccountKey([u8; 32]);

impl AccountKey {
    /// A new random key, for the first device of an account.
    #[must_use]
    pub fn generate() -> Self {
        Self(XChaCha20Poly1305::generate_key(&mut OsRng).into())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        Ok(Self(
            bytes.try_into().map_err(|_| CryptoError::BadKeyLength)?,
        ))
    }

    /// The raw key, for the device's secure storage and for nothing else.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountKey([REDACTED])")
    }
}

/// What a blob is bound to. All four values are authenticated with the
/// ciphertext, so changing any of them makes decryption fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobContext {
    pub account_id: Uuid,
    pub note_id: NoteId,
    pub key_version: i32,
    pub format_version: i32,
}

impl BlobContext {
    /// The context for a blob written under the account's first key.
    #[must_use]
    pub const fn current(account_id: Uuid, note_id: NoteId) -> Self {
        Self::under(account_id, note_id, KEY_VERSION)
    }

    /// The context for a blob this version of the app writes under the
    /// key of `key_version`.
    #[must_use]
    pub const fn under(account_id: Uuid, note_id: NoteId, key_version: i32) -> Self {
        Self {
            account_id,
            note_id,
            key_version,
            format_version: FORMAT_VERSION,
        }
    }

    fn associated_data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(DOMAIN.len() + 40);
        data.extend_from_slice(DOMAIN);
        data.extend_from_slice(self.account_id.as_bytes());
        data.extend_from_slice(self.note_id.as_uuid().as_bytes());
        data.extend_from_slice(&self.key_version.to_be_bytes());
        data.extend_from_slice(&self.format_version.to_be_bytes());
        data
    }
}

/// Encrypts a note's document for upload. Each call uses a fresh random
/// nonce, so encrypting the same document twice gives different blobs.
#[must_use]
pub fn encrypt_note(key: &AccountKey, context: &BlobContext, document: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: document,
                aad: &context.associated_data(),
            },
        )
        // The cipher fails only for inputs beyond 2^38 bytes, which a note
        // cannot reach.
        .expect("a note fits within the cipher's limit");
    let mut blob = Vec::with_capacity(1 + NONCE_BYTES + ciphertext.len());
    blob.push(BLOB_LAYOUT);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    blob
}

/// Decrypts a blob made by [`encrypt_note`] for exactly this context.
pub fn decrypt_note(
    key: &AccountKey,
    context: &BlobContext,
    blob: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let (&layout, rest) = blob.split_first().ok_or(CryptoError::UnknownLayout)?;
    if layout != BLOB_LAYOUT || rest.len() < NONCE_BYTES {
        return Err(CryptoError::UnknownLayout);
    }
    let (nonce, ciphertext) = rest.split_at(NONCE_BYTES);
    XChaCha20Poly1305::new(key.as_bytes().into())
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &context.associated_data(),
            },
        )
        .map_err(|_| CryptoError::Rejected)
}

/// 256 random bits as URL-safe text, 43 characters long: a PKCE verifier or
/// an OAuth `state` value.
#[must_use]
pub fn random_token() -> String {
    use base64::Engine as _;
    use chacha20poly1305::aead::rand_core::RngCore as _;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A device's X25519 key pair. The public half is registered with the
/// server; step 6 uses the pair to receive the account key from another
/// device.
#[derive(Clone)]
pub struct DeviceKeyPair(StaticSecret);

impl DeviceKeyPair {
    #[must_use]
    pub fn generate() -> Self {
        Self(StaticSecret::random_from_rng(OsRng))
    }

    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| CryptoError::BadKeyLength)?;
        Ok(Self(StaticSecret::from(bytes)))
    }

    /// The private half, for the device's secure storage and nothing else.
    #[must_use]
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    #[must_use]
    pub fn public_bytes(&self) -> [u8; 32] {
        PublicKey::from(&self.0).to_bytes()
    }

    /// The secret this device and the holder of `their_public` can both
    /// compute, or `None` for one of the few public values that give the
    /// same secret to everybody.
    #[must_use]
    pub fn agree(&self, their_public: &[u8; 32]) -> Option<[u8; 32]> {
        let shared = self.0.diffie_hellman(&PublicKey::from(*their_public));
        shared.was_contributory().then(|| shared.to_bytes())
    }
}

impl std::fmt::Debug for DeviceKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceKeyPair([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> BlobContext {
        BlobContext::current(
            Uuid::from_u128(0xA11CE),
            "01932aaa-0000-7000-8000-000000000123".parse().unwrap(),
        )
    }

    #[test]
    fn a_blob_decrypts_to_the_document_it_was_made_from() {
        let key = AccountKey::generate();
        let document = b"the note's document".to_vec();
        let blob = encrypt_note(&key, &context(), &document);
        assert_eq!(decrypt_note(&key, &context(), &blob).unwrap(), document);
    }

    #[test]
    fn the_blob_does_not_contain_the_document() {
        let key = AccountKey::generate();
        let document = b"a phrase that must not appear in the blob";
        let blob = encrypt_note(&key, &context(), document);
        assert!(!blob
            .windows(document.len())
            .any(|window| window == document));
        assert_eq!(blob.len(), 1 + NONCE_BYTES + document.len() + 16);
    }

    #[test]
    fn encrypting_twice_gives_different_blobs() {
        let key = AccountKey::generate();
        let first = encrypt_note(&key, &context(), b"same");
        let second = encrypt_note(&key, &context(), b"same");
        assert_ne!(first, second);
    }

    #[test]
    fn an_empty_document_round_trips() {
        let key = AccountKey::generate();
        let blob = encrypt_note(&key, &context(), b"");
        assert_eq!(decrypt_note(&key, &context(), &blob).unwrap(), b"");
    }

    #[test]
    fn any_flipped_bit_is_rejected() {
        let key = AccountKey::generate();
        let blob = encrypt_note(&key, &context(), b"tamper with me");
        // Skip the layout byte, which has its own error.
        for index in 1..blob.len() {
            let mut changed = blob.clone();
            changed[index] ^= 0x01;
            assert_eq!(
                decrypt_note(&key, &context(), &changed),
                Err(CryptoError::Rejected),
                "a flip at byte {index} was accepted"
            );
        }
    }

    #[test]
    fn a_blob_moved_to_another_note_account_key_version_or_format_is_rejected() {
        let key = AccountKey::generate();
        let blob = encrypt_note(&key, &context(), b"bound to its place");
        let other_note = BlobContext {
            note_id: "01932aaa-0000-7000-8000-000000000999".parse().unwrap(),
            ..context()
        };
        let other_account = BlobContext {
            account_id: Uuid::from_u128(0xB0B),
            ..context()
        };
        let other_key_version = BlobContext {
            key_version: 2,
            ..context()
        };
        let other_format = BlobContext {
            format_version: 2,
            ..context()
        };
        for moved in [other_note, other_account, other_key_version, other_format] {
            assert_eq!(
                decrypt_note(&key, &moved, &blob),
                Err(CryptoError::Rejected),
                "{moved:?}"
            );
        }
    }

    #[test]
    fn another_accounts_key_cannot_read_a_blob() {
        let blob = encrypt_note(&AccountKey::generate(), &context(), b"private");
        assert_eq!(
            decrypt_note(&AccountKey::generate(), &context(), &blob),
            Err(CryptoError::Rejected)
        );
    }

    #[test]
    fn a_truncated_or_foreign_blob_has_an_unknown_layout() {
        let key = AccountKey::generate();
        let blob = encrypt_note(&key, &context(), b"x");
        assert_eq!(
            decrypt_note(&key, &context(), &[]),
            Err(CryptoError::UnknownLayout)
        );
        assert_eq!(
            decrypt_note(&key, &context(), &blob[..10]),
            Err(CryptoError::UnknownLayout)
        );
        let mut newer = blob;
        newer[0] = 2;
        assert_eq!(
            decrypt_note(&key, &context(), &newer),
            Err(CryptoError::UnknownLayout)
        );
    }

    #[test]
    fn keys_round_trip_through_their_stored_bytes_and_never_print() {
        let key = AccountKey::generate();
        let restored = AccountKey::from_bytes(key.as_bytes()).unwrap();
        let blob = encrypt_note(&key, &context(), b"same key");
        assert!(decrypt_note(&restored, &context(), &blob).is_ok());
        assert_eq!(
            AccountKey::from_bytes(&[0; 31]).unwrap_err(),
            CryptoError::BadKeyLength
        );
        assert!(!format!("{key:?}").contains(&format!("{:?}", key.as_bytes())));

        let pair = DeviceKeyPair::generate();
        let again = DeviceKeyPair::from_secret_bytes(&pair.secret_bytes()).unwrap();
        assert_eq!(pair.public_bytes(), again.public_bytes());
        assert_ne!(
            DeviceKeyPair::generate().public_bytes(),
            pair.public_bytes()
        );
        assert_eq!(format!("{pair:?}"), "DeviceKeyPair([REDACTED])");
    }
}
