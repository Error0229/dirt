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
//! - [`pile`]: grains and the pile drawn as a heap of sand (6.17).
//! - [`files`]: the file tile and the file preview (6.5, 6.16), with the
//!   bundled PDF renderer.
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
//! | `data-dui-list` | a list | Its children with `data-key` slide to their new places when the page changes, settle in when new, and are drawn leaving when removed. `data-dui-exit` on the list is `tear` (the default), `fold`, `sink` (an archive: folds, and its grain sinks), `shrink`, `fade` or `none`; `data-dui-enter` on a row is `pop` or `none`. A child `.dui-sel-ind` is the list's one highlight, moved to the row with the class `sel` |
//! | `data-dui-grain`, `data-dui-settle` | a row or card | Its grain's shade; marked to settle, it is held out of sight while the new thought's words gather into the grain and the grain is tossed onto it (section 5.5). See [`pile`] |
//! | `data-dui-pile` | the pile under a list | Its newest grain drops onto the heap as a thought settles, if the heap is in sight ([`components::Pile`]) |
//! | `data-dui-scroll` | a scrolling element | The drawn scrollbar ([`components::ScrollArea`] renders it) |
//! | `data-dui-swipe` | a card | It follows a finger; see [`components::Swipeable`] |
//! | `data-dui-arrows` | a group of buttons | Up and Down move between them; Up from the first goes to the element the attribute names |
//! | `data-dui-menu` | the app's menu | Shown on a right click or the menu key; see [`components::ContextMenu`] |
//! | `data-dui-more` | a mark at a list's end | Says once that it has come near; see [`components::LoadMore`] |
//! | `data-dui-oneline` | a text field | A pasted line break becomes a space |
//! | `data-dui-home` | a screen | Takes the keyboard's focus whenever it would fall to the page's body, so that the screen's keys are still heard |
//! | `data-dui-near` | a control that hides | Marked `data-near` while the pointer is within that many pixels of it, counting only a pointer that moves; with `data-dui-reach`, says once that the pointer came onto it ([`events::Outcome::Reach`]) |
//! | `data-dui-keep-focus` | a group of buttons | A press inside it leaves the keyboard's focus where it was |
//! | `data-dui-typed` | a note's text field | What is typed is told to Rust as changes, in order, through the carrier beside it, however long the note: see [`typing`] |
//! | `data-dui-page` | the desktop's page | <kbd>Esc</kbd>, <kbd>Ctrl Enter</kbd> and <kbd>Ctrl N</kbd> empty it as they are pressed, so that what is typed next is the next thought's |
//! | `data-dui-drag` | a window's root | A press on an empty part of its top rows (the height in pixels) says [`events::Outcome::Drag`], a double-click [`events::Outcome::Maximize`] |
//! | `data-dui-resize` | an edge of the window | A press on it says [`events::Outcome::Resize`] |
//! | `data-dui-focus`, `data-dui-writing` | the desktop's screen | Focus while writing: the style, and `hush` or `on`. The rows of its list fold away and come back, and the grains of `data-dui-rail` pop in |
//! | `data-dui-press` | an element that is not a button | <kbd>Space</kbd> and <kbd>Enter</kbd> press it: a file's tile |
//! | `data-dui-enter="grow"`, `data-dui-enter-first` | a row; its list | The row grows out of the point last pressed or dropped on; with the list's mark, also when the list was empty |
//! | `data-dui-drop` | a screen | Has the class `dropping` while files are dragged over the window |
//! | `data-dui-modal="preview"` | the file preview | A dialog that grows out of the tile `data-dui-from` names: ← → say [`events::Outcome::Previous`] and [`events::Outcome::Next`], Space and Esc close it, and on Android swipes do the same; see [`files::FilePreview`] |
//! | `data-dui-zoom` | a picture | A double click or a pinch zooms it to twice its size |
//! | `data-dui-capture` | the desktop's quick-capture box | Comes into focus as it appears; set to `tray`, `fall` or `fade`, it shrinks to its grain (`data-dui-grain`) or fades, saying [`events::Outcome::Away`] as the window should start moving and [`events::Outcome::Gone`] when it can hide |
//! | `data-dui-audio`, `data-dui-pdf` | a sound's card; a PDF's pages | Played, and drawn by the bundled PDF.js; see [`files::pdf`] |
//!
//! What the app asks of a field it does not hold (focus, its text) is in
//! [`dom`].
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
pub mod dom;
pub mod events;
pub mod files;
pub mod find;
pub mod fonts;
#[cfg(feature = "gallery")]
pub mod gallery;
pub mod icons;
pub mod motion;
pub mod pile;
mod root;
pub mod settings;
pub mod text;
pub mod time;
pub mod tokens;
pub mod typing;
pub mod voice;
pub mod webview;

pub use appearance::{Appearance, Platform, TextSize};
pub use dirt_core::models::{Accent, ThemeMode};
pub use icons::{Icon, IconSize, IconSvg};
pub use root::{stylesheet, UiRoot};
