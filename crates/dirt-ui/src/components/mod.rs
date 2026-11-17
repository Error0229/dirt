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
pub mod confirm;
pub mod icon_button;
pub mod panel;
mod radio;
pub mod scroll;
pub mod segmented;
pub mod slider;
pub mod swipe;
pub mod switch;
pub mod toast;

pub use accent_dots::AccentDots;
pub use banner::Banner;
pub use button::{Button, ButtonKind};
pub use confirm::ConfirmInPlace;
pub use icon_button::{IconButton, IconButtonSize};
pub use panel::{Panel, Sheet};
pub use scroll::{ListExit, ScrollArea};
pub use segmented::{Segment, Segmented};
pub use slider::{Slider, TextSizeSlider};
pub use swipe::Swipeable;
pub use switch::Switch;
pub use toast::{use_toasts, Toast, Toasts};
