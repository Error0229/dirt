use dioxus::prelude::*;
use dioxus_primitives::slider::SliderValue;

use dirt_core::models::{FocusStyle, Settings};
use dirt_ui::TextSize;

use super::row::SettingRow;
use crate::components::select::{
    Select, SelectItemIndicator, SelectList, SelectOption, SelectTrigger, SelectValue,
};
use crate::components::slider::{Slider, SliderRange, SliderThumb, SliderTrack};

#[component]
pub(super) fn ThemeSettingsTab(
    hotkey_bg: &'static str,
    hotkey_border: &'static str,
    current_settings: Settings,
    current_theme_value: String,
    on_theme_change: EventHandler<String>,
    on_font_size_change: EventHandler<u32>,
    on_focus_style_change: EventHandler<String>,
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
            label: "Text size",
            description: "Size of text in notes (13-20 px). Ctrl + and Ctrl - change it too",

            div {
                style: "display: flex; align-items: center; gap: 8px;",
                Slider {
                    min: f64::from(TextSize::MIN.pixels()),
                    max: f64::from(TextSize::MAX.pixels()),
                    step: 1.0,
                    default_value: SliderValue::Single(f64::from(current_settings.font_size)),
                    on_value_change: move |slider_value: SliderValue| {
                        let SliderValue::Single(size) = slider_value;
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let clamped = TextSize::new(size as u32).pixels();
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
            label: "Focus while writing",
            description: "How the list gets out of the way while you write",

            Select::<String> {
                default_value: current_settings.focus_style.name().to_owned(),
                on_value_change: move |value: Option<String>| {
                    if let Some(value) = value {
                        on_focus_style_change.call(value);
                    }
                },

                SelectTrigger {
                    style: "width: 150px;",
                    SelectValue {}
                }

                SelectList {
                    for (index , style) in FocusStyle::ALL.into_iter().enumerate() {
                        SelectOption::<String> {
                            key: "{style.name()}",
                            index,
                            value: style.name().to_owned(),
                            text_value: style.label(),
                            "{style.label()}"
                            SelectItemIndicator {}
                        }
                    }
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
