//! The window that had the keyboard before quick capture opened. It gets
//! the keyboard back as soon as quick capture is put away, not when the
//! grain lands: what is typed meanwhile is meant for that app.
//!
//! Asking which window is in front, and giving it the keyboard, are calls
//! into Windows, so the `unsafe_code` allow is scoped to this module.

#![allow(unsafe_code)]

/// The window that had the keyboard before quick capture opened, to give
/// it back to.
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
