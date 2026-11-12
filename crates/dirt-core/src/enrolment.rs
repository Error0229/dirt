//! The approval exchange: how a new device gets the account key from an
//! approved one without the server being able to read it or to stand
//! between the two.
//!
//! The exchange is Matrix's short-code verification. The key agreement and
//! the code come from the `vodozemac` crate, which implements them for
//! Matrix; nothing here invents a primitive.
//!
//! 1. The **new device** makes a temporary key pair and sends a *commitment*
//!    to its public half: a hash, which fixes the key without showing it.
//! 2. The **approving device** makes its own temporary key pair and sends
//!    its public half.
//! 3. The new device *reveals* its public half. The approving device checks
//!    it against the commitment.
//! 4. Both now share a secret, and each derives a short code from it and
//!    from everything said so far. The person compares the two codes.
//! 5. If they match, the approving device encrypts the account key under a
//!    key derived from the shared secret, and the new device opens it.
//!
//! The server relays every message and could change any of them. The
//! commitment is what makes that useless: whoever sits in the middle must
//! fix its key towards one device before it sees the other's, so it cannot
//! search for keys that give both devices the same code. With a code of
//! about forty bits it gets one guess in a trillion. If the person compares
//! the codes, a server in the middle is caught; if they do not, it is not.
//!
//! The code also covers each device's ID and registered public key, the
//! account, and a random ID for the exchange, so a server that swaps any of
//! those changes the code as well.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use vodozemac::sas::{EstablishedSas, Sas};
use vodozemac::Curve25519PublicKey;
use zeroize::Zeroizing;

use crate::crypto::{random_token, AccountKey};

/// Why an exchange cannot go on.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnrolmentError {
    /// A message is not one this version understands, or does not belong
    /// at this point of the exchange.
    #[error("the other device sent something that was not expected")]
    Unexpected,
    /// The new device revealed a key other than the one it committed to.
    /// Somebody changed a message on the way.
    #[error(
        "the new device's key does not match what it committed to; someone may be interfering"
    )]
    CommitmentBroken,
    /// The server lists a different public key for the new device than the
    /// device itself sent.
    #[error("the new device's registered key was changed on the way; someone may be interfering")]
    RegisteredKeyChanged,
    /// The two keys do not combine into a usable secret.
    #[error("the other device sent a key that cannot be used")]
    BadKey,
    /// The encrypted account key does not open with this exchange's secret.
    #[error("the account key could not be opened; it was not made for this exchange")]
    KeyRejected,
}

/// Why one side called the exchange off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    /// The person said the two codes are different.
    Mismatch,
    /// The person chose not to go on.
    Declined,
}

/// One message of the exchange, as it travels through the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Message {
    /// From the new device: the start of an exchange.
    Commit {
        exchange: String,
        /// The new device's registered public key, as the device knows it.
        device_key: String,
        /// The hash that fixes its temporary key.
        commitment: String,
    },
    /// From the approving device: its temporary key.
    Key {
        exchange: String,
        /// The approving device's registered public key.
        device_key: String,
        key: String,
    },
    /// From the new device: the temporary key it committed to.
    Reveal { exchange: String, key: String },
    /// From either: the exchange is off.
    Cancel {
        exchange: String,
        reason: CancelReason,
    },
}

impl Message {
    /// The exchange this message belongs to.
    #[must_use]
    pub fn exchange(&self) -> &str {
        match self {
            Self::Commit { exchange, .. }
            | Self::Key { exchange, .. }
            | Self::Reveal { exchange, .. }
            | Self::Cancel { exchange, .. } => exchange,
        }
    }

    /// The message as the bytes the server relays.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        // A value of this type always serialises.
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Reads a relayed message. Anything unreadable is `None`: it is not
    /// this exchange's, or not this version's, and is passed over.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

fn commitment(exchange: &str, device: Uuid, device_key: &str, temporary_key: &str) -> String {
    let mut hash = Sha256::new();
    for part in [
        "dirt enrolment commitment v1",
        exchange,
        &device.to_string(),
        device_key,
        temporary_key,
    ] {
        // Each part is prefixed with its length, so that no two different
        // lists of parts hash the same bytes.
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    STANDARD.encode(hash.finalize())
}

fn decode_key(text: &str) -> Result<Curve25519PublicKey, EnrolmentError> {
    Curve25519PublicKey::from_base64(text).map_err(|_| EnrolmentError::BadKey)
}

/// Everything both devices must agree on, written out as the text the code
/// and the key are derived from.
struct Transcript {
    account: Uuid,
    exchange: String,
    new_device: Uuid,
    new_device_key: String,
    new_temporary_key: String,
    approver: Uuid,
    approver_key: String,
    approver_temporary_key: String,
}

impl Transcript {
    fn info(&self, purpose: &str) -> String {
        [
            purpose,
            &self.account.to_string(),
            &self.exchange,
            &self.new_device.to_string(),
            &self.new_device_key,
            &self.new_temporary_key,
            &self.approver.to_string(),
            &self.approver_key,
            &self.approver_temporary_key,
        ]
        .join("|")
    }
}

/// The two devices' shared secret and what it yields. Both sides hold one
/// once the keys have been exchanged.
pub struct Agreed {
    secret: EstablishedSas,
    transcript: Transcript,
}

impl std::fmt::Debug for Agreed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agreed")
            .field("exchange", &self.transcript.exchange)
            .finish_non_exhaustive()
    }
}

impl Agreed {
    /// The ID of this exchange.
    #[must_use]
    pub fn exchange(&self) -> &str {
        &self.transcript.exchange
    }

    /// The code the person compares on the two devices: three groups of
    /// four digits.
    #[must_use]
    pub fn code(&self) -> String {
        let (a, b, c) = self
            .secret
            .bytes(&self.transcript.info("dirt enrolment code v1"))
            .decimals();
        format!("{a} {b} {c}")
    }

    fn wrapping_cipher(&self) -> XChaCha20Poly1305 {
        let mut key = Zeroizing::new([0u8; 32]);
        // Thirty-two bytes is far below what the derivation can give, so
        // this always fills the key.
        if let Ok(bytes) = self
            .secret
            .bytes_raw(&self.transcript.info("dirt enrolment key wrap v1"), 32)
        {
            key.copy_from_slice(&Zeroizing::new(bytes));
        }
        XChaCha20Poly1305::new((&*key).into())
    }

    fn associated_data(&self) -> Vec<u8> {
        self.transcript
            .info("dirt enrolment key wrap v1")
            .into_bytes()
    }

    /// Encrypts the account key for the other device. Only a device that
    /// took part in this exchange can open the result.
    #[must_use]
    pub fn seal_key(&self, key: &AccountKey) -> Vec<u8> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = self
            .wrapping_cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: key.as_bytes(),
                    aad: &self.associated_data(),
                },
            )
            // Encrypting 32 bytes in memory cannot fail.
            .unwrap_or_default();
        [nonce.as_slice(), &sealed].concat()
    }

    /// Opens an account key that the other device sealed in this exchange.
    pub fn open_key(&self, sealed: &[u8]) -> Result<AccountKey, EnrolmentError> {
        const NONCE: usize = 24;
        if sealed.len() <= NONCE {
            return Err(EnrolmentError::KeyRejected);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE);
        let opened = Zeroizing::new(
            self.wrapping_cipher()
                .decrypt(
                    XNonce::from_slice(nonce),
                    Payload {
                        msg: ciphertext,
                        aad: &self.associated_data(),
                    },
                )
                .map_err(|_| EnrolmentError::KeyRejected)?,
        );
        AccountKey::from_bytes(&opened).map_err(|_| EnrolmentError::KeyRejected)
    }
}

/// The new device's side, from its first message until the keys are
/// exchanged.
pub struct Joining {
    temporary: Sas,
    account: Uuid,
    device: Uuid,
    device_key: String,
    exchange: String,
}

impl std::fmt::Debug for Joining {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joining")
            .field("exchange", &self.exchange)
            .finish_non_exhaustive()
    }
}

impl Joining {
    /// Starts an exchange for the device `device` of `account`, whose
    /// registered public key is `device_key`. Returns the state to keep and
    /// the first message to send.
    #[must_use]
    pub fn start(account: Uuid, device: Uuid, device_key: &[u8; 32]) -> (Self, Message) {
        let temporary = Sas::new();
        let exchange = random_token();
        let device_key = STANDARD.encode(device_key);
        let message = Message::Commit {
            exchange: exchange.clone(),
            device_key: device_key.clone(),
            commitment: commitment(
                &exchange,
                device,
                &device_key,
                &temporary.public_key().to_base64(),
            ),
        };
        (
            Self {
                temporary,
                account,
                device,
                device_key,
                exchange,
            },
            message,
        )
    }

    /// The ID of this exchange. Messages of any other are not for it.
    #[must_use]
    pub fn exchange(&self) -> &str {
        &self.exchange
    }

    /// Takes the approving device's key, which `approver` sent. Returns the
    /// agreed secret and the message that reveals this device's own key.
    pub fn receive_key(
        self,
        approver: Uuid,
        message: &Message,
    ) -> Result<(Agreed, Message), EnrolmentError> {
        let Message::Key {
            exchange,
            device_key,
            key,
        } = message
        else {
            return Err(EnrolmentError::Unexpected);
        };
        if *exchange != self.exchange {
            return Err(EnrolmentError::Unexpected);
        }
        let their_key = decode_key(key)?;
        let my_temporary_key = self.temporary.public_key().to_base64();
        let secret = self
            .temporary
            .diffie_hellman(their_key)
            .map_err(|_| EnrolmentError::BadKey)?;
        let reveal = Message::Reveal {
            exchange: self.exchange.clone(),
            key: my_temporary_key.clone(),
        };
        Ok((
            Agreed {
                secret,
                transcript: Transcript {
                    account: self.account,
                    exchange: self.exchange,
                    new_device: self.device,
                    new_device_key: self.device_key,
                    new_temporary_key: my_temporary_key,
                    approver,
                    approver_key: device_key.clone(),
                    approver_temporary_key: key.clone(),
                },
            },
            reveal,
        ))
    }
}

/// The approving device's side, from its answer until the new device has
/// revealed its key.
pub struct Approving {
    temporary: Sas,
    account: Uuid,
    me: Uuid,
    my_key: String,
    new_device: Uuid,
    new_device_key: String,
    exchange: String,
    commitment: String,
}

impl std::fmt::Debug for Approving {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Approving")
            .field("exchange", &self.exchange)
            .finish_non_exhaustive()
    }
}

impl Approving {
    /// Answers the commitment that `new_device` sent. `listed_key` is the
    /// public key the server's device list gives for that device: the
    /// device's own statement of its key must agree with it.
    ///
    /// Returns the state to keep and the message to send.
    pub fn answer(
        account: Uuid,
        me: Uuid,
        my_key: &[u8; 32],
        new_device: Uuid,
        listed_key: &[u8],
        commit: &Message,
    ) -> Result<(Self, Message), EnrolmentError> {
        let Message::Commit {
            exchange,
            device_key,
            commitment,
        } = commit
        else {
            return Err(EnrolmentError::Unexpected);
        };
        if *device_key != STANDARD.encode(listed_key) {
            return Err(EnrolmentError::RegisteredKeyChanged);
        }
        let temporary = Sas::new();
        let my_key = STANDARD.encode(my_key);
        let message = Message::Key {
            exchange: exchange.clone(),
            device_key: my_key.clone(),
            key: temporary.public_key().to_base64(),
        };
        Ok((
            Self {
                temporary,
                account,
                me,
                my_key,
                new_device,
                new_device_key: device_key.clone(),
                exchange: exchange.clone(),
                commitment: commitment.clone(),
            },
            message,
        ))
    }

    /// The ID of this exchange.
    #[must_use]
    pub fn exchange(&self) -> &str {
        &self.exchange
    }

    /// Takes the key the new device reveals and checks it against the
    /// commitment. Returns the agreed secret.
    pub fn receive_reveal(self, message: &Message) -> Result<Agreed, EnrolmentError> {
        let Message::Reveal { exchange, key } = message else {
            return Err(EnrolmentError::Unexpected);
        };
        if *exchange != self.exchange {
            return Err(EnrolmentError::Unexpected);
        }
        // The check that defeats a server in the middle: the key shown now
        // must be the one fixed before this device sent its own.
        if commitment(&self.exchange, self.new_device, &self.new_device_key, key) != self.commitment
        {
            return Err(EnrolmentError::CommitmentBroken);
        }
        let their_key = decode_key(key)?;
        let my_temporary_key = self.temporary.public_key().to_base64();
        let secret = self
            .temporary
            .diffie_hellman(their_key)
            .map_err(|_| EnrolmentError::BadKey)?;
        Ok(Agreed {
            secret,
            transcript: Transcript {
                account: self.account,
                exchange: self.exchange,
                new_device: self.new_device,
                new_device_key: self.new_device_key,
                new_temporary_key: key.clone(),
                approver: self.me,
                approver_key: self.my_key,
                approver_temporary_key: my_temporary_key,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: Uuid = Uuid::from_u128(0xACC0);
    const PHONE: Uuid = Uuid::from_u128(0xF0E);
    const LAPTOP: Uuid = Uuid::from_u128(0x1A7);
    const PHONE_KEY: [u8; 32] = [7; 32];
    const LAPTOP_KEY: [u8; 32] = [9; 32];

    /// Runs an exchange in which `tamper` may change each message on its
    /// way, as a server could. Returns what each side ends with.
    fn exchange(
        mut tamper: impl FnMut(&mut Message),
    ) -> (
        Result<Agreed, EnrolmentError>,
        Result<Agreed, EnrolmentError>,
    ) {
        let (joining, mut commit) = Joining::start(ACCOUNT, PHONE, &PHONE_KEY);
        tamper(&mut commit);
        let answered = Approving::answer(ACCOUNT, LAPTOP, &LAPTOP_KEY, PHONE, &PHONE_KEY, &commit);
        let (approving, mut key) = match answered {
            Ok(pair) => pair,
            Err(error) => return (Err(EnrolmentError::Unexpected), Err(error)),
        };
        tamper(&mut key);
        let (phone, mut reveal) = match joining.receive_key(LAPTOP, &key) {
            Ok(pair) => pair,
            Err(error) => return (Err(error), Err(EnrolmentError::Unexpected)),
        };
        tamper(&mut reveal);
        (Ok(phone), approving.receive_reveal(&reveal))
    }

    fn fresh_key() -> String {
        Sas::new().public_key().to_base64()
    }

    #[test]
    fn two_honest_devices_see_the_same_code_and_pass_the_key() {
        let (phone, laptop) = exchange(|_| {});
        let (phone, laptop) = (phone.unwrap(), laptop.unwrap());

        let code = laptop.code();
        assert_eq!(phone.code(), code);
        // Three groups of four digits.
        let groups: Vec<&str> = code.split(' ').collect();
        assert_eq!(groups.len(), 3);
        assert!(groups
            .iter()
            .all(|group| group.len() == 4 && group.chars().all(|c| c.is_ascii_digit())));

        let account_key = AccountKey::generate();
        let sealed = laptop.seal_key(&account_key);
        assert_eq!(
            phone.open_key(&sealed).unwrap().as_bytes(),
            account_key.as_bytes()
        );
        // Sealing twice gives different bytes, and the key is not in them.
        assert_ne!(laptop.seal_key(&account_key), sealed);
        assert!(!sealed
            .windows(32)
            .any(|window| window == account_key.as_bytes()));
    }

    #[test]
    fn each_exchange_has_its_own_code() {
        let (first, _) = exchange(|_| {});
        let (second, _) = exchange(|_| {});
        assert_ne!(first.unwrap().code(), second.unwrap().code());
    }

    #[test]
    fn a_server_that_swaps_the_approving_devices_key_changes_the_codes() {
        // The server keeps the laptop's key for itself and gives the phone
        // one of its own.
        let (phone, laptop) = exchange(|message| {
            if let Message::Key { key, .. } = message {
                *key = fresh_key();
            }
        });
        let (phone, laptop) = (phone.unwrap(), laptop.unwrap());

        assert_ne!(phone.code(), laptop.code());
        // And the phone cannot open what the laptop seals.
        let sealed = laptop.seal_key(&AccountKey::generate());
        assert_eq!(
            phone.open_key(&sealed).unwrap_err(),
            EnrolmentError::KeyRejected
        );
    }

    #[test]
    fn a_server_that_swaps_the_new_devices_key_is_caught_by_the_commitment() {
        let (_, laptop) = exchange(|message| {
            if let Message::Reveal { key, .. } = message {
                *key = fresh_key();
            }
        });
        assert_eq!(laptop.unwrap_err(), EnrolmentError::CommitmentBroken);
    }

    #[test]
    fn a_server_that_commits_to_its_own_key_changes_the_codes() {
        // The server replaces the phone's commitment and reveal with its
        // own pair, which the laptop cannot tell from the phone's.
        let mine = fresh_key();
        let (phone, laptop) = exchange(|message| match message {
            Message::Commit {
                exchange,
                device_key,
                commitment: sent,
            } => *sent = commitment(exchange, PHONE, device_key, &mine),
            Message::Reveal { key, .. } => key.clone_from(&mine),
            _ => {}
        });
        // The laptop accepts the reveal, because it matches the commitment
        // it was shown. The codes are what give the server away.
        assert_ne!(phone.unwrap().code(), laptop.unwrap().code());
    }

    #[test]
    fn a_server_that_changes_a_registered_key_changes_the_codes_or_is_refused() {
        // The phone's registered key, changed in the message: the laptop
        // compares it with the device list and refuses.
        let (_, laptop) = exchange(|message| {
            if let Message::Commit { device_key, .. } = message {
                *device_key = STANDARD.encode([1; 32]);
            }
        });
        assert_eq!(laptop.unwrap_err(), EnrolmentError::RegisteredKeyChanged);

        // The laptop's registered key, changed on its way to the phone.
        let (phone, laptop) = exchange(|message| {
            if let Message::Key { device_key, .. } = message {
                *device_key = STANDARD.encode([1; 32]);
            }
        });
        assert_ne!(phone.unwrap().code(), laptop.unwrap().code());
    }

    #[test]
    fn the_code_depends_on_who_the_devices_are() {
        // The same messages read as coming from another approving device
        // give another code: the phone is told who answered by the server.
        let (joining, commit) = Joining::start(ACCOUNT, PHONE, &PHONE_KEY);
        let (approving, key) =
            Approving::answer(ACCOUNT, LAPTOP, &LAPTOP_KEY, PHONE, &PHONE_KEY, &commit).unwrap();
        let someone_else = Uuid::from_u128(0xBAD);
        let (phone, reveal) = joining.receive_key(someone_else, &key).unwrap();
        let laptop = approving.receive_reveal(&reveal).unwrap();
        assert_ne!(phone.code(), laptop.code());
    }

    #[test]
    fn a_key_sealed_in_one_exchange_does_not_open_in_another() {
        let (_, first_laptop) = exchange(|_| {});
        let (second_phone, _) = exchange(|_| {});
        let sealed = first_laptop.unwrap().seal_key(&AccountKey::generate());

        let second_phone = second_phone.unwrap();
        assert_eq!(
            second_phone.open_key(&sealed).unwrap_err(),
            EnrolmentError::KeyRejected
        );
        assert_eq!(
            second_phone.open_key(b"short").unwrap_err(),
            EnrolmentError::KeyRejected
        );
        // A flipped bit is rejected too.
        let (phone, laptop) = exchange(|_| {});
        let mut damaged = laptop.unwrap().seal_key(&AccountKey::generate());
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        assert_eq!(
            phone.unwrap().open_key(&damaged).unwrap_err(),
            EnrolmentError::KeyRejected
        );
    }

    #[test]
    fn messages_out_of_place_or_from_another_exchange_are_refused() {
        let (joining, commit) = Joining::start(ACCOUNT, PHONE, &PHONE_KEY);
        // A reveal where a key is expected.
        let reveal = Message::Reveal {
            exchange: joining.exchange().to_owned(),
            key: fresh_key(),
        };
        assert_eq!(
            joining.receive_key(LAPTOP, &reveal).unwrap_err(),
            EnrolmentError::Unexpected
        );
        // A key message where a commitment is expected.
        let key = Message::Key {
            exchange: "x".to_owned(),
            device_key: String::new(),
            key: fresh_key(),
        };
        assert_eq!(
            Approving::answer(ACCOUNT, LAPTOP, &LAPTOP_KEY, PHONE, &PHONE_KEY, &key).unwrap_err(),
            EnrolmentError::Unexpected
        );
        // A key for another exchange.
        let (joining, _) = Joining::start(ACCOUNT, PHONE, &PHONE_KEY);
        let (_, other_key) =
            Approving::answer(ACCOUNT, LAPTOP, &LAPTOP_KEY, PHONE, &PHONE_KEY, &commit).unwrap();
        assert_eq!(
            joining.receive_key(LAPTOP, &other_key).unwrap_err(),
            EnrolmentError::Unexpected
        );
        // A key that is not a key.
        let (joining, _) = Joining::start(ACCOUNT, PHONE, &PHONE_KEY);
        let nonsense = Message::Key {
            exchange: joining.exchange().to_owned(),
            device_key: String::new(),
            key: "not base64!".to_owned(),
        };
        assert_eq!(
            joining.receive_key(LAPTOP, &nonsense).unwrap_err(),
            EnrolmentError::BadKey
        );
    }

    #[test]
    fn messages_survive_the_trip_through_the_server() {
        let (_, commit) = Joining::start(ACCOUNT, PHONE, &PHONE_KEY);
        assert_eq!(Message::decode(&commit.encode()), Some(commit));
        let cancel = Message::Cancel {
            exchange: "abc".to_owned(),
            reason: CancelReason::Mismatch,
        };
        assert_eq!(cancel.exchange(), "abc");
        assert_eq!(Message::decode(&cancel.encode()), Some(cancel));
        assert_eq!(Message::decode(b"not a message"), None);
        assert_eq!(Message::decode(br#"{"kind":"from_the_future"}"#), None);
    }
}
