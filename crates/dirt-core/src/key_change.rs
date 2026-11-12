//! Changing the account's key: the keys a device holds, and how a new key
//! travels to each remaining device.
//!
//! Removing a device stops it reaching the server, but it keeps the account
//! key it had. A new key ends that. The device that makes the new key wraps
//! it once for every device that stays, and the server stores the wrapped
//! copies until each device fetches its own (design section 5.5).
//!
//! # How a key is wrapped
//!
//! The key that seals the new account key is derived, with HKDF-SHA256,
//! from two secrets together:
//!
//! - **An X25519 agreement** between a key pair made for this one message
//!   and the receiving device's registered key. Only that device can
//!   compute it. This is what keeps the new key from a device that was
//!   removed: it has the old account key, but not the remaining devices'
//!   private keys.
//! - **The account key being replaced.** Every device of the account has
//!   it and the server never has. This is what keeps the server from
//!   making a key change of its own: it knows every device's public key and
//!   could wrap a key it chose, but a device will not open what was wrapped
//!   without the old key.
//!
//! The new key is then sealed with XChaCha20-Poly1305. The account, the
//! receiving device, the new key's version and both public keys are bound
//! in, so a copy cannot be given to another device, or passed off as
//! another version.
//!
//! These are the primitives the rest of the system uses, from the same
//! audited crates, put together the way a sealed box is. Nothing here is a
//! new primitive.
//!
//! # What this does not stop
//!
//! A removed device and the server acting together. The device that makes
//! the new key learns the other devices' public keys from the server. It
//! has checked, by comparing a code, only the keys of devices it let in or
//! was let in by. If the server gives it a false key for another device and
//! shares what it receives with a removed device, the two together can open
//! that copy. `docs/SECURITY.md` says so.

use std::collections::BTreeMap;

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;
use uuid::Uuid;
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

use crate::crypto::{AccountKey, CryptoError, DeviceKeyPair, KEY_VERSION};

/// The first byte of a wrapped key, naming this layout: the sender's
/// one-time public key, then the nonce, then the sealed key.
const WRAP_LAYOUT: u8 = 1;
const PUBLIC_KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
/// A sealed 32-byte key with its 16-byte tag.
const SEALED_BYTES: usize = 32 + 16;
const WRAPPED_BYTES: usize = 1 + PUBLIC_KEY_BYTES + NONCE_BYTES + SEALED_BYTES;
/// Mixed into the key derivation and the associated data, so a wrapped key
/// can never be confused with anything else these keys protect.
const DOMAIN: &[u8] = b"dirt/account-key-change/v1";

/// The account keys a device holds: the one notes are written under, and
/// the ones before it that it still needs.
///
/// An older key is kept while notes under it remain on the server, to read
/// them, and it is what opens the wrapped copy of the key that follows it.
#[derive(Clone)]
pub struct KeyRing {
    version: i32,
    current: AccountKey,
    older: BTreeMap<i32, AccountKey>,
}

impl KeyRing {
    /// The keys of a device that has only ever held the account's first.
    #[must_use]
    pub const fn first(key: AccountKey) -> Self {
        Self::at(KEY_VERSION, key)
    }

    /// The keys of a device given `key` as version `version`, with none
    /// before it. A device let in after a key change starts like this.
    #[must_use]
    pub const fn at(version: i32, key: AccountKey) -> Self {
        Self {
            version,
            current: key,
            older: BTreeMap::new(),
        }
    }

    /// The keys as secure storage held them.
    #[must_use]
    pub const fn from_parts(
        version: i32,
        current: AccountKey,
        older: BTreeMap<i32, AccountKey>,
    ) -> Self {
        Self {
            version,
            current,
            older,
        }
    }

    /// The version notes are written under.
    #[must_use]
    pub const fn version(&self) -> i32 {
        self.version
    }

    /// The key notes are written under.
    #[must_use]
    pub const fn current(&self) -> &AccountKey {
        &self.current
    }

    /// The keys before the current one, by version.
    #[must_use]
    pub const fn older(&self) -> &BTreeMap<i32, AccountKey> {
        &self.older
    }

    /// The key of `version`, if this device holds it.
    #[must_use]
    pub fn get(&self, version: i32) -> Option<&AccountKey> {
        if version == self.version {
            Some(&self.current)
        } else {
            self.older.get(&version)
        }
    }

    /// Makes `key` the current key, as the version after the present one.
    /// The present key is kept among the older ones.
    pub fn adopt(&mut self, key: AccountKey) {
        let replaced = std::mem::replace(&mut self.current, key);
        self.older.insert(self.version, replaced);
        self.version += 1;
    }

    /// Forgets every key but the current one. Done when no note under an
    /// older key remains.
    pub fn forget_older(&mut self) {
        self.older.clear();
    }

    /// Whether both hold the same keys under the same versions.
    #[must_use]
    pub fn same_as(&self, other: &Self) -> bool {
        let bytes = |ring: &Self| -> Vec<(i32, [u8; 32])> {
            ring.older
                .iter()
                .map(|(version, key)| (*version, *key.as_bytes()))
                .collect()
        };
        self.version == other.version
            && self.current.as_bytes() == other.current.as_bytes()
            && bytes(self) == bytes(other)
    }
}

impl std::fmt::Debug for KeyRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyRing")
            .field("version", &self.version)
            .field("older", &self.older.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// What a wrapped key is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrapContext {
    pub account_id: Uuid,
    /// The device the copy is for.
    pub device_id: Uuid,
    /// The version of the key inside.
    pub key_version: i32,
}

/// The key that seals a new account key, and what is bound to it.
fn sealing(
    agreement: &[u8; 32],
    replaced: &AccountKey,
    one_time_public: &[u8; 32],
    device_public: &[u8; 32],
    context: &WrapContext,
) -> (XChaCha20Poly1305, Vec<u8>) {
    let mut bound = Vec::with_capacity(DOMAIN.len() + 2 * PUBLIC_KEY_BYTES + 36);
    bound.extend_from_slice(DOMAIN);
    bound.extend_from_slice(one_time_public);
    bound.extend_from_slice(device_public);
    bound.extend_from_slice(context.account_id.as_bytes());
    bound.extend_from_slice(context.device_id.as_bytes());
    bound.extend_from_slice(&context.key_version.to_be_bytes());

    let mut secrets = Zeroizing::new([0u8; 64]);
    secrets[..32].copy_from_slice(agreement);
    secrets[32..].copy_from_slice(replaced.as_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    // Thirty-two bytes is within what HKDF-SHA256 can give.
    let _ = Hkdf::<Sha256>::new(Some(context.account_id.as_bytes()), &*secrets)
        .expand(&bound, &mut *key);
    (XChaCha20Poly1305::new((&*key).into()), bound)
}

/// Wraps `new`, the key that replaces `replaced`, for the device whose
/// registered public key is `device_public`.
///
/// Fails if `device_public` is not a usable X25519 key: one of the wrong
/// length, or one of the few values that make the agreement the same for
/// everybody.
pub fn wrap_account_key(
    new: &AccountKey,
    replaced: &AccountKey,
    device_public: &[u8],
    context: &WrapContext,
) -> Result<Vec<u8>, CryptoError> {
    let device_public: [u8; 32] = device_public
        .try_into()
        .map_err(|_| CryptoError::BadKeyLength)?;
    let one_time = EphemeralSecret::random_from_rng(OsRng);
    let one_time_public = PublicKey::from(&one_time).to_bytes();
    let agreement = one_time.diffie_hellman(&PublicKey::from(device_public));
    if !agreement.was_contributory() {
        return Err(CryptoError::Rejected);
    }
    let (cipher, bound) = sealing(
        agreement.as_bytes(),
        replaced,
        &one_time_public,
        &device_public,
        context,
    );
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let sealed = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: new.as_bytes(),
                aad: &bound,
            },
        )
        // The cipher fails only for inputs far larger than a key.
        .expect("a key fits within the cipher's limit");

    let mut wrapped = Vec::with_capacity(WRAPPED_BYTES);
    wrapped.push(WRAP_LAYOUT);
    wrapped.extend_from_slice(&one_time_public);
    wrapped.extend_from_slice(&nonce);
    wrapped.extend_from_slice(&sealed);
    Ok(wrapped)
}

/// Opens a copy made by [`wrap_account_key`] for this device, with the key
/// it replaces.
///
/// Fails for a copy made for another device, account or version, one made
/// without the replaced key, and one that was changed.
pub fn unwrap_account_key(
    wrapped: &[u8],
    replaced: &AccountKey,
    device_key: &DeviceKeyPair,
    context: &WrapContext,
) -> Result<AccountKey, CryptoError> {
    let (&layout, rest) = wrapped.split_first().ok_or(CryptoError::UnknownLayout)?;
    if layout != WRAP_LAYOUT || wrapped.len() != WRAPPED_BYTES {
        return Err(CryptoError::UnknownLayout);
    }
    let (one_time_public, rest) = rest.split_at(PUBLIC_KEY_BYTES);
    let (nonce, sealed) = rest.split_at(NONCE_BYTES);
    let one_time_public: [u8; 32] = one_time_public
        .try_into()
        .map_err(|_| CryptoError::UnknownLayout)?;

    let agreement = device_key.agree(&one_time_public);
    let Some(agreement) = agreement else {
        return Err(CryptoError::Rejected);
    };
    let (cipher, bound) = sealing(
        &agreement,
        replaced,
        &one_time_public,
        &device_key.public_bytes(),
        context,
    );
    let key = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: sealed,
                aad: &bound,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| CryptoError::Rejected)?;
    AccountKey::from_bytes(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> WrapContext {
        WrapContext {
            account_id: Uuid::from_u128(0xA11CE),
            device_id: Uuid::from_u128(0xB0B),
            key_version: 2,
        }
    }

    #[test]
    fn the_device_it_was_wrapped_for_opens_it_with_the_replaced_key() {
        let (old, new) = (AccountKey::generate(), AccountKey::generate());
        let phone = DeviceKeyPair::generate();
        let wrapped = wrap_account_key(&new, &old, &phone.public_bytes(), &context()).unwrap();
        assert_eq!(wrapped.len(), WRAPPED_BYTES);
        let opened = unwrap_account_key(&wrapped, &old, &phone, &context()).unwrap();
        assert_eq!(opened.as_bytes(), new.as_bytes());

        // Wrapping twice gives different copies of one key.
        let again = wrap_account_key(&new, &old, &phone.public_bytes(), &context()).unwrap();
        assert_ne!(wrapped, again);
    }

    #[test]
    fn a_removed_device_cannot_open_a_copy_made_for_another() {
        // It has the old account key, and its own key pair, and whatever
        // the server holds. It does not have the phone's private key.
        let (old, new) = (AccountKey::generate(), AccountKey::generate());
        let (phone, removed) = (DeviceKeyPair::generate(), DeviceKeyPair::generate());
        let wrapped = wrap_account_key(&new, &old, &phone.public_bytes(), &context()).unwrap();
        assert_eq!(
            unwrap_account_key(&wrapped, &old, &removed, &context()).err(),
            Some(CryptoError::Rejected)
        );
    }

    /// The removed device as an attacker and not as an honest caller: it
    /// has the old account key and every public value, and derives the
    /// sealing key itself. Without the phone's private key it has to guess
    /// the agreement, and a guess does not open the copy.
    #[test]
    fn the_old_account_key_and_everything_public_do_not_open_a_copy() {
        let (old, new) = (AccountKey::generate(), AccountKey::generate());
        let (phone, removed) = (DeviceKeyPair::generate(), DeviceKeyPair::generate());
        let wrapped = wrap_account_key(&new, &old, &phone.public_bytes(), &context()).unwrap();
        let one_time_public: [u8; 32] = wrapped[1..33].try_into().unwrap();
        let (nonce, sealed) = wrapped[33..].split_at(NONCE_BYTES);

        let opens_with = |agreement: &[u8; 32]| {
            let (cipher, bound) = sealing(
                agreement,
                &old,
                &one_time_public,
                &phone.public_bytes(),
                &context(),
            );
            cipher
                .decrypt(
                    XNonce::from_slice(nonce),
                    Payload {
                        msg: sealed,
                        aad: &bound,
                    },
                )
                .is_ok()
        };
        // The agreement the removed device can compute with its own key,
        // and two that anyone can write down.
        let its_own = removed.agree(&one_time_public).unwrap();
        for guess in [its_own, [0u8; 32], one_time_public] {
            assert!(!opens_with(&guess));
        }
        // The phone's does open it: the check above tests something.
        assert!(opens_with(&phone.agree(&one_time_public).unwrap()));
    }

    #[test]
    fn a_key_the_server_wraps_by_itself_is_refused() {
        // The server knows the phone's public key, the account, the device
        // and the version. It has never had the account key being replaced.
        let (old, servers) = (AccountKey::generate(), AccountKey::generate());
        let phone = DeviceKeyPair::generate();
        let guess = AccountKey::generate();
        let forged = wrap_account_key(&servers, &guess, &phone.public_bytes(), &context()).unwrap();
        assert_eq!(
            unwrap_account_key(&forged, &old, &phone, &context()).err(),
            Some(CryptoError::Rejected)
        );
    }

    #[test]
    fn a_copy_moved_to_another_device_account_or_version_is_refused() {
        let (old, new) = (AccountKey::generate(), AccountKey::generate());
        let phone = DeviceKeyPair::generate();
        let wrapped = wrap_account_key(&new, &old, &phone.public_bytes(), &context()).unwrap();
        let other_device = WrapContext {
            device_id: Uuid::from_u128(0xCA7),
            ..context()
        };
        let other_account = WrapContext {
            account_id: Uuid::from_u128(0xD06),
            ..context()
        };
        let other_version = WrapContext {
            key_version: 3,
            ..context()
        };
        for moved in [other_device, other_account, other_version] {
            assert_eq!(
                unwrap_account_key(&wrapped, &old, &phone, &moved).err(),
                Some(CryptoError::Rejected)
            );
        }
    }

    #[test]
    fn a_changed_copy_is_refused_wherever_the_change_is() {
        let (old, new) = (AccountKey::generate(), AccountKey::generate());
        let phone = DeviceKeyPair::generate();
        let wrapped = wrap_account_key(&new, &old, &phone.public_bytes(), &context()).unwrap();
        for index in 1..wrapped.len() {
            let mut changed = wrapped.clone();
            changed[index] ^= 1;
            assert!(
                unwrap_account_key(&changed, &old, &phone, &context()).is_err(),
                "a change at byte {index} went unnoticed"
            );
        }
        let mut other_layout = wrapped.clone();
        other_layout[0] = 2;
        assert_eq!(
            unwrap_account_key(&other_layout, &old, &phone, &context()).err(),
            Some(CryptoError::UnknownLayout)
        );
        assert_eq!(
            unwrap_account_key(&wrapped[..wrapped.len() - 1], &old, &phone, &context()).err(),
            Some(CryptoError::UnknownLayout)
        );
        assert_eq!(
            unwrap_account_key(&[], &old, &phone, &context()).err(),
            Some(CryptoError::UnknownLayout)
        );
    }

    #[test]
    fn a_public_key_that_is_not_one_is_refused() {
        let (old, new) = (AccountKey::generate(), AccountKey::generate());
        assert_eq!(
            wrap_account_key(&new, &old, &[1u8; 31], &context()),
            Err(CryptoError::BadKeyLength)
        );
        // With this value the agreement is zero whoever makes it, so the
        // old account key alone would open the copy.
        assert_eq!(
            wrap_account_key(&new, &old, &[0u8; 32], &context()),
            Err(CryptoError::Rejected)
        );
    }

    #[test]
    fn a_ring_keeps_each_key_it_has_had_until_told_to_forget() {
        let first = AccountKey::generate();
        let mut ring = KeyRing::first(first.clone());
        assert_eq!(ring.version(), 1);
        assert_eq!(ring.get(1).unwrap().as_bytes(), first.as_bytes());
        assert!(ring.get(2).is_none());

        let second = AccountKey::generate();
        ring.adopt(second.clone());
        assert_eq!(ring.version(), 2);
        assert_eq!(ring.current().as_bytes(), second.as_bytes());
        assert_eq!(ring.get(1).unwrap().as_bytes(), first.as_bytes());

        let copy = ring.clone();
        assert!(ring.same_as(&copy));
        ring.forget_older();
        assert!(ring.get(1).is_none());
        assert!(!ring.same_as(&copy));
        assert_eq!(ring.get(2).unwrap().as_bytes(), second.as_bytes());

        let late = KeyRing::at(2, second);
        assert!(late.same_as(&ring));
    }
}
