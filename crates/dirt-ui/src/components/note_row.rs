//! A note in a list (interface design, sections 6.2 and 6.19).
//!
//! [`NoteSummary`] is what a list shows of one note, worked out once from
//! the note: its first line as the title, the rest as a preview with what
//! Find matched marked, the tags at its end apart, and when it was edited.
//! [`NoteRow`] draws it as the desktop's row. The Android app's card
//! ([`crate::components::NoteCard`]) shows the same summary and is built
//! from the same parts, [`Marked`] and [`TagLinks`].

use dioxus::prelude::*;
use dirt_core::models::{Note, NoteId};
use dirt_core::sync::overview::NOT_ON_SERVER_YET;

use crate::components::icon_button::{IconButton, IconButtonSize};
use crate::icons::{Icon, IconSvg};
use crate::text::{pieces, Piece, PieceKind};
use crate::time::{edited_full, edited_short, Clock};

/// How many of a note's trailing tags a row shows before "+1".
const TAGS_SHOWN: usize = 2;

/// What a list shows of one note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSummary {
    /// Which note.
    pub id: NoteId,
    /// The note's first line, in pieces. Empty for a note with no words.
    pub title: Vec<Piece>,
    /// What stands in for the title of a note with no words: its first
    /// file's name, or "Empty note".
    pub untitled: String,
    /// The rest of the note, in pieces.
    pub preview: Vec<Piece>,
    /// The tags at the end of the note, as written, without their `#`.
    pub labels: Vec<String>,
    /// When it was last edited, in a few characters: "4m", "Thu".
    pub time: String,
    /// The same in full, for the tooltip: "Edited Nov 13, 09:12".
    pub time_in_full: String,
    /// The note has files.
    pub has_files: bool,
    /// The note has not reached the server as it is now.
    pub waiting: bool,
    /// The note is archived: its title is a shade lighter, and what its
    /// row offers is the bin, not the box.
    pub archived: bool,
    /// A picture attached to it, as something an `img` can show: Android's
    /// card shows it as a 44 px thumbnail (section 6.2).
    pub thumbnail: Option<String>,
}

impl NoteSummary {
    /// The summary of `note`, seen at `clock`, with the words of
    /// `find_words` (what [`crate::text::find_words`] gives for Find's
    /// text) marked where they match.
    #[must_use]
    pub fn of(note: &Note, find_words: &[String], clock: Clock) -> Self {
        let outline = note.outline();
        Self {
            id: note.id,
            title: pieces(&outline.title, find_words),
            untitled: "Empty note".to_owned(),
            preview: pieces(&outline.preview, find_words),
            labels: outline.labels,
            time: edited_short(note.updated_at, clock),
            time_in_full: edited_full(note.updated_at, clock),
            has_files: false,
            waiting: false,
            archived: note.archived,
            thumbnail: None,
        }
    }

    /// Whether the note has files, and the name of the first: a note with
    /// a file and no words goes by the file's name.
    #[must_use]
    pub fn files(mut self, has_files: bool, first_name: Option<&str>) -> Self {
        self.has_files = has_files;
        if let Some(name) = first_name {
            name.clone_into(&mut self.untitled);
        }
        self
    }

    /// Whether the note has not reached the server yet.
    #[must_use]
    pub const fn waiting(mut self, waiting: bool) -> Self {
        self.waiting = waiting;
        self
    }

    /// The thumbnail of a picture attached to it, if the device has one.
    #[must_use]
    pub fn thumbnail(mut self, thumbnail: Option<String>) -> Self {
        self.thumbnail = thumbnail;
        self
    }
}

/// The ID a row's element has in the page, for `aria-activedescendant`.
#[must_use]
pub fn row_element_id(note: &NoteId) -> String {
    format!("dui-note-{note}")
}

/// Text in pieces: matches marked, tags in the accent.
#[component]
pub fn Marked(pieces: Vec<Piece>) -> Element {
    rsx! {
        for (index , piece) in pieces.into_iter().enumerate() {
            match piece.kind {
                PieceKind::Plain => rsx! {
                    span { key: "{index}", "{piece.text}" }
                },
                PieceKind::Match => rsx! {
                    mark { key: "{index}", "{piece.text}" }
                },
                PieceKind::Tag => rsx! {
                    span { key: "{index}", class: "dui-tg", "{piece.text}" }
                },
                PieceKind::TagMatch => rsx! {
                    span { key: "{index}", class: "dui-tg",
                        mark { "{piece.text}" }
                    }
                },
            }
        }
    }
}

/// The tags at the end of a note, as buttons: pressing one narrows the
/// list to it. At most two are shown, and "+1" for the rest.
///
/// `in_tab_order` is false for the rows that are not the chosen one, so
/// that Tab does not stop at every tag of every note.
#[component]
pub fn TagLinks(
    labels: Vec<String>,
    #[props(default = true)] in_tab_order: bool,
    on_tag: EventHandler<String>,
) -> Element {
    // A tag written twice is one tag, and one button: each button's key is
    // its tag, and keys must differ.
    let mut seen = std::collections::HashSet::new();
    let labels: Vec<String> = labels
        .into_iter()
        .filter(|label| seen.insert(label.to_lowercase()))
        .collect();
    let more = labels.len().saturating_sub(TAGS_SHOWN);
    let rest = labels
        .iter()
        .skip(TAGS_SHOWN)
        .map(|label| format!("#{label}"))
        .collect::<Vec<_>>()
        .join(" ");
    rsx! {
        for label in labels.into_iter().take(TAGS_SHOWN) {
            button {
                key: "{label}",
                r#type: "button",
                class: "dui-tag-link",
                "data-tip": "Only #{label.to_lowercase()}",
                tabindex: (!in_tab_order).then_some("-1"),
                onclick: {
                    let tag = label.to_lowercase();
                    move |event: MouseEvent| {
                        event.stop_propagation();
                        on_tag.call(tag.clone());
                    }
                },
                "#{label}"
            }
        }
        if more > 0 {
            span { class: "dui-tag-more", "data-tip": "{rest}", "+{more}" }
        }
    }
}

/// A note as a row of the desktop's list.
///
/// It is an option of the list it sits in, which is a list box: the list
/// has the keyboard's focus and says which row is chosen. Pressing the row
/// opens the note. Under the pointer the time gives way to the archive
/// box, or to the bin on an archived note; the same button shows while it
/// has the keyboard's focus.
#[component]
pub fn NoteRow(
    summary: NoteSummary,
    /// The note is the one open on the page.
    #[props(default)]
    selected: bool,
    /// It has just arrived from another device: it flashes once.
    #[props(default)]
    flash: bool,
    /// It has just come back by Undo: it drops in with a bounce.
    #[props(default)]
    back: bool,
    on_open: EventHandler<()>,
    on_tag: EventHandler<String>,
    /// The box was pressed.
    on_archive: EventHandler<()>,
    /// The bin was pressed, on an archived note.
    on_delete: EventHandler<()>,
) -> Element {
    let classes = format!(
        "dui-row{}{}{}",
        if selected { " sel" } else { "" },
        if flash { " flash" } else { "" },
        if summary.archived { " arch" } else { "" },
    );
    let NoteSummary {
        id,
        title,
        untitled,
        preview,
        labels,
        time,
        time_in_full,
        has_files,
        waiting,
        archived,
        thumbnail: _,
    } = summary;
    rsx! {
        div {
            class: "{classes}",
            id: row_element_id(&id),
            role: "option",
            "aria-selected": if selected { "true" } else { "false" },
            "data-key": "{id}",
            "data-dui-enter": back.then_some("pop"),
            onclick: move |_| on_open.call(()),
            div { class: "dui-row-t",
                if title.is_empty() {
                    "{untitled}"
                } else {
                    Marked { pieces: title }
                }
            }
            if !preview.is_empty() {
                div { class: "dui-row-s",
                    Marked { pieces: preview }
                }
            }
            if !labels.is_empty() {
                div { class: "dui-row-tags",
                    TagLinks { labels, in_tab_order: selected, on_tag }
                }
            }
            div { class: "dui-row-r",
                span { class: "dui-row-time", "data-tip": "{time_in_full}", "{time}" }
                if has_files {
                    span {
                        class: "dui-row-mark",
                        role: "img",
                        "aria-label": "Has files",
                        "data-tip": "Has files",
                        IconSvg { icon: Icon::Paperclip }
                    }
                }
                if waiting {
                    span {
                        class: "dui-wait",
                        role: "img",
                        "aria-label": NOT_ON_SERVER_YET,
                        "data-tip": NOT_ON_SERVER_YET,
                    }
                }
                if archived {
                    IconButton {
                        icon: Icon::Trash,
                        label: "Delete for good",
                        shortcut: "Del",
                        size: IconButtonSize::Small,
                        danger: true,
                        skipped_by_tab: !selected,
                        class: "dui-row-act",
                        onclick: move |event: MouseEvent| {
                            event.stop_propagation();
                            on_delete.call(());
                        },
                    }
                } else {
                    IconButton {
                        icon: Icon::Archive,
                        label: "Archive",
                        shortcut: "Del",
                        size: IconButtonSize::Small,
                        skipped_by_tab: !selected,
                        class: "dui-row-act",
                        onclick: move |event: MouseEvent| {
                            event.stop_propagation();
                            on_archive.call(());
                        },
                    }
                }
            }
        }
    }
}

/// The line a layer of the list starts with (section 6.17): its name, and
/// a line of grains to the edge. Today, the surface, has none.
///
/// `title` is true for the one line that is not a layer: "Archived", over
/// the archived notes Find has found.
#[component]
pub fn LayerLine(name: String) -> Element {
    rsx! {
        div {
            class: "dui-layer",
            role: "presentation",
            "data-key": "layer {name}",
            "{name}"
        }
    }
}

/// A list with no notes at all (section 6.14): a faint drawing, and no
/// words.
#[component]
pub fn EmptyList(#[props(default = Icon::Feather)] icon: Icon) -> Element {
    rsx! {
        div { class: "dui-list-empty", "data-key": "empty",
            IconSvg { icon, size: crate::icons::IconSize::Large }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock_at(now_ms: i64) -> Clock {
        Clock {
            now_ms,
            utc_offset_secs: 0,
        }
    }

    #[test]
    fn a_summary_is_the_notes_outline_with_matches_marked() {
        let mut note = Note::new("Sourdough starter\nfeed it on Sunday #baking");
        note.updated_at = 1_800_000_000_000;
        let summary = NoteSummary::of(
            &note,
            &crate::text::find_words("sun"),
            clock_at(note.updated_at + 5 * 60_000),
        );
        assert_eq!(summary.title.len(), 1);
        assert_eq!(summary.title[0].text, "Sourdough starter");
        assert!(summary
            .preview
            .iter()
            .any(|piece| piece.kind == PieceKind::Match && piece.text == "Sun"));
        assert_eq!(summary.labels, ["baking"]);
        assert_eq!(summary.time, "5m");
        assert!(!summary.archived && !summary.waiting && !summary.has_files);
    }

    #[test]
    fn a_note_with_a_file_and_no_words_goes_by_the_files_name() {
        let note = Note::new("  ");
        let summary = NoteSummary::of(&note, &[], clock_at(note.updated_at));
        assert!(summary.title.is_empty());
        assert_eq!(summary.untitled, "Empty note");
        let summary = summary.files(true, Some("receipt.pdf")).waiting(true);
        assert_eq!(summary.untitled, "receipt.pdf");
        assert!(summary.has_files && summary.waiting);
    }
}
