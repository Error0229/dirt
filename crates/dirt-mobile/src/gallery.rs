//! The interface's gallery on Android, for development: every component
//! of `dirt-ui` on one screen. A build with the `gallery` feature shows
//! this in place of the app.
//!
//! The theme, the accent and the text size chosen in it are read from and
//! written to the device's settings, in the store the app itself uses.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::models::Settings;
use dirt_core::store::StoreName;
use dirt_core::Store;
use dirt_ui::gallery::Gallery;
use dirt_ui::Appearance;

use crate::config::default_mobile_data_directory;

/// The device's settings as they were when the gallery opened, with the
/// store to write them back to. Without a store the gallery still shows;
/// what is chosen in it is then forgotten when it closes.
#[derive(Clone)]
struct Kept {
    store: Option<Arc<Store>>,
    settings: Settings,
}

async fn load() -> Kept {
    let store = match Store::open_in(&default_mobile_data_directory(), &StoreName::Local) {
        Ok(store) => Arc::new(store),
        Err(error) => {
            tracing::warn!("The gallery could not open the device's settings: {error}");
            return Kept {
                store: None,
                settings: Settings::default(),
            };
        }
    };
    let settings = store.load_settings().await.unwrap_or_else(|error| {
        tracing::warn!("The gallery could not read the device's settings: {error}");
        Settings::default()
    });
    Kept {
        store: Some(store),
        settings,
    }
}

#[component]
pub fn GalleryApp() -> Element {
    let kept = use_resource(load);
    let Some(kept) = kept.read().clone() else {
        return rsx! {};
    };
    let initial = Appearance::from_settings(&kept.settings);
    rsx! {
        Gallery {
            initial,
            on_appearance: move |chosen: Appearance| {
                let Some(store) = kept.store.clone() else {
                    return;
                };
                spawn(async move {
                    // Read again, so that a setting the gallery does not
                    // show is written back as it is now.
                    let mut settings = store.load_settings().await.unwrap_or_default();
                    chosen.write_to(&mut settings);
                    if let Err(error) = store.save_settings(&settings).await {
                        tracing::warn!("The gallery could not keep the appearance: {error}");
                    }
                });
            },
        }
    }
}
