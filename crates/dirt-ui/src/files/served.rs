//! A file's bytes held in memory while the preview shows them, and handed
//! to the web view at an address of the app's own (interface design,
//! section 6.16; `docs/SECURITY.md`).
//!
//! The web view asks for a picture, a sound, a video or a PDF by an
//! address; it cannot be handed bytes another way without copying them
//! into the page as text. So the bytes are kept here, under an address
//! that names them, and the app answers the web view's request for that
//! address from here: each app registers one handler for [`ADDRESS`] and
//! responds with [`response`]. Nothing is written to disk, and every answer tells
//! the web view not to keep what it was given (`Cache-Control: no-store`).
//!
//! The preview holds one file at a time and lets it go as it moves to
//! another or closes ([`release_all`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// The first part of the address: the app's handler for the web view's
/// requests is registered under this name.
pub const ADDRESS: &str = "dirt-file";

/// One file's bytes, and the type the web view is told they have.
struct Held {
    bytes: Arc<Vec<u8>>,
    media_type: &'static str,
}

fn held() -> &'static Mutex<HashMap<String, Held>> {
    static HELD: OnceLock<Mutex<HashMap<String, Held>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<String, Held>> {
    // A panic while it was held leaves nothing half done in a map.
    held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Keeps a file's bytes in memory and returns the address the web view
/// asks for them by.
///
/// Each call makes a new address, so that an element given it loads it
/// afresh. `media_type` is one of the app's own
/// ([`super::kind::served_type`]).
pub fn hold(name: &str, bytes: Vec<u8>, media_type: &'static str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    // The file's name ends the address, for the web view's sake (a PDF's
    // title, a guess at a type); it is never read back.
    let tail: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '-'
            }
        })
        .take(60)
        .collect();
    let key = format!("{serial}/{tail}");
    lock().insert(
        key.clone(),
        Held {
            bytes: Arc::new(bytes),
            media_type,
        },
    );
    format!("/{ADDRESS}/{key}")
}

/// What turns an Ogg Opus sound into a WAV, for a web view that cannot
/// play Ogg Opus (by its documentation, macOS's before 15.4; interface
/// design 6.18). An app sets it only where its web view says so.
static OGG_AS_WAV: OnceLock<fn(&[u8]) -> Option<Vec<u8>>> = OnceLock::new();

/// Has the preview hand the web view an Ogg Opus sound decoded, as a WAV
/// made in memory, because the web view cannot play it as it is. Nothing
/// is written to disk.
pub fn play_ogg_as_wav(decode: fn(&[u8]) -> Option<Vec<u8>>) {
    let _ = OGG_AS_WAV.set(decode);
}

/// The bytes the web view is handed for a file of this type: the file's
/// own, or, for an Ogg sound the web view cannot play, the same sound as a
/// WAV.
pub(crate) fn playable(media_type: &'static str, bytes: Vec<u8>) -> (&'static str, Vec<u8>) {
    if media_type == "audio/ogg" {
        if let Some(wav) = OGG_AS_WAV.get().and_then(|decode| decode(&bytes)) {
            return ("audio/wav", wav);
        }
    }
    (media_type, bytes)
}

/// Lets go of every file held. The web view keeps what it has already
/// drawn until the element is taken away.
pub fn release_all() {
    lock().clear();
}

/// How many files are held: for the tests, and for anyone who doubts that
/// a closed preview holds nothing.
#[must_use]
pub fn held_count() -> usize {
    lock().len()
}

/// The answer to one request of the web view's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// 200, 206 for part of a file, 404 for an address that holds
    /// nothing (any more), 416 for a part that is not there.
    pub status: u16,
    /// What to send as `Content-Type`: one of the app's own.
    pub media_type: &'static str,
    /// What to send as `Content-Range`, for a part.
    pub range: Option<String>,
    pub body: Vec<u8>,
}

impl Reply {
    /// The headers the answer is sent with, besides its type: the web view
    /// keeps nothing, guesses no other type than the one it is told, and
    /// may ask for parts (a video it seeks in).
    pub const HEADERS: [(&'static str, &'static str); 3] = [
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Accept-Ranges", "bytes"),
    ];

    const fn nothing(status: u16) -> Self {
        Self {
            status,
            media_type: "text/plain",
            range: None,
            body: Vec::new(),
        }
    }
}

/// The part of `size` bytes a `Range` header asks for, first and last, or
/// `None` for one that cannot be had. Only one part is given.
fn part_asked(range: &str, size: usize) -> Option<(usize, usize)> {
    let range = range.trim().strip_prefix("bytes=")?;
    let (start, end) = range.split(',').next()?.trim().split_once('-')?;
    let last = size.checked_sub(1)?;
    let (first, until) = if start.is_empty() {
        // The last so many bytes.
        let wanted: usize = end.trim().parse().ok()?;
        (size.checked_sub(wanted.min(size))?, last)
    } else {
        let first: usize = start.trim().parse().ok()?;
        let until = if end.trim().is_empty() {
            last
        } else {
            end.trim().parse::<usize>().ok()?.min(last)
        };
        (first, until)
    };
    (first <= until).then_some((first, until))
}

/// Answers the web view's request for `path` (the address's path, which
/// begins with [`ADDRESS`]), with the part `range` asks for if it asks.
#[must_use]
pub fn answer(path: &str, range: Option<&str>) -> Reply {
    let key = path
        .trim_start_matches('/')
        .strip_prefix(ADDRESS)
        .unwrap_or_default()
        .trim_start_matches('/');
    let found = lock()
        .get(key)
        .map(|held| (Arc::clone(&held.bytes), held.media_type));
    let Some((bytes, media_type)) = found else {
        return Reply::nothing(404);
    };
    let Some(range) = range else {
        return Reply {
            status: 200,
            media_type,
            range: None,
            body: bytes.as_ref().clone(),
        };
    };
    match part_asked(range, bytes.len()) {
        Some((first, last)) => Reply {
            status: 206,
            media_type,
            range: Some(format!("bytes {first}-{last}/{}", bytes.len())),
            body: bytes[first..=last].to_vec(),
        },
        None => Reply {
            range: Some(format!("bytes */{}", bytes.len())),
            ..Reply::nothing(416)
        },
    }
}

/// The answer to the web view's request for `path`, as an HTTP response.
///
/// `range` is the `Range` header it sent, if any. The app's asset handler
/// hands the answer on. Every request is answered: a response that cannot be built
/// is a plain 500, so that nothing waits on one that never comes.
pub fn response(path: &str, range: Option<&str>) -> http::Response<Vec<u8>> {
    let reply = answer(path, range);
    let mut built = http::Response::builder()
        .status(reply.status)
        .header(http::header::CONTENT_TYPE, reply.media_type);
    for (name, value) in Reply::HEADERS {
        built = built.header(name, value);
    }
    if let Some(range) = reply.range {
        built = built.header(http::header::CONTENT_RANGE, range);
    }
    built.body(reply.body).unwrap_or_else(|error| {
        tracing::error!("Could not answer for a file: {error}");
        let mut failed = http::Response::new(Vec::new());
        *failed.status_mut() = http::StatusCode::INTERNAL_SERVER_ERROR;
        failed
    })
}

/// The `Range` header of a request, if it has one that is text.
pub fn range_of<T>(request: &http::Request<T>) -> Option<&str> {
    request
        .headers()
        .get(http::header::RANGE)
        .and_then(|range| range.to_str().ok())
}

/// Held by each test that holds files, since the files held are the
/// app's one set.
#[cfg(test)]
pub(super) static TESTS: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_held_answered_and_let_go() {
        let _one_at_a_time = TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        release_all();
        let address = hold(
            "receipt oct.pdf",
            b"%PDF-1.7 the pages".to_vec(),
            "application/pdf",
        );
        assert!(address.starts_with("/dirt-file/"), "{address}");
        assert!(address.ends_with("/receipt-oct.pdf"), "{address}");
        assert_eq!(held_count(), 1);

        let whole = answer(&address, None);
        assert_eq!(whole.status, 200);
        assert_eq!(whole.media_type, "application/pdf");
        assert_eq!(whole.body, b"%PDF-1.7 the pages");

        // A part, as a video seeking asks.
        let part = answer(&address, Some("bytes=1-3"));
        assert_eq!((part.status, part.body.as_slice()), (206, &b"PDF"[..]));
        assert_eq!(part.range.as_deref(), Some("bytes 1-3/18"));
        assert_eq!(answer(&address, Some("bytes=15-")).body, b"ges");
        assert_eq!(answer(&address, Some("bytes=-3")).body, b"ges");
        assert_eq!(answer(&address, Some("bytes=10-99")).body, b"he pages");
        assert_eq!(answer(&address, Some("bytes=40-50")).status, 416);
        assert_eq!(answer(&address, Some("pages=1-2")).status, 416);

        // Each holding has an address of its own.
        let again = hold("receipt oct.pdf", b"other".to_vec(), "application/pdf");
        assert_ne!(again, address);
        assert_eq!(held_count(), 2);

        // As the asset handler hands it on: the app's type, nosniff, a part.
        let sent = response(&again, Some("bytes=0-1"));
        assert_eq!(sent.status(), 206);
        assert_eq!(sent.headers()["content-type"], "application/pdf");
        assert_eq!(sent.headers()["x-content-type-options"], "nosniff");
        assert_eq!(sent.headers()["cache-control"], "no-store");
        assert_eq!(sent.headers()["content-range"], "bytes 0-1/5");
        assert_eq!(sent.body(), b"ot");

        release_all();
        assert_eq!(held_count(), 0);
        assert_eq!(answer(&address, None).status, 404);
        assert_eq!(response(&again, None).status(), 404);
        assert_eq!(answer("/dirt-file/", None).status, 404);
        assert_eq!(answer("/elsewhere/1/x", None).status, 404);
    }
}
