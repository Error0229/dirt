use dioxus::prelude::*;
use dioxus_primitives::slider::SliderValue;

use dirt_core::models::Settings;

use super::row::SettingRow;
use crate::components::select::{
    Select, SelectItemIndicator, SelectList, SelectOption, SelectTrigger, SelectValue,
};
use crate::components::slider::{Slider, SliderRange, SliderThumb, SliderTrack};

/// Font family options.
const FONT_FAMILIES: &[(&str, &str)] = &[
    ("system-ui", "System Default"),
    ("JetBrains Mono", "JetBrains Mono"),
    ("Fira Code", "Fira Code"),
    ("Consolas", "Consolas"),
    ("Monaco", "Monaco"),
    ("Menlo", "Menlo"),
];

#[component]
pub(super) fn ThemeSettingsTab(
    hotkey_bg: &'static str,
    hotkey_border: &'static str,
    current_settings: Settings,
    current_theme_value: String,
    on_theme_change: EventHandler<String>,
    on_font_family_change: EventHandler<String>,
    on_font_size_change: EventHandler<u32>,
) -> Element {
    rsx! {
        SettingRow {
            label: "Theme",
            description: "Choose your preferred color scheme",

            Select::<String> {
                default_value: current_theme_value,
                on_value_change: move |value: Option<String>| {
                    if let Some(value) = value {
                        on_theme_change.call(value);
                    }
                },

                SelectTrigger {
                    style: "width: 150px;",
                    SelectValue {}
                }

                SelectList {
                    SelectOption::<String> {
                        index: 0usize,
                        value: "system".to_string(),
                        text_value: "System",
                        "System"
                        SelectItemIndicator {}
                    }
                    SelectOption::<String> {
                        index: 1usize,
                        value: "light".to_string(),
                        text_value: "Light",
                        "Light"
                        SelectItemIndicator {}
                    }
                    SelectOption::<String> {
                        index: 2usize,
                        value: "dark".to_string(),
                        text_value: "Dark",
                        "Dark"
                        SelectItemIndicator {}
                    }
                }
            }
        }

        SettingRow {
            label: "Font Family",
            description: "Font used for note content",

            Select::<String> {
                default_value: current_settings.font_family.clone(),
                on_value_change: move |value: Option<String>| {
                    if let Some(value) = value {
                        on_font_family_change.call(value);
                    }
                },

                SelectTrigger {
                    style: "width: 170px;",
                    SelectValue {}
                }

                SelectList {
                    for (index, (value, label)) in FONT_FAMILIES.iter().enumerate() {
                        SelectOption::<String> {
                            key: "{value}",
                            index,
                            value: (*value).to_string(),
                            text_value: *label,
                            "{label}"
                            SelectItemIndicator {}
                        }
                    }
                }
            }
        }

        SettingRow {
            label: "Font Size",
            description: "Size of text in notes (10-24px)",

            div {
                style: "display: flex; align-items: center; gap: 8px;",
                Slider {
                    min: 10.0,
                    max: 24.0,
                    step: 1.0,
                    default_value: SliderValue::Single(f64::from(current_settings.font_size)),
                    on_value_change: move |slider_value: SliderValue| {
                        let SliderValue::Single(size) = slider_value;
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let clamped = (size as u32).clamp(10, 24);
                        on_font_size_change.call(clamped);
                    },
                    style: "width: 100px;",

                    SliderTrack {
                        SliderRange {}
                    }
                    SliderThumb {}
                }
                span {
                    class: "slider-value",
                    "{current_settings.font_size}px"
                }
            }
        }

        SettingRow {
            label: "Capture Hotkey",
            description: "Global shortcut for quick capture",

            div {
                class: "hotkey-display",
                style: "
                    background: {hotkey_bg};
                    border: 1px solid {hotkey_border};
                ",
                "Ctrl + Alt + N"
            }
        }
    }
}
