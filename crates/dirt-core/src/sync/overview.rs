//! What every interface shows about sync, as one value with one set of
//! words (design section 9.4).
//!
//! The desktop app, the Android app and the CLI all show the same things:
//! when the last sync succeeded, how many notes and files wait to be sent,
//! why the last cycle failed, and each note that could not be synced and
//! each file that could not be sent, with the reason and what would
//! release it. The words are here so that the three cannot drift apart.
//!
//! [`SyncIndicator`] is the same in short: which of six states sync is
//! in, for the small status an app always shows (interface design 6.6),
//! with one to three words for it.

use dirt_proto::api::{ErrorClass, ErrorCode};

use super::engine::FileError;
use super::worker::{SyncFailure, SyncStatus};
use crate::store::{AttachedFile, FileProblem, FileStanding, NoteProblem, ProblemKind};
use crate::util::as_sentence;

/// How many characters of a note's first line name it.
const TITLE_LENGTH: usize = 60;

/// How sync stands for the signed-in account.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncOverview {
    /// A cycle is running now.
    pub syncing: bool,
    /// When a cycle last completed, as Unix milliseconds.
    pub last_synced_ms: Option<i64>,
    /// Notes waiting to be sent.
    pub unsent: usize,
    /// Files this device holds that the server has not taken.
    pub files_unsent: usize,
    /// The last cycle completed, but a file did not get through and is
    /// being tried again.
    pub files_delayed: bool,
    /// Why the last cycle failed, if it did.
    pub failure: Option<SyncFailure>,
    /// Each note that could not be synced.
    pub problems: Vec<NoteProblem>,
    /// Each file the server refused. It is on this device and nowhere
    /// else.
    pub file_problems: Vec<FileProblem>,
    /// Notes the server still holds under the key before the account's
    /// present one, as of the last sync. While there are any, a change of
    /// the account's key is not complete.
    pub old_key_notes: u64,
}

impl From<&SyncStatus> for SyncOverview {
    /// The worker's own figures, without the list of notes. For an
    /// interface that has not read the store yet, or shows one line.
    fn from(status: &SyncStatus) -> Self {
        Self {
            syncing: status.syncing,
            last_synced_ms: status.last_synced_ms,
            unsent: status.unsent,
            files_unsent: status.files_unsent,
            files_delayed: status.files_delayed,
            failure: status.failure.clone(),
            problems: Vec::new(),
            file_problems: Vec::new(),
            old_key_notes: status.old_key_notes,
        }
    }
}

impl SyncOverview {
    /// When the last sync was, in words.
    #[must_use]
    pub fn last_synced(&self, now_ms: i64) -> String {
        if self.syncing {
            return "Syncing...".to_owned();
        }
        let Some(at) = self.last_synced_ms else {
            return "Never synced".to_owned();
        };
        let minutes = now_ms.saturating_sub(at).max(0) / 60_000;
        match minutes {
            0 => "Synced just now".to_owned(),
            1..=59 => format!("Synced {minutes}m ago"),
            60..=1439 => format!("Synced {}h ago", minutes / 60),
            _ => format!("Synced {}d ago", minutes / 1440),
        }
    }

    /// How many notes could not be synced, in words that lead into the
    /// list of them, or nothing when none.
    #[must_use]
    pub fn problem_count(&self) -> Option<String> {
        match self.problems.len() {
            0 => None,
            1 => Some("1 note could not be synced:".to_owned()),
            count => Some(format!("{count} notes could not be synced:")),
        }
    }

    /// How many files could not be sent, in words that lead into the list
    /// of them, or nothing when none.
    #[must_use]
    pub fn file_problem_count(&self) -> Option<String> {
        match self.file_problems.len() {
            0 => None,
            1 => Some("1 file could not be sent:".to_owned()),
            count => Some(format!("{count} files could not be sent:")),
        }
    }

    /// What is left of a change of the account's key, in words, or nothing
    /// when no change is under way.
    #[must_use]
    pub fn key_change(&self) -> Option<String> {
        match self.old_key_notes {
            0 => None,
            1 => Some(
                "The account's key was changed. 1 note is still to be sent again under the \
                 new key; it goes as your devices sync."
                    .to_owned(),
            ),
            count => Some(format!(
                "The account's key was changed. {count} notes are still to be sent again \
                 under the new key; they go as your devices sync."
            )),
        }
    }

    /// How many notes and files wait to be sent, in words, or nothing
    /// when none do.
    #[must_use]
    pub fn waiting(&self) -> Option<String> {
        let (what, several) = notes_and_files(self.unsent, self.files_unsent)?;
        let verb = if several { "are" } else { "is" };
        Some(format!("{what} {verb} waiting to be sent"))
    }

    /// The one sentence at the head of Settings → Sync (interface design,
    /// sections 9.4 and 9.5): what holds for everything on this device,
    /// beside the notes and files listed under it.
    #[must_use]
    pub fn headline(&self) -> String {
        if self.syncing {
            return "Syncing...".to_owned();
        }
        let waiting = notes_and_files(self.unsent, self.files_unsent);
        match &self.failure {
            Some(failure) if failure.class == ErrorClass::TryAgainLater => {
                return match waiting {
                    Some((what, several)) => {
                        let verbs = if several {
                            "wait here and go"
                        } else {
                            "waits here and goes"
                        };
                        format!("Offline. {what} {verbs} when you're back.")
                    }
                    None => "Offline. Nothing is waiting.".to_owned(),
                };
            }
            Some(failure) => return failure.advice(),
            None => {}
        }
        if let Some(waiting) = self.waiting() {
            return format!("{waiting}.");
        }
        if self.problems.is_empty() && self.file_problems.is_empty() {
            "Everything on this device has reached the server.".to_owned()
        } else {
            "Everything else has reached the server.".to_owned()
        }
    }

    /// When the last sync was, as a line of its own: "Last synced 2
    /// minutes ago", or "Not synced yet".
    #[must_use]
    pub fn last_synced_in_full(&self, now_ms: i64) -> String {
        last_synced_in_full(self.last_synced_ms, now_ms)
    }

    /// The whole view as lines of text, for an interface that has only
    /// text.
    #[must_use]
    pub fn lines(&self, now_ms: i64) -> Vec<String> {
        let mut lines = vec![self.last_synced(now_ms)];
        lines.push(
            self.waiting()
                .unwrap_or_else(|| "Nothing is waiting to be sent".to_owned()),
        );
        if let Some(failure) = &self.failure {
            lines.push(format!("The last sync failed: {}", failure.advice()));
        }
        lines.extend(self.key_change());
        lines.extend(self.problem_count());
        for problem in &self.problems {
            lines.push(format!("  {} ({})", problem.title(), problem.note_id));
            lines.push(format!("    {}", as_sentence(&problem.reason)));
            lines.push(format!("    {}", problem.release()));
        }
        lines.extend(self.file_problem_count());
        for problem in &self.file_problems {
            lines.push(format!("  {} ({})", problem.title(), problem.note_id));
            lines.push(format!("    {}", as_sentence(&problem.reason)));
            lines.push(format!("    {}", problem.release()));
        }
        lines
    }
}

/// Settings → Sync on a device that is signed out.
pub const NOT_SYNCING_SIGNED_OUT: &str = "Not syncing. Notes are on this device only.";

/// Settings → Account on a device whose sync stopped until it signs in
/// again.
pub const SIGN_IN_AGAIN_TO_SYNC: &str = "This device has to sign in again before it can sync";

/// Settings → Sync while a change of the account's key is under way, over
/// the sentence [`SyncOverview::key_change`] gives.
pub const KEY_BEING_CHANGED: &str = "The key is being changed";

/// Settings → Sync on a device that has signed in and waits to be let in.
pub const NOT_SYNCING_UNTIL_LET_IN: &str =
    "Not syncing until this device is let in. Notes written here wait on it.";

/// How long after a cycle completes the status shows that it has just
/// synced, in milliseconds. An app that shows [`SyncIndicator::JustSynced`]
/// asks again when this has passed.
pub const JUST_SYNCED_MS: i64 = 1_400;

/// What the list says of a note this device has not yet sent to the
/// server, beside the note's row (interface design, section 6.6).
pub const NOT_ON_SERVER_YET: &str = "Not on the server yet";

/// Which state sync is in, for the status an app always shows.
///
/// The status is an icon, and for three of the states a few words beside
/// it (interface design 6.6). An app chooses the icon by the state and
/// takes the words from [`Self::short`] and [`Self::tooltip`].
/// [`crate::session::Session::sync_indicator`] gives it for a session.
/// Where two states hold at once, the first of these wins: signed out,
/// syncing, just synced, offline, a problem, synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncIndicator {
    /// Nothing is wrong and nothing is happening.
    Synced {
        /// When a cycle last completed, as Unix milliseconds. Nothing if
        /// none ever has.
        last_synced_ms: Option<i64>,
    },
    /// A cycle is running now.
    Syncing,
    /// A cycle completed within the last [`JUST_SYNCED_MS`].
    JustSynced,
    /// The last cycle could not reach the server, or was told to come
    /// back later. Sync tries again by itself.
    Offline {
        /// How many notes wait to be sent. A note with a new file is one
        /// thing that waits, not two, so files are counted only when no
        /// note waits and files do.
        waiting: usize,
    },
    /// Something will not sync until the person does something about it:
    /// notes that could not be synced, files the server refused, or a
    /// cycle that failed for a reason that waiting does not cure (this
    /// device has to sign in again, or the app is too old). Settings →
    /// Sync says which, in the overview's sentences.
    Problem {
        /// Notes that could not be synced.
        notes: usize,
        /// Files that could not be sent.
        files: usize,
    },
    /// This device is not syncing: nobody is signed in, or the device has
    /// not been let into the account yet. Its notes are on it alone.
    SignedOut,
}

impl SyncIndicator {
    /// The state of a device that is signed in and let in, from the whole
    /// overview.
    #[must_use]
    pub fn of(overview: &SyncOverview, now_ms: i64) -> Self {
        Self::from_figures(&Figures {
            syncing: overview.syncing,
            last_synced_ms: overview.last_synced_ms,
            unsent: overview.unsent,
            files_unsent: overview.files_unsent,
            files_delayed: overview.files_delayed,
            failure: overview.failure.as_ref().map(|failure| failure.class),
            problems: overview.problems.len(),
            file_problems: overview.file_problems.len(),
            now_ms,
        })
    }

    /// The same from the worker's own figures, which an app has at every
    /// change without reading the store.
    #[must_use]
    pub fn from_status(status: &SyncStatus, now_ms: i64) -> Self {
        Self::from_figures(&Figures {
            syncing: status.syncing,
            last_synced_ms: status.last_synced_ms,
            unsent: status.unsent,
            files_unsent: status.files_unsent,
            files_delayed: status.files_delayed,
            failure: status.failure.as_ref().map(|failure| failure.class),
            problems: status.problems,
            file_problems: status.file_problems,
            now_ms,
        })
    }

    fn from_figures(figures: &Figures) -> Self {
        if figures.syncing {
            return Self::Syncing;
        }
        let problem = Self::Problem {
            notes: figures.problems,
            files: figures.file_problems,
        };
        let waiting = Self::Offline {
            waiting: if figures.unsent > 0 {
                figures.unsent
            } else {
                figures.files_unsent
            },
        };
        match figures.failure {
            Some(ErrorClass::TryAgainLater) => waiting,
            // Waiting will not cure it, whether or not a note is named.
            Some(_) => problem,
            // The notes went and a file did not. The cycle completed, so
            // there is no failure to read, but the file is on this device
            // alone until the next try gets it through.
            None if figures.files_delayed && figures.files_unsent > 0 => waiting,
            None => {
                let since = figures
                    .last_synced_ms
                    .map(|at| figures.now_ms.saturating_sub(at));
                if since.is_some_and(|since| (0..JUST_SYNCED_MS).contains(&since)) {
                    Self::JustSynced
                } else if figures.problems + figures.file_problems > 0 {
                    problem
                } else {
                    Self::Synced {
                        last_synced_ms: figures.last_synced_ms,
                    }
                }
            }
        }
    }

    /// The state in one to three words: "Synced", "Syncing", "Offline · 2
    /// waiting", "1 note can't sync", "This device only".
    ///
    /// Every state has them, for a screen reader and wherever the icon
    /// needs a name. [`Self::is_quiet`] says whether they are shown.
    #[must_use]
    pub fn short(&self) -> String {
        let cannot = |count: usize, one: &str| match count {
            1 => format!("1 {one} can't sync"),
            count => format!("{count} {one}s can't sync"),
        };
        match *self {
            Self::Synced { .. } | Self::JustSynced => "Synced".to_owned(),
            Self::Syncing => "Syncing".to_owned(),
            Self::Offline { waiting: 0 } => "Offline".to_owned(),
            Self::Offline { waiting } => format!("Offline \u{b7} {waiting} waiting"),
            Self::Problem { notes: 0, files: 0 } => "Can't sync".to_owned(),
            Self::Problem { notes: 0, files } => cannot(files, "file"),
            Self::Problem { notes, .. } => cannot(notes, "note"),
            Self::SignedOut => "This device only".to_owned(),
        }
    }

    /// Whether the state is shown as its icon alone. Synced, syncing and
    /// just synced need no words; the other three are said.
    #[must_use]
    pub const fn is_quiet(&self) -> bool {
        matches!(self, Self::Synced { .. } | Self::Syncing | Self::JustSynced)
    }

    /// The longer line shown when the status is pointed at, or nothing
    /// for the two states that pass in a moment.
    #[must_use]
    pub fn tooltip(&self, now_ms: i64) -> Option<String> {
        match *self {
            Self::Synced { last_synced_ms } => Some(last_synced_in_full(last_synced_ms, now_ms)),
            Self::Syncing | Self::JustSynced => None,
            Self::Offline { .. } => Some("Notes wait here and go when you're back".to_owned()),
            Self::Problem { .. } => Some("See why".to_owned()),
            Self::SignedOut => Some("Not syncing. Sign in to sync".to_owned()),
        }
    }

    /// Everything the status says, in one sentence: for a status drawn as
    /// an icon alone, as on Android's top bar, where a long press is the
    /// only way to read it. The words come first where they say more than
    /// "Synced", then the tooltip: "Offline · 2 waiting. Notes wait
    /// here and go when you're back". A quiet state gives its tooltip, or
    /// its words when it has none.
    #[must_use]
    pub fn in_full(&self, now_ms: i64) -> String {
        match self.tooltip(now_ms) {
            Some(tooltip) if !self.is_quiet() => format!("{}. {tooltip}", self.short()),
            Some(tooltip) => tooltip,
            None => self.short(),
        }
    }
}

/// What [`SyncIndicator`] is worked out from.
struct Figures {
    syncing: bool,
    last_synced_ms: Option<i64>,
    unsent: usize,
    files_unsent: usize,
    files_delayed: bool,
    failure: Option<ErrorClass>,
    problems: usize,
    file_problems: usize,
    now_ms: i64,
}

/// When the last sync was, as a line of its own: "Last synced 2 minutes
/// ago". The units change where [`SyncOverview::last_synced`] changes
/// its, so the two never say different things.
fn last_synced_in_full(last_synced_ms: Option<i64>, now_ms: i64) -> String {
    let Some(at) = last_synced_ms else {
        return "Not synced yet".to_owned();
    };
    let ago = |count: i64, unit: &str| {
        let s = if count == 1 { "" } else { "s" };
        format!("Last synced {count} {unit}{s} ago")
    };
    let minutes = now_ms.saturating_sub(at).max(0) / 60_000;
    match minutes {
        0 => "Last synced just now".to_owned(),
        1..=59 => ago(minutes, "minute"),
        60..=1439 => ago(minutes / 60, "hour"),
        _ => ago(minutes / 1440, "day"),
    }
}

/// A size in bytes, in words: "812 B", "150 KiB", "2.3 MiB".
#[must_use]
pub fn size_words(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    // Tenths, without going through a float.
    let tenths = |unit: u64| (bytes * 10 + unit / 2) / unit;
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < MIB {
        format!("{} KiB", (bytes + KIB / 2) / KIB)
    } else {
        let tenths = tenths(MIB);
        format!("{}.{} MiB", tenths / 10, tenths % 10)
    }
}

impl AttachedFile {
    /// What there is to say about where the file is, or nothing for a
    /// file that is on this device and on the server.
    #[must_use]
    pub fn standing_words(&self) -> Option<String> {
        match (&self.standing, self.on_device) {
            (FileStanding::Refused(reason), _) => Some(format!("could not be sent: {reason}")),
            (FileStanding::NotTaken, true) => Some("waiting to be sent".to_owned()),
            (_, false) => Some("not on this device; fetched when it is wanted".to_owned()),
            (FileStanding::Taken, true) => None,
        }
    }
}

impl FileError {
    /// Why a file could not be had, as sentences for the person, with
    /// what can be done. `can_fetch` says whether this device is signed in
    /// and so could have fetched the file.
    #[must_use]
    pub fn advice(&self, can_fetch: bool) -> String {
        match self {
            Self::NotAttached => "The note has no such file.".to_owned(),
            Self::Missing if can_fetch => {
                "The file is not on this device and the server does not have it. A device \
                 that holds it sends it when it next syncs."
                    .to_owned()
            }
            Self::Missing => {
                "The file is not on this device, and this device is not signed in to fetch it."
                    .to_owned()
            }
            Self::Damaged => {
                "The server's copy of the file could not be decrypted, and nothing was kept. \
                 A device that holds the file still has it."
                    .to_owned()
            }
            Self::Api(error) if error.code() == Some(ErrorCode::FilesUnavailable) => {
                "The server has no storage for files just now.".to_owned()
            }
            Self::Api(error) => match error.class() {
                ErrorClass::TryAgainLater => {
                    "The file could not be fetched: the server or the file storage could not \
                     be reached."
                        .to_owned()
                }
                ErrorClass::StopUntilSignedIn => "This device has to sign in again.".to_owned(),
                ErrorClass::UpdateTheApp => {
                    "This version of Dirt is too old to sync. Update it.".to_owned()
                }
                ErrorClass::ProblemWithOneNote => as_sentence(error),
            },
            Self::Store(error) => as_sentence(error),
        }
    }
}

/// So many notes and files, in words: "1 note", "2 notes and 1 file".
/// With it, whether that is more than one thing. Nothing for none.
fn notes_and_files(notes: usize, files: usize) -> Option<(String, bool)> {
    let counted = |count: usize, one: &str| match count {
        0 => None,
        1 => Some(format!("1 {one}")),
        count => Some(format!("{count} {one}s")),
    };
    let what = match (counted(notes, "note"), counted(files, "file")) {
        (None, None) => return None,
        (Some(notes), Some(files)) => format!("{notes} and {files}"),
        (Some(one), None) | (None, Some(one)) => one,
    };
    Some((what, notes + files > 1))
}

/// What signing out now would delete, for an interface to show before it
/// asks whether to sign out all the same. `cause` is why the last sync
/// did not send them, if it failed.
#[must_use]
pub fn sign_out_would_lose(notes: usize, files: usize, cause: Option<&str>) -> String {
    let Some((what, several)) = notes_and_files(notes, files) else {
        return "Everything on this device has reached the server.".to_owned();
    };
    let (verb, them) = if several {
        ("have", "them")
    } else {
        ("has", "it")
    };
    let why = cause.map_or_else(String::new, |cause| format!(" ({cause})"));
    format!("{what} {verb} not reached the server{why}. Signing out would delete {them}.")
}

impl SyncFailure {
    /// The failure in words that say what the person can do.
    #[must_use]
    pub fn advice(&self) -> String {
        match self.class {
            ErrorClass::TryAgainLater => {
                "The server could not be reached. Your notes are saved and will be sent later."
                    .to_owned()
            }
            ErrorClass::StopUntilSignedIn => "This device has to sign in again.".to_owned(),
            ErrorClass::UpdateTheApp => {
                "This version of Dirt is too old to sync. Update it.".to_owned()
            }
            ErrorClass::ProblemWithOneNote => as_sentence(&self.message),
        }
    }
}

/// What to call a note with this text: the start of its first line, or
/// nothing for a note with no text.
pub(crate) fn first_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut title: String = line.chars().take(TITLE_LENGTH).collect();
    if line.chars().count() > TITLE_LENGTH {
        title.push('…');
    }
    Some(title)
}

impl NoteProblem {
    /// What to call the note: the start of its first line, or words for a
    /// note with no text or one this device does not hold.
    #[must_use]
    pub fn title(&self) -> String {
        let Some(text) = &self.text else {
            return "A note this device has never been able to read".to_owned();
        };
        first_line(text).unwrap_or_else(|| "A note with no text".to_owned())
    }
}

impl FileProblem {
    /// What to call the file: its name, and the note it is attached to.
    #[must_use]
    pub fn title(&self) -> String {
        let file = self.name.as_deref().unwrap_or("A file");
        let note = first_line(&self.note_text).unwrap_or_else(|| "a note with no text".to_owned());
        format!("{file}, attached to: {note}")
    }

    /// What becomes of the file meanwhile, as a sentence for the person.
    #[must_use]
    pub const fn release(&self) -> &'static str {
        "It is kept on this device, which is the only place it is, and is tried again at \
         each sync."
    }
}

impl NoteProblem {
    /// What ends the problem, as a sentence for the person. A held note
    /// this device has a copy of and one it never had are different things
    /// to wait for.
    #[must_use]
    pub const fn release(&self) -> &'static str {
        match (self.kind, self.text.is_some()) {
            (ProblemKind::Held, true) => {
                "It will sync again when a device that can still read it edits it. \
                 Until then this device keeps its own copy and does not send it."
            }
            (ProblemKind::Held, false) => "It will arrive when a device that can read it edits it.",
            (ProblemKind::RetryAfterEdit, _) => "Edit it on this device to try again.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoteId;

    fn problem(text: Option<&str>, kind: ProblemKind) -> NoteProblem {
        NoteProblem {
            note_id: NoteId::new(),
            reason: "the server's copy could not be decrypted".to_owned(),
            kind,
            text: text.map(str::to_owned),
        }
    }

    #[test]
    fn the_head_of_settings_says_what_holds_for_everything() {
        let base = SyncOverview::default();
        assert_eq!(
            base.headline(),
            "Everything on this device has reached the server."
        );
        let waiting = SyncOverview {
            unsent: 2,
            ..SyncOverview::default()
        };
        assert_eq!(waiting.headline(), "2 notes are waiting to be sent.");
        let offline = SyncOverview {
            failure: Some(failed(ErrorClass::TryAgainLater)),
            ..waiting
        };
        assert_eq!(
            offline.headline(),
            "Offline. 2 notes wait here and go when you're back."
        );
        let one = SyncOverview {
            unsent: 1,
            ..offline.clone()
        };
        assert_eq!(
            one.headline(),
            "Offline. 1 note waits here and goes when you're back."
        );
        let nothing = SyncOverview {
            unsent: 0,
            ..offline
        };
        assert_eq!(nothing.headline(), "Offline. Nothing is waiting.");
        let again = SyncOverview {
            failure: Some(failed(ErrorClass::StopUntilSignedIn)),
            ..SyncOverview::default()
        };
        assert_eq!(again.headline(), "This device has to sign in again.");
        let problem = SyncOverview {
            problems: vec![problem(Some("Too long"), ProblemKind::RetryAfterEdit)],
            ..SyncOverview::default()
        };
        assert_eq!(
            problem.headline(),
            "Everything else has reached the server."
        );
        let syncing = SyncOverview {
            syncing: true,
            ..problem
        };
        assert_eq!(syncing.headline(), "Syncing...");
    }

    #[test]
    fn the_time_of_the_last_sync_is_said_in_the_largest_unit_that_fits() {
        let minute = 60_000;
        let at = |ago: i64| SyncOverview {
            last_synced_ms: Some(1_000_000_000 - ago),
            ..SyncOverview::default()
        };
        let said = |ago: i64| at(ago).last_synced(1_000_000_000);
        assert_eq!(said(0), "Synced just now");
        assert_eq!(said(minute - 1), "Synced just now");
        assert_eq!(said(minute), "Synced 1m ago");
        assert_eq!(said(59 * minute), "Synced 59m ago");
        assert_eq!(said(60 * minute), "Synced 1h ago");
        assert_eq!(said(24 * 60 * minute), "Synced 1d ago");
        // A clock that went backwards does not give a negative time.
        assert_eq!(said(-5 * minute), "Synced just now");
        assert_eq!(
            SyncOverview::default().last_synced(1_000_000_000),
            "Never synced"
        );
        let running = SyncOverview {
            syncing: true,
            ..at(minute)
        };
        assert_eq!(running.last_synced(1_000_000_000), "Syncing...");
    }

    fn failed(class: ErrorClass) -> SyncFailure {
        SyncFailure {
            class,
            message: "the cause, in an error's words".to_owned(),
        }
    }

    /// The six states of the status, with the words the design gives each
    /// (interface design 6.6).
    #[test]
    fn the_status_is_one_of_six_states_with_a_few_words_each() {
        const NOW: i64 = 1_800_000_000_000;
        let minute = 60_000;
        let synced = SyncOverview {
            last_synced_ms: Some(NOW - 2 * minute),
            ..SyncOverview::default()
        };

        let state = SyncIndicator::of(&synced, NOW);
        assert_eq!(
            state,
            SyncIndicator::Synced {
                last_synced_ms: Some(NOW - 2 * minute)
            }
        );
        assert_eq!(state.short(), "Synced");
        assert!(state.is_quiet());
        assert_eq!(
            state.tooltip(NOW).as_deref(),
            Some("Last synced 2 minutes ago")
        );

        let state = SyncIndicator::of(
            &SyncOverview {
                syncing: true,
                ..synced.clone()
            },
            NOW,
        );
        assert_eq!(state, SyncIndicator::Syncing);
        assert_eq!(state.short(), "Syncing");
        assert!(state.is_quiet());
        assert_eq!(state.tooltip(NOW), None);

        let state = SyncIndicator::of(
            &SyncOverview {
                last_synced_ms: Some(NOW - 900),
                ..SyncOverview::default()
            },
            NOW,
        );
        assert_eq!(state, SyncIndicator::JustSynced);
        assert_eq!(state.short(), "Synced");
        assert!(state.is_quiet());
        assert_eq!(state.tooltip(NOW), None);

        let offline = SyncOverview {
            unsent: 2,
            failure: Some(failed(ErrorClass::TryAgainLater)),
            ..synced.clone()
        };
        let state = SyncIndicator::of(&offline, NOW);
        assert_eq!(state, SyncIndicator::Offline { waiting: 2 });
        assert_eq!(state.short(), "Offline · 2 waiting");
        assert!(!state.is_quiet());
        assert_eq!(
            state.tooltip(NOW).as_deref(),
            Some("Notes wait here and go when you're back")
        );

        let state = SyncIndicator::of(
            &SyncOverview {
                problems: vec![problem(Some("too long"), ProblemKind::RetryAfterEdit)],
                ..synced
            },
            NOW,
        );
        assert_eq!(state, SyncIndicator::Problem { notes: 1, files: 0 });
        assert_eq!(state.short(), "1 note can't sync");
        assert!(!state.is_quiet());
        assert_eq!(state.tooltip(NOW).as_deref(), Some("See why"));

        let state = SyncIndicator::SignedOut;
        assert_eq!(state.short(), "This device only");
        assert!(!state.is_quiet());
        assert_eq!(
            state.tooltip(NOW).as_deref(),
            Some("Not syncing. Sign in to sync")
        );
    }

    #[test]
    fn the_status_counts_what_waits_and_what_cannot_sync() {
        const NOW: i64 = 1_800_000_000_000;
        let offline = |unsent, files_unsent| {
            SyncIndicator::of(
                &SyncOverview {
                    unsent,
                    files_unsent,
                    failure: Some(failed(ErrorClass::TryAgainLater)),
                    ..SyncOverview::default()
                },
                NOW,
            )
        };
        assert_eq!(offline(0, 0).short(), "Offline");
        assert_eq!(offline(1, 0).short(), "Offline · 1 waiting");
        // A note and the picture attached to it, with its thumbnail, are
        // one thing that waits.
        assert_eq!(offline(1, 2).short(), "Offline · 1 waiting");
        // Only files are left to go.
        assert_eq!(offline(0, 2).short(), "Offline · 2 waiting");

        let note = || problem(Some("held"), ProblemKind::Held);
        let file = || FileProblem {
            file_id: uuid::Uuid::from_u128(1),
            note_id: NoteId::new(),
            reason: "the account's storage for files is full".to_owned(),
            name: None,
            note_text: String::new(),
        };
        let trouble = |notes: usize, files: usize, failure| {
            SyncIndicator::of(
                &SyncOverview {
                    last_synced_ms: Some(NOW - 600_000),
                    problems: (0..notes).map(|_| note()).collect(),
                    file_problems: (0..files).map(|_| file()).collect(),
                    failure,
                    ..SyncOverview::default()
                },
                NOW,
            )
        };
        assert_eq!(trouble(3, 0, None).short(), "3 notes can't sync");
        assert_eq!(trouble(2, 5, None).short(), "2 notes can't sync");
        assert_eq!(trouble(0, 1, None).short(), "1 file can't sync");
        assert_eq!(trouble(0, 2, None).short(), "2 files can't sync");
        // A cycle that failed for a reason waiting does not cure is a
        // problem, with or without a note to name.
        for class in [
            ErrorClass::StopUntilSignedIn,
            ErrorClass::UpdateTheApp,
            ErrorClass::ProblemWithOneNote,
        ] {
            let state = trouble(0, 0, Some(failed(class)));
            assert_eq!(state, SyncIndicator::Problem { notes: 0, files: 0 });
            assert_eq!(state.short(), "Can't sync");
            assert_eq!(state.tooltip(NOW).as_deref(), Some("See why"));
            assert_eq!(
                trouble(1, 0, Some(failed(class))).short(),
                "1 note can't sync"
            );
        }
    }

    /// Where two states hold at once: syncing, then just synced, then
    /// offline, then a problem.
    #[test]
    fn the_status_shows_the_state_that_matters_most() {
        const NOW: i64 = 1_800_000_000_000;
        let everything = SyncOverview {
            syncing: true,
            last_synced_ms: Some(NOW - 100),
            unsent: 4,
            files_unsent: 1,
            files_delayed: false,
            failure: Some(failed(ErrorClass::TryAgainLater)),
            problems: vec![problem(None, ProblemKind::Held)],
            file_problems: Vec::new(),
            old_key_notes: 0,
        };
        assert_eq!(SyncIndicator::of(&everything, NOW), SyncIndicator::Syncing);
        let stopped = SyncOverview {
            syncing: false,
            ..everything
        };
        // A cycle that failed has not just synced, whenever the last one
        // that completed was.
        assert_eq!(
            SyncIndicator::of(&stopped, NOW),
            SyncIndicator::Offline { waiting: 4 }
        );
        let completed = SyncOverview {
            failure: None,
            ..stopped
        };
        assert_eq!(
            SyncIndicator::of(&completed, NOW),
            SyncIndicator::JustSynced
        );
        // The tick passes, and the note that cannot sync is still there.
        for (later, state) in [
            (JUST_SYNCED_MS - 101, SyncIndicator::JustSynced),
            (
                JUST_SYNCED_MS - 100,
                SyncIndicator::Problem { notes: 1, files: 0 },
            ),
        ] {
            assert_eq!(SyncIndicator::of(&completed, NOW + later), state);
        }
        // A clock that went backwards has not just synced.
        assert_eq!(
            SyncIndicator::of(&completed, NOW - 5_000),
            SyncIndicator::Problem { notes: 1, files: 0 }
        );
    }

    /// A cycle that sent the notes and not a file has completed, and has
    /// not synced: the file waits for the next try.
    #[test]
    fn a_file_that_did_not_get_through_is_waiting_not_synced() {
        const NOW: i64 = 1_800_000_000_000;
        let delayed = SyncStatus {
            last_synced_ms: Some(NOW - 200),
            files_unsent: 1,
            files_delayed: true,
            ..SyncStatus::default()
        };
        assert_eq!(
            SyncIndicator::from_status(&delayed, NOW),
            SyncIndicator::Offline { waiting: 1 }
        );
        assert_eq!(
            SyncIndicator::of(&SyncOverview::from(&delayed), NOW + 60_000),
            SyncIndicator::Offline { waiting: 1 }
        );
        // Once it has gone there is nothing delayed, whatever the mark says.
        let gone = SyncStatus {
            files_unsent: 0,
            ..delayed
        };
        assert_eq!(
            SyncIndicator::from_status(&gone, NOW),
            SyncIndicator::JustSynced
        );
    }

    /// The worker's figures give the state the whole overview gives.
    #[test]
    fn the_status_is_the_same_from_the_workers_figures() {
        const NOW: i64 = 1_800_000_000_000;
        let status = SyncStatus {
            last_synced_ms: Some(NOW - 60_000),
            unsent: 1,
            problems: 2,
            file_problems: 1,
            ..SyncStatus::default()
        };
        assert_eq!(
            SyncIndicator::from_status(&status, NOW),
            SyncIndicator::Problem { notes: 2, files: 1 }
        );
        let offline = SyncStatus {
            failure: Some(failed(ErrorClass::TryAgainLater)),
            ..status
        };
        assert_eq!(
            SyncIndicator::from_status(&offline, NOW),
            SyncIndicator::Offline { waiting: 1 }
        );
        let running = SyncStatus {
            syncing: true,
            ..offline
        };
        assert_eq!(
            SyncIndicator::from_status(&running, NOW),
            SyncIndicator::Syncing
        );
        assert_eq!(
            SyncIndicator::from_status(&SyncStatus::default(), NOW),
            SyncIndicator::Synced {
                last_synced_ms: None
            }
        );
        // Whatever the figures, the overview made from them agrees, as
        // far as it knows: it has the counts of what waits, and no list
        // of what cannot sync.
        let plain = SyncStatus {
            problems: 0,
            file_problems: 0,
            ..status
        };
        assert_eq!(
            SyncIndicator::from_status(&plain, NOW),
            SyncIndicator::of(&SyncOverview::from(&plain), NOW)
        );
    }

    #[test]
    fn in_full_the_words_come_first_where_they_say_more_than_synced() {
        let now = 1_800_000_000_000;
        assert_eq!(
            SyncIndicator::Offline { waiting: 2 }.in_full(now),
            "Offline \u{b7} 2 waiting. Notes wait here and go when you're back"
        );
        assert_eq!(
            SyncIndicator::Problem { notes: 1, files: 0 }.in_full(now),
            "1 note can't sync. See why"
        );
        assert_eq!(
            SyncIndicator::SignedOut.in_full(now),
            "This device only. Not syncing. Sign in to sync"
        );
        assert_eq!(
            SyncIndicator::Synced {
                last_synced_ms: Some(now)
            }
            .in_full(now),
            "Last synced just now"
        );
        assert_eq!(SyncIndicator::Syncing.in_full(now), "Syncing");
    }

    #[test]
    fn the_tooltip_says_when_the_last_sync_was_in_whole_words() {
        const NOW: i64 = 1_800_000_000_000;
        let minute = 60_000;
        let said = |ago: Option<i64>| {
            SyncIndicator::Synced {
                last_synced_ms: ago.map(|ago| NOW - ago),
            }
            .tooltip(NOW)
            .unwrap()
        };
        assert_eq!(said(None), "Not synced yet");
        assert_eq!(said(Some(1_500)), "Last synced just now");
        assert_eq!(said(Some(minute)), "Last synced 1 minute ago");
        assert_eq!(said(Some(59 * minute)), "Last synced 59 minutes ago");
        assert_eq!(said(Some(60 * minute)), "Last synced 1 hour ago");
        assert_eq!(said(Some(23 * 60 * minute)), "Last synced 23 hours ago");
        assert_eq!(said(Some(24 * 60 * minute)), "Last synced 1 day ago");
        assert_eq!(said(Some(72 * 60 * minute)), "Last synced 3 days ago");
        assert_eq!(said(Some(-5 * minute)), "Last synced just now");
        // It changes its unit where the sentence in Settings does.
        for ago in [0, minute - 1, minute, 60 * minute - 1, 60 * minute] {
            let sentence = SyncOverview {
                last_synced_ms: Some(NOW - ago),
                ..SyncOverview::default()
            }
            .last_synced(NOW);
            let number = |words: &str| {
                words
                    .chars()
                    .filter(char::is_ascii_digit)
                    .collect::<String>()
            };
            assert_eq!(number(&sentence), number(&said(Some(ago))), "{ago}");
        }
    }

    #[test]
    fn what_waits_is_counted_in_notes_and_files() {
        let waiting = |unsent, files_unsent| {
            SyncOverview {
                unsent,
                files_unsent,
                ..SyncOverview::default()
            }
            .waiting()
        };
        assert_eq!(waiting(0, 0), None);
        assert_eq!(waiting(1, 0).unwrap(), "1 note is waiting to be sent");
        assert_eq!(waiting(0, 1).unwrap(), "1 file is waiting to be sent");
        assert_eq!(waiting(0, 3).unwrap(), "3 files are waiting to be sent");
        assert_eq!(
            waiting(1, 1).unwrap(),
            "1 note and 1 file are waiting to be sent"
        );
        assert_eq!(
            waiting(2, 5).unwrap(),
            "2 notes and 5 files are waiting to be sent"
        );
    }

    #[test]
    fn the_text_view_names_each_file_that_could_not_be_sent() {
        let file = |name: Option<&str>, note_text: &str| FileProblem {
            file_id: uuid::Uuid::from_u128(1),
            note_id: NoteId::new(),
            reason: "the account's storage for files is full".to_owned(),
            name: name.map(str::to_owned),
            note_text: note_text.to_owned(),
        };
        let photo = file(Some("whiteboard.jpg"), "\nthe meeting\nwho came");
        let overview = SyncOverview {
            last_synced_ms: Some(0),
            file_problems: vec![photo.clone()],
            ..SyncOverview::default()
        };
        assert_eq!(
            overview.lines(0)[1..],
            [
                "Nothing is waiting to be sent".to_owned(),
                "1 file could not be sent:".to_owned(),
                format!(
                    "  whiteboard.jpg, attached to: the meeting ({})",
                    photo.note_id
                ),
                "    The account's storage for files is full.".to_owned(),
                "    It is kept on this device, which is the only place it is, and is tried again \
                 at each sync."
                    .to_owned(),
            ]
        );
        assert_eq!(
            file(None, "  ").title(),
            "A file, attached to: a note with no text"
        );
        let two = SyncOverview {
            file_problems: vec![photo.clone(), photo],
            ..SyncOverview::default()
        };
        assert_eq!(
            two.file_problem_count().unwrap(),
            "2 files could not be sent:"
        );
        assert_eq!(SyncOverview::default().file_problem_count(), None);
    }

    #[test]
    fn a_size_is_said_in_the_unit_that_fits() {
        assert_eq!(size_words(0), "0 B");
        assert_eq!(size_words(1_023), "1023 B");
        assert_eq!(size_words(1_024), "1 KiB");
        assert_eq!(size_words(150_000), "146 KiB");
        assert_eq!(size_words(1_048_576), "1.0 MiB");
        assert_eq!(size_words(2_400_000), "2.3 MiB");
        assert_eq!(size_words(10 * 1_048_576), "10.0 MiB");
    }

    #[test]
    fn where_a_file_is_is_said_only_when_there_is_something_to_say() {
        let file = |standing, on_device| AttachedFile {
            attachment: crate::files::Attachment {
                file_id: uuid::Uuid::from_u128(1),
                name: "photo.jpg".to_owned(),
                media_type: "image/jpeg".to_owned(),
                size: 10,
                key: crate::files::FileKey::generate(),
                added_at_ms: 0,
                thumbnail: None,
            },
            on_device,
            thumbnail_on_device: false,
            standing,
        };
        assert_eq!(file(FileStanding::Taken, true).standing_words(), None);
        assert_eq!(
            file(FileStanding::NotTaken, true).standing_words().unwrap(),
            "waiting to be sent"
        );
        assert_eq!(
            file(FileStanding::Taken, false).standing_words().unwrap(),
            "not on this device; fetched when it is wanted"
        );
        // A device that neither holds the file nor has been told it is
        // there has nothing of its own to send.
        assert_eq!(
            file(FileStanding::NotTaken, false)
                .standing_words()
                .unwrap(),
            "not on this device; fetched when it is wanted"
        );
        assert_eq!(
            file(
                FileStanding::Refused("the account's storage for files is full".to_owned()),
                true
            )
            .standing_words()
            .unwrap(),
            "could not be sent: the account's storage for files is full"
        );
    }

    #[test]
    fn a_file_that_cannot_be_had_is_explained() {
        use crate::api::ApiError;

        assert!(FileError::Missing
            .advice(true)
            .contains("A device that holds it sends it"));
        assert!(FileError::Missing
            .advice(false)
            .contains("not signed in to fetch it"));
        assert!(FileError::Damaged.advice(true).contains("nothing was kept"));
        let no_storage = FileError::Api(ApiError::Server {
            code: ErrorCode::FilesUnavailable,
            message: "the file storage could not be reached".to_owned(),
            retry_after_secs: None,
        });
        assert_eq!(
            no_storage.advice(true),
            "The server has no storage for files just now."
        );
        let offline = FileError::Api(ApiError::Network("no route".to_owned()));
        assert!(offline.advice(true).contains("could not be reached"));
        // A link to the storage is never part of what is said.
        assert!(!offline.advice(true).contains("no route"));
    }

    #[test]
    fn signing_out_says_what_it_would_delete() {
        assert_eq!(
            sign_out_would_lose(1, 0, None),
            "1 note has not reached the server. Signing out would delete it."
        );
        assert_eq!(
            sign_out_would_lose(0, 1, Some("there is no network")),
            "1 file has not reached the server (there is no network). \
             Signing out would delete it."
        );
        assert_eq!(
            sign_out_would_lose(2, 1, None),
            "2 notes and 1 file have not reached the server. Signing out would delete them."
        );
        assert_eq!(
            sign_out_would_lose(0, 0, None),
            "Everything on this device has reached the server."
        );
    }

    #[test]
    fn a_note_is_named_by_its_first_line_with_text() {
        assert_eq!(
            problem(Some("\n  \nshopping list\nmilk"), ProblemKind::Held).title(),
            "shopping list"
        );
        let long = "x".repeat(TITLE_LENGTH + 1);
        let title = problem(Some(&long), ProblemKind::Held).title();
        assert_eq!(title.chars().count(), TITLE_LENGTH + 1);
        assert!(title.ends_with('…'));
        assert_eq!(
            problem(Some("x".repeat(TITLE_LENGTH).as_str()), ProblemKind::Held).title(),
            "x".repeat(TITLE_LENGTH)
        );
        assert_eq!(
            problem(Some("  \n"), ProblemKind::Held).title(),
            "A note with no text"
        );
        assert_eq!(
            problem(None, ProblemKind::Held).title(),
            "A note this device has never been able to read"
        );
    }

    #[test]
    fn the_text_view_names_each_note_with_its_reason_and_what_releases_it() {
        let held = problem(Some("held note"), ProblemKind::Held);
        let refused = problem(Some("refused note"), ProblemKind::RetryAfterEdit);
        let overview = SyncOverview {
            syncing: false,
            last_synced_ms: Some(0),
            unsent: 2,
            files_unsent: 0,
            files_delayed: false,
            failure: Some(SyncFailure {
                class: ErrorClass::TryAgainLater,
                message: "connection refused".to_owned(),
            }),
            problems: vec![held.clone(), refused.clone()],
            file_problems: Vec::new(),
            old_key_notes: 0,
        };
        let lines = overview.lines(120_000);
        assert_eq!(lines[0], "Synced 2m ago");
        assert_eq!(lines[1], "2 notes are waiting to be sent");
        assert!(lines[2].starts_with("The last sync failed: The server could not be reached."));
        assert_eq!(lines[3], "2 notes could not be synced:");
        assert_eq!(lines[4], format!("  held note ({})", held.note_id));
        assert_eq!(lines[5], "    The server's copy could not be decrypted.");
        assert!(lines[6].contains("a device that can still read it edits it"));
        assert_eq!(lines[7], format!("  refused note ({})", refused.note_id));
        assert_eq!(lines[9], "    Edit it on this device to try again.");
        assert_eq!(lines.len(), 10);

        // A note this device never had has no copy here to speak of.
        let never_held = problem(None, ProblemKind::Held);
        assert_eq!(
            never_held.release(),
            "It will arrive when a device that can read it edits it."
        );
        assert!(held.release().contains("keeps its own copy"));

        let quiet = SyncOverview::default().lines(0);
        assert_eq!(quiet, ["Never synced", "Nothing is waiting to be sent"]);

        // A change of the account's key that is not finished is said.
        let changing = SyncOverview {
            old_key_notes: 3,
            ..SyncOverview::default()
        };
        assert_eq!(
            changing.lines(0)[2],
            "The account's key was changed. 3 notes are still to be sent again under the new \
             key; they go as your devices sync."
        );
        let nearly = SyncOverview {
            old_key_notes: 1,
            ..SyncOverview::default()
        };
        assert!(nearly.key_change().unwrap().contains("1 note is still"));
        assert_eq!(SyncOverview::default().key_change(), None);
    }
}
