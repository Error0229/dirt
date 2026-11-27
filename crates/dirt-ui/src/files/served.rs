//! Files' bytes held in memory while the preview is open, and handed to
//! the web view at an address of the app's own (interface design, section
//! 6.16; `docs/SECURITY.md`).
//!
//! The web view asks for a picture, a sound, a video or a PDF by an
//! address; it cannot be handed bytes another way without copying them
//! into the page as text. So the bytes are kept here, under an address
//! that names them, and the app answers the web view's request for that
//! address from here: each app registers one handler for [`ADDRESS`] and
//! responds with [`response`]. Nothing is written to disk, and every answer tells
//! the web view not to keep what it was given (`Cache-Control: no-store`).
//!
//! While the preview is open it holds the file it shows, the pictures
//! either side of it, read ahead, and the files shown most recently: the
//! least recently shown are let go of first once those besides the file
//! shown come to more than [`BUDGET`] bytes. The file shown is never let
//! go of for the budget. A file keeps its address for as long as it is
//! held, so that a step back to it names what the page has already drawn.
//! All of it is let go of as the preview closes, or another note opens
//! ([`release_all`]); one file as it is removed ([`release`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use uuid::Uuid;

/// The first part of the address: the app's handler for the web view's
/// requests is registered under this name.
pub const ADDRESS: &str = "dirt-file";

const MIB: usize = 1024 * 1024;

/// How many bytes of files the preview keeps in memory besides the file
/// it shows: 96 MiB on Android, 256 MiB on the desktop (design 6.16).
///
/// A file is 10 MiB at most, and a phone's photograph some 3 to 5 MiB. So
/// Android keeps the two either side and at least seven more of the
/// largest files, some twenty photographs, which for most notes is all
/// of them (a note has 20 files at most). The web view holds what it
/// draws besides, and an app in the background is the first the system
/// ends when memory runs short, so it is held to that. A computer keeps a
/// whole note of the largest files but a few, which costs it little.
pub const BUDGET: usize = if cfg!(target_os = "android") {
    96 * MIB
} else {
    256 * MIB
};

/// One file's bytes, and the type the web view is told they have.
struct Held {
    file: Uuid,
    bytes: Arc<Vec<u8>>,
    media_type: &'static str,
    /// When it was last held or shown: the least recent goes first.
    used: u64,
}

/// The files held, by the key their address ends with.
struct Holding {
    held: HashMap<String, Held>,
    /// The file the preview shows: never let go of for the budget.
    shown: Option<Uuid>,
    budget: usize,
    clock: u64,
}

impl Holding {
    fn new(budget: usize) -> Self {
        Self {
            held: HashMap::new(),
            shown: None,
            budget,
            clock: 0,
        }
    }

    const fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn key_of(&self, file: Uuid) -> Option<&String> {
        self.held
            .iter()
            .find(|(_, held)| held.file == file)
            .map(|(key, _)| key)
    }

    /// Holds a file's bytes under `key`, unless the file is held already:
    /// then what is held stays, at its address, and these bytes are
    /// dropped. With `shown`, it is the file shown from now, before any
    /// other is let go of for it. The key the file is held under, or
    /// `None` if it does not fit in the budget.
    fn hold(
        &mut self,
        key: String,
        file: Uuid,
        bytes: Vec<u8>,
        media_type: &'static str,
        shown: bool,
    ) -> Option<String> {
        if shown {
            self.shown = Some(file);
        }
        if let Some(found) = self.key_of(file).cloned() {
            if shown {
                self.show(file);
            }
            return Some(found);
        }
        // Read ahead, a file larger than the whole budget would only push
        // out everything else before going itself: it is not held.
        if !shown && Some(file) != self.shown && bytes.len() > self.budget {
            return None;
        }
        let used = self.tick();
        self.held.insert(
            key.clone(),
            Held {
                file,
                bytes: Arc::new(bytes),
                media_type,
                used,
            },
        );
        self.keep_to_budget();
        self.held.contains_key(&key).then_some(key)
    }

    /// A file held is the one shown now: the most recent, and never let
    /// go of for the budget. The key it is held under; `None`, and nothing
    /// changed, if it is not held.
    fn show(&mut self, file: Uuid) -> Option<String> {
        let key = self.key_of(file)?.clone();
        self.shown = Some(file);
        let used = self.tick();
        if let Some(held) = self.held.get_mut(&key) {
            held.used = used;
        }
        self.keep_to_budget();
        Some(key)
    }

    /// Lets go of the least recently shown files, never the one shown,
    /// until the others come to no more than the budget.
    fn keep_to_budget(&mut self) {
        let shown = self.shown;
        let others = |holding: &Self| -> usize {
            holding
                .held
                .values()
                .filter(|held| Some(held.file) != shown)
                .map(|held| held.bytes.len())
                .sum()
        };
        while others(self) > self.budget {
            let oldest = self
                .held
                .iter()
                .filter(|(_, held)| Some(held.file) != shown)
                .min_by_key(|(_, held)| held.used)
                .map(|(key, _)| key.clone());
            let Some(key) = oldest else { return };
            self.held.remove(&key);
        }
    }

    fn release(&mut self, file: Uuid) {
        self.held.retain(|_, held| held.file != file);
        if self.shown == Some(file) {
            self.shown = None;
        }
    }

    fn clear(&mut self) {
        self.held.clear();
        self.shown = None;
    }

    fn files(&self) -> Vec<(Uuid, String)> {
        self.held
            .iter()
            .map(|(key, held)| (held.file, format!("/{ADDRESS}/{key}")))
            .collect()
    }

    fn bytes(&self) -> usize {
        self.held.values().map(|held| held.bytes.len()).sum()
    }
}

fn held() -> &'static Mutex<Holding> {
    static HELD: OnceLock<Mutex<Holding>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(Holding::new(BUDGET)))
}

fn lock() -> std::sync::MutexGuard<'static, Holding> {
    // A panic while it was held leaves nothing half done in a map.
    held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A new key for a file of this name. Each holding has its own, so that
/// an element given the address of a file read again loads it afresh.
fn new_key(name: &str) -> String {
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
    format!("{serial}/{tail}")
}

/// Keeps a file's bytes in memory and returns the address the web view
/// asks for them by, or `None` if they do not fit in the budget.
///
/// A file held already keeps the bytes and the address it has, and these
/// are dropped. `media_type` is one of the app's own
/// ([`super::kind::served_type`]).
pub fn hold(file: Uuid, name: &str, bytes: Vec<u8>, media_type: &'static str) -> Option<String> {
    lock()
        .hold(new_key(name), file, bytes, media_type, false)
        .map(|key| format!("/{ADDRESS}/{key}"))
}

/// As [`hold`], for the file the preview shows from now: never let go of
/// for the budget, however large, and the most recently shown.
pub fn hold_shown(
    file: Uuid,
    name: &str,
    bytes: Vec<u8>,
    media_type: &'static str,
) -> Option<String> {
    lock()
        .hold(new_key(name), file, bytes, media_type, true)
        .map(|key| format!("/{ADDRESS}/{key}"))
}

/// A file held is the one the preview shows now, and its address.
///
/// It is never let go of for the budget, and is the most recently shown.
/// A file not held gives `None`, and nothing changes: the file shown until
/// now stays kept.
pub fn show(file: Uuid) -> Option<String> {
    lock().show(file).map(|key| format!("/{ADDRESS}/{key}"))
}

/// Every file held, and its address.
#[must_use]
pub fn held_files() -> Vec<(Uuid, String)> {
    lock().files()
}

/// Lets go of one file: one removed from the note.
pub fn release(file: Uuid) {
    lock().release(file);
}

/// Lets go of every file held that `keep` does not keep: those the note
/// no longer has.
pub fn release_unless(keep: impl Fn(Uuid) -> bool) {
    let mut holding = lock();
    let gone: Vec<Uuid> = holding
        .held
        .values()
        .map(|held| held.file)
        .filter(|file| !keep(*file))
        .collect();
    for file in gone {
        holding.release(file);
    }
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
    lock().held.len()
}

/// How many bytes are held, the file shown's among them: for the tests.
#[must_use]
pub fn held_bytes() -> usize {
    lock().bytes()
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
        .held
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
        let receipt = Uuid::now_v7();
        let address = hold(
            receipt,
            "receipt oct.pdf",
            b"%PDF-1.7 the pages".to_vec(),
            "application/pdf",
        )
        .expect("it fits");
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

        // A file held already keeps its bytes and its address.
        let same = hold(
            receipt,
            "receipt oct.pdf",
            b"other".to_vec(),
            "application/pdf",
        );
        assert_eq!(same.as_deref(), Some(address.as_str()));
        assert_eq!(answer(&address, None).body, b"%PDF-1.7 the pages");

        // Each file has an address of its own.
        let again = hold(
            Uuid::now_v7(),
            "receipt oct.pdf",
            b"other".to_vec(),
            "application/pdf",
        )
        .expect("it fits");
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

    fn holding(budget: usize) -> (Holding, Vec<Uuid>) {
        (
            Holding::new(budget),
            (0..6).map(|_| Uuid::now_v7()).collect(),
        )
    }

    fn put(holding: &mut Holding, file: Uuid, size: usize) -> Option<String> {
        holding.hold(new_key("a.jpg"), file, vec![0; size], "image/jpeg", false)
    }

    fn put_shown(holding: &mut Holding, file: Uuid, size: usize) -> Option<String> {
        holding.hold(new_key("a.jpg"), file, vec![0; size], "image/jpeg", true)
    }

    fn holds(holding: &Holding, file: Uuid) -> bool {
        holding.key_of(file).is_some()
    }

    /// Past the budget, the files shown least recently go first; the one
    /// shown never does, and one shown again is the most recent.
    #[test]
    #[allow(clippy::many_single_char_names)]
    fn the_least_recently_shown_go_first_and_never_the_one_shown() {
        let (mut holding, files) = holding(30);
        let [a, b, c, d, ..] = files[..] else {
            unreachable!()
        };
        put_shown(&mut holding, a, 10);
        put(&mut holding, b, 10);
        put(&mut holding, c, 10);
        put(&mut holding, d, 10);
        // The one shown is not counted against the budget: b, c and d fit.
        assert!([a, b, c, d].iter().all(|file| holds(&holding, *file)));
        assert_eq!(holding.bytes(), 40);

        // b is shown, then c: a, shown before d was held, is the least
        // recent now, and goes first.
        holding.show(b);
        holding.show(c);
        let e = files[4];
        put(&mut holding, e, 10);
        assert!(!holds(&holding, a), "a was shown least recently");
        assert!([b, c, d, e].iter().all(|file| holds(&holding, *file)));

        // Then d, b and e, in that order, for a larger one; c, shown, stays
        // whatever comes.
        let f = files[5];
        put(&mut holding, f, 25);
        assert!(holds(&holding, c), "the file shown is never let go of");
        assert!(holds(&holding, f));
        assert!(![b, d, e].iter().any(|file| holds(&holding, *file)));
        assert_eq!(holding.bytes(), 35);
    }

    /// A file larger than the budget is held only while it is the one
    /// shown; read ahead, it does not fit, and nothing else is let go of
    /// for it.
    #[test]
    fn a_file_past_the_budget_is_held_only_as_the_one_shown() {
        let (mut holding, files) = holding(30);
        let [shown, ahead, other, ..] = files[..] else {
            unreachable!()
        };
        // Not held, it cannot be shown, and nothing changes.
        assert_eq!(holding.show(shown), None);
        assert_eq!(holding.shown, None);
        assert!(put_shown(&mut holding, shown, 50).is_some());
        assert!(holds(&holding, shown));
        put(&mut holding, other, 10);
        assert_eq!(put(&mut holding, ahead, 31), None);
        assert!(!holds(&holding, ahead));
        assert!(holds(&holding, shown));
        assert!(holds(&holding, other), "nothing is let go of for it");
    }

    /// The address of a file stays the same for as long as it is held,
    /// and a file let go of and held again has a new one.
    #[test]
    fn a_file_keeps_its_address_while_it_is_held() {
        let (mut holding, files) = holding(100);
        let a = files[0];
        let first = put(&mut holding, a, 10).expect("it fits");
        assert_eq!(put(&mut holding, a, 10).as_deref(), Some(first.as_str()));
        assert_eq!(holding.show(a).as_deref(), Some(first.as_str()));
        assert_eq!(holding.files(), vec![(a, format!("/{ADDRESS}/{first}"))]);
        holding.release(a);
        assert_eq!(holding.show(a), None);
        let second = put(&mut holding, a, 10).expect("it fits");
        assert_ne!(first, second);
        holding.clear();
        assert_eq!(holding.bytes(), 0);
        assert!(holding.files().is_empty());
    }

    /// The budgets the design gives (6.16).
    #[test]
    fn the_budget_is_the_design_s() {
        let expected = if cfg!(target_os = "android") { 96 } else { 256 };
        assert_eq!(BUDGET, expected * MIB);
    }
}
