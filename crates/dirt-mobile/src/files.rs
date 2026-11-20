//! What Android does with the open note's files that only it can do
//! (interface design, sections 6.5, 6.16 and 8): attach files chosen on
//! Android's own screen for choosing files, lend one to another app, save
//! a copy where the person says on Android's own screen for that, and
//! answer the web view's requests for the bytes the preview shows.
//!
//! The tiles and the preview are `dirt-ui`'s ([`dirt_ui::files`]), the
//! same as the desktop's.
//!
//! A file is lent to another app as a copy in one folder of the app's own
//! files, the only one its `FileProvider` may lend from, by a `content:`
//! address with a grant to read that one file (`MainActivity.openFile`).
//! Nothing is written where other apps can read it. The copies are removed
//! when the app next starts, and with the account at sign-out.

use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::files::{copy_for_opening, safe_file_name};
use dirt_core::models::NoteId;
use dirt_core::store::AttachedFile;
use dirt_core::sync::worker::Trigger;
use dirt_core::util::as_sentence;
use dirt_ui::components::{Toast, Toasts};
use dirt_ui::files::{served, NoteFiles};
use dirt_ui::Icon;

use crate::config::default_mobile_data_directory;
use crate::incoming::{attach_incoming, Incoming, Picked};
use crate::services::activity::{self, SaveOutcome};
use crate::state::AppState;

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
    // A name for this asking, so that what is chosen comes back to it and
    // to no other: files chosen for one note may still be on their way in
    // when the person chooses again for another.
    let request = activity::new_request();
    if !activity::pick_files(&request) {
        return None;
    }
    // Nothing, if the activity went before the person chose.
    match activity::outcome_of(&request, ASK_EVERY, activity::take_picked).await? {
        Picked::Cancelled => None,
        Picked::Files(files) => Some(files),
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

/// Runs work on files away from the thread that draws the screen.
async fn off_the_screen<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| Err(as_sentence(&error)))
}

/// Lends one of the note's files to another app, which Android lets the
/// person choose: a copy in the one folder the app's file provider lends
/// from, handed over by a `content:` address that grants reading it alone.
/// It is fetched first if the device does not hold it, with the ring on its
/// tile. A file the system would run or install is never offered, and
/// `copy_for_opening` refuses it whatever asks.
pub fn open_in_another_app(files: &NoteFiles, toasts: Toasts, file: AttachedFile) {
    let files = *files;
    spawn(async move {
        let attachment = file.attachment.clone();
        let opened = match files.held(&file).await {
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
        if let Err(why) = opened {
            toasts.show_error(
                Toast::new(format!("{} was not opened", attachment.name))
                    .icon(Icon::Alert)
                    .detail(why),
            );
        }
    });
}

/// Saves a copy of one of the note's files where the person says, on
/// Android's screen for choosing a place, as an export is saved. It is
/// fetched first if the device does not hold it. Backing out says nothing.
pub fn save_a_copy(files: &NoteFiles, toasts: Toasts, file: AttachedFile) {
    let files = *files;
    spawn(async move {
        let attachment = file.attachment.clone();
        // Fetched first: Android's screen is given a file to write.
        let saved = match files.held(&file).await {
            Ok(path) => {
                // This save's own name: its outcome is not another's.
                let request = activity::new_request();
                if activity::save_copy(
                    &request,
                    &safe_file_name(&attachment.name),
                    &attachment.media_type,
                    &path.to_string_lossy(),
                ) {
                    activity::outcome_of(&request, ASK_EVERY, activity::take_save_outcome)
                        .await
                        .ok_or_else(|| {
                            "The app's screen closed before the place was chosen.".to_owned()
                        })
                } else {
                    Err("Android's screen for saving a file could not be opened.".to_owned())
                }
            }
            Err(why) => Err(why),
        };
        let failed = |why: String| {
            toasts.show_error(
                Toast::new(format!("{} was not saved", attachment.name))
                    .icon(Icon::Alert)
                    .detail(why),
            );
        };
        match saved {
            Ok(SaveOutcome::Saved) => {
                toasts.show(Toast::new("Saved a copy").icon(Icon::Download));
            }
            Ok(SaveOutcome::Cancelled) => {}
            Ok(SaveOutcome::Failed(why)) => failed(as_sentence(&why)),
            Err(why) => failed(why),
        }
    });
}

/// Answers the web view's requests for the bytes the preview shows, from
/// memory ([`served`]).
pub fn use_served_files() {
    dioxus::mobile::use_asset_handler(served::ADDRESS, |request, responder| {
        responder.respond(served::response(
            request.uri().path(),
            served::range_of(&request),
        ));
    });
}
