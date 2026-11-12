//! Encrypting a file.
//!
//! A file is encrypted in pieces of 64 KiB, each sealed with
//! XChaCha20-Poly1305, the cipher the notes use. A device never holds a
//! whole file in memory, and a file is read back piece by piece with each
//! one checked before it is used.
//!
//! The pieces are chained by the STREAM construction (Hoang, Reyhanitabar,
//! Rogaway and Vizár), as the `aead` crate implements it: every piece's
//! nonce carries its position and whether it is the last. So a piece that
//! is moved, repeated or left out does not open, and neither does a file
//! cut short at the end of a piece. Nothing here implements a cipher.
//!
//! Layout of a sealed file:
//!
//! ```text
//! 1 byte    the layout, 1
//! 19 bytes  a random value, the start of every piece's nonce
//! pieces    each up to 64 KiB of the file, followed by its 16-byte tag
//! ```
//!
//! An empty file is one empty piece. The account and the file's ID are
//! bound into every piece, so a file cannot be passed off as another, nor
//! as a file of another account.

use std::io::{self, Read, Write};

use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::aead::{KeyInit, OsRng, Payload};
use chacha20poly1305::XChaCha20Poly1305;
use uuid::Uuid;

use super::FileKey;

/// How much of a file goes into one piece.
pub const PIECE_BYTES: usize = 64 * 1024;
const TAG_BYTES: usize = 16;
/// The first byte of every sealed file, naming this layout.
const LAYOUT: u8 = 1;
/// A nonce is 24 bytes. The last five are the piece's position and whether
/// it is the last, which the construction fills in.
const NONCE_PREFIX_BYTES: usize = 19;
const HEADER_BYTES: usize = 1 + NONCE_PREFIX_BYTES;
/// Mixed into the associated data, so that a piece of a file can never be
/// taken for anything else encrypted on this device.
const DOMAIN: &[u8] = b"dirt/file/v1";

#[derive(Debug, thiserror::Error)]
pub enum SealError {
    /// The file could not be read or written.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// The sealed file is too short or starts with a layout this version
    /// does not know. A newer app wrote it, or it is not a sealed file.
    #[error("the file has an unknown layout")]
    UnknownLayout,
    /// A piece did not open: the key is wrong, the file was changed or cut
    /// short, or it is another file's.
    #[error("the file could not be decrypted")]
    Rejected,
}

/// What a sealed file is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileContext {
    pub account_id: Uuid,
    pub file_id: Uuid,
}

impl FileContext {
    fn associated_data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(DOMAIN.len() + 32);
        data.extend_from_slice(DOMAIN);
        data.extend_from_slice(self.account_id.as_bytes());
        data.extend_from_slice(self.file_id.as_bytes());
        data
    }
}

/// The size of a file of `plain` bytes once it is sealed. The server is
/// told this before the file is sent, and counts it.
#[must_use]
pub const fn sealed_size(plain: u64) -> u64 {
    let piece = PIECE_BYTES as u64;
    let pieces = if plain == 0 { 1 } else { plain.div_ceil(piece) };
    HEADER_BYTES as u64 + plain + pieces * TAG_BYTES as u64
}

/// Reads until `buffer` holds `size` bytes or the source ends.
fn read_piece(source: &mut impl Read, buffer: &mut Vec<u8>, size: usize) -> io::Result<()> {
    buffer.clear();
    source.take(size as u64).read_to_end(buffer)?;
    Ok(())
}

/// Encrypts everything `source` yields into `sink`. Returns how many bytes
/// were written, which is [`sealed_size`] of what was read.
pub fn seal(
    key: &FileKey,
    context: &FileContext,
    mut source: impl Read,
    mut sink: impl Write,
) -> Result<u64, SealError> {
    let mut prefix = [0u8; NONCE_PREFIX_BYTES];
    OsRng.fill_bytes(&mut prefix);
    sink.write_all(&[LAYOUT])?;
    sink.write_all(&prefix)?;
    let mut written = HEADER_BYTES as u64;

    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let mut pieces = EncryptorBE32::from_aead(cipher, prefix.as_slice().into());
    let aad = context.associated_data();
    // One piece is read ahead, because the last piece is sealed as the
    // last, and a piece is known to be the last only when nothing follows.
    let (mut piece, mut next) = (Vec::new(), Vec::new());
    read_piece(&mut source, &mut piece, PIECE_BYTES)?;
    loop {
        read_piece(&mut source, &mut next, PIECE_BYTES)?;
        let payload = Payload {
            msg: &piece,
            aad: &aad,
        };
        if next.is_empty() {
            let sealed = pieces
                .encrypt_last(payload)
                .map_err(|_| SealError::Rejected)?;
            sink.write_all(&sealed)?;
            return Ok(written + sealed.len() as u64);
        }
        let sealed = pieces
            .encrypt_next(payload)
            .map_err(|_| SealError::Rejected)?;
        sink.write_all(&sealed)?;
        written += sealed.len() as u64;
        std::mem::swap(&mut piece, &mut next);
    }
}

/// Decrypts a sealed file from `source` into `sink`. Returns how many
/// bytes of the file were written.
///
/// Each piece is written once it has been checked. If a later piece does
/// not open, what was written is the true beginning of the file and the
/// answer is an error: the caller writes to a place it can throw away.
pub fn open(
    key: &FileKey,
    context: &FileContext,
    mut source: impl Read,
    mut sink: impl Write,
) -> Result<u64, SealError> {
    let mut header = [0u8; HEADER_BYTES];
    source
        .read_exact(&mut header)
        .map_err(|_| SealError::UnknownLayout)?;
    if header[0] != LAYOUT {
        return Err(SealError::UnknownLayout);
    }
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let mut pieces = DecryptorBE32::from_aead(cipher, header[1..].into());
    let aad = context.associated_data();
    let sealed_piece = PIECE_BYTES + TAG_BYTES;
    let (mut piece, mut next) = (Vec::new(), Vec::new());
    read_piece(&mut source, &mut piece, sealed_piece)?;
    let mut written = 0u64;
    loop {
        read_piece(&mut source, &mut next, sealed_piece)?;
        let payload = Payload {
            msg: &piece,
            aad: &aad,
        };
        if next.is_empty() {
            let plain = pieces
                .decrypt_last(payload)
                .map_err(|_| SealError::Rejected)?;
            sink.write_all(&plain)?;
            return Ok(written + plain.len() as u64);
        }
        let plain = pieces
            .decrypt_next(payload)
            .map_err(|_| SealError::Rejected)?;
        sink.write_all(&plain)?;
        written += plain.len() as u64;
        std::mem::swap(&mut piece, &mut next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> FileContext {
        FileContext {
            account_id: Uuid::from_u128(1),
            file_id: Uuid::from_u128(2),
        }
    }

    /// Bytes that differ from piece to piece, so that pieces cannot be
    /// swapped unnoticed by the comparison.
    fn file_of(size: usize) -> Vec<u8> {
        (0..size)
            .map(|index| u8::try_from((index / 7 + index / PIECE_BYTES) % 251).unwrap())
            .collect()
    }

    fn sealed(key: &FileKey, plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let written = seal(key, &context(), plain, &mut out).unwrap();
        assert_eq!(written, out.len() as u64);
        out
    }

    fn opened(key: &FileKey, context: &FileContext, sealed: &[u8]) -> Result<Vec<u8>, SealError> {
        let mut out = Vec::new();
        let written = open(key, context, sealed, &mut out)?;
        assert_eq!(written, out.len() as u64);
        Ok(out)
    }

    #[test]
    fn a_file_of_any_size_opens_as_it_was_and_seals_to_the_size_foretold() {
        let key = FileKey::generate();
        for size in [
            0,
            1,
            PIECE_BYTES - 1,
            PIECE_BYTES,
            PIECE_BYTES + 1,
            2 * PIECE_BYTES,
            3 * PIECE_BYTES + 5,
        ] {
            let plain = file_of(size);
            let sealed = sealed(&key, &plain);
            assert_eq!(sealed.len() as u64, sealed_size(size as u64), "{size}");
            assert_eq!(opened(&key, &context(), &sealed).unwrap(), plain, "{size}");
        }
    }

    #[test]
    fn the_sealed_file_does_not_hold_the_file() {
        let key = FileKey::generate();
        let plain = b"a line of text that would be easy to find in what is stored".repeat(40);
        let sealed = sealed(&key, &plain);
        assert!(!sealed
            .windows(24)
            .any(|window| plain.windows(24).any(|p| p == window)));
        // Sealed twice, the same file is different bytes.
        assert_ne!(sealed, super::tests::sealed(&key, &plain));
    }

    #[test]
    fn another_key_account_or_file_opens_nothing() {
        let key = FileKey::generate();
        let sealed = sealed(&key, &file_of(100));
        let other_key = FileKey::generate();
        assert!(matches!(
            opened(&other_key, &context(), &sealed),
            Err(SealError::Rejected)
        ));
        for other in [
            FileContext {
                account_id: Uuid::from_u128(9),
                ..context()
            },
            FileContext {
                file_id: Uuid::from_u128(9),
                ..context()
            },
        ] {
            assert!(matches!(
                opened(&key, &other, &sealed),
                Err(SealError::Rejected)
            ));
        }
    }

    #[test]
    fn a_file_that_was_changed_does_not_open() {
        let key = FileKey::generate();
        let sealed = sealed(&key, &file_of(2 * PIECE_BYTES + 10));
        for index in [
            HEADER_BYTES,
            HEADER_BYTES + PIECE_BYTES + TAG_BYTES + 3,
            sealed.len() - 1,
        ] {
            let mut changed = sealed.clone();
            changed[index] ^= 1;
            assert!(
                matches!(opened(&key, &context(), &changed), Err(SealError::Rejected)),
                "a change at byte {index} went unnoticed"
            );
        }
        // A change to the random value every nonce starts with.
        let mut changed = sealed;
        changed[3] ^= 1;
        assert!(matches!(
            opened(&key, &context(), &changed),
            Err(SealError::Rejected)
        ));
    }

    #[test]
    fn pieces_cannot_be_dropped_repeated_or_reordered() {
        let key = FileKey::generate();
        let sealed = sealed(&key, &file_of(3 * PIECE_BYTES));
        let piece = PIECE_BYTES + TAG_BYTES;
        let part = |index: usize| {
            &sealed[HEADER_BYTES + index * piece..HEADER_BYTES + (index + 1) * piece]
        };
        let rebuilt = |order: &[usize]| {
            let mut file = sealed[..HEADER_BYTES].to_vec();
            for index in order {
                file.extend_from_slice(part(*index));
            }
            file
        };
        assert!(opened(&key, &context(), &rebuilt(&[0, 1, 2])).is_ok());
        for order in [
            &[0, 1][..],    // cut short at the end of a piece
            &[1, 2][..],    // the first piece left out
            &[0, 2][..],    // the middle piece left out
            &[1, 0, 2][..], // two pieces exchanged
            &[0, 1, 1, 2][..],
            &[0, 1, 2, 2][..],
        ] {
            assert!(
                matches!(
                    opened(&key, &context(), &rebuilt(order)),
                    Err(SealError::Rejected)
                ),
                "pieces in the order {order:?} opened"
            );
        }
        // Cut short in the middle of a piece.
        assert!(matches!(
            opened(&key, &context(), &sealed[..sealed.len() - 5]),
            Err(SealError::Rejected)
        ));
    }

    #[test]
    fn what_is_not_a_sealed_file_is_told_apart_from_one_that_was_changed() {
        let key = FileKey::generate();
        for not_sealed in [&b""[..], &b"short"[..]] {
            assert!(matches!(
                opened(&key, &context(), not_sealed),
                Err(SealError::UnknownLayout)
            ));
        }
        let mut newer = sealed(&key, b"x");
        newer[0] = 2;
        assert!(matches!(
            opened(&key, &context(), &newer),
            Err(SealError::UnknownLayout)
        ));
        // The header alone: no piece at all, where an empty file has one.
        let header_only = &sealed(&key, b"")[..HEADER_BYTES];
        assert!(matches!(
            opened(&key, &context(), header_only),
            Err(SealError::Rejected)
        ));
    }
}
