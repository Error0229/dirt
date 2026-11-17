//! What Dirt's desktop app and Android app share of the interface
//! (interface design, `docs/ui/design.md`).
//!
//! - [`tokens`]: the colours, type, spacing, radii and shadows of section 4,
//!   as one style sheet for both apps. What differs between them hangs on
//!   a class of the page's root, `.desktop` or `.android`.
//! - [`contrast`]: how well one colour reads on another; its tests hold
//!   every pair the design names to the ratio it promises.
//! - [`fonts`] and [`icons`]: the two typefaces, bundled, and the icons of
//!   section 4.4.
//! - [`motion`]: the springs and curves of section 5, and the one script
//!   that animates lists, follows fingers and honours reduced motion.
//! - [`components`]: the small components of section 6.
//! - [`UiRoot`]: the component an app puts around its screens. It brings
//!   the style sheet and the script, and applies the theme, the accent and
//!   the text size the person chose.
//!
//! This is a client crate: it has no SQL, HTTP or cryptography in it.
//!
//! # What the script does, and how a component asks for it
//!
//! Rust renders elements; the script finds them by attribute as they
//! appear and never needs calling.
//!
//! | Attribute | On | Does |
//! |---|---|---|
//! | `data-tip`, `data-kbd` | anything | A tooltip after a hover, a long press on Android, or keyboard focus |
//! | `data-dui-list` | a list | Its children with `data-key` slide to their new places when the page changes, settle in when new, and are drawn leaving when removed. `data-dui-exit` on the list is `tear` (the default), `fade` or `none`; `data-dui-enter` on a row is `pop` or `none` |
//! | `data-dui-scroll` | a scrolling element | The drawn scrollbar ([`components::ScrollArea`] renders it) |
//! | `data-dui-swipe` | a card | It follows a finger; see [`components::Swipeable`] |
//!
//! What ends in the script and matters to the app (a card swiped away, a
//! sheet dragged shut, a toast run out) comes back once, as an
//! [`events::Outcome`].

// Dioxus `asset!` macro expansion triggers false positives for
// `clippy::volatile_composites` on newer toolchains.
#![allow(unknown_lints, clippy::volatile_composites)]

pub mod appearance;
pub mod components;
pub mod contrast;
pub mod events;
pub mod fonts;
#[cfg(feature = "gallery")]
pub mod gallery;
pub mod icons;
pub mod motion;
mod root;
pub mod tokens;
pub mod webview;

pub use appearance::{Appearance, Platform, TextSize};
pub use dirt_core::models::{Accent, ThemeMode};
pub use icons::{Icon, IconSize, IconSvg};
pub use root::{stylesheet, UiRoot};
