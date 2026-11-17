//! The web view's own keys, turned off (interface design, sections 7.4
//! and 10).
//!
//! The desktop app is drawn in a web view, and a web view has a browser's
//! keys: reload, find, print, zoom, developer tools, and a browser's menu
//! on a right click. In a release build none of them may do anything:
//! reloading loses what is being typed, and the keys have meanings of the
//! app's own.
//!
//! Two things turn them off. The web view's settings do most of it, and
//! `dirt-desktop` sets those where it makes its window. The script here
//! does the rest and covers a web view whose settings do not: it is put in
//! the page's head, so it is there before anything is drawn. It stops what
//! the browser would do and lets the key go on to the app.

/// Whether this build turns the web view's own keys off: release builds
/// do, and development builds keep reload and the developer tools.
#[must_use]
pub const fn own_keys_off() -> bool {
    !cfg!(debug_assertions)
}

/// The script that stops the web view's own keys, zoom and menu.
pub const GUARD_SCRIPT: &str = include_str!("js/webview-keys.js");

/// [`GUARD_SCRIPT`] as an element for the page's head.
#[must_use]
pub fn guard_head() -> String {
    format!("<script>{GUARD_SCRIPT}</script>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_can_sit_in_a_script_element() {
        assert!(!GUARD_SCRIPT.contains("</script"));
        assert!(guard_head().starts_with("<script>"));
    }
}
