//! Application settings model

use serde::{Deserialize, Serialize};

/// Theme mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Light theme
    Light,
    /// Dark theme
    Dark,
    /// Follow system preference
    #[default]
    System,
}

/// Application settings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Font family for note content
    pub font_family: String,
    /// Font size in pixels
    pub font_size: u32,
    /// Theme mode
    pub theme: ThemeMode,
    /// Global capture hotkey (e.g., "Ctrl+Shift+D")
    pub capture_hotkey: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: "system-ui".to_string(),
            font_size: 14,
            theme: ThemeMode::System,
            capture_hotkey: "Ctrl+Shift+D".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.font_size, 14);
        assert_eq!(settings.theme, ThemeMode::System);
    }
}
