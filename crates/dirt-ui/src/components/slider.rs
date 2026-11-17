//! The slider (interface design, section 6.11), with a small and a large
//! "A" at its ends for text size.
//!
//! While the thumb is held, what it sets follows it in the script: the
//! script writes the value into a custom property of the page's root.
//! Rust hears once, when the thumb is let go.

use dioxus::prelude::*;

use crate::appearance::TextSize;

/// A slider. `label` is its name for a screen reader.
#[component]
pub fn Slider(
    label: String,
    min: f64,
    max: f64,
    #[props(default = 1.0)] step: f64,
    value: f64,
    /// A custom property of the page's root that follows the thumb while
    /// it is held, such as `--note-size`, and the unit to write after the
    /// number.
    #[props(default)]
    live_property: Option<String>,
    #[props(default)] live_unit: String,
    /// Small and large letters at the two ends.
    #[props(default)]
    letters: bool,
    /// The value the thumb was let go at.
    onchange: EventHandler<f64>,
) -> Element {
    rsx! {
        span { class: "dui-slider",
            if letters {
                span { class: "dui-slider-a small", "aria-hidden": "true", "A" }
            }
            input {
                r#type: "range",
                "aria-label": "{label}",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                "data-dui-live": live_property,
                "data-dui-unit": "{live_unit}",
                // `change` comes when the thumb is let go, and for each
                // arrow key. `input`, which comes with every movement, is
                // the script's.
                onchange: move |event| {
                    if let Ok(value) = event.value().parse::<f64>() {
                        onchange.call(value);
                    }
                },
            }
            if letters {
                span { class: "dui-slider-a large", "aria-hidden": "true", "A" }
            }
        }
    }
}

/// The slider for the size of a note's text, 13 to 20 px. The text on the
/// page grows and shrinks as the thumb moves.
#[component]
pub fn TextSizeSlider(value: TextSize, onchange: EventHandler<TextSize>) -> Element {
    rsx! {
        Slider {
            label: "Text size",
            min: f64::from(TextSize::MIN.pixels()),
            max: f64::from(TextSize::MAX.pixels()),
            value: f64::from(value.pixels()),
            live_property: "--note-size",
            live_unit: "px",
            letters: true,
            onchange: move |pixels: f64| {
                // The slider's own range keeps this between 13 and 20.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let pixels = pixels.round().clamp(0.0, 100.0) as u32;
                onchange.call(TextSize::new(pixels));
            },
        }
    }
}
