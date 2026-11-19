//! Who has the keyboard. Dirt takes it only when the person asks for Dirt:
//! the hotkey, the tray's menu, starting the app, or a click on its window.
//! Anything else that shows a window (an error the main window comes back
//! for) shows it without the keyboard, and asks to be looked at instead.
//! Otherwise keys typed in another app would land in Dirt, and could become
//! a note.
//!
//! Both of the app's windows are made with `with_focused(false)`, and tao
//! then shows them, each time, without activating them. Taking the keyboard
//! is always a separate, deliberate `set_focus`.
//!
//! Which window is in front, giving it the keyboard back, and showing a
//! window without activating it are calls into Windows, so the
//! `unsafe_code` allow is scoped to this module.

#![allow(unsafe_code)]
// Away from Windows these are stand-ins that ask nothing, which clippy
// would have made `const` and without `self`; on Windows they cannot be.
#![cfg_attr(not(windows), allow(clippy::missing_const_for_fn, clippy::unused_self))]

use dioxus::desktop::tao::window::Window;

/// Whether Dirt may take the keyboard even when the person asks for it.
/// Not in a development build kept off every screen (`DIRT_WINDOW_AT`): a
/// window nobody can see must not take the keys of the person at the
/// computer, nor ask for their attention.
pub fn may_take() -> bool {
    crate::app::asked_position().is_none()
}

/// Shows `window` without giving it the keyboard: in its place among the
/// other windows, not in front of the one being typed in.
pub fn show_without_keyboard(window: &Window) {
    #[cfg(windows)]
    {
        use dioxus::desktop::tao::platform::windows::WindowExtWindows;
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        let handle = window.hwnd() as windows_sys::Win32::Foundation::HWND;
        // SAFETY: the handle is this window's own, which lives as long as
        // `window` does.
        unsafe {
            ShowWindow(handle, SW_SHOWNOACTIVATE);
        }
    }
    // Told to tao as well, so that it knows the window is shown and hides
    // it when asked. A window made with `with_focused(false)` it shows
    // without activating; on Windows the window is already shown by then.
    window.set_visible(true);
}

/// Whether `window` is the one in front, with the keyboard. Where that
/// cannot be asked, it is taken to be.
pub fn is_in_front(window: &Window) -> bool {
    #[cfg(windows)]
    {
        use dioxus::desktop::tao::platform::windows::WindowExtWindows;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        // SAFETY: takes nothing, and only reads which window is in front.
        let front = unsafe { GetForegroundWindow() };
        front as isize == window.hwnd()
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        true
    }
}

/// The window that had the keyboard before quick capture opened, to give
/// it back to as soon as quick capture is put away, not when the grain
/// lands: what is typed meanwhile is meant for that app.
#[derive(Debug, Clone, Copy, Default)]
pub struct Foreground {
    #[cfg(windows)]
    window: Option<isize>,
}

impl Foreground {
    /// The window in front now.
    pub fn now() -> Self {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
            // SAFETY: takes nothing, and only reads which window is in front.
            let window = unsafe { GetForegroundWindow() };
            Self {
                window: (!window.is_null()).then_some(window as isize),
            }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }

    /// Gives the keyboard back to that window. Says whether it could:
    /// where it cannot (it has gone, or this is not Windows, where it has
    /// not been tried), the window should hide at once, and the system
    /// gives the keyboard to what is in front then.
    pub fn give_focus_back(self) -> bool {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};
            let Some(window) = self.window else {
                return false;
            };
            let window = window as windows_sys::Win32::Foundation::HWND;
            // SAFETY: both take a window's handle, which may be stale: then
            // `IsWindow` says so and nothing else is done.
            unsafe { IsWindow(window) != 0 && SetForegroundWindow(window) != 0 }
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}
