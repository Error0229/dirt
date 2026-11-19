//! The icons (interface design, section 4.4): line icons on a 24 px grid
//! with a 1.75 px stroke and round caps and joins, drawn inline so that
//! they take the colour of the text around them.
//!
//! One icon has one meaning everywhere; the meanings are in the design's
//! table and on each variant below. The drawings are Lucide's, some
//! simplified (ISC licence, with the Feather icons among them under MIT;
//! the text is in `assets/icons/Lucide-LICENSE.txt`).

use dioxus::prelude::*;

/// An icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// Find.
    Search,
    /// Talk; on a card, "said, not typed".
    Mic,
    /// The microphone may not be used: the mic with a slash (6.18).
    MicOff,
    /// A language: Settings → Voice.
    Globe,
    /// Where something comes from: the speech model's credit.
    Info,
    /// Done: put this thought on the pile.
    Check,
    /// Done with a new page on Android.
    ChevronDown,
    /// Up: the way back from the Rail.
    ChevronUp,
    /// Back; the previous file.
    ChevronLeft,
    /// The next file; a row that opens.
    ChevronRight,
    /// Tags.
    Hash,
    /// Archive: out of the list, still in the pile and in Find.
    Archive,
    /// Back to the list, on an archived note.
    Unarchive,
    /// Delete for good.
    Trash,
    /// Attach; on a card, "has files".
    Paperclip,
    /// Copy the note's text.
    Copy,
    /// Synced.
    CloudCheck,
    /// Syncing; "Sync now".
    Refresh,
    /// Offline.
    CloudOff,
    /// Something needs you.
    Alert,
    /// A computer; "this device only" when signed out.
    Laptop,
    /// A phone.
    Phone,
    /// Recovery phrase, account key.
    Key,
    /// Encrypted.
    Lock,
    /// Settings.
    Settings,
    /// Close; throw away; remove.
    X,
    /// Undo.
    Undo,
    /// Add.
    Plus,
    /// A file with no picture of its own.
    File,
    /// A sound file.
    Music,
    /// Save a copy.
    Download,
    /// Open in another app.
    External,
    /// The dark theme.
    Moon,
    /// The light theme.
    Sun,
    /// The system's theme.
    Monitor,
    /// The account.
    User,
    /// Play.
    Play,
    /// Pause.
    Pause,
    /// Minimise.
    Minus,
    /// Maximise.
    Square,
    /// Restore a maximised window.
    Restore,
    /// A list with no notes at all.
    Feather,
    /// Motion.
    Motion,
}

impl Icon {
    /// Every icon, for the gallery.
    pub const ALL: [Self; 43] = [
        Self::Search,
        Self::Mic,
        Self::MicOff,
        Self::Globe,
        Self::Info,
        Self::Check,
        Self::ChevronDown,
        Self::ChevronUp,
        Self::ChevronLeft,
        Self::ChevronRight,
        Self::Hash,
        Self::Archive,
        Self::Unarchive,
        Self::Trash,
        Self::Paperclip,
        Self::Copy,
        Self::CloudCheck,
        Self::Refresh,
        Self::CloudOff,
        Self::Alert,
        Self::Laptop,
        Self::Phone,
        Self::Key,
        Self::Lock,
        Self::Settings,
        Self::X,
        Self::Undo,
        Self::Plus,
        Self::File,
        Self::Music,
        Self::Download,
        Self::External,
        Self::Moon,
        Self::Sun,
        Self::Monitor,
        Self::User,
        Self::Play,
        Self::Pause,
        Self::Minus,
        Self::Square,
        Self::Restore,
        Self::Feather,
        Self::Motion,
    ];

    /// What is drawn inside the 24 px box.
    #[must_use]
    pub const fn shapes(self) -> &'static str {
        match self {
            Self::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.6-3.6"/>"#,
            Self::Mic => {
                r#"<rect x="9" y="2.5" width="6" height="12" rx="3"/><path d="M18.5 11a6.5 6.5 0 0 1-13 0M12 17.5V21"/>"#
            }
            Self::MicOff => {
                r#"<rect x="9" y="2.5" width="6" height="12" rx="3"/><path d="M18.5 11a6.5 6.5 0 0 1-13 0M12 17.5V21M3 3l18 18"/>"#
            }
            Self::Globe => {
                r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>"#
            }
            Self::Info => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>"#,
            Self::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
            Self::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
            Self::ChevronUp => r#"<path d="m18 15-6-6-6 6"/>"#,
            Self::ChevronLeft => r#"<path d="m15 18-6-6 6-6"/>"#,
            Self::ChevronRight => r#"<path d="m9 18 6-6-6-6"/>"#,
            Self::Hash => r#"<path d="M4 9h16M4 15h16M10 3 8 21M16 3l-2 18"/>"#,
            Self::Archive => {
                r#"<rect x="2" y="3" width="20" height="5" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8M10 12h4"/>"#
            }
            Self::Unarchive => {
                r#"<rect x="2" y="3" width="20" height="5" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h2M20 8v11a2 2 0 0 1-2 2h-2M9 15l3-3 3 3M12 12v9"/>"#
            }
            Self::Trash => {
                r#"<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M10 11v6M14 11v6"/>"#
            }
            Self::Paperclip => {
                r#"<path d="m21.4 11-9.2 9.2a6 6 0 0 1-8.5-8.5l8.6-8.6a4 4 0 0 1 5.7 5.7l-8.6 8.6a2 2 0 0 1-2.8-2.8l8.5-8.5"/>"#
            }
            Self::Copy => {
                r#"<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M4 16a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2"/>"#
            }
            Self::CloudCheck => {
                r#"<path d="M5 17.7A7 7 0 1 1 15.7 10h1.8a4.5 4.5 0 0 1 1.5 8.7"/><path d="m16 15-4.5 4.5L9 17"/>"#
            }
            Self::Refresh => {
                r#"<path d="M3 12a9 9 0 0 1 15.4-6.4L21 8M21 3v5h-5M21 12a9 9 0 0 1-15.4 6.4L3 16M8 16H3v5"/>"#
            }
            Self::CloudOff => {
                r#"<path d="m2 2 20 20M5.8 5.8A7 7 0 0 0 9 19h8.5a4.5 4.5 0 0 0 1.3-.2M21.5 16.5A4.5 4.5 0 0 0 17.5 10h-1.8A7 7 0 0 0 10 5.1"/>"#
            }
            Self::Alert => {
                r#"<path d="m21.7 18-8-14a2 2 0 0 0-3.5 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.7-3M12 9v4M12 17h.01"/>"#
            }
            Self::Laptop => {
                r#"<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M2 20h20"/>"#
            }
            Self::Phone => {
                r#"<rect x="6" y="2" width="12" height="20" rx="2.5"/><path d="M11 18h2"/>"#
            }
            Self::Key => {
                r#"<path d="M2.6 17.4A2 2 0 0 0 2 18.8V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.2a2 2 0 0 0 1.4-.6l.8-.8a6.5 6.5 0 1 0-4-4z"/><circle cx="16.5" cy="7.5" r=".6"/>"#
            }
            Self::Lock => {
                r#"<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>"#
            }
            Self::Settings => {
                r#"<path d="M12.2 2h-.4a2 2 0 0 0-2 2v.2a2 2 0 0 1-1 1.7l-.4.3a2 2 0 0 1-2 0l-.2-.1a2 2 0 0 0-2.7.7l-.2.4a2 2 0 0 0 .7 2.7l.2.1a2 2 0 0 1 1 1.7v.5a2 2 0 0 1-1 1.8l-.2.1a2 2 0 0 0-.7 2.7l.2.4a2 2 0 0 0 2.7.7l.2-.1a2 2 0 0 1 2 0l.4.3a2 2 0 0 1 1 1.7v.2a2 2 0 0 0 2 2h.4a2 2 0 0 0 2-2v-.2a2 2 0 0 1 1-1.7l.4-.3a2 2 0 0 1 2 0l.2.1a2 2 0 0 0 2.7-.7l.2-.4a2 2 0 0 0-.7-2.7l-.2-.1a2 2 0 0 1-1-1.8v-.5a2 2 0 0 1 1-1.7l.2-.1a2 2 0 0 0 .7-2.7l-.2-.4a2 2 0 0 0-2.7-.7l-.2.1a2 2 0 0 1-2 0l-.4-.3a2 2 0 0 1-1-1.7V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#
            }
            Self::X => r#"<path d="M18 6 6 18M6 6l12 12"/>"#,
            Self::Undo => r#"<path d="M9 14 4 9l5-5M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/>"#,
            Self::Plus => r#"<path d="M5 12h14M12 5v14"/>"#,
            Self::File => {
                r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>"#
            }
            Self::Music => {
                r#"<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>"#
            }
            Self::Download => {
                r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/>"#
            }
            Self::External => {
                r#"<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>"#
            }
            Self::Moon => r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z"/>"#,
            Self::Sun => {
                r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M6.3 17.7l-1.4 1.4M19.1 4.9l-1.4 1.4"/>"#
            }
            Self::Monitor => {
                r#"<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>"#
            }
            Self::User => r#"<circle cx="12" cy="8" r="4"/><path d="M20 21a8 8 0 0 0-16 0"/>"#,
            Self::Play => r#"<path d="M6 4v16l14-8z"/>"#,
            Self::Pause => r#"<path d="M8 5v14M16 5v14" stroke-width="3"/>"#,
            Self::Minus => r#"<path d="M5 12h14"/>"#,
            Self::Square => r#"<rect x="5" y="5" width="14" height="14" rx="1"/>"#,
            Self::Restore => {
                r#"<path d="M8 8V5.5A1.5 1.5 0 0 1 9.5 4h9A1.5 1.5 0 0 1 20 5.5v9a1.5 1.5 0 0 1-1.5 1.5H16"/><rect x="4" y="8" width="12" height="12" rx="1.5"/>"#
            }
            Self::Feather => {
                r#"<path d="M12.7 19a2 2 0 0 0 1.4-.6l6.2-6.2a6 6 0 0 0-8.5-8.5l-6.2 6.2a2 2 0 0 0-.6 1.4V18a1 1 0 0 0 1 1zM16 8 2 22M17.5 15H9"/>"#
            }
            Self::Motion => r#"<path d="M4 12h3M2 8h5M3 16h4"/><circle cx="15" cy="12" r="6"/>"#,
        }
    }
}

/// How large an icon is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconSize {
    /// 15 px, in small buttons.
    Small,
    /// 18 px.
    #[default]
    Regular,
    /// 22 px, for empty states.
    Large,
}

impl IconSize {
    const fn class(self) -> &'static str {
        match self {
            Self::Small => "dui-i s",
            Self::Regular => "dui-i",
            Self::Large => "dui-i l",
        }
    }
}

/// An icon, drawn inline. It is hidden from screen readers: what it means
/// is said by the name of the control it sits in (section 12).
#[component]
pub fn IconSvg(icon: Icon, #[props(default)] size: IconSize) -> Element {
    rsx! {
        svg {
            class: size.class(),
            view_box: "0 0 24 24",
            "aria-hidden": "true",
            "focusable": "false",
            dangerous_inner_html: icon.shapes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_icon_is_listed_once_and_drawn() {
        let listed: HashSet<Icon> = Icon::ALL.into_iter().collect();
        assert_eq!(listed.len(), Icon::ALL.len());
        for icon in Icon::ALL {
            let shapes = icon.shapes();
            assert!(
                shapes.starts_with('<') && shapes.ends_with("/>"),
                "{icon:?}"
            );
            // The stroke comes from the style sheet; a drawing sets none of
            // its own colour.
            assert!(
                !shapes.contains("fill=") && !shapes.contains("stroke=\""),
                "{icon:?}"
            );
        }
    }
}
