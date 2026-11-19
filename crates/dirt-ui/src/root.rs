//! The component an app puts around its screens.

use std::sync::OnceLock;

use dioxus::prelude::*;

use crate::appearance::{theme_name, Appearance, Platform};
use crate::components::toast::{ToastLayer, Toasts};
use crate::events::{Bus, Message, CONNECT};
use crate::{fonts, motion, tokens};

/// The components' styles. The tokens and the fonts come before them.
const COMPONENTS: &str = include_str!("style/components.css");

/// The whole style sheet: the fonts, the tokens of section 4, the motion
/// tokens of section 5 and the components of section 6. It is one sheet
/// for both apps.
#[must_use]
pub fn stylesheet() -> &'static str {
    static SHEET: OnceLock<String> = OnceLock::new();
    SHEET.get_or_init(|| {
        let mut sheet = fonts::css();
        sheet.push_str(&tokens::css());
        sheet.push_str(&motion::css());
        sheet.push_str(COMPONENTS);
        sheet
    })
}

/// What the script is told about how the page should look.
fn looks(appearance: Appearance, platform: Platform, reduced_motion: bool) -> serde_json::Value {
    serde_json::json!({
        "theme": theme_name(appearance.theme),
        "accent": appearance.accent.name(),
        "size": appearance.text_size.pixels(),
        "platform": platform.name(),
        "reduced": reduced_motion,
    })
}

/// Brings the style sheet and the script to the page, applies the theme,
/// the accent and the text size, and shows toasts. Everything of the new
/// interface is drawn inside one of these.
///
/// A change of theme or accent spreads in a circle from the point last
/// pressed where the web view can draw that, and cross-fades where it
/// cannot (section 5.5).
///
/// `reduced_motion` asks for reduced motion whatever the system says; the
/// system's own setting is honoured without it.
#[component]
pub fn UiRoot(
    appearance: Appearance,
    #[props(default = Platform::current())] platform: Platform,
    #[props(default)] reduced_motion: bool,
    children: Element,
) -> Element {
    let bus = use_context_provider(Bus::new);
    use_context_provider(Toasts::new);

    // The script, once, and the looks with it, so that the first thing
    // drawn is already in the theme chosen.
    use_hook(|| {
        let options = serde_json::json!({
            "tokens": motion::script_tokens(),
            "platform": platform.name(),
            "pdfjs": crate::files::pdf::FOLDER.to_string(),
        });
        let first = looks(appearance, platform, reduced_motion);
        document::eval(&format!(
            "{}\nDirtUi.install({options});\nDirtUi.apply({first});",
            motion::SCRIPT
        ));
        spawn(async move {
            let mut channel = document::eval(CONNECT);
            loop {
                match channel.recv::<Message>().await {
                    Ok(message) => bus.deliver(&message),
                    Err(error) => {
                        tracing::warn!("The interface's script stopped answering: {error}");
                        break;
                    }
                }
            }
        });
    });

    use_effect(use_reactive!(|(appearance, platform, reduced_motion)| {
        let now = looks(appearance, platform, reduced_motion);
        document::eval(&format!("DirtUi.apply({now});"));
    }));

    rsx! {
        div { class: "dui-root",
            // In the page beside what it styles, not sent to the head: it
            // arrives with the first render. Sent separately it came a
            // moment later on a slow emulator, and the first thing drawn was a
            // sheet and a banner that should have been out of sight.
            style { "{stylesheet()}" }
            {children}
            ToastLayer { platform }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_is_whole() {
        let sheet = stylesheet();
        assert!(sheet.starts_with("@font-face"));
        assert!(sheet.contains(":root{--font:"));
        assert!(sheet.contains(".dui-icon-btn{"));
        assert_eq!(sheet.matches('{').count(), sheet.matches('}').count());
        // Nothing is fetched at run time (section 4.2).
        assert!(!sheet.contains("http://") && !sheet.contains("https://"));
    }
}
