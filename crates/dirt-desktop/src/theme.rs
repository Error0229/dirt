//! Theme configuration for the desktop app

use std::sync::OnceLock;

pub use dirt_core::models::ThemeMode;

/// Cached system dark mode preference (detected once at startup)
static SYSTEM_DARK_MODE: OnceLock<bool> = OnceLock::new();

/// Resolved theme (light or dark)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResolvedTheme {
    #[default]
    Light,
    Dark,
}

impl ResolvedTheme {
    /// Check if the theme is dark
    #[must_use]
    #[allow(dead_code)] // Will be used for CSS class names
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

/// Resolve theme mode to actual light/dark theme
#[must_use]
pub fn resolve_theme(mode: ThemeMode) -> ResolvedTheme {
    match mode {
        ThemeMode::Light => ResolvedTheme::Light,
        ThemeMode::Dark => ResolvedTheme::Dark,
        ThemeMode::System => {
            if is_system_dark_mode() {
                ResolvedTheme::Dark
            } else {
                ResolvedTheme::Light
            }
        }
    }
}

/// Detect system dark mode preference (cached after first call)
#[must_use]
pub fn is_system_dark_mode() -> bool {
    *SYSTEM_DARK_MODE.get_or_init(detect_system_dark_mode)
}

/// Perform the actual system dark mode detection
/// This is expensive (spawns subprocess) so we only call it once
fn detect_system_dark_mode() -> bool {
    detect_system_dark_mode_impl()
}

#[cfg(target_os = "windows")]
fn detect_system_dark_mode_impl() -> bool {
    use std::process::Command;
    // Check Windows AppsUseLightTheme registry value
    // 0 = dark mode, 1 = light mode
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // If AppsUseLightTheme is 0x0, system is in dark mode
            let is_dark = stdout.contains("0x0");
            tracing::debug!(
                "System theme detected: {}",
                if is_dark { "dark" } else { "light" }
            );
            is_dark
        }
        Err(e) => {
            tracing::warn!(
                "Failed to detect system theme: {}. Defaulting to light mode.",
                e
            );
            false
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_system_dark_mode_impl() -> bool {
    use std::process::Command;
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let is_dark = stdout.trim().eq_ignore_ascii_case("dark");
            tracing::debug!(
                "System theme detected: {}",
                if is_dark { "dark" } else { "light" }
            );
            is_dark
        }
        Err(e) => {
            tracing::warn!(
                "Failed to detect system theme: {}. Defaulting to light mode.",
                e
            );
            false
        }
    }
}

#[cfg(target_os = "linux")]
fn detect_system_dark_mode_impl() -> bool {
    // Check GTK theme or use environment variable
    if let Ok(theme) = std::env::var("GTK_THEME") {
        let is_dark = theme.to_lowercase().contains("dark");
        tracing::debug!(
            "System theme detected from GTK_THEME: {}",
            if is_dark { "dark" } else { "light" }
        );
        is_dark
    } else {
        tracing::debug!("GTK_THEME not set, defaulting to light mode");
        false
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn detect_system_dark_mode_impl() -> bool {
    tracing::debug!("Unsupported platform for system theme detection, defaulting to light mode");
    false
}

/// Color palette for the application
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // All colors defined for completeness, not all used yet
pub struct ColorPalette {
    pub bg_primary: &'static str,
    pub bg_secondary: &'static str,
    pub bg_tertiary: &'static str,
    pub text_primary: &'static str,
    pub text_secondary: &'static str,
    pub text_muted: &'static str,
    pub border: &'static str,
    pub border_light: &'static str,
    pub accent: &'static str,
    pub accent_hover: &'static str,
    pub accent_text: &'static str,
    pub error: &'static str,
    pub success: &'static str,
}

/// Light theme colors
pub const LIGHT_PALETTE: ColorPalette = ColorPalette {
    bg_primary: "#ffffff",
    bg_secondary: "#f8f9fa",
    bg_tertiary: "#f1f3f4",
    text_primary: "#1a1a1a",
    text_secondary: "#5f6368",
    text_muted: "#9aa0a6",
    border: "#dadce0",
    border_light: "#e8eaed",
    accent: "#4f46e5",
    accent_hover: "#4338ca",
    accent_text: "#ffffff",
    error: "#dc2626",
    success: "#16a34a",
};

/// Dark theme colors
pub const DARK_PALETTE: ColorPalette = ColorPalette {
    bg_primary: "#1a1a1a",
    bg_secondary: "#242424",
    bg_tertiary: "#2d2d2d",
    text_primary: "#e8eaed",
    text_secondary: "#9aa0a6",
    text_muted: "#5f6368",
    border: "#3c4043",
    border_light: "#5f6368",
    accent: "#818cf8",
    accent_hover: "#a5b4fc",
    accent_text: "#1a1a1a",
    error: "#f87171",
    success: "#4ade80",
};

impl ResolvedTheme {
    /// Get the color palette for this theme
    #[must_use]
    pub const fn palette(self) -> &'static ColorPalette {
        match self {
            Self::Light => &LIGHT_PALETTE,
            Self::Dark => &DARK_PALETTE,
        }
    }
}
