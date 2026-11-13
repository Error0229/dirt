//! The files attached to the note that is open: a row for each, under the
//! text, with what can be done with it.
//!
//! A file is attached from Android's own screen for choosing files. A
//! picture shows its thumbnail. A file is opened by lending a copy of it to
//! another app, saved as a copy where the person says, or removed from the
//! note. A file the system would run or install is saved and not opened.
//!
//! Where a file stands, and why one could not be had, is said in the words
//! `dirt-core` gives every client.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::files::thumbnail::as_data_uri;
use dirt_core::files::{copy_for_opening, runs_as_a_program, safe_file_name};
use dirt_core::models::NoteId;
use dirt_core::store::AttachedFile;
use dirt_core::sync::engine::{file_on_device, FileError};
use dirt_core::sync::overview::size_words;
use dirt_core::sync::worker::Trigger;
use dirt_core::util::as_sentence;
use uuid::Uuid;

use crate::config::default_mobile_data_directory;
use crate::incoming::{attach_incoming, note_text, Incoming, Picked};
use crate::services::activity::{self, SaveOutcome};
use crate::state::AppState;
use crate::ui::{ButtonVariant, UiButton};

const TEXT: &str = "margin: 0; font-size: 12px; color: #6b7280; line-height: 1.4;";

/// A thumbnail larger than this is not shown. One made by Dirt is a few
/// tens of kilobytes; a note may name any file as a thumbnail.
const MOST_THUMBNAIL_BYTES: u64 = 512 * 1024;

/// How often the activity is asked whether the person has finished on one
/// of Android's own screens.
const ASK_EVERY: Duration = Duration::from_millis(300);

/// The folder a file is copied into to be opened by another app. It is the
/// one folder the app's file provider may lend from (`build.rs`).
pub fn opened_folder() -> PathBuf {
    default_mobile_data_directory().join("opened")
}

/// Asks the person which files to attach, on Android's screen for that,
/// and waits for them. `None` if they chose none.
pub async fn choose_files() -> Option<Vec<Incoming>> {
    if !activity::pick_files() {
        return None;
    }
    loop {
        if let Some(picked) = activity::take_picked() {
            return match picked {
                Picked::Cancelled => None,
                Picked::Files(files) => Some(files),
            };
        }
        tokio::time::sleep(ASK_EVERY).await;
    }
}

/// The note files are being chosen for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A note that is saved.
    Note(NoteId),
    /// A note that is not saved yet, with the text written so far. It is
    /// saved first, with that text or, if there is none, called after the
    /// files.
    New { text: String },
}

/// What came of choosing files for a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    /// The note the files went to, with its text: the one that was open,
    /// or the one that was made. `None` if no note could be made.
    pub note: Option<(NoteId, String)>,
    /// A line for each file that could not be attached.
    pub failed: Vec<String>,
}

/// Asks the person which files to attach and attaches them, and says how
/// it went: `None` if they chose none.
///
/// The whole of it runs apart from the screen that asked. Between the
/// person choosing and the files being attached the activity copies them
/// into the app, which takes a moment for a photograph, and the person may
/// leave the note meanwhile. Files that were chosen are attached all the
/// same, to the note they were chosen for. The answer comes back for the
/// screen to show, if it is still there to hear it.
pub fn choose_and_attach(
    mut state: AppState,
    target: Target,
) -> tokio::sync::oneshot::Receiver<Option<Attached>> {
    let (answer, heard) = tokio::sync::oneshot::channel();
    dioxus::core::spawn_forever(async move {
        let attached = match choose_files().await {
            None => None,
            Some(files) => Some(attach_chosen(&mut state, target, files).await),
        };
        // The screen that asked may be gone. That is no failure.
        let _ = answer.send(attached);
    });
    heard
}

async fn attach_chosen(state: &mut AppState, target: Target, files: Vec<Incoming>) -> Attached {
    let Some(store) = state.store.peek().clone() else {
        crate::incoming::discard_all(&files);
        return Attached {
            note: None,
            failed: vec!["The notes are not open.".to_owned()],
        };
    };
    let (note, text, made) = match target {
        Target::Note(note) => (note, String::new(), false),
        Target::New { text } => match store.create_note(&note_text(&text, &files)).await {
            Ok(note) => (note.id, note.content, true),
            Err(error) => {
                crate::incoming::discard_all(&files);
                return Attached {
                    note: None,
                    failed: vec![format!("The note could not be saved: {error}")],
                };
            }
        },
    };
    let failed = attach(state, note, files).await;
    if made {
        // A new note belongs in the list whether or not its editor is
        // still open. The list is the app's, not the editor's.
        let listed = *state.listed_notes.peek();
        if let Ok(notes) = store.list_notes(listed).await {
            state.notes.set(notes);
        }
    }
    Attached {
        note: Some((note, text)),
        failed,
    }
}

/// Attaches files to a note as a task of its own, which finishes whatever
/// the screen does meanwhile, and tells the sync worker when it has.
/// Returns a line for each file that could not be attached.
pub async fn attach(state: &AppState, note: NoteId, files: Vec<Incoming>) -> Vec<String> {
    let Some(store) = state.store.peek().clone() else {
        crate::incoming::discard_all(&files);
        return vec!["The notes are not open.".to_owned()];
    };
    let session = state.session.peek().clone();
    let core = store.core().clone();
    tokio::spawn(async move {
        let failed = attach_incoming(core, note, files).await;
        if let Some(session) = session {
            session.notify(Trigger::Changed);
        }
        failed
    })
    .await
    .unwrap_or_default()
}

/// Where one of the note's files is on this device, fetching it first if
/// the device does not hold it and is signed in.
async fn held(state: &AppState, note: NoteId, file: Uuid) -> Result<PathBuf, String> {
    let session = state.session.peek().clone();
    let can_fetch = session.is_some();
    let found = match session {
        Some(session) => session.fetch_file(&note, file).await,
        None => match state.store.peek().clone() {
            Some(store) => file_on_device(store.core(), &note, file).await,
            None => Err(FileError::NotAttached),
        },
    };
    found.map_err(|error| error.advice(can_fetch))
}

/// Runs work on files away from the thread that draws the screen.
async fn off_the_screen<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| Err(as_sentence(&error)))
}

/// A thumbnail this device holds, as something the screen can show.
async fn thumbnail_held(state: &AppState, thumbnail: Uuid) -> Option<String> {
    let store = state.store.peek().clone()?;
    let picture = store
        .core()
        .small_file(thumbnail, MOST_THUMBNAIL_BYTES)
        .await
        .ok()??;
    Some(as_data_uri(&picture))
}

/// The files of the open note, and the way to add one.
#[component]
pub fn AttachmentPanel(note_id: NoteId) -> Element {
    let state = use_context::<AppState>();

    // Raised by whatever changes the note's files here, to read them again.
    let mut changed = use_signal(|| 0u64);
    let mut busy = use_signal(|| None::<String>);
    let mut message = use_signal(|| None::<String>);

    // Read again when a file is added or removed here, and when a sync has
    // brought something or sent something. Reading the list fetches
    // nothing, so the rows are there at once.
    let listed = use_resource(move || {
        let _ = changed();
        let sync = (state.sync_status)();
        let _ = (sync.pulled, sync.files_unsent, sync.syncing);
        async move {
            match state.store.peek().clone() {
                Some(store) => store
                    .core()
                    .attached_files(&note_id)
                    .await
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        }
    });

    // The thumbnails come after the rows, each as it is had. A fetch is
    // tried once for as long as the note is open.
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

    let choose = move |_| {
        // Chosen and attached apart from this panel, which only shows how
        // it went: leaving the note must not drop files already chosen.
        let attaching = choose_and_attach(state, Target::Note(note_id));
        spawn(async move {
            busy.set(Some("Attaching...".to_owned()));
            message.set(None);
            let attached = attaching.await.ok().flatten();
            busy.set(None);
            if let Some(attached) = attached {
                if !attached.failed.is_empty() {
                    message.set(Some(attached.failed.join(" ")));
                }
            }
            changed.set(changed() + 1);
        });
    };

    let open = move |file: AttachedFile| {
        spawn(async move {
            let attachment = file.attachment;
            busy.set(Some(format!("Opening {}...", attachment.name)));
            message.set(None);
            let opened = match held(&state, note_id, attachment.file_id).await {
                Ok(path) => {
                    let (file_id, name) = (attachment.file_id, attachment.name.clone());
                    off_the_screen(move || {
                        copy_for_opening(&opened_folder(), &path, file_id, &name)
                            .map_err(|error| as_sentence(&error))
                    })
                    .await
                }
                Err(why) => Err(why),
            };
            let opened = opened.and_then(|copy| {
                if activity::open_file(&copy.to_string_lossy(), &attachment.media_type) {
                    Ok(())
                } else {
                    Err("No app on this device opens files of this kind.".to_owned())
                }
            });
            busy.set(None);
            if let Err(why) = opened {
                message.set(Some(format!("{} was not opened. {why}", attachment.name)));
            }
            changed.set(changed() + 1);
        });
    };

    let save = move |file: AttachedFile| {
        spawn(async move {
            let attachment = file.attachment;
            busy.set(Some(format!("Saving {}...", attachment.name)));
            message.set(None);
            // Fetched first: Android's screen is given a file to write.
            let saved = match held(&state, note_id, attachment.file_id).await {
                Ok(path) => {
                    let name = safe_file_name(&attachment.name);
                    if activity::save_copy(&name, &attachment.media_type, &path.to_string_lossy()) {
                        loop {
                            if let Some(outcome) = activity::take_save_outcome() {
                                break Ok(outcome);
                            }
                            tokio::time::sleep(ASK_EVERY).await;
                        }
                    } else {
                        Err("Android's screen for saving a file could not be opened.".to_owned())
                    }
                }
                Err(why) => Err(why),
            };
            busy.set(None);
            message.set(match saved {
                Ok(SaveOutcome::Saved) => Some(format!("Saved a copy of {}.", attachment.name)),
                Ok(SaveOutcome::Cancelled) => None,
                Ok(SaveOutcome::Failed(why)) => Some(format!(
                    "{} was not saved: {}",
                    attachment.name,
                    as_sentence(&why)
                )),
                Err(why) => Some(format!("{} was not saved. {why}", attachment.name)),
            });
            changed.set(changed() + 1);
        });
    };

    let remove = move |file: AttachedFile| {
        let Some(store) = state.store.peek().clone() else {
            return;
        };
        spawn(async move {
            message.set(None);
            if let Err(error) = store
                .core()
                .detach_file(&note_id, file.attachment.file_id)
                .await
            {
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
            style: "display: flex; flex-direction: column; gap: 8px;",

            div {
                style: "display: flex; align-items: center; gap: 10px;",
                UiButton {
                    r#type: "button",
                    variant: ButtonVariant::Outline,
                    disabled: working.is_some(),
                    onclick: choose,
                    "Attach a file"
                }
                if let Some(doing) = working {
                    p { style: "{TEXT}", "{doing}" }
                }
            }

            if let Some(text) = message() {
                p { role: "status", style: "{TEXT}", "{text}" }
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
    // A file the system would run or install is saved, and opened from
    // where it was saved by somebody who means to.
    let can_open = !runs_as_a_program(&name);
    let kind = name
        .rsplit_once('.')
        .map_or_else(|| "file".to_owned(), |(_, ending)| ending.to_lowercase());
    let (for_open, for_save, for_remove) = (file.clone(), file.clone(), file.clone());

    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                gap: 6px;
                padding: 8px 10px;
                border: 1px solid #e5e7eb;
                border-radius: 10px;
                background: #ffffff;
            ",
            div {
                style: "display: flex; align-items: center; gap: 10px; min-width: 0;",
                if let Some(picture) = thumbnail {
                    img {
                        src: "{picture}",
                        alt: "",
                        style: "
                            width: 44px;
                            height: 44px;
                            object-fit: cover;
                            border-radius: 6px;
                            flex-shrink: 0;
                        ",
                    }
                } else {
                    div {
                        style: "
                            width: 44px;
                            height: 44px;
                            border-radius: 6px;
                            background: #f3f4f6;
                            color: #6b7280;
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
                    p {
                        style: "
                            margin: 0;
                            font-size: 13px;
                            color: #111827;
                            white-space: nowrap;
                            overflow: hidden;
                            text-overflow: ellipsis;
                        ",
                        "{name}"
                    }
                    p { style: "{TEXT}", "{details}" }
                }
            }
            div {
                style: "display: flex; gap: 6px;",
                if can_open {
                    UiButton {
                        r#type: "button",
                        variant: ButtonVariant::Ghost,
                        onclick: move |_| on_open.call(for_open.clone()),
                        "Open"
                    }
                }
                UiButton {
                    r#type: "button",
                    variant: ButtonVariant::Ghost,
                    onclick: move |_| on_save.call(for_save.clone()),
                    "Save a copy"
                }
                UiButton {
                    r#type: "button",
                    variant: ButtonVariant::Ghost,
                    onclick: move |_| on_remove.call(for_remove.clone()),
                    "Remove"
                }
            }
        }
    }
}
