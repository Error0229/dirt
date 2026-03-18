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
    std::env::var("GTK_THEME").map_or_else(
        |_| {
            tracing::debug!("GTK_THEME not set, defaulting to light mode");
            false
        },
        |theme| {
            let is_dark = theme.to_lowercase().contains("dark");
            tracing::debug!(
                "System theme detected from GTK_THEME: {}",
                if is_dark { "dark" } else { "light" }
            );
            is_dark
        },
    )
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
    bg_primary: "#faf9f7",
    bg_secondary: "#f0eeeb",
    bg_tertiary: "#e6e3df",
    text_primary: "#1a1816",
    text_secondary: "#6b6560",
    text_muted: "#9a948e",
    border: "#ddd8d2",
    border_light: "#e8e4de",
    accent: "#b07d4f",
    accent_hover: "#976a42",
    accent_text: "#ffffff",
    error: "#c45c5c",
    success: "#5a9a68",
};

/// Dark theme colors — warm walnut palette
pub const DARK_PALETTE: ColorPalette = ColorPalette {
    bg_primary: "#17151a",
    bg_secondary: "#1e1b22",
    bg_tertiary: "#362f42",
    text_primary: "#e8e4ed",
    text_secondary: "#9a93a6",
    text_muted: "#6b6478",
    border: "#2a2730",
    border_light: "#2d2936",
    accent: "#c49a6c",
    accent_hover: "#d4aa7c",
    accent_text: "#17151a",
    error: "#c45c5c",
    success: "#6bba7a",
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
