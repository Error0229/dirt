//! The tray icon and its menu (interface design, sections 7.5 and 7.12).
//!
//! A click on the icon opens Dirt on Windows; on a Mac it opens the menu,
//! as every icon in the menu bar does; Linux's trays send no clicks and
//! always open the menu. The menu is the system's own, with **Open Dirt**,
//! **New note** and its hotkey, and **Quit Dirt**. Closing the window keeps
//! Dirt in the tray, so that the hotkey still works.
//!
//! The icon is the logo, in the accent the person chose, and on a Mac the
//! sprout alone, which the system colours as it colours the menu bar.
//!
//! The icon is the `tray-icon` Dioxus builds with, and what is done with it
//! comes through Dioxus's event loop (`use_tray`).

use std::cell::RefCell;
use std::panic::catch_unwind;

use dioxus::desktop::trayicon::{
    dpi::{PhysicalPosition, PhysicalSize},
    menu::{Menu, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use dioxus::desktop::{use_tray_icon_event_handler, use_tray_menu_event_handler};
use dioxus::prelude::*;
use dirt_core::models::Accent;
use dirt_ui::tokens::{accent_colors, Color, Scheme};

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

/// Whether the system shows the menu for a left click as well as a right
/// one. A Mac does, as for every icon in the menu bar; on Windows a left
/// click opens Dirt. Linux's trays show the menu for any click, whatever
/// this says.
const MENU_ON_LEFT_CLICK: bool = cfg!(target_os = "macos");

/// Makes the tray icon and its menu, and has `choose` called with what is
/// chosen in the menu, or with Open Dirt for a click on the icon where a
/// click opens Dirt. Call from the main window's root, which is there for
/// as long as the app runs.
///
/// Without a tray (some Linux desktops have none) the app runs on without
/// it: this says why and carries on.
pub fn use_tray(choose: Callback<Choice>) {
    use_hook(|| match catch_unwind(start) {
        Ok(Ok(())) => tracing::info!("System tray initialized"),
        Ok(Err(error)) => tracing::error!("Failed to initialize system tray: {error}"),
        // On Linux without an app indicator library, `tray-icon` panics
        // rather than saying so. A release build on Windows has no
        // console for the panic's own message, so it is logged here.
        Err(panic) => {
            let why = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("no tray on this desktop");
            tracing::error!("Failed to initialize system tray: {why}");
        }
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
        if let TrayIconEvent::Click {
            button,
            button_state,
            ..
        } = event
        {
            if let Some(choice) = clicked(*button, *button_state) {
                choose.call(choice);
            }
        }
    });
}

/// What a click on the icon chooses: Open Dirt for a left button let go,
/// where the menu is not shown for it. A double click is two of those, and
/// opens Dirt once more, which changes nothing. The right button is the
/// system's, which shows the menu.
const fn clicked(button: MouseButton, state: MouseButtonState) -> Option<Choice> {
    match (button, state) {
        (MouseButton::Left, MouseButtonState::Up) if !MENU_ON_LEFT_CLICK => Some(Choice::OpenDirt),
        _ => None,
    }
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id("open-dirt", "Open Dirt", true, None),
        &new_note(),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", "Quit Dirt", true, None),
    ])?;
    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(TOOLTIP)
        .with_icon(icon(Accent::Clay)?)
        .with_icon_as_template(cfg!(target_os = "macos"))
        .with_menu_on_left_click(MENU_ON_LEFT_CLICK)
        .build()?;
    ICON.with(|kept| *kept.borrow_mut() = Some(icon));
    Ok(())
}

/// What the icon says under the pointer: on Windows, what a click does.
#[cfg(target_os = "windows")]
const TOOLTIP: &str = "Dirt\nClick to open. Ctrl+Alt+N for a new note";
#[cfg(not(target_os = "windows"))]
const TOOLTIP: &str = "Dirt";

/// **New note**, with the hotkey beside it as the system shows a shortcut:
/// "Ctrl+Alt+N" on Windows and "⌥⌘N" on a Mac, made from the hotkey
/// itself. A Linux tray's menu shows no shortcut it is given, so there it
/// is in the words.
fn new_note() -> MenuItem {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        use dioxus::desktop::trayicon::menu::accelerator::Accelerator;
        let hotkey = crate::hotkey::default_hotkey();
        let shortcut = Accelerator::new(Some(hotkey.mods), hotkey.key);
        MenuItem::with_id("new-note", "New note", true, Some(shortcut))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        MenuItem::with_id("new-note", "New note    Ctrl+Alt+N", true, None)
    }
}

/// Draws the icon again in `accent`, the one chosen in Settings. A Mac's
/// icon is the sprout alone, in the menu bar's colour, and is left as it
/// is. On the main thread only.
pub fn show_accent(accent: Accent) {
    if cfg!(target_os = "macos") {
        return;
    }
    ICON.with(|kept| {
        let kept = kept.borrow();
        let Some(tray) = kept.as_ref() else {
            return;
        };
        match icon(accent) {
            Ok(icon) => {
                if let Err(error) = tray.set_icon(Some(icon)) {
                    tracing::warn!("The tray icon could not be drawn again: {error}");
                }
            }
            Err(error) => tracing::warn!("The tray icon could not be drawn: {error}"),
        }
    });
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

/// The icon's width and height in pixels. The system scales it to the
/// tray: 16 px at 100% on Windows, 32 at 200%.
const SIZE: u16 = 32;

/// Samples taken across each pixel, each way, to smooth the edges.
const SAMPLES: u16 = 4;

/// The tray's icon in `accent`, or on a Mac the sprout alone.
fn icon(accent: Accent) -> Result<Icon, Box<dyn std::error::Error>> {
    // The light scheme's accent: the deeper one, with a white sprout,
    // which reads on a light taskbar and a dark one alike.
    let colours = accent_colors(accent, Scheme::Light);
    let on_square = (!cfg!(target_os = "macos")).then_some((colours.accent, colours.on_accent));
    Ok(Icon::from_rgba(
        logo_pixels(on_square),
        u32::from(SIZE),
        u32::from(SIZE),
    )?)
}

/// The logo (`dirt_ui::components::Logo`, the mockup's drawing) as RGBA
/// pixels, `SIZE` square: the sprout in the second colour on a rounded
/// square of the first, or with none the sprout alone in black, whose
/// shape is all a Mac's menu bar takes of it.
fn logo_pixels(on_square: Option<(Color, Color)>) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(usize::from(SIZE) * usize::from(SIZE) * 4);
    let per_pixel = f32::from(SAMPLES * SAMPLES);
    for row in 0..SIZE {
        for column in 0..SIZE {
            // How much of the pixel the square covers, and how much the
            // sprout, summed over its samples.
            let (mut square, mut sprout) = (0.0, 0.0);
            for i in 0..SAMPLES {
                for j in 0..SAMPLES {
                    // The sample's place in the logo's 32 units.
                    let unit = 32.0 / f32::from(SIZE);
                    let x = (f32::from(column) + (f32::from(i) + 0.5) / f32::from(SAMPLES)) * unit;
                    let y = (f32::from(row) + (f32::from(j) + 0.5) / f32::from(SAMPLES)) * unit;
                    if on_square.is_none() || in_square(x, y) {
                        square += 1.0;
                        sprout += sprout_at(x, y);
                    }
                }
            }
            match on_square {
                Some((accent, on_accent)) => {
                    let share = if square > 0.0 { sprout / square } else { 0.0 };
                    let (a, b) = (accent.rgb(), on_accent.rgb());
                    for (from, to) in [(a.0, b.0), (a.1, b.1), (a.2, b.2)] {
                        rgba.push(byte(
                            f32::from(from).mul_add(1.0 - share, f32::from(to) * share) / 255.0,
                        ));
                    }
                    rgba.push(byte(square / per_pixel));
                }
                None => rgba.extend([0, 0, 0, byte(sprout / per_pixel)]),
            }
        }
    }
    rgba
}

/// Whether the point is on the logo's square: 32 units, corners of 9.
fn in_square(x: f32, y: f32) -> bool {
    let (dx, dy) = ((x - 16.0).abs() - 7.0, (y - 16.0).abs() - 7.0);
    dx <= 0.0 && dy <= 9.0 || dy <= 0.0 && dx <= 9.0 || dx.hypot(dy) <= 9.0
}

/// How much of the point the sprout covers, 0 to 1: the stem, the two
/// leaves (the second at 75%) and the ground (55%), as the logo draws
/// them. Each leaf is two quarter circles, which is what its curves are.
fn sprout_at(x: f32, y: f32) -> f32 {
    let disk = |cx: f32, cy: f32, r: f32| (x - cx).hypot(y - cy) <= r;
    // A line 2.2 units wide with round ends, across from (x0, y) to (x1, y)
    // or down from (x, y0) to (x, y1).
    let across = |x0: f32, x1: f32, at: f32| (x - x.clamp(x0, x1)).hypot(y - at) <= 1.1;
    let down = |at: f32, y0: f32, y1: f32| (x - at).hypot(y - y.clamp(y0, y1)) <= 1.1;
    let mut bare = 1.0;
    for (inside, opacity) in [
        (down(16.0, 17.0, 25.0), 1.0),
        (disk(23.0, 17.0, 7.0) && disk(16.0, 10.0, 7.0), 1.0),
        (disk(10.5, 18.5, 5.5) && disk(16.0, 13.0, 5.5), 0.75),
        (across(8.5, 23.5, 25.0), 0.55),
    ] {
        if inside {
            bare *= 1.0 - opacity;
        }
    }
    1.0 - bare
}

/// A share from 0 to 1 as a byte.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn byte(share: f32) -> u8 {
    (share.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], x: u16, y: u16) -> [u8; 4] {
        let at = (usize::from(y) * usize::from(SIZE) + usize::from(x)) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    }

    #[test]
    fn the_icon_is_the_logo_in_the_accent() {
        let colours = accent_colors(Accent::Clay, Scheme::Light);
        let rgba = logo_pixels(Some((colours.accent, colours.on_accent)));
        let (r, g, b) = colours.accent.rgb();
        // The corner is off the rounded square; beside the stem is the
        // square; the stem is the sprout's white.
        assert_eq!(pixel(&rgba, 0, 0)[3], 0);
        assert_eq!(pixel(&rgba, 6, 6), [r, g, b, 255]);
        assert_eq!(pixel(&rgba, 15, 21), [255, 255, 255, 255]);
        assert_eq!(pixel(&rgba, 16, 21), [255, 255, 255, 255]);
    }

    #[test]
    fn the_template_is_the_sprout_alone() {
        let rgba = logo_pixels(None);
        assert_eq!(pixel(&rgba, 6, 6), [0, 0, 0, 0]);
        assert_eq!(pixel(&rgba, 15, 21), [0, 0, 0, 255]);
    }

    #[test]
    fn a_left_click_opens_dirt_where_it_does_not_open_the_menu() {
        let opens = clicked(MouseButton::Left, MouseButtonState::Up);
        if MENU_ON_LEFT_CLICK {
            assert_eq!(opens, None);
        } else {
            assert_eq!(opens, Some(Choice::OpenDirt));
        }
        assert_eq!(clicked(MouseButton::Left, MouseButtonState::Down), None);
        assert_eq!(clicked(MouseButton::Right, MouseButtonState::Up), None);
    }
}
