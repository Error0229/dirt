//! Theme configuration for the desktop app

/// Application theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Light theme
    #[default]
    Light,
    /// Dark theme
    #[allow(dead_code)] // Will be used when theme toggle is implemented
    Dark,
}

impl Theme {
    /// Check if the theme is dark
    #[must_use]
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    /// Toggle between light and dark theme
    #[must_use]
    #[allow(dead_code)] // Will be used when theme toggle UI is implemented
    pub const fn toggle(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }
}
