//! The protocol between a Dirt client and the server.
//!
//! Every body is JSON. Binary values (public keys and encrypted note blobs)
//! travel as base64 text. The server never reads a note: a blob is opaque
//! ciphertext, and its size is the only thing the server can judge.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::NoteId;

/// The version of this protocol. A client sends it in [`PROTOCOL_HEADER`];
/// a server that sees a higher number than it knows refuses the request with
/// [`ErrorCode::UnsupportedProtocol`].
pub const PROTOCOL_VERSION: u32 = 1;

/// Request header carrying the client's protocol version.
pub const PROTOCOL_HEADER: &str = "dirt-protocol";

/// Response header carrying the account's epoch on every authenticated call.
///
/// When the number changes, the account was reset or restored from a backup:
/// the client forgets its cursor and every change number it has seen, then
/// merges everything again.
pub const EPOCH_HEADER: &str = "dirt-account-epoch";

/// Limits both sides must agree on. The server enforces them; a client that
/// respects them is never refused for size.
pub mod limits {
    /// Largest encrypted blob for one note.
    pub const MAX_NOTE_BYTES: usize = 1024 * 1024;
    /// Most ciphertext in one push batch or one pull page. The hosting
    /// platform rejects bodies over 4.5 MB, and base64 adds a third.
    pub const MAX_BATCH_BYTES: usize = 3_000_000;
    /// Most notes in one push batch.
    pub const MAX_BATCH_ITEMS: usize = 500;
    /// Most notes one account may hold, deleted notes included.
    pub const MAX_NOTES_PER_ACCOUNT: i64 = 20_000;
    /// Most ciphertext one account may hold.
    pub const MAX_ACCOUNT_BYTES: i64 = 50 * 1024 * 1024;
    /// A full account still accepts a push that grows a note by no more
    /// than this, so that a delete, which adds a small marker, goes through.
    pub const FULL_ACCOUNT_GROWTH_BYTES: i64 = 1024;
    /// The point past which a full account accepts no growth at all, as a
    /// percentage of [`MAX_ACCOUNT_BYTES`].
    pub const ACCOUNT_CEILING_PERCENT: i64 = 105;
    /// Authenticated requests one account may make in one window.
    pub const REQUESTS_PER_WINDOW: i32 = 600;
    /// Length of the request-counting window, in seconds.
    pub const REQUEST_WINDOW_SECS: i64 = 600;
    /// Devices one account may have waiting for approval.
    pub const MAX_PENDING_DEVICES: i64 = 3;
    /// Longest device name, in bytes.
    pub const MAX_DEVICE_NAME_BYTES: usize = 100;
    /// Length of a device's public key (X25519).
    pub const PUBLIC_KEY_BYTES: usize = 32;
    /// A device token stops working after this long without use.
    pub const DEVICE_TOKEN_IDLE_DAYS: i64 = 90;
    /// How long a sign-in nonce can be used after it is issued.
    pub const NONCE_LIFETIME_SECS: u32 = 600;
}

// ---- Errors ----

/// What went wrong, as a stable name a client can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The server failed. Nothing about the request was wrong.
    ServerError,
    /// The account made too many requests; wait and retry.
    RateLimited,
    /// No device token, or one the server does not know.
    Unauthenticated,
    /// The device was removed from the account.
    DeviceRevoked,
    /// The device token went unused for too long.
    DeviceExpired,
    /// The device is waiting to be approved by another device.
    DevicePending,
    /// The Clerk sign-in or its nonce was not accepted.
    SignInRejected,
    /// The account already has as many devices waiting as it may.
    TooManyPendingDevices,
    /// The client speaks a newer protocol than this server.
    UnsupportedProtocol,
    /// The request is malformed or breaks a rule of the protocol.
    InvalidRequest,
    /// Nothing exists at this address or with this ID.
    NotFound,
    /// The request body is larger than a batch may be.
    PayloadTooLarge,
    /// One note's blob is larger than a note may be.
    NoteTooLarge,
    /// The account is at its storage or note-count quota.
    QuotaExceeded,
}

/// What a client does about an error. The class alone decides: no client
/// code looks at an error's message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Back off and retry; show the account as offline meanwhile.
    TryAgainLater,
    /// Stop syncing until the person signs in again or the device is
    /// approved. Local notes are untouched.
    StopUntilSignedIn,
    /// Stop syncing until the app is updated. Never overwrite what this
    /// version cannot read.
    UpdateTheApp,
    /// Flag the one note concerned and keep syncing the rest.
    ProblemWithOneNote,
}

impl ErrorCode {
    /// The class this code belongs to.
    #[must_use]
    pub const fn class(self) -> ErrorClass {
        match self {
            Self::ServerError | Self::RateLimited => ErrorClass::TryAgainLater,
            Self::Unauthenticated
            | Self::DeviceRevoked
            | Self::DeviceExpired
            | Self::DevicePending
            | Self::SignInRejected
            | Self::TooManyPendingDevices => ErrorClass::StopUntilSignedIn,
            // A request the server cannot make sense of means the two sides
            // disagree about the protocol, which only an update can fix.
            Self::UnsupportedProtocol | Self::InvalidRequest | Self::PayloadTooLarge => {
                ErrorClass::UpdateTheApp
            }
            Self::NotFound | Self::NoteTooLarge | Self::QuotaExceeded => {
                ErrorClass::ProblemWithOneNote
            }
        }
    }
}

/// The body of every error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    /// For logs and developers. Clients act on `code`, never on this.
    pub message: String,
    /// How long to wait before retrying, when the server knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u32>,
}

// ---- Devices ----

/// `POST /v1/nonces`: a value to put in the Clerk sign-in as its nonce,
/// with where to sign in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NonceResponse {
    pub nonce: String,
    pub expires_in_secs: u32,
    /// The identity provider this server accepts sign-ins from. A client
    /// is configured with the server's address only and learns the rest
    /// here, so the two can never disagree about where to sign in.
    pub issuer: String,
    /// The OAuth client ID to sign in with. It is a public client: there
    /// is no secret.
    pub client_id: String,
}

/// `POST /v1/devices`: registers the device that just signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterDeviceRequest {
    /// The ID token Clerk issued for this sign-in.
    pub id_token: String,
    /// The nonce from `POST /v1/nonces` that the sign-in carried.
    pub nonce: String,
    /// A name the person will recognise the device by.
    pub name: String,
    /// The device's public key. The private half never leaves the device.
    #[serde(with = "base64_bytes")]
    pub public_key: Vec<u8>,
    /// The token this device held before, when it signs in again because
    /// that token expired. With the same public key it shows the server
    /// that this is a device it already knows, which then keeps its ID and
    /// its status. The public key alone would prove nothing: every approved
    /// device of the account can read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    /// Waiting for an approved device to hand over the account key.
    Pending,
    Approved,
    /// Removed from the account. Its token no longer works.
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterDeviceResponse {
    pub account_id: Uuid,
    pub device_id: Uuid,
    /// The device's token, shown this once. The server keeps only a hash.
    pub token: String,
    pub status: DeviceStatus,
    /// True for the device that must create the account key: the first one
    /// the account has ever had.
    pub first_device: bool,
    pub epoch: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "base64_bytes")]
    pub public_key: Vec<u8>,
    pub status: DeviceStatus,
    pub created_at_ms: i64,
    pub last_seen_ms: i64,
    /// True for the device that made the request.
    pub current: bool,
}

/// `GET /v1/devices`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceList {
    pub devices: Vec<DeviceInfo>,
}

// ---- Notes ----

/// One note's encrypted document as the server holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteBlob {
    pub note_id: NoteId,
    /// The account-wide change number of this version. It is the note's
    /// version and the pull cursor in one.
    pub change: i64,
    /// Which account key encrypted the blob.
    pub key_version: i32,
    /// Which document format is inside the blob.
    pub format_version: i32,
    /// The blob, or `None` for a note the server has purged.
    #[serde(with = "base64_bytes_option")]
    pub ciphertext: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushItem {
    pub note_id: NoteId,
    /// The change number of the version this one was made from: zero for a
    /// note the server has never had.
    pub base_change: i64,
    pub key_version: i32,
    pub format_version: i32,
    #[serde(with = "base64_bytes")]
    pub ciphertext: Vec<u8>,
}

/// `POST /v1/notes/push`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRequest {
    pub items: Vec<PushItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PushOutcome {
    /// Stored. `change` is the note's new change number.
    Accepted { change: i64 },
    /// Somebody else wrote the note first, or the base named a version the
    /// server does not have.
    Stale {
        /// The change number of the version the server holds, or `None`
        /// if it has no such note.
        current_change: Option<i64>,
        /// The server's copy, to merge with before pushing again. It is
        /// left out when the server has no such note, and when the
        /// response already carries as much as a batch may: the client
        /// then gets the copy from its next pull.
        current: Option<NoteBlob>,
    },
    /// Refused for a reason that concerns this note alone.
    Rejected { code: ErrorCode, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushResult {
    pub note_id: NoteId,
    #[serde(flatten)]
    pub outcome: PushOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushResponse {
    pub epoch: i32,
    /// One result per item, in the order the items were sent.
    pub results: Vec<PushResult>,
}

/// `GET /v1/notes/pull?after=N`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullResponse {
    pub epoch: i32,
    /// Blobs changed after the cursor, lowest change number first.
    pub items: Vec<NoteBlob>,
    /// The cursor for the next pull.
    pub next_after: i64,
    /// True if more changes were waiting than fitted in this page.
    pub more: bool,
}

mod base64_bytes {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

mod base64_bytes_option {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::ref_option)] // the signature serde's `with` requires
    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(bytes) => serializer.serialize_some(&STANDARD.encode(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| STANDARD.decode(text).map_err(serde::de::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> NoteId {
        "01932aaa-0000-7000-8000-000000000123".parse().unwrap()
    }

    #[test]
    fn every_error_code_has_the_class_the_design_gives_it() {
        use ErrorClass::{ProblemWithOneNote, StopUntilSignedIn, TryAgainLater, UpdateTheApp};
        let expected = [
            (ErrorCode::ServerError, TryAgainLater),
            (ErrorCode::RateLimited, TryAgainLater),
            (ErrorCode::Unauthenticated, StopUntilSignedIn),
            (ErrorCode::DeviceRevoked, StopUntilSignedIn),
            (ErrorCode::DeviceExpired, StopUntilSignedIn),
            (ErrorCode::DevicePending, StopUntilSignedIn),
            (ErrorCode::SignInRejected, StopUntilSignedIn),
            (ErrorCode::TooManyPendingDevices, StopUntilSignedIn),
            (ErrorCode::UnsupportedProtocol, UpdateTheApp),
            (ErrorCode::InvalidRequest, UpdateTheApp),
            (ErrorCode::PayloadTooLarge, UpdateTheApp),
            (ErrorCode::NotFound, ProblemWithOneNote),
            (ErrorCode::NoteTooLarge, ProblemWithOneNote),
            (ErrorCode::QuotaExceeded, ProblemWithOneNote),
        ];
        for (code, class) in expected {
            assert_eq!(code.class(), class, "{code:?}");
        }
    }

    #[test]
    fn error_codes_travel_as_snake_case_names() {
        let body = ErrorBody {
            code: ErrorCode::QuotaExceeded,
            message: "full".to_owned(),
            retry_after_secs: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert_eq!(json, r#"{"code":"quota_exceeded","message":"full"}"#);
        assert_eq!(serde_json::from_str::<ErrorBody>(&json).unwrap(), body);
    }

    #[test]
    fn blobs_travel_as_base64_and_a_purged_note_as_null() {
        let blob = NoteBlob {
            note_id: note(),
            change: 7,
            key_version: 1,
            format_version: 1,
            ciphertext: Some(vec![0, 255, 16]),
        };
        let json = serde_json::to_value(&blob).unwrap();
        assert_eq!(json["ciphertext"], "AP8Q");
        assert_eq!(serde_json::from_value::<NoteBlob>(json).unwrap(), blob);

        let purged = NoteBlob {
            ciphertext: None,
            ..blob
        };
        let json = serde_json::to_value(&purged).unwrap();
        assert!(json["ciphertext"].is_null());
        assert_eq!(serde_json::from_value::<NoteBlob>(json).unwrap(), purged);
    }

    #[test]
    fn a_push_result_names_its_outcome_in_a_status_field() {
        let accepted = PushResult {
            note_id: note(),
            outcome: PushOutcome::Accepted { change: 3 },
        };
        let json = serde_json::to_value(&accepted).unwrap();
        assert_eq!(json["status"], "accepted");
        assert_eq!(json["change"], 3);
        assert_eq!(
            serde_json::from_value::<PushResult>(json).unwrap(),
            accepted
        );

        let stale = PushResult {
            note_id: note(),
            outcome: PushOutcome::Stale {
                current_change: None,
                current: None,
            },
        };
        let json = serde_json::to_value(&stale).unwrap();
        assert_eq!(json["status"], "stale");
        assert_eq!(serde_json::from_value::<PushResult>(json).unwrap(), stale);
    }

    #[test]
    fn text_that_is_not_base64_is_rejected() {
        let json = serde_json::json!({
            "note_id": note(), "base_change": 0, "key_version": 1,
            "format_version": 1, "ciphertext": "not base64!"
        });
        assert!(serde_json::from_value::<PushItem>(json).is_err());
    }

    #[test]
    fn a_full_batch_fits_under_the_hosting_limit_once_encoded() {
        // Base64 turns three bytes into four characters.
        let encoded = limits::MAX_BATCH_BYTES.div_ceil(3) * 4;
        assert!(encoded < 4_400_000, "{encoded}");
    }
}
