//! What the desktop does with the open note's files that only it can do
//! (interface design, sections 6.5, 6.16 and 7.3): open one in another
//! app, save a copy where the person says, take files dropped on the
//! window or a picture pasted into the page, and answer the web view's
//! requests for the bytes the preview shows.
//!
//! The tiles and the preview are `dirt-ui`'s ([`dirt_ui::files`]), the
//! same as Android's.

use std::path::PathBuf;

use dioxus::prelude::*;
use dirt_core::files::safe_file_name;
use dirt_core::store::AttachedFile;
use dirt_core::util::as_sentence;
use dirt_ui::components::{Toast, Toasts};
use dirt_ui::files::{served, NoteFiles};
use dirt_ui::Icon;

use crate::services::{copy_for_opening, open_with_system, save_where_asked};
use crate::settings::test_hook;
use crate::state::AppState;

/// Runs work on files away from the thread that draws the window, and
/// waits for it.
///
/// Copying a file takes as long as the disk it goes to takes: a network
/// drive, a stick that is slow or has stalled. Done on the window's own
/// thread it would stop the window drawing and answering for that long.
async fn off_the_window<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| Err(as_sentence(&error)))
}

/// Opens one of the note's files in another app: a copy of it, under the
/// name the note gives it, in a folder beside the store, handed to the
/// system (`services::files`). It is fetched first if the device does not
/// hold it, with the ring on its tile. A file the system would run is
/// never offered.
pub fn open_in_another_app(app: &AppState, files: &NoteFiles, toasts: Toasts, file: AttachedFile) {
    let (app, files) = (*app, *files);
    spawn(async move {
        let attachment = file.attachment.clone();
        let store = app
            .db_service
            .peek()
            .as_ref()
            .map(|store| store.path().to_path_buf());
        let opened = match (files.held(&file).await, store) {
            (Ok(path), Some(store)) => {
                let (file_id, name) = (attachment.file_id, attachment.name.clone());
                off_the_window(move || {
                    copy_for_opening(&store, &path, file_id, &name)
                        .and_then(|copy| open_with_system(&copy))
                        .map_err(|error| as_sentence(&error))
                })
                .await
            }
            (Err(why), _) => Err(why),
            (Ok(_), None) => Err("The note store is not open.".to_owned()),
        };
        if let Err(why) = opened {
            toasts.show_error(
                Toast::new(format!("{} was not opened", attachment.name))
                    .icon(Icon::Alert)
                    .detail(why),
            );
        }
    });
}

/// Saves a copy of one of the note's files where the person says, in the
/// system's save dialog. It is fetched first if the device does not hold
/// it. Nothing chosen is nothing done.
pub fn save_a_copy(files: &NoteFiles, toasts: Toasts, file: AttachedFile) {
    let files = *files;
    spawn(async move {
        let attachment = file.attachment.clone();
        let name = safe_file_name(&attachment.name);
        // Asked where first: fetching a file nobody then saves is waste.
        let target: PathBuf = if let Some(folder) = test_hook("DIRT_EXPORT_TO") {
            folder.join(&name)
        } else {
            let chosen = rfd::AsyncFileDialog::new()
                .set_title("Save a copy of the file")
                .set_file_name(&name)
                .save_file()
                .await;
            let Some(chosen) = chosen else {
                return;
            };
            chosen.path().to_path_buf()
        };
        // If something is there, the dialog has just asked about replacing
        // it. If nothing is, nobody agreed to replace anything, whatever
        // turns up while the file is fetched.
        let looked = target.clone();
        let was_there = off_the_window(move || Ok(looked.exists()))
            .await
            .unwrap_or(false);
        let written = target.clone();
        let saved = match files.held(&file).await {
            Ok(path) => off_the_window(move || save_where_asked(&path, &written, was_there)).await,
            Err(why) => Err(why),
        };
        match saved {
            Ok(()) => {
                let folder = target
                    .parent()
                    .and_then(|folder| folder.file_name())
                    .map(|folder| folder.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let words = if folder.is_empty() {
                    "Saved a copy".to_owned()
                } else {
                    format!("Saved a copy to {folder}")
                };
                toasts.show(Toast::new(words).icon(Icon::Download));
            }
            Err(why) => toasts.show_error(
                Toast::new(format!("{} was not saved", attachment.name))
                    .icon(Icon::Alert)
                    .detail(why),
            ),
        }
    });
}

/// Files dropped on the window, as the web view gives them.
///
/// In a development build with `DIRT_DROP` set, a drop takes its files
/// from the file that names, a path on each line, read at each drop: a
/// drop made by a script, to try the app without a hand on the mouse,
/// carries no files of the computer's.
pub fn dropped(event: &Event<DragData>) -> Vec<PathBuf> {
    if let Some(list) = test_hook("DIRT_DROP") {
        return std::fs::read_to_string(list)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    event
        .data_transfer()
        .files()
        .iter()
        .map(dioxus::html::FileData::path)
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

/// Answers the web view's requests for the bytes the preview shows, from
/// memory ([`served`]).
pub fn use_served_files() {
    dioxus::desktop::use_asset_handler(served::ADDRESS, |request, responder| {
        responder.respond(served::response(
            request.uri().path(),
            served::range_of(&request),
        ));
    });
}
