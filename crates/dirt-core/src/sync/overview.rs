//! What every interface shows about sync, as one value with one set of
//! words (design section 9.4).
//!
//! The desktop app, the Android app and the CLI all show the same things:
//! when the last sync succeeded, how many notes wait to be sent, why the
//! last cycle failed, and each note that could not be synced, with the
//! reason and what would release it. The words are here so that the three
//! cannot drift apart.

use dirt_proto::api::ErrorClass;

use super::worker::SyncFailure;
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
    /// Why the last cycle failed, if it did.
    pub failure: Option<SyncFailure>,
    /// Each note that could not be synced.
    pub problems: Vec<NoteProblem>,
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

    /// How many notes wait to be sent, in words, or nothing when none do.
    #[must_use]
    pub fn waiting(&self) -> Option<String> {
        match self.unsent {
            0 => None,
            1 => Some("1 note is waiting to be sent".to_owned()),
            count => Some(format!("{count} notes are waiting to be sent")),
        }
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
        match self.problems.len() {
            0 => {}
            1 => lines.push("1 note could not be synced:".to_owned()),
            count => lines.push(format!("{count} notes could not be synced:")),
        }
        for problem in &self.problems {
            lines.push(format!("  {} ({})", problem.title(), problem.note_id));
            lines.push(format!("    {}", as_sentence(&problem.reason)));
            lines.push(format!("    {}", problem.kind.release()));
        }
        lines
    }
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

impl ProblemKind {
    /// What ends the problem, as a sentence for the person.
    #[must_use]
    pub const fn release(self) -> &'static str {
        match self {
            Self::Held => {
                "It will sync again when a device that can still read it edits it. \
                 Until then this device keeps its own copy and does not send it."
            }
            Self::RetryAfterEdit => "Edit it on this device to try again.",
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
            failure: Some(SyncFailure {
                class: ErrorClass::TryAgainLater,
                message: "connection refused".to_owned(),
            }),
            problems: vec![held.clone(), refused.clone()],
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

        let quiet = SyncOverview::default().lines(0);
        assert_eq!(quiet, ["Never synced", "Nothing is waiting to be sent"]);
    }
}
