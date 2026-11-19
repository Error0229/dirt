//! The file tile (interface design, section 6.5): a file attached to the
//! note that is open, under its text.
//!
//! Its thumbnail or its kind, its name, "1.8 MiB · JPEG", and where it is
//! when that matters. **Open in another app**, **Save a copy** and
//! **Remove** are icons on it: shown under the pointer on the desktop, and
//! always on Android. A file the system would run has no **Open**. A click
//! or a tap anywhere else on it, or <kbd>Space</kbd> or <kbd>Enter</kbd>
//! while it has the keyboard, previews it. On a file of a kind the preview
//! does not show it gives the keyboard to the tile's first icon instead,
//! which shows them: opening a file in another app is always a press of
//! its own, never a click on the file.
//!
//! A file being fetched has a clay ring turning on its thumbnail; one being
//! sent, a clay line running along its foot.
//!
//! New tiles grow out of the point last pressed (the paperclip) or where
//! files were dropped, and bounce into place (5.5): the script does that
//! (`data-dui-enter="grow"`).

use dioxus::prelude::*;
use dirt_core::files::runs_as_a_program;
use dirt_core::models::NoteId;
use dirt_core::store::AttachedFile;
use dirt_core::sync::overview::{size_words, Whereabouts};
use uuid::Uuid;

use super::kind::{kind_word, preview_kind, PreviewKind};
use super::note_files::NoteFiles;
use crate::components::{IconButton, IconButtonSize};
use crate::dom::{self, Caret};
use crate::icons::{Icon, IconSize, IconSvg};

/// The ID of a file's tile in the page: the preview grows out of it.
#[must_use]
pub fn tile_element_id(file: Uuid) -> String {
    format!("dui-file-{file}")
}

/// The tiles of the files of `note`, the open note. `can_send` is whether
/// this device can send files now (signed in, let in, online), for the
/// words of a file not yet sent. `on_open` and `on_save` are the app's:
/// another app, and a save dialog. Nothing is drawn until the note's files
/// have been read, so that a note opening draws its tiles at once and not
/// one by one, and never the last note's.
#[component]
pub fn FileTiles(
    files: NoteFiles,
    note: NoteId,
    can_send: bool,
    on_open: EventHandler<AttachedFile>,
    on_save: EventHandler<AttachedFile>,
    /// More classes, for where the tiles sit.
    #[props(default)]
    class: String,
) -> Element {
    let Some(found) = files
        .listed()
        .and_then(|(held, found)| (held == note).then_some(found))
    else {
        return rsx! {};
    };
    rsx! {
        div {
            class: if class.is_empty() { "dui-files".to_owned() } else { format!("dui-files {class}") },
            role: "list",
            "aria-label": "Files",
            "data-dui-list": "",
            "data-dui-exit": "fade",
            "data-dui-enter-first": "",
            for file in found {
                FileTile {
                    key: "{file.attachment.file_id}",
                    thumbnail: files.thumbnail(file.attachment.file_id),
                    fetching: files.is_fetching(file.attachment.file_id),
                    whereabouts: file.whereabouts(can_send),
                    file,
                    on_preview: move |file: AttachedFile| files.preview(&file),
                    on_open,
                    on_save,
                    on_remove: move |file: AttachedFile| files.remove(&file),
                }
            }
        }
    }
}

/// The icon for a file of a kind with no thumbnail.
const fn kind_icon(kind: Option<PreviewKind>) -> Icon {
    match kind {
        Some(PreviewKind::Audio) => Icon::Music,
        _ => Icon::File,
    }
}

/// One file's tile.
#[component]
pub fn FileTile(
    file: AttachedFile,
    thumbnail: Option<String>,
    fetching: bool,
    whereabouts: Option<Whereabouts>,
    on_preview: EventHandler<AttachedFile>,
    on_open: EventHandler<AttachedFile>,
    on_save: EventHandler<AttachedFile>,
    on_remove: EventHandler<AttachedFile>,
) -> Element {
    let attachment = &file.attachment;
    let name = attachment.name.clone();
    let kind = preview_kind(&name, &attachment.media_type);
    let mut details = format!(
        "{} \u{b7} {}",
        size_words(attachment.size),
        kind_word(&name)
    );
    if let Some(at) = &whereabouts {
        details.push_str(" \u{b7} ");
        details.push_str(&at.words());
    }
    let uploading = whereabouts == Some(Whereabouts::Uploading);
    // A file the system would run is saved, and opened from where it was
    // saved by somebody who means to.
    let can_open = !runs_as_a_program(&name);
    let tip = match kind {
        Some(_) => "Preview",
        None if can_open => "Not shown here: open or save it from its icons",
        None => "Not shown here: save it to open it yourself",
    };
    let tile_id = tile_element_id(attachment.file_id);
    let first_icon = format!("#{tile_id} .dui-file-acts button");
    let (for_preview, for_open, for_save, for_remove) =
        (file.clone(), file.clone(), file.clone(), file.clone());

    rsx! {
        div {
            id: "{tile_id}",
            class: if fetching { "dui-file fetching" } else { "dui-file" },
            role: "listitem",
            "data-key": "{attachment.file_id}",
            "data-dui-enter": "grow",
            div {
                class: "dui-file-main",
                role: "button",
                tabindex: "0",
                "aria-label": "{tip}: {name}",
                "aria-busy": fetching.then_some("true"),
                "data-tip": "{tip}",
                "data-kbd": kind.is_some().then_some("Space"),
                // Space and Enter press it, in the script.
                "data-dui-press": "",
                onclick: move |_| {
                    if kind.is_some() {
                        on_preview.call(for_preview.clone());
                    } else {
                        // Its icons show while one has the keyboard.
                        dom::focus(&[first_icon.as_str()], Caret::Kept);
                    }
                },
                div { class: "dui-file-thumb",
                    if let Some(picture) = thumbnail {
                        img { src: "{picture}", alt: "" }
                    } else {
                        IconSvg { icon: kind_icon(kind), size: IconSize::Regular }
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
                div { class: "dui-file-words",
                    div { class: "dui-file-name", "{name}" }
                    div { class: "dui-file-meta", "{details}" }
                }
            }
            div { class: "dui-file-acts",
                if can_open {
                    IconButton {
                        icon: Icon::External,
                        label: "Open in another app",
                        size: IconButtonSize::Small,
                        onclick: move |_| on_open.call(for_open.clone()),
                    }
                }
                IconButton {
                    icon: Icon::Download,
                    label: if can_open { "Save a copy" } else { "Save a copy. This kind of file can run as a program: open it yourself if you trust it" },
                    size: IconButtonSize::Small,
                    onclick: move |_| on_save.call(for_save.clone()),
                }
                IconButton {
                    icon: Icon::X,
                    label: "Remove from the note",
                    size: IconButtonSize::Small,
                    danger: true,
                    onclick: move |_| on_remove.call(for_remove.clone()),
                }
            }
            if uploading {
                span { class: "dui-file-line", "aria-hidden": "true" }
            }
        }
    }
}
