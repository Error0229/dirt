//! The shelf (interface design, section 6.5): the files of the note that
//! is open, at the foot of its page, held at the bottom of the screen
//! while the text scrolls above it.
//!
//! It is one line, "3 files", which folds and unfolds the tiles under it.
//! Folded, the line also holds each file as a small square, its thumbnail
//! or its kind, as many as fit and the rest a sideways scroll away: a
//! square previews its file, so that looking at a file is still one
//! action (section 2). Unfolded, the tiles (6.5) are under the line, at
//! most three rows and a third of the window high; more scroll inside it.
//!
//! Every note opens with its files folded, whatever their count, and only
//! the person unfolds them (interface design 6.5, 7.14). What they choose
//! holds while the note is open, and is not kept: see [`NoteFiles::folded`].
//! A file attached, or brought by a sync, does not unfold it: its square
//! pops into the folded line, and the count changes.

use dioxus::prelude::*;
use dirt_core::models::NoteId;
use dirt_core::store::AttachedFile;
use uuid::Uuid;

use super::kind::{preview_kind, PreviewKind};
use super::note_files::NoteFiles;
use super::tile::{tile_element_id, FileTiles};
use crate::components::ScrollArea;
use crate::dom::{self, Caret};
use crate::icons::{Icon, IconSize, IconSvg};

/// The ID of a file's square on the folded shelf. The preview grows out of
/// it while the shelf is folded, and out of the tile while it is not.
#[must_use]
pub fn mini_element_id(file: Uuid) -> String {
    format!("{}-mini", tile_element_id(file))
}

/// What the line of a shelf says: "1 file", "3 files".
#[must_use]
pub fn count_words(count: usize) -> String {
    if count == 1 {
        "1 file".to_owned()
    } else {
        format!("{count} files")
    }
}

/// What the count's live region says when the files have changed since the
/// note was opened: the count, or "No files" once the last has gone.
#[must_use]
pub fn count_told(count: usize) -> String {
    if count == 0 {
        "No files".to_owned()
    } else {
        count_words(count)
    }
}

/// The shelf of the files of `note`, the open note, as both apps show it.
/// Its props are [`FileTiles`]'s. Nothing is drawn until the note's files
/// have been read, nor for a note with none; the count's live region is
/// there all the while the note is open, so that the first file attached
/// and the last removed are told too.
#[component]
pub fn FileShelf(
    files: NoteFiles,
    note: NoteId,
    can_send: bool,
    on_open: EventHandler<AttachedFile>,
    on_save: EventHandler<AttachedFile>,
    /// More classes, for where the shelf sits.
    #[props(default)]
    class: String,
    /// The shelf is out of sight (Zen, 7.7): it is inert as well, so that
    /// <kbd>Tab</kbd> and a screen reader pass it by.
    #[props(default)]
    hidden: bool,
) -> Element {
    // The tile whose first icon is to have the keyboard once the shelf
    // has unfolded: a file the preview does not show, pressed folded.
    let mut show_tile = use_signal(|| None::<Uuid>);
    use_effect(move || {
        if let Some(file) = show_tile() {
            let first = format!("#{} .dui-file-acts button", tile_element_id(file));
            if cfg!(target_os = "android") {
                // On Android the tile's icons always show. The keyboard
                // goes only if it was not up for the note's text: a touch
                // on the square left the text the focus, and taking it
                // would resize the page under the unfolding (#322).
                dom::focus_unless_typing(&[first.as_str()]);
            } else {
                dom::focus(&[first.as_str()], Caret::Kept);
            }
            show_tile.set(None);
        }
    });
    // The count is told when it changes while the note is open, and not
    // as the note opens: the live region says nothing until then.
    let count = files
        .listed()
        .and_then(|(held, found)| (held == note).then_some(found.len()));
    let told = if files.count_changed() {
        count.map(count_told).unwrap_or_default()
    } else {
        String::new()
    };
    // Whether the shelf came with the note (its first reading since the
    // note was opened found files), rather than with a file attached
    // later: the script lets a page ride up as the shelf comes only in the
    // second case.
    let came_with_note = files.opened_with().is_none_or(|first| first > 0);
    let found = files
        .listed()
        .and_then(|(held, found)| (held == note).then_some(found))
        .filter(|found| !found.is_empty());
    let folded = files.folded();
    // One template, whether or not there is a shelf: the live region is
    // the same node from the note's opening to its closing, so a change
    // in it is told, the first file and the last among them. Two returns
    // would be two templates, and the region made anew, already holding
    // its words, which a screen reader does not read.
    rsx! {
        span { class: "dui-sr", "aria-live": "polite", "{told}" }
        if let Some(found) = found {
            ShelfFrame {
                note: note.to_string(),
                count: found.len(),
                folded,
                class,
                hidden,
                came_with_note,
                on_fold: move |fold: bool| files.set_folded(fold),
                minis: rsx! {
                    for file in found {
                        FileMini {
                            key: "{file.attachment.file_id}",
                            thumbnail: files.thumbnail(file.attachment.file_id),
                            fetching: files.is_fetching(file.attachment.file_id),
                            file,
                            on_preview: move |file: AttachedFile| files.preview(&file),
                            on_show: move |file: AttachedFile| {
                                files.set_folded(false);
                                show_tile.set(Some(file.attachment.file_id));
                            },
                        }
                    }
                },
                FileTiles { files, note, can_send, on_open, on_save }
            }
        }
    }
}

/// The shelf as it is drawn: the line with the count and, folded, the
/// squares (`minis`); under it the tiles (`children`), shown while it is
/// unfolded. `note` names it, for the line's `aria-controls`. Given the
/// squares and the tiles by [`FileShelf`], or by the gallery.
#[component]
pub fn ShelfFrame(
    note: String,
    count: usize,
    folded: bool,
    on_fold: EventHandler<bool>,
    minis: Element,
    children: Element,
    #[props(default)] class: String,
    /// Out of sight, and inert (see [`FileShelf`]).
    #[props(default)]
    hidden: bool,
    /// It came with the note, rather than with a file attached later (see
    /// [`FileShelf`]).
    #[props(default)]
    came_with_note: bool,
) -> Element {
    let body = format!("dui-shelf-{note}");
    let words = count_words(count);
    rsx! {
        div {
            class: if class.is_empty() { "dui-shelf".to_owned() } else { format!("dui-shelf {class}") },
            "data-folded": folded.then_some(""),
            // The script keeps room for it at the foot of what scrolls,
            // so that the caret is never revealed behind it.
            "data-dui-shelf": if came_with_note { "with-note" } else { "" },
            "inert": hidden.then_some(""),
            div { class: "dui-shelf-head",
                button {
                    r#type: "button",
                    class: "dui-shelf-fold",
                    "aria-expanded": if folded { "false" } else { "true" },
                    "aria-controls": "{body}",
                    "data-tip": if folded { "Show the files" } else { "Fold the files" },
                    // A press leaves the keyboard's focus where it is: in
                    // the note's text, the cursor stays and Android's
                    // keyboard does not go and resize the page under the
                    // movement.
                    "data-dui-keep-focus": "",
                    onclick: move |_| on_fold.call(!folded),
                    IconSvg { icon: Icon::ChevronUp, size: IconSize::Small }
                    span { "{words}" }
                }
                // The squares are a list the script watches: a new one pops
                // into the line where it lands (5.5).
                div {
                    class: "dui-shelf-minis",
                    role: "list",
                    "aria-label": "Files",
                    "inert": (!folded).then_some(""),
                    "data-dui-list": "",
                    "data-dui-exit": "fade",
                    {minis}
                }
            }
            div {
                id: "{body}",
                class: "dui-shelf-body",
                "inert": folded.then_some(""),
                div { class: "dui-shelf-clip",
                    ScrollArea { class: "dui-shelf-scroll", {children} }
                }
            }
        }
    }
}

/// One file's square on the folded shelf: its thumbnail, or its kind.
/// Pressed, it previews the file; one the preview does not show unfolds
/// the shelf and gives the keyboard to its tile's first icon (`on_show`),
/// as a press on its tile does (6.16).
#[component]
pub fn FileMini(
    file: AttachedFile,
    thumbnail: Option<String>,
    fetching: bool,
    on_preview: EventHandler<AttachedFile>,
    on_show: EventHandler<AttachedFile>,
) -> Element {
    let attachment = &file.attachment;
    let name = attachment.name.clone();
    let kind = preview_kind(&name, &attachment.media_type);
    let label = if kind.is_some() {
        format!("Preview: {name}")
    } else {
        format!("Show its icons: {name}")
    };
    let icon = match kind {
        Some(PreviewKind::Audio) => Icon::Music,
        _ => Icon::File,
    };
    rsx! {
        span { role: "listitem", class: "dui-mini-at",
            button {
                r#type: "button",
                id: "{mini_element_id(attachment.file_id)}",
                class: if fetching { "dui-mini fetching" } else { "dui-mini" },
                "data-key": "{attachment.file_id}",
                "data-dui-enter": "pop-in",
                "aria-label": "{label}",
                // A press leaves the focus in the note's text: the preview
                // takes it as it opens, and a kind it does not show only
                // unfolds the shelf, where on Android the keyboard staying
                // up keeps the page from being resized under the movement.
                "data-dui-keep-focus": "",
                "aria-busy": fetching.then_some("true"),
                "data-tip": "{name}",
                onclick: move |_| {
                    if kind.is_some() {
                        on_preview.call(file.clone());
                    } else {
                        on_show.call(file.clone());
                    }
                },
                if let Some(picture) = thumbnail {
                    img { src: "{picture}", alt: "" }
                } else {
                    IconSvg { icon, size: IconSize::Small }
                }
                if fetching {
                    svg {
                        class: "dui-file-ring",
                        view_box: "0 0 44 44",
                        "aria-hidden": "true",
                        circle { cx: "22", cy: "22", r: "19" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_counts_in_words() {
        assert_eq!(count_words(1), "1 file");
        assert_eq!(count_words(3), "3 files");
        assert_eq!(count_words(20), "20 files");
        assert_eq!(count_told(0), "No files");
        assert_eq!(count_told(2), "2 files");
    }
}
