//! Dirt's logo: a sprout on a rounded square of the accent.
//!
//! It is the mockup's own drawing. The square follows the accent the person
//! chose (section 4.1.1), and the sprout is drawn in the colour that reads
//! on it.

use dioxus::prelude::*;

/// The sprout, drawn on the square in `--on-accent`.
const SPROUT: &str = r#"<rect width="32" height="32" rx="9" fill="var(--accent)"/><path d="M16 25v-8" stroke="var(--on-accent)" stroke-width="2.2" stroke-linecap="round" fill="none"/><path d="M16 17c0-4.2 2.8-7 7-7 0 4.2-2.8 7-7 7z" fill="var(--on-accent)"/><path d="M16 18.5c0-3.3-2.2-5.5-5.5-5.5 0 3.3 2.2 5.5 5.5 5.5z" fill="var(--on-accent)" opacity=".75"/><path d="M8.5 25h15" stroke="var(--on-accent)" stroke-width="2.2" stroke-linecap="round" opacity=".55"/>"#;

/// The logo, `size` pixels square. It says nothing to a screen reader: it
/// sits beside what names the place it is in.
#[component]
pub fn Logo(#[props(default = 26)] size: u32, #[props(default)] class: String) -> Element {
    rsx! {
        svg {
            class: "dui-logo {class}",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 32 32",
            "aria-hidden": "true",
            "focusable": "false",
            dangerous_inner_html: SPROUT,
        }
    }
}
