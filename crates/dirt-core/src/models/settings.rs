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

/// The one colour of the interface the person chooses (interface design,
/// section 4.1.1). Kept on the device and not synced: a phone and a laptop
/// may want different ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    /// The colour of soil, and the default.
    #[default]
    Clay,
    /// Green.
    Moss,
    /// Blue.
    Lake,
    /// Violet.
    Plum,
    /// Amber.
    Honey,
    /// No colour at all.
    Graphite,
}

impl Accent {
    /// Every accent, in the order the interface offers them.
    pub const ALL: [Self; 6] = [
        Self::Clay,
        Self::Moss,
        Self::Lake,
        Self::Plum,
        Self::Honey,
        Self::Graphite,
    ];

    /// The name the accent is stored under and the style sheet knows it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Clay => "clay",
            Self::Moss => "moss",
            Self::Lake => "lake",
            Self::Plum => "plum",
            Self::Honey => "honey",
            Self::Graphite => "graphite",
        }
    }

    /// The name a person reads.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clay => "Clay",
            Self::Moss => "Moss",
            Self::Lake => "Lake",
            Self::Plum => "Plum",
            Self::Honey => "Honey",
            Self::Graphite => "Graphite",
        }
    }

    /// The accent stored under `name`, if this build has one of that name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|accent| accent.name() == name)
    }
}

/// How the desktop's list gets out of the way while a note is written
/// (interface design, section 7.7). Kept on the device, like the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FocusStyle {
    /// The list slides out to the left; the note's column glides to the
    /// middle. The default.
    #[default]
    Slide,
    /// The list folds to a rail of grains, one for each note.
    Rail,
    /// Slide, and a calmer page: warmer paper, larger type.
    Zen,
}

impl FocusStyle {
    /// Every style, in the order the interface offers them.
    pub const ALL: [Self; 3] = [Self::Slide, Self::Rail, Self::Zen];

    /// The name the style is stored under and the style sheet knows it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Slide => "slide",
            Self::Rail => "rail",
            Self::Zen => "zen",
        }
    }

    /// The name a person reads.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Slide => "Slide",
            Self::Rail => "Rail",
            Self::Zen => "Zen",
        }
    }

    /// The style stored under `name`, if this build has one of that name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.name() == name)
    }
}

/// What is kept of a thought said aloud once its words are made
/// (Settings → Voice, interface design 6.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VoiceRecording {
    /// The words, and the recording on the note as a file.
    #[default]
    Keep,
    /// The words alone: the recording is taken off the note once its words
    /// are made. Until then, or if none are found, it stays.
    TextOnly,
}

impl VoiceRecording {
    /// The name it is stored under.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::TextOnly => "text_only",
        }
    }

    /// The one stored under `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Keep, Self::TextOnly]
            .into_iter()
            .find(|choice| choice.name() == name)
    }
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
    /// The accent colour
    pub accent: Accent,
    /// Global capture hotkey (e.g., "Ctrl+Shift+D")
    pub capture_hotkey: String,
    /// How the desktop's list gets out of the way while a note is written.
    pub focus_style: FocusStyle,
    /// What is kept of a thought said aloud.
    #[serde(default)]
    pub voice_recording: VoiceRecording,
    /// The desktop's microphone, by the ID the system gives it; nothing
    /// for the system's default. One that is not connected when the
    /// person talks gives way to the default.
    #[serde(default)]
    pub microphone: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: "system-ui".to_string(),
            font_size: 15,
            theme: ThemeMode::System,
            accent: Accent::Clay,
            capture_hotkey: "Ctrl+Shift+D".to_string(),
            focus_style: FocusStyle::Slide,
            voice_recording: VoiceRecording::Keep,
            microphone: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.font_size, 15);
        assert_eq!(settings.theme, ThemeMode::System);
        assert_eq!(settings.accent, Accent::Clay);
    }

    #[test]
    fn an_accent_is_found_by_the_name_it_is_stored_under() {
        for accent in Accent::ALL {
            assert_eq!(Accent::from_name(accent.name()), Some(accent));
        }
        assert_eq!(Accent::from_name("wallpaper"), None);
    }

    #[test]
    fn a_focus_style_is_found_by_the_name_it_is_stored_under() {
        for style in FocusStyle::ALL {
            assert_eq!(FocusStyle::from_name(style.name()), Some(style));
        }
        assert_eq!(FocusStyle::default(), FocusStyle::Slide);
        assert_eq!(FocusStyle::from_name("dim"), None);
    }
}
