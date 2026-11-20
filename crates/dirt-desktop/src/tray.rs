//! The tray icon and its menu (interface design, section 7.5): **New
//! Note**, **Open Dirt** and **Quit**. Closing the window keeps Dirt in the
//! tray, so that the hotkey still works.
//!
//! The icon is the `tray-icon` Dioxus builds with, and what is done with it
//! comes through Dioxus's event loop (`use_tray`).

use std::cell::RefCell;
use std::panic::catch_unwind;

use dioxus::desktop::trayicon::{
    dpi::{PhysicalPosition, PhysicalSize},
    menu::{Menu, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use dioxus::desktop::{use_tray_icon_event_handler, use_tray_menu_event_handler};
use dioxus::prelude::*;

thread_local! {
    /// The icon, on the main thread, which made it and asks where it is.
    static ICON: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

/// What can be chosen in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    NewNote,
    OpenDirt,
    Quit,
}

/// Makes the tray icon and its menu, and has `choose` called with what is
/// chosen in the menu, or with Open Dirt for a double click on the icon.
/// Call from the main window's root, which is there for as long as the app
/// runs.
///
/// Without a tray (some Linux desktops have none) the app runs on without
/// it: this says why and carries on.
pub fn use_tray(choose: Callback<Choice>) {
    use_hook(|| match catch_unwind(start) {
        Ok(Ok(())) => tracing::info!("System tray initialized"),
        Ok(Err(error)) => tracing::error!("Failed to initialize system tray: {error}"),
        // On Linux without an app indicator library, `tray-icon` panics
        // rather than saying so.
        Err(_) => tracing::error!("Failed to initialize system tray: no tray on this desktop"),
    });
    use_tray_menu_event_handler(move |event| {
        let choice = match event.id.0.as_str() {
            "new-note" => Choice::NewNote,
            "open-dirt" => Choice::OpenDirt,
            "quit" => Choice::Quit,
            _ => return,
        };
        choose.call(choice);
    });
    use_tray_icon_event_handler(move |event| {
        if let TrayIconEvent::DoubleClick { .. } = event {
            choose.call(Choice::OpenDirt);
        }
    });
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id("new-note", "New Note\tCtrl+Alt+N", true, None),
        &MenuItem::with_id("open-dirt", "Open Dirt", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", "Quit", true, None),
    ])?;
    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Dirt - Quick Capture")
        .with_icon(create_default_icon()?)
        .with_menu_on_left_click(false)
        .build()?;
    ICON.with(|kept| *kept.borrow_mut() = Some(icon));
    Ok(())
}

/// Where the tray icon is on the screen, in pixels, if the system says:
/// quick capture's grain flies to it. Linux does not say. On the main
/// thread only.
pub fn place() -> Option<(PhysicalPosition<f64>, PhysicalSize<u32>)> {
    ICON.with(|kept| {
        let rect = kept.borrow().as_ref()?.rect()?;
        Some((rect.position, rect.size))
    })
}

/// Create a simple default icon (32x32 colored square)
fn create_default_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    const SIZE: u32 = 32;
    const HALF: u32 = SIZE / 2;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);

    // Create a purple/indigo colored icon (#4f46e5 - the app's accent color)
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Add slight rounded corner effect using unsigned math
            let dx = x.abs_diff(HALF);
            let dy = y.abs_diff(HALF);
            let max_dist = dx.max(dy);

            // Scale: max_dist / HALF, threshold at 0.9 means max_dist < 0.9 * HALF
            if max_dist < (HALF * 9 / 10) {
                // Main color: #4f46e5 (indigo)
                rgba.push(79); // R
                rgba.push(70); // G
                rgba.push(229); // B
                rgba.push(255); // A
            } else {
                // Transparent corners
                rgba.push(0);
                rgba.push(0);
                rgba.push(0);
                rgba.push(0);
            }
        }
    }

    Ok(Icon::from_rgba(rgba, SIZE, SIZE)?)
}
