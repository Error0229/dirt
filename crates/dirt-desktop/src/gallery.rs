//! The interface's gallery as a desktop window, for development: every
//! component of `dirt-ui` on one screen. A build with the `gallery`
//! feature shows this in place of the app.
//!
//! The theme, the accent and the text size chosen in it are read from and
//! written to the device's settings, in the store the app itself uses.
//! Point `DIRT_DATA_DIR` at a folder of its own to leave a real
//! installation's settings alone.
//!
//! `DIRT_GALLERY_AT=x,y` puts the window's top left corner there, in
//! screen points. That is for looking at it without it covering what the
//! person at the computer is doing: a place off every screen works, and
//! the window is moved there before it is first shown.

use std::sync::Arc;

use dioxus::desktop::{window, Config, LogicalPosition, LogicalSize, WindowBuilder};
use dioxus::prelude::*;
use dirt_core::models::Settings;
use dirt_core::Store;
use dirt_ui::gallery::Gallery;
use dirt_ui::Appearance;

use crate::services::open_local_store;

/// Opens the gallery's window and runs it until it is closed.
pub fn launch() {
    tracing::info!("Starting the Dirt UI gallery...");
    let window = WindowBuilder::new()
        .with_title("Dirt UI gallery")
        .with_inner_size(LogicalSize::new(1040.0, 760.0))
        .with_always_on_top(false);
    dioxus::LaunchBuilder::new()
        .with_cfg(Config::new().with_window(window).with_menu(None))
        .launch(GalleryApp);
}

/// Where `DIRT_GALLERY_AT` asks for the window, if it does.
fn asked_position() -> Option<(f64, f64)> {
    let asked = std::env::var("DIRT_GALLERY_AT").ok()?;
    let (x, y) = asked.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// The device's settings as they were when the gallery opened, with the
/// store to write them back to. Without a store the gallery still shows;
/// what is chosen in it is then forgotten when it closes.
#[derive(Clone)]
struct Kept {
    store: Option<Arc<Store>>,
    settings: Settings,
}

async fn load() -> Kept {
    let store = match open_local_store() {
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
fn GalleryApp() -> Element {
    // A window cannot be made at a place that is on no screen: the
    // windowing library puts it somewhere visible instead. It can be moved
    // there, and the first render happens before the window is shown.
    use_hook(|| {
        if let Some((x, y)) = asked_position() {
            window().set_outer_position(LogicalPosition::new(x, y));
        }
    });
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
