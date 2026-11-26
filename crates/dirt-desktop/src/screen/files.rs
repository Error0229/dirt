//! What the desktop does with the open note's files that only it can do
//! (interface design, sections 6.5, 6.16 and 7.3): open one in another
//! app, save a copy where the person says, take files dropped on the
//! window or a picture pasted into the page, and answer the web view's
//! requests for the bytes the preview shows.
//!
//! The tiles and the preview are `dirt-ui`'s ([`dirt_ui::files`]), the
//! same as Android's.

use std::path::PathBuf;

use dioxus::html::HasFileData;
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
    paths_dropped(event)
}

/// Where the files of a drop are on the disk.
///
/// They are the paths the system gave the window (wry's drag and drop
/// handler), which Dioxus hands over as the event's files
/// ([`HasFileData`]), not the files of the page's own drop
/// (`data_transfer()`). On Windows the web view hears no drop of files
/// from outside while that handler is on: Dioxus plays one on the page
/// itself, whose only file is a stand-in, "file.txt", with nothing in
/// it, and gives a file of the page a path only where the name of a real
/// one ends in it (`dioxus-desktop`, `webview.rs`). Taken from the page,
/// the stand-in was opened beside the app's working folder, and failed.
/// Elsewhere the page's own drop is heard, and each of its files is given
/// the first real path that ends in its name: two files of one name from
/// different folders both became the first.
///
/// The paths the system gave are those of the last drag of files that
/// came into the window, so a drop on the page that carries no files (a
/// selection of text dragged within it) takes none of them.
fn paths_dropped(event: &DragData) -> Vec<PathBuf> {
    if event.data_transfer().files().is_empty() {
        return Vec::new();
    }
    event
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::{MouseButton, MouseButtonSet};
    use dioxus::html::point_interaction::{
        InteractionElementOffset, InteractionLocation, ModifiersInteraction, PointerInteraction,
        SerializedPointInteraction,
    };
    use dioxus::html::{
        DataTransfer, DragData, FileData, HasDataTransferData, HasDragData, HasFileData,
        HasMouseData, Modifiers, SerializedDataTransfer, SerializedFileData,
    };

    use super::paths_dropped;

    /// A drop as `dioxus-desktop` makes it (its `DesktopFileDragEvent`):
    /// the page's drop as the page told it, and the paths the system gave
    /// the window as the event's files.
    struct Dropped {
        mouse: SerializedPointInteraction,
        page: SerializedDataTransfer,
        system: Vec<PathBuf>,
    }

    fn file(path: &str) -> SerializedFileData {
        SerializedFileData {
            path: PathBuf::from(path),
            ..SerializedFileData::empty()
        }
    }

    fn drop_of(page: &[&str], system: &[&str]) -> DragData {
        DragData::new(Dropped {
            mouse: SerializedPointInteraction::default(),
            page: SerializedDataTransfer {
                items: Vec::new(),
                files: page.iter().copied().map(file).collect(),
                effect_allowed: "all".to_owned(),
                drop_effect: "none".to_owned(),
            },
            system: system.iter().map(PathBuf::from).collect(),
        })
    }

    impl HasFileData for Dropped {
        fn files(&self) -> Vec<FileData> {
            self.system
                .iter()
                .map(|path| FileData::new(file(&path.to_string_lossy())))
                .collect()
        }
    }

    impl HasDataTransferData for Dropped {
        fn data_transfer(&self) -> DataTransfer {
            DataTransfer::new(self.page.clone())
        }
    }

    impl HasDragData for Dropped {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl HasMouseData for Dropped {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl InteractionLocation for Dropped {
        fn client_coordinates(&self) -> ClientPoint {
            self.mouse.client_coordinates()
        }

        fn page_coordinates(&self) -> PagePoint {
            self.mouse.page_coordinates()
        }

        fn screen_coordinates(&self) -> ScreenPoint {
            self.mouse.screen_coordinates()
        }
    }

    impl InteractionElementOffset for Dropped {
        fn element_coordinates(&self) -> ElementPoint {
            self.mouse.element_coordinates()
        }

        fn coordinates(&self) -> Coordinates {
            self.mouse.coordinates()
        }
    }

    impl ModifiersInteraction for Dropped {
        fn modifiers(&self) -> Modifiers {
            self.mouse.modifiers()
        }
    }

    impl PointerInteraction for Dropped {
        fn held_buttons(&self) -> MouseButtonSet {
            self.mouse.held_buttons()
        }

        fn trigger_button(&self) -> Option<MouseButton> {
            self.mouse.trigger_button()
        }
    }

    #[test]
    fn a_drop_on_windows_takes_the_systems_paths_not_the_pages_stand_in() {
        // Windows: the page's drop is played by Dioxus with one stand-in,
        // whatever was dropped (D18).
        let dropped = drop_of(
            &["file.txt"],
            &[r"C:\Papers\plan.pdf", r"C:\Pictures\cat.png"],
        );
        assert_eq!(
            paths_dropped(&dropped),
            vec![
                PathBuf::from(r"C:\Papers\plan.pdf"),
                PathBuf::from(r"C:\Pictures\cat.png"),
            ]
        );
    }

    #[test]
    fn a_drop_elsewhere_keeps_two_files_of_one_name_apart() {
        // Linux and macOS: the page's drop is the web view's own, and
        // Dioxus gives each of its files the first real path ending in its
        // name, so two of one name both carry the first folder's.
        let dropped = drop_of(
            &["/srv/papers/plan.pdf", "/srv/papers/plan.pdf"],
            &["/srv/papers/plan.pdf", "/srv/old/plan.pdf"],
        );
        assert_eq!(
            paths_dropped(&dropped),
            vec![
                PathBuf::from("/srv/papers/plan.pdf"),
                PathBuf::from("/srv/old/plan.pdf"),
            ]
        );
    }

    #[test]
    fn a_drop_that_carries_no_files_takes_none_of_the_last_drags() {
        // Text dragged within the page, after files were dropped before.
        let dropped = drop_of(&[], &[r"C:\Papers\plan.pdf"]);
        assert!(paths_dropped(&dropped).is_empty());
    }

    #[test]
    fn a_path_the_system_left_empty_is_not_taken() {
        let dropped = drop_of(&["file.txt"], &[""]);
        assert!(paths_dropped(&dropped).is_empty());
    }
}
