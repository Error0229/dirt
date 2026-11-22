//! Settings → Appearance (interface design, sections 4.1, 4.1.1, 6.11 and
//! 7.7): the theme, the accent, the text size and, on the desktop, focus
//! while writing and the quick-capture keys. All of it is kept on this
//! device.

use dioxus::prelude::*;
use dirt_core::models::{FocusBegins, FocusStyle, ThemeMode};

use crate::appearance::{theme_name, Appearance, Platform};
use crate::components::{AccentDots, Segment, Segmented, TextSizeSlider};
use crate::icons::Icon;

fn theme_of(name: &str) -> ThemeMode {
    match name {
        "light" => ThemeMode::Light,
        "dark" => ThemeMode::Dark,
        _ => ThemeMode::System,
    }
}

#[component]
pub(super) fn Look(
    appearance: Appearance,
    on_appearance: EventHandler<Appearance>,
    focus_style: Option<FocusStyle>,
    on_focus_style: EventHandler<FocusStyle>,
    focus_begins: Option<FocusBegins>,
    on_focus_begins: EventHandler<FocusBegins>,
    platform: Platform,
) -> Element {
    // On Android the theme's segments are icons alone, as the mockup has
    // them: three words do not fit beside "Theme" on a phone.
    let words = platform == Platform::Desktop;
    let segment = |value: &str, label: &str, icon: Icon| {
        let segment = Segment::new(value, label).icon(icon);
        if words {
            segment
        } else {
            segment.icon_only()
        }
    };
    let themes = vec![
        segment("system", "Auto", Icon::Monitor),
        segment("light", "Light", Icon::Sun),
        segment("dark", "Dark", Icon::Moon),
    ];
    rsx! {
        div { class: "dui-set-card",
            div { class: "dui-set-row",
                div { class: "dui-set-words", div { class: "dui-set-t", "Theme" } }
                Segmented {
                    label: "Theme",
                    segments: themes,
                    value: theme_name(appearance.theme),
                    onchange: move |value: String| {
                        on_appearance.call(Appearance { theme: theme_of(&value), ..appearance });
                    },
                }
            }
            div { class: "dui-set-row",
                div { class: "dui-set-words", div { class: "dui-set-t", "Accent" } }
                AccentDots {
                    value: appearance.accent,
                    onchange: move |accent| on_appearance.call(Appearance { accent, ..appearance }),
                }
            }
            if let Some(style) = focus_style {
                div { class: "dui-set-row",
                    div { class: "dui-set-words", div { class: "dui-set-t", "Focus while writing" } }
                    Segmented {
                        label: "Focus while writing",
                        small: true,
                        segments: FocusStyle::ALL.iter().map(|style| Segment::new(style.name(), style.label())).collect::<Vec<_>>(),
                        value: style.name(),
                        onchange: move |value: String| {
                            on_focus_style.call(FocusStyle::from_name(&value).unwrap_or_default());
                        },
                    }
                }
            }
            // When the list goes (7.14), with what the choice does in one
            // grey line.
            if let Some(begins) = focus_begins {
                div { class: "dui-set-row",
                    div { class: "dui-set-words",
                        div { class: "dui-set-t", "Focus begins" }
                        div { class: "dui-set-s",
                            if begins == FocusBegins::FirstKey {
                                "The list goes at the first key; the words stay put"
                            } else {
                                "The list goes when typing pauses"
                            }
                        }
                    }
                    Segmented {
                        label: "Focus begins",
                        small: true,
                        segments: FocusBegins::ALL.iter().map(|begins| Segment::new(begins.name(), begins.label())).collect::<Vec<_>>(),
                        value: begins.name(),
                        onchange: move |value: String| {
                            on_focus_begins.call(FocusBegins::from_name(&value).unwrap_or_default());
                        },
                    }
                }
            }
            div { class: "dui-set-row",
                div { class: "dui-set-words", div { class: "dui-set-t", "Text size" } }
                TextSizeSlider {
                    value: appearance.text_size,
                    onchange: move |text_size| on_appearance.call(Appearance { text_size, ..appearance }),
                }
            }
            if platform == Platform::Desktop {
                div { class: "dui-set-row",
                    div { class: "dui-set-words",
                        div { class: "dui-set-t", "Quick capture" }
                        div { class: "dui-set-s", "Works from any app" }
                    }
                    span { class: "dui-keys", kbd { "Ctrl" } kbd { "Alt" } kbd { "N" } }
                }
            }
        }
    }
}
