//! System tray integration
//!
//! Provides system tray icon with menu for quick access to Dirt features.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

/// Atomic flags for tray events - shared with UI
pub static SHOW_MAIN_WINDOW: AtomicBool = AtomicBool::new(false);
pub static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Static menu item IDs (set during initialization)
static NEW_NOTE_ID: OnceLock<MenuId> = OnceLock::new();
static OPEN_DIRT_ID: OnceLock<MenuId> = OnceLock::new();
static QUIT_ID: OnceLock<MenuId> = OnceLock::new();

/// System tray manager
pub struct TrayManager {
    #[allow(dead_code)]
    tray_icon: TrayIcon,
}

impl TrayManager {
    /// Create and initialize the system tray
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Create menu items
        let new_note_item = MenuItem::new("New Note\tCtrl+Alt+N", true, None);
        let open_item = MenuItem::new("Open Dirt", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        // Store IDs in statics for event handling
        let _ = NEW_NOTE_ID.set(new_note_item.id().clone());
        let _ = OPEN_DIRT_ID.set(open_item.id().clone());
        let _ = QUIT_ID.set(quit_item.id().clone());

        // Build menu
        let menu = Menu::new();
        menu.append_items(&[
            &new_note_item,
            &open_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])?;

        // Create a simple icon (32x32 purple square)
        let icon = create_default_icon()?;

        // Build tray icon
        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Dirt - Quick Capture")
            .with_icon(icon)
            .with_menu_on_left_click(false)
            .build()?;

        tracing::info!("System tray initialized");

        Ok(Self { tray_icon })
    }
}

/// Process pending tray events (menu clicks and icon interactions)
#[allow(clippy::cognitive_complexity)]
pub fn process_tray_events() {
    // Process menu events
    let menu_receiver = MenuEvent::receiver();
    while let Ok(event) = menu_receiver.try_recv() {
        let id = &event.id;

        if NEW_NOTE_ID.get().is_some_and(|nid| nid == id) {
            tracing::info!("Tray: New Note clicked");
            crate::HOTKEY_TRIGGERED.store(true, Ordering::SeqCst);
        } else if OPEN_DIRT_ID.get().is_some_and(|oid| oid == id) {
            tracing::info!("Tray: Open Dirt clicked");
            SHOW_MAIN_WINDOW.store(true, Ordering::SeqCst);
        } else if QUIT_ID.get().is_some_and(|qid| qid == id) {
            tracing::info!("Tray: Quit clicked");
            QUIT_REQUESTED.store(true, Ordering::SeqCst);
        }
    }

    // Process tray icon events (double-click to open)
    let icon_receiver = TrayIconEvent::receiver();
    while let Ok(event) = icon_receiver.try_recv() {
        if let TrayIconEvent::DoubleClick { .. } = event {
            tracing::info!("Tray: Double-click - opening main window");
            SHOW_MAIN_WINDOW.store(true, Ordering::SeqCst);
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
