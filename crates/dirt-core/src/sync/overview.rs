//! What every interface shows about sync, as one value with one set of
//! words (design section 9.4).
//!
//! The desktop app, the Android app and the CLI all show the same things:
//! when the last sync succeeded, how many notes wait to be sent, why the
//! last cycle failed, and each note that could not be synced, with the
//! reason and what would release it. The words are here so that the three
//! cannot drift apart.

use dirt_proto::api::ErrorClass;

use super::worker::{SyncFailure, SyncStatus};
use crate::store::{NoteProblem, ProblemKind};
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
    /// Why the last cycle failed, if it did.
    pub failure: Option<SyncFailure>,
    /// Each note that could not be synced.
    pub problems: Vec<NoteProblem>,
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
            failure: status.failure.clone(),
            problems: Vec::new(),
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
        lines
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

impl NoteProblem {
    /// What to call the note: the start of its first line, or words for a
    /// note with no text or one this device does not hold.
    #[must_use]
    pub fn title(&self) -> String {
        let Some(text) = &self.text else {
            return "A note this device has never been able to read".to_owned();
        };
        let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
            return "A note with no text".to_owned();
        };
        let mut title: String = line.chars().take(TITLE_LENGTH).collect();
        if line.chars().count() > TITLE_LENGTH {
            title.push('…');
        }
        title
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
            failure: Some(SyncFailure {
                class: ErrorClass::TryAgainLater,
                message: "connection refused".to_owned(),
            }),
            problems: vec![held.clone(), refused.clone()],
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
