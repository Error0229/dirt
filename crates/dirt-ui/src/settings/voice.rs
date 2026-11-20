//! Settings → Voice (interface design, sections 6.9 and 6.18): where speech
//! becomes text, what is kept of a thought said aloud, the language, how
//! to talk from anywhere, and the credit the speech model's licence asks
//! for.

use dioxus::prelude::*;
use dirt_core::models::VoiceRecording;

use super::rows::{Card, Note, Row};
use crate::appearance::Platform;
use crate::components::{Segment, Segmented};
use crate::icons::Icon;

/// The one line the section opens with (6.18).
pub const HEARD_HERE: &str = "Turned into text here. Your voice is never sent anywhere.";

/// What Android's Settings → Voice says of talking from a hardware key.
pub const SIDE_KEY: &str = "Set your phone's side key, or Quick Tap, to open Dirt Talk: it listens at once, over the lock screen too where the phone allows, and keeps the thought at a pause";

/// The credit the model's licence, CC BY 4.0, asks for: who made it, the
/// licence, and that it was changed.
pub const MODEL_CREDIT: &str =
    "Parakeet TDT-CTC 110M by NVIDIA, under CC BY 4.0, converted to ONNX by the sherpa-onnx project";

#[component]
pub(super) fn Voice(
    recording: VoiceRecording,
    on_recording: EventHandler<VoiceRecording>,
    platform: Platform,
) -> Element {
    let choices = vec![
        Segment::new(VoiceRecording::Keep.name(), "Keep"),
        Segment::new(VoiceRecording::TextOnly.name(), "Text only"),
    ];
    rsx! {
        Card {
            Note { text: HEARD_HERE }
            Row {
                icon: Icon::Mic,
                title: "The recording",
                detail: match recording {
                    VoiceRecording::Keep => "Kept on the note, with its words",
                    VoiceRecording::TextOnly => "Taken off once its words are made",
                },
                Segmented {
                    label: "The recording",
                    small: true,
                    segments: choices,
                    value: recording.name(),
                    onchange: move |value: String| {
                        on_recording.call(VoiceRecording::from_name(&value).unwrap_or_default());
                    },
                }
            }
            // English alone has been tried (decided 2026-11-16), and the
            // list holds only what has been: there is nothing to choose.
            Row { icon: Icon::Globe, title: "Language", detail: "Other languages come later",
                span { class: "dui-set-value", "English" }
            }
            if platform == Platform::Desktop {
                Row { icon: Icon::Laptop, title: "Talk from any app", detail: "Hold the keys, speak, let go",
                    span { class: "dui-keys", kbd { "Ctrl" } kbd { "Alt" } kbd { "N" } }
                }
            } else {
                // Android lets a hardware key open an app, not a page of
                // one: Talk is among the apps as "Dirt Talk" (8.9).
                Row { icon: Icon::Phone, title: "Talk without opening Dirt", detail: SIDE_KEY }
            }
            Row { icon: Icon::Info, title: "Speech model", detail: MODEL_CREDIT }
        }
    }
}
