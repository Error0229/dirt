//! The files attached to the note that is open: a row for each, under the
//! editor, with what can be done with it.
//!
//! A file is attached from a file dialog, or by dropping it on the editor.
//! A picture shows its thumbnail. A file is opened by handing a copy of it
//! to the system, saved as a copy where the person says, or removed from
//! the note. A file the system would run and not show can be saved and not
//! opened.
//!
//! Where a file stands, and why one could not be had, is said in the words
//! `dirt-core` gives every client.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use dioxus::prelude::*;
use dirt_core::files::thumbnail::as_data_uri;
use dirt_core::files::{media_type_for, runs_as_a_program, safe_file_name, save_copy};
use dirt_core::store::AttachedFile;
use dirt_core::sync::engine::{file_on_device, FileError};
use dirt_core::sync::overview::size_words;
use dirt_core::util::as_sentence;
use dirt_core::NoteId;
use uuid::Uuid;

use crate::components::button::{Button, ButtonVariant};
use crate::services::{copy_for_opening, open_with_system};
use crate::state::AppState;

/// The buttons here are smaller than the app's others: a row has three, and
/// they have to leave room for the file's name.
const SMALL_BUTTON: &str = "font-size: 12px; padding: 4px 10px; min-height: 0; height: auto;";

/// A thumbnail larger than this is not shown. One made by Dirt is a few
/// tens of kilobytes; a note may name any file as a thumbnail.
const MOST_THUMBNAIL_BYTES: u64 = 512 * 1024;

/// Where one of the note's files is on this device, fetching it first if
/// the device does not hold it and is signed in.
async fn held(state: &AppState, note: NoteId, file: Uuid) -> Result<PathBuf, String> {
    let session = state.session.peek().clone();
    let can_fetch = session.is_some();
    let found = match session {
        Some(session) => session.fetch_file(&note, file).await,
        None => match state.db_service.peek().clone() {
            Some(store) => file_on_device(&store, &note, file).await,
            None => Err(FileError::NotAttached),
        },
    };
    found.map_err(|error| error.advice(can_fetch))
}

/// The note's files, as the store has them. Nothing is fetched for this.
async fn files_of(state: &AppState, note: NoteId) -> Vec<AttachedFile> {
    match state.db_service.peek().clone() {
        Some(store) => store.attached_files(&note).await.unwrap_or_default(),
        None => Vec::new(),
    }
}

/// A thumbnail this device holds, as something the window can show.
async fn thumbnail_held(state: &AppState, thumbnail: Uuid) -> Option<String> {
    let store = state.db_service.peek().clone()?;
    let picture = store
        .small_file(thumbnail, MOST_THUMBNAIL_BYTES)
        .await
        .ok()??;
    Some(as_data_uri(&picture))
}

/// The files of the open note, and the ways to add one.
///
/// `dropped` is the files dropped on the editor since they were last
/// taken: the editor owns the place a file is dropped on, which is all of
/// it, and this component attaches them.
#[component]
pub fn AttachmentPanel(note_id: NoteId, dropped: Signal<Vec<PathBuf>>) -> Element {
    let state = use_context::<AppState>();
    let colors = (state.theme)().palette();

    // Raised by whatever changes the note's files here, to read them again.
    let mut changed = use_signal(|| 0u64);
    let mut busy = use_signal(|| None::<String>);
    let mut message = use_signal(|| None::<String>);

    // Read again when the note changes, when a file is added or removed
    // here, and when a sync has brought something or sent something.
    // Reading the list fetches nothing, so the rows are there at once.
    let listed = use_resource(move || {
        let _ = changed();
        let sync = (state.sync_status)();
        let _ = (sync.pulled, sync.files_unsent, sync.syncing);
        async move { files_of(&state, note_id).await }
    });

    // The thumbnails come after the rows, each as it is had. One this
    // device holds is read from it. One it does not hold is fetched, which
    // may take long or fail: the row shows the file's kind meanwhile, and
    // everything else about the file works without it. A fetch is tried
    // once for as long as the note is open.
    let mut thumbnails = use_signal(HashMap::<Uuid, String>::new);
    let mut asked = use_signal(HashSet::<Uuid>::new);
    use_effect(move || {
        let Some(files) = listed() else {
            return;
        };
        spawn(async move {
            let wanted: Vec<(Uuid, Uuid, bool)> = files
                .iter()
                .filter_map(|file| {
                    let thumbnail = file.attachment.thumbnail.as_ref()?;
                    Some((
                        file.attachment.file_id,
                        thumbnail.file_id,
                        file.thumbnail_on_device,
                    ))
                })
                .filter(|(file, _, _)| !thumbnails.peek().contains_key(file))
                .collect();
            for (file, thumbnail, _) in wanted.iter().filter(|(_, _, here)| *here) {
                if let Some(picture) = thumbnail_held(&state, *thumbnail).await {
                    thumbnails.write().insert(*file, picture);
                }
            }
            for (file, thumbnail, _) in wanted.iter().filter(|(_, _, here)| !*here) {
                if !asked.write().insert(*thumbnail) {
                    continue;
                }
                if held(&state, note_id, *thumbnail).await.is_ok() {
                    if let Some(picture) = thumbnail_held(&state, *thumbnail).await {
                        thumbnails.write().insert(*file, picture);
                    }
                }
            }
        });
    });

    let attach = move |paths: Vec<PathBuf>| {
        if paths.is_empty() {
            return;
        }
        let Some(store) = state.db_service.peek().clone() else {
            return;
        };
        spawn(async move {
            busy.set(Some("Attaching...".to_owned()));
            message.set(None);
            let mut failed = Vec::new();
            for path in paths {
                let given = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // The name goes into the note as a name, never as a path.
                let name = safe_file_name(&given);
                let attached = store
                    .attach_file(&note_id, &path, &name, media_type_for(&name))
                    .await;
                if let Err(error) = attached {
                    failed.push(format!("{name}: {}", as_sentence(&error)));
                }
            }
            busy.set(None);
            if !failed.is_empty() {
                message.set(Some(format!("Not attached. {}", failed.join(" "))));
            }
            state.note_changed();
            changed.set(changed() + 1);
        });
    };

    // Files dropped on the editor.
    use_effect(move || {
        let paths = dropped();
        if !paths.is_empty() {
            dropped.set(Vec::new());
            attach(paths);
        }
    });

    let choose = move |_| {
        spawn(async move {
            let chosen = rfd::AsyncFileDialog::new()
                .set_title("Attach files to the note")
                .pick_files()
                .await;
            if let Some(chosen) = chosen {
                attach(
                    chosen
                        .iter()
                        .map(|file| file.path().to_path_buf())
                        .collect(),
                );
            }
        });
    };

    let open = move |file: AttachedFile| {
        spawn(async move {
            let attachment = file.attachment;
            busy.set(Some(format!("Opening {}...", attachment.name)));
            message.set(None);
            let store = state
                .db_service
                .peek()
                .as_ref()
                .map(|store| store.path().to_path_buf());
            let opened = match (held(&state, note_id, attachment.file_id).await, store) {
                (Ok(path), Some(store)) => {
                    copy_for_opening(&store, &path, attachment.file_id, &attachment.name)
                        .and_then(|copy| open_with_system(&copy))
                        .map_err(|error| as_sentence(&error))
                }
                (Err(why), _) => Err(why),
                (Ok(_), None) => Err("The note store is not open.".to_owned()),
            };
            busy.set(None);
            if let Err(why) = opened {
                message.set(Some(format!("{} was not opened. {why}", attachment.name)));
            }
            // It may have been fetched: the row no longer says it is not here.
            changed.set(changed() + 1);
        });
    };

    let save = move |file: AttachedFile| {
        spawn(async move {
            let attachment = file.attachment;
            // Asked where first: fetching a file nobody then saves is waste.
            let chosen = rfd::AsyncFileDialog::new()
                .set_title("Save a copy of the file")
                .set_file_name(safe_file_name(&attachment.name))
                .save_file()
                .await;
            let Some(chosen) = chosen else {
                return;
            };
            busy.set(Some(format!("Saving {}...", attachment.name)));
            message.set(None);
            let saved = match held(&state, note_id, attachment.file_id).await {
                // The dialog has already asked about a file that is there.
                Ok(path) => {
                    save_copy(&path, chosen.path(), true).map_err(|error| as_sentence(&error))
                }
                Err(why) => Err(why),
            };
            busy.set(None);
            message.set(Some(match saved {
                Ok(()) => format!("Saved to {}.", chosen.path().display()),
                Err(why) => format!("{} was not saved. {why}", attachment.name),
            }));
            changed.set(changed() + 1);
        });
    };

    let remove = move |file: AttachedFile| {
        let Some(store) = state.db_service.peek().clone() else {
            return;
        };
        spawn(async move {
            message.set(None);
            if let Err(error) = store.detach_file(&note_id, file.attachment.file_id).await {
                message.set(Some(format!(
                    "{} was not removed. {}",
                    file.attachment.name,
                    as_sentence(&error)
                )));
            }
            state.note_changed();
            changed.set(changed() + 1);
        });
    };

    let files = listed().unwrap_or_default();
    let thumbnails = thumbnails();
    let working = busy();

    rsx! {
        div {
            class: "attachments",
            style: "
                border-top: 1px solid {colors.border_light};
                padding: 10px 32px 14px;
                display: flex;
                flex-direction: column;
                gap: 8px;
                max-height: 40%;
                overflow-y: auto;
            ",

            div {
                style: "display: flex; align-items: center; gap: 12px;",
                Button {
                    variant: ButtonVariant::Secondary,
                    style: "{SMALL_BUTTON}",
                    disabled: working.is_some(),
                    onclick: choose,
                    "Attach a file"
                }
                span {
                    style: "font-size: 12px; color: {colors.text_muted};",
                    if let Some(doing) = working {
                        "{doing}"
                    } else if files.is_empty() {
                        "or drop one on the note"
                    }
                }
            }

            if let Some(text) = message() {
                div {
                    class: "attachments-message",
                    role: "status",
                    style: "font-size: 12px; color: {colors.text_secondary};",
                    "{text}"
                }
            }

            for file in files {
                AttachmentRow {
                    key: "{file.attachment.file_id}",
                    thumbnail: thumbnails.get(&file.attachment.file_id).cloned(),
                    file: file.clone(),
                    on_open: open,
                    on_save: save,
                    on_remove: remove,
                }
            }
        }
    }
}

/// One file: its thumbnail or its kind, its name, its size and where it
/// stands, and what can be done with it.
#[component]
fn AttachmentRow(
    file: AttachedFile,
    thumbnail: Option<String>,
    on_open: EventHandler<AttachedFile>,
    on_save: EventHandler<AttachedFile>,
    on_remove: EventHandler<AttachedFile>,
) -> Element {
    let state = use_context::<AppState>();
    let colors = (state.theme)().palette();
    let attachment = &file.attachment;
    let name = attachment.name.clone();
    let mut details = format!(
        "{} · {}",
        size_words(attachment.size),
        attachment.media_type
    );
    if let Some(standing) = file.standing_words() {
        details.push_str(" · ");
        details.push_str(&standing);
    }
    // A file the system would run is saved, and opened from where it was
    // saved by somebody who means to.
    let can_open = !runs_as_a_program(&name);
    let kind = name
        .rsplit_once('.')
        .map_or_else(|| "file".to_owned(), |(_, ending)| ending.to_lowercase());
    let (for_open, for_save, for_remove) = (file.clone(), file.clone(), file.clone());

    rsx! {
        div {
            class: "attachment",
            style: "display: flex; align-items: center; gap: 12px; min-width: 0;",

            if let Some(picture) = thumbnail {
                img {
                    src: "{picture}",
                    alt: "",
                    style: "
                        width: 44px;
                        height: 44px;
                        object-fit: cover;
                        border-radius: 4px;
                        border: 1px solid {colors.border_light};
                        flex-shrink: 0;
                    ",
                }
            } else {
                div {
                    style: "
                        width: 44px;
                        height: 44px;
                        border-radius: 4px;
                        border: 1px solid {colors.border_light};
                        background: {colors.bg_secondary};
                        color: {colors.text_muted};
                        font-size: 10px;
                        display: flex;
                        align-items: center;
                        justify-content: center;
                        overflow: hidden;
                        flex-shrink: 0;
                    ",
                    "{kind}"
                }
            }

            div {
                style: "flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px;",
                div {
                    class: "attachment-name",
                    style: "
                        font-size: 13px;
                        color: {colors.text_primary};
                        white-space: nowrap;
                        overflow: hidden;
                        text-overflow: ellipsis;
                    ",
                    title: "{name}",
                    "{name}"
                }
                div {
                    class: "attachment-details",
                    style: "font-size: 11px; color: {colors.text_muted};",
                    "{details}"
                }
            }

            div {
                style: "display: flex; gap: 2px; flex-shrink: 0;",
                if can_open {
                    Button {
                        variant: ButtonVariant::Ghost,
                        style: "{SMALL_BUTTON}",
                        aria_label: "Open {name}",
                        onclick: move |_| on_open.call(for_open.clone()),
                        "Open"
                    }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    style: "{SMALL_BUTTON}",
                    aria_label: "Save a copy of {name}",
                    title: if can_open { "" } else { "This kind of file can run as a program. Save it, and open it yourself if you trust it." },
                    onclick: move |_| on_save.call(for_save.clone()),
                    "Save a copy"
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    style: "{SMALL_BUTTON}",
                    aria_label: "Remove {name} from the note",
                    onclick: move |_| on_remove.call(for_remove.clone()),
                    "Remove"
                }
            }
        }
    }
}
