//! The small components of the interface design, section 6.
//!
//! They come with the accessibility section 12 asks for: every control has
//! a name, focus is always visible, a panel or sheet takes focus and gives
//! it back, and a toast is announced.
//!
//! A component renders elements and classes. What moves is in the style
//! sheet, and what follows a finger is in the script (`motion::SCRIPT`).

pub mod accent_dots;
pub mod banner;
pub mod button;
pub mod card;
pub mod confirm;
pub mod find_field;
pub mod grow;
pub mod icon_button;
pub mod logo;
pub mod menu;
pub mod more;
pub mod note_row;
pub mod panel;
pub mod pile;
pub mod pull;
mod radio;
pub mod scroll;
pub mod segmented;
pub mod slider;
pub mod swipe;
pub mod switch;
pub mod sync_status;
pub mod toast;
pub mod voice;

pub use accent_dots::AccentDots;
pub use banner::Banner;
pub use button::{Button, ButtonKind};
pub use card::{card_element_id, NoteCard};
pub use confirm::ConfirmInPlace;
pub use find_field::{FindField, TagList, TagOption, TagStrip};
pub use grow::{GrowFrom, GrowPage};
pub use icon_button::{IconButton, IconButtonSize};
pub use logo::Logo;
pub use menu::{ContextMenu, MenuItem};
pub use more::LoadMore;
pub use note_row::{EmptyList, LayerLine, Marked, NoteRow, NoteSummary, TagLinks};
pub use panel::{Panel, Sheet};
pub use pile::Pile;
pub use pull::PullToSync;
pub use scroll::{ListExit, ScrollArea};
pub use segmented::{Segment, Segmented};
pub use slider::{Slider, TextSizeSlider};
pub use swipe::Swipeable;
pub use switch::Switch;
pub use sync_status::SyncStatus;
pub use toast::{set_screen_reader, use_toasts, Toast, Toasts};
pub use voice::{Ended, Listening, MicButton, Talk};
