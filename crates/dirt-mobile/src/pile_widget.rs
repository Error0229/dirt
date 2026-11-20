//! The pile on the home screen (interface design 8.5): what the app gives
//! the pile widget, which Kotlin draws (`PileWidget.kt` in the native
//! module).
//!
//! A home-screen widget cannot run the app's Rust or its web view, and
//! cannot draw SVG. So the heap's grains, where each lies and in which
//! shade, and the line under it are written to a small file in the app's
//! data folder (`dirt/pile`, [`dirt_ui::pile::Heap::widget`]), which the
//! widget reads whenever it is drawn. It is written from the store in use
//! each time the heap is worked out again: as the list is read after a
//! note is made, deleted or brought back, and when the capture sheet's
//! work takes thoughts in. It says how many thoughts there are and nothing
//! of any note, so the widget is as private as the bar.

// The app calls [`show`]; the host tests only [`write_from`].
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use dirt_core::Store;
use dirt_ui::pile::{Heap, PileView, LIST_WIDTH};

/// The widget's file, under the app's data folder.
pub const PILE_FILE: &str = "pile";

/// The line under the heap: "1,254 thoughts", or that there are none yet.
#[must_use]
pub fn line(thoughts: usize) -> String {
    if thoughts == 0 {
        "No thoughts yet".to_owned()
    } else {
        dirt_ui::pile::thoughts(thoughts)
    }
}

/// Writes the widget's file for `heap`, unless it says that already.
/// Written aside and renamed, so the widget never reads half of it.
/// Returns whether it changed.
pub fn write(data_folder: &Path, heap: &Heap) -> std::io::Result<bool> {
    let text = heap.widget(&line(heap.grains));
    let path = data_folder.join(PILE_FILE);
    if std::fs::read_to_string(&path).is_ok_and(|now| now == text) {
        return Ok(false);
    }
    std::fs::create_dir_all(data_folder)?;
    let aside = data_folder.join(format!("{PILE_FILE}.part"));
    std::fs::write(&aside, text)?;
    std::fs::rename(aside, path)?;
    Ok(true)
}

/// The app's list worked the pile out: the widget's file is written if
/// the heap is not the one written last.
pub fn show(data_folder: &Path, pile: &PileView) {
    static LAST: Mutex<Option<Arc<Heap>>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    if last
        .as_ref()
        .is_some_and(|last| Arc::ptr_eq(last, &pile.heap))
    {
        return;
    }
    *last = Some(Arc::clone(&pile.heap));
    drop(last);
    if let Err(error) = write(data_folder, &pile.heap) {
        tracing::warn!("Could not write the pile for the widget: {error}");
    }
}

/// Works out the heap of `store`'s notes and writes the widget's file: for
/// the capture sheet's work, which takes thoughts in with no screen.
/// Returns whether the file changed.
pub async fn write_from(store: &Store, data_folder: &Path) -> dirt_core::Result<bool> {
    let notes = store.pile().await?;
    let heap = tokio::task::spawn_blocking(move || Heap::of(&notes, LIST_WIDTH))
        .await
        .unwrap_or_default();
    Ok(write(data_folder, &heap)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_widget_is_given_the_pile_of_the_store_and_written_once() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("notes.db")).unwrap();
        let data = folder.path().join("dirt");
        assert!(write_from(&store, &data).await.unwrap());
        let empty = std::fs::read_to_string(data.join(PILE_FILE)).unwrap();
        assert_eq!(empty, "dirt-pile 1\nNo thoughts yet\n");
        // Nothing changed: not written again.
        assert!(!write_from(&store, &data).await.unwrap());

        store
            .create_note("A secret about the garden")
            .await
            .unwrap();
        store.create_note("Another").await.unwrap();
        assert!(write_from(&store, &data).await.unwrap());
        let two = std::fs::read_to_string(data.join(PILE_FILE)).unwrap();
        assert!(two.starts_with("dirt-pile 1\n2 thoughts\n"));
        // The grains, never the words.
        assert!(!two.contains("garden"));
    }
}
