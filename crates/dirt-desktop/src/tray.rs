//! The tray icon and its menu (interface design, section 7.5): **New
//! Note**, **Open Dirt** and **Quit**. Closing the window keeps Dirt in the
//! tray, so that the hotkey still works.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use tray_icon::{
    dpi::{PhysicalPosition, PhysicalSize},
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

/// Raised for the main window to show itself, and to quit.
pub static SHOW_MAIN_WINDOW: AtomicBool = AtomicBool::new(false);
pub static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The menu's items, set when the icon is made.
static NEW_NOTE_ID: OnceLock<MenuId> = OnceLock::new();
static OPEN_DIRT_ID: OnceLock<MenuId> = OnceLock::new();
static QUIT_ID: OnceLock<MenuId> = OnceLock::new();

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

/// Makes the tray icon and its menu. Call on the main thread, before the
/// app's window is made.
pub fn start() -> Result<(), Box<dyn std::error::Error>> {
    let new_note_item = MenuItem::new("New Note\tCtrl+Alt+N", true, None);
    let open_item = MenuItem::new("Open Dirt", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let _ = NEW_NOTE_ID.set(new_note_item.id().clone());
    let _ = OPEN_DIRT_ID.set(open_item.id().clone());
    let _ = QUIT_ID.set(quit_item.id().clone());

    let menu = Menu::new();
    menu.append_items(&[
        &new_note_item,
        &open_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
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

/// Does what was chosen in the menu. From any thread.
pub fn chosen(choice: Choice) {
    tracing::info!("Tray: {choice:?} chosen");
    match choice {
        Choice::NewNote => crate::capture::link().open(Instant::now()),
        Choice::OpenDirt => SHOW_MAIN_WINDOW.store(true, Ordering::SeqCst),
        Choice::Quit => QUIT_REQUESTED.store(true, Ordering::SeqCst),
    }
}

/// Handles what was chosen in the menu, and a double click on the icon,
/// which opens the main window.
pub fn process_tray_events() {
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        let id = &event.id;
        let choice = if NEW_NOTE_ID.get() == Some(id) {
            Choice::NewNote
        } else if OPEN_DIRT_ID.get() == Some(id) {
            Choice::OpenDirt
        } else if QUIT_ID.get() == Some(id) {
            Choice::Quit
        } else {
            continue;
        };
        chosen(choice);
    }
    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        if let TrayIconEvent::DoubleClick { .. } = event {
            chosen(Choice::OpenDirt);
        }
    }
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
