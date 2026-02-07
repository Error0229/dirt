use dioxus::prelude::*;
use dioxus_primitives::separator::Separator;

#[component]
pub fn App() -> Element {
    let placeholder = dirt_core::Note::new("Mobile shell ready #android");
    let note_id = placeholder.id.to_string();

    rsx! {
        div {
            style: "padding: 24px; font-family: sans-serif; line-height: 1.4;",

            h1 { style: "font-size: 28px; margin: 0 0 8px 0;", "Dirt" }
            p { style: "margin: 0;", "Android app shell initialized." }

            Separator {
                decorative: true,
                style: "height: 1px; background: #d4d4d8; margin: 12px 0;",
            }

            p { style: "margin: 0;", "dirt-core connected. Example note id: {note_id}" }
            p { style: "margin: 8px 0 0 0;", "Next milestone: F4.2 note list + editor (mobile UI)." }
        }
    }
}
