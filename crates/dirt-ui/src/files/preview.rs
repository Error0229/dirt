//! The file preview (interface design, section 6.16): a file looked at
//! inside Dirt, on a near-black backdrop, with nothing around it but a few
//! quiet controls.
//!
//! - Top right: **Open in another app**, **Save a copy**, **Remove from
//!   note** and **Close**. On Android a tap on the backdrop hides and shows
//!   them.
//! - Top left: the file's name, under the pointer (desktop).
//! - The sides: ‹ and ›, under the pointer, when the note has more than one
//!   file the preview shows (desktop). Android swipes.
//! - The foot: a dot for each such file, the one shown wider. None for one.
//!
//! It is a dialog: focus goes into it as it opens, <kbd>Tab</kbd> stays
//! inside, and focus goes back to the tile afterwards. <kbd>←</kbd> and
//! <kbd>→</kbd> go to the other files; <kbd>Esc</kbd>, <kbd>Space</kbd>, a
//! click on the backdrop or ✕ close it; on Android a swipe down or Back.
//! It grows out of its file's tile and shrinks back into it. The script
//! does the keys, the gestures and the movement, and tells Rust only how
//! they ended; a picture zooms to twice its size there too, on a double
//! click or a pinch. A PDF's pages are drawn there by the bundled renderer
//! (`crate::pdf`), and a sound's card plays there.
//!
//! It is always in the page while a note is open, so that it can move in
//! and out.

use dioxus::prelude::*;
use dirt_core::files::runs_as_a_program;
use dirt_core::store::AttachedFile;

use super::kind::PreviewKind;
use super::note_files::{NoteFiles, Showing};
use super::tile::tile_element_id;
use crate::components::{IconButton, ScrollArea};
use crate::events::{use_outcome, Outcome};
use crate::icons::{Icon, IconSize, IconSvg};

/// The preview of the open note's files. `on_open` and `on_save` are the
/// app's, as on the tiles.
#[component]
pub fn FilePreview(
    files: NoteFiles,
    on_open: EventHandler<AttachedFile>,
    on_save: EventHandler<AttachedFile>,
) -> Element {
    let id = use_outcome(move |outcome| match outcome {
        Outcome::Dismiss | Outcome::Back => files.close(),
        Outcome::Next => files.step(true),
        Outcome::Previous => files.step(false),
        _ => {}
    });
    let all = files.previewable();
    let open = files.previewing().is_some();
    // While it closes it still shows the file it showed.
    let index = files
        .shown_file()
        .and_then(|shown| all.iter().position(|file| file.attachment.file_id == shown))
        .unwrap_or(0);
    let current = all.get(index).cloned();
    let name = current
        .as_ref()
        .map(|file| file.attachment.name.clone())
        .unwrap_or_default();
    let from = current
        .as_ref()
        .map(|file| tile_element_id(file.attachment.file_id));
    let thumbnail = current
        .as_ref()
        .and_then(|file| files.thumbnail(file.attachment.file_id));
    let can_open = !runs_as_a_program(&name);
    let several = all.len() > 1;
    let (for_open, for_save, for_remove) = (current.clone(), current.clone(), current.clone());
    let showing = files.showing();
    // A new file, or the same file read again, is a new item: the script
    // brings it in.
    let file_id = current
        .as_ref()
        .map(|file| file.attachment.file_id.to_string())
        .unwrap_or_default();
    let item_key = format!("{file_id}-{}", item_name(&showing));

    rsx! {
        div {
            class: if open { "dui-preview on" } else { "dui-preview" },
            role: "dialog",
            "aria-modal": "true",
            "aria-label": "Preview of {name}",
            "data-dui-modal": "preview",
            "data-dui-id": "{id}",
            "data-dui-from": from,
            "inert": if open { None } else { Some("") },
            tabindex: "-1",
            div { class: "dui-pv-scrim", "aria-hidden": "true" }
            div { class: "dui-pv-name", "aria-hidden": "true", "{name}" }
            div { class: "dui-pv-stage", "data-dui-pv-stage": "",
                div { key: "{item_key}", class: "dui-pv-item", "data-file": "{file_id}",
                    PreviewItem { showing, name: name.clone(), thumbnail }
                }
            }
            if several {
                button {
                    r#type: "button",
                    class: "dui-pv-nav prev",
                    "aria-label": "Previous",
                    "data-tip": "Previous",
                    "data-kbd": "\u{2190}",
                    onclick: move |_| files.step(false),
                    IconSvg { icon: Icon::ChevronLeft, size: IconSize::Regular }
                }
                button {
                    r#type: "button",
                    class: "dui-pv-nav next",
                    "aria-label": "Next",
                    "data-tip": "Next",
                    "data-kbd": "\u{2192}",
                    onclick: move |_| files.step(true),
                    IconSvg { icon: Icon::ChevronRight, size: IconSize::Regular }
                }
                div { class: "dui-pv-dots", "aria-hidden": "true",
                    for (place , file) in all.iter().enumerate() {
                        i {
                            key: "{file.attachment.file_id}",
                            class: if place == index { "on" } else { "" },
                        }
                    }
                }
            }
            div { class: "dui-pv-bar", role: "toolbar", "aria-label": "The file",
                if can_open {
                    IconButton {
                        icon: Icon::External,
                        label: "Open in another app",
                        onclick: move |_| {
                            if let Some(file) = for_open.clone() {
                                on_open.call(file);
                            }
                        },
                    }
                }
                IconButton {
                    icon: Icon::Download,
                    label: "Save a copy",
                    onclick: move |_| {
                        if let Some(file) = for_save.clone() {
                            on_save.call(file);
                        }
                    },
                }
                IconButton {
                    icon: Icon::Trash,
                    label: "Remove from note",
                    danger: true,
                    onclick: move |_| {
                        if let Some(file) = for_remove.clone() {
                            files.remove(&file);
                        }
                    },
                }
                IconButton {
                    icon: Icon::X,
                    label: "Close",
                    shortcut: "Esc",
                    onclick: move |_| files.close(),
                }
            }
        }
    }
}

/// A name for what is shown, so that a change of it is a new item.
const fn item_name(showing: &Showing) -> &'static str {
    match showing {
        Showing::Nothing | Showing::Loading => "loading",
        Showing::Media { .. } | Showing::Text(_) => "shown",
        Showing::Failed(_) => "failed",
    }
}

/// What the stage shows.
#[component]
fn PreviewItem(showing: Showing, name: String, thumbnail: Option<String>) -> Element {
    match showing {
        Showing::Nothing => rsx! {},
        Showing::Loading => rsx! {
            if let Some(picture) = thumbnail {
                img { class: "dui-pv-picture dui-pv-standin", src: "{picture}", alt: "" }
            }
            svg {
                class: "dui-pv-ring",
                view_box: "0 0 44 44",
                role: "progressbar",
                "aria-label": "Fetching {name}",
                circle { cx: "22", cy: "22", r: "19" }
            }
        },
        Showing::Failed(why) => rsx! {
            div { class: "dui-pv-failed", role: "alert",
                IconSvg { icon: Icon::Alert, size: IconSize::Large }
                p { "{name} could not be shown." }
                p { class: "why", "{why}" }
            }
        },
        Showing::Text(text) => rsx! {
            ScrollArea { class: "dui-pv-text-wrap", inner_class: "dui-pv-text-scroll",
                pre { class: "dui-pv-text", tabindex: "0", "{text}" }
            }
        },
        Showing::Media { kind, address } => match kind {
            PreviewKind::Picture => rsx! {
                img {
                    class: "dui-pv-picture",
                    src: "{address}",
                    alt: "{name}",
                    draggable: "false",
                    "data-dui-zoom": "",
                }
            },
            PreviewKind::Video => rsx! {
                // The web view's own controls, without the ones that write
                // a copy of the video or send it to another screen.
                video {
                    class: "dui-pv-video",
                    src: "{address}",
                    controls: true,
                    autoplay: true,
                    playsinline: true,
                    "controlslist": "nodownload noremoteplayback noplaybackrate",
                    "disablepictureinpicture": "",
                    "disableremoteplayback": "",
                }
            },
            PreviewKind::Audio => rsx! {
                div { class: "dui-pv-audio", "data-dui-audio": "{address}",
                    button {
                        r#type: "button",
                        class: "dui-pv-play",
                        "aria-label": "Play",
                        "data-tip": "Play",
                        "data-kbd": "Space",
                        span { class: "play", IconSvg { icon: Icon::Play, size: IconSize::Regular } }
                        span { class: "pause", IconSvg { icon: Icon::Pause, size: IconSize::Regular } }
                    }
                    div { class: "dui-pv-wave", "aria-hidden": "true" }
                    span { class: "dui-pv-time", "0:00" }
                }
            },
            PreviewKind::Pdf => rsx! {
                ScrollArea { class: "dui-pv-pdf-wrap", inner_class: "dui-pv-pdf-scroll",
                    div {
                        class: "dui-pv-pdf",
                        role: "document",
                        "aria-label": "{name}",
                        tabindex: "0",
                        "data-dui-pdf": "{address}",
                    }
                }
            },
            PreviewKind::Text => rsx! {},
        },
    }
}
