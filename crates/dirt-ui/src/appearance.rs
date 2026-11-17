//! The three things a person chooses about how Dirt looks: the theme, the
//! accent and the text size (interface design, sections 4.1, 4.1.1, 4.2).
//!
//! They are kept on the device, in `dirt_core::models::Settings`, and are
//! not synced. This module gives them one shape for [`crate::UiRoot`].

use dirt_core::models::{Accent, Settings, ThemeMode};

/// The size of a note's text, in CSS pixels: 13 to 20.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextSize(u32);

impl TextSize {
    /// The smallest size offered.
    pub const MIN: Self = Self(13);
    /// The largest size offered.
    pub const MAX: Self = Self(20);
    /// The size the design is drawn at.
    pub const DEFAULT: Self = Self(15);

    /// The size nearest to `pixels` that is offered.
    #[must_use]
    pub const fn new(pixels: u32) -> Self {
        if pixels < Self::MIN.0 {
            Self::MIN
        } else if pixels > Self::MAX.0 {
            Self::MAX
        } else {
            Self(pixels)
        }
    }

    /// The size in CSS pixels.
    #[must_use]
    pub const fn pixels(self) -> u32 {
        self.0
    }

    /// One step larger, as <kbd>Ctrl +</kbd> asks, up to the largest.
    #[must_use]
    pub const fn larger(self) -> Self {
        Self::new(self.0 + 1)
    }

    /// One step smaller, down to the smallest.
    #[must_use]
    pub const fn smaller(self) -> Self {
        Self::new(self.0.saturating_sub(1))
    }
}

impl Default for TextSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How the interface looks on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Appearance {
    /// Auto (the system's), light or dark.
    pub theme: ThemeMode,
    /// The accent colour.
    pub accent: Accent,
    /// The size of a note's text.
    pub text_size: TextSize,
}

impl Appearance {
    /// What the device's settings say.
    #[must_use]
    pub const fn from_settings(settings: &Settings) -> Self {
        Self {
            theme: settings.theme,
            accent: settings.accent,
            text_size: TextSize::new(settings.font_size),
        }
    }

    /// Writes the three into the device's settings, leaving the rest.
    pub const fn write_to(self, settings: &mut Settings) {
        settings.theme = self.theme;
        settings.accent = self.accent;
        settings.font_size = self.text_size.pixels();
    }
}

/// Which of the two apps the interface is drawn in. It is the class on the
/// page's root that the style sheet's differences hang on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// The desktop app: a pointer and a keyboard.
    Desktop,
    /// The Android app: a finger.
    Android,
}

impl Platform {
    /// The app this build is.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "android") {
            Self::Android
        } else {
            Self::Desktop
        }
    }

    /// The class on the page's root, and the script's name for it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Android => "android",
        }
    }
}

/// The theme as the script and the style sheet name it.
#[must_use]
pub const fn theme_name(theme: ThemeMode) -> &'static str {
    match theme {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_size_stays_between_thirteen_and_twenty() {
        assert_eq!(TextSize::new(4).pixels(), 13);
        assert_eq!(TextSize::new(15).pixels(), 15);
        assert_eq!(TextSize::new(99).pixels(), 20);
        assert_eq!(TextSize::MAX.larger(), TextSize::MAX);
        assert_eq!(TextSize::MIN.smaller(), TextSize::MIN);
        assert_eq!(TextSize::new(15).larger().pixels(), 16);
    }

    #[test]
    fn the_three_go_into_the_settings_and_come_back() {
        let chosen = Appearance {
            theme: ThemeMode::Dark,
            accent: Accent::Plum,
            text_size: TextSize::new(18),
        };
        let mut settings = Settings::default();
        let hotkey = settings.capture_hotkey.clone();
        chosen.write_to(&mut settings);
        assert_eq!(Appearance::from_settings(&settings), chosen);
        assert_eq!(settings.capture_hotkey, hotkey);
    }
}
