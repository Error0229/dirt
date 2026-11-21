//! Settings → Voice (interface design, sections 6.9 and 6.18): where speech
//! becomes text, the desktop's microphone, what is kept of a thought said
//! aloud, the language, how to talk from anywhere, and the credit the
//! speech model's licence asks for.

use dioxus::prelude::*;
use dirt_core::models::VoiceRecording;

use super::rows::{Card, Note, Row, RowIcon, Words};
use super::Task;
use crate::appearance::Platform;
use crate::components::{Segment, Segmented};
use crate::icons::{Icon, IconSize, IconSvg};

/// A microphone the desktop can record from: the system's ID for it, which
/// is what is kept, and the name it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Microphone {
    /// The system's ID for it, as text.
    pub id: String,
    /// Its name, as the system shows it.
    pub name: String,
}

/// The desktop's microphones, for Settings → Voice: the one chosen
/// (nothing for the system's default), and how to list those connected.
/// Android records from the system's microphone, and has no such row.
#[derive(Clone, PartialEq)]
pub struct Microphones {
    /// The ID of the one chosen; nothing for the system's default.
    pub chosen: Option<String>,
    /// Lists the microphones connected now.
    pub list: Callback<(), Task<Vec<Microphone>>>,
}

/// The first choice, and what the row says while it is chosen.
const DEFAULT_MIC: &str = "The system\u{2019}s default";

/// What the row says of a chosen microphone that is not connected.
const MIC_GONE: &str = "Not connected: the system\u{2019}s default is used";

/// What the microphone row says of the one chosen. Nothing while the
/// connected ones are still being listed, rather than a guess.
fn chosen_words(chosen: Option<&str>, connected: Option<&[Microphone]>) -> Option<String> {
    let Some(id) = chosen else {
        return Some(DEFAULT_MIC.to_owned());
    };
    let connected = connected?;
    Some(
        connected
            .iter()
            .find(|mic| mic.id == id)
            .map_or_else(|| MIC_GONE.to_owned(), |mic| mic.name.clone()),
    )
}

/// The microphone row, and under it, opened, the choices: the system's
/// default and each one connected, the chosen with a tick. The list is
/// asked for when the section is drawn and each time it opens, so that a
/// microphone plugged in meanwhile is there.
#[component]
fn MicrophoneChoice(microphones: Microphones, on_choose: EventHandler<Option<String>>) -> Element {
    let mut open = use_signal(|| false);
    let mut connected = use_signal(|| None::<Vec<Microphone>>);
    let list = microphones.list;
    let ask = move || {
        spawn(async move {
            let found = list.call(()).await;
            connected.set(Some(found));
        });
    };
    use_hook(ask);
    let chosen = microphones.chosen;
    let detail = chosen_words(chosen.as_deref(), connected.read().as_deref());
    let is_open = open();
    let listed = connected.read().clone().unwrap_or_default();
    rsx! {
        button {
            r#type: "button",
            class: "dui-set-row dui-set-press",
            "aria-expanded": if is_open { "true" } else { "false" },
            onclick: move |_| {
                if !open() {
                    ask();
                }
                open.toggle();
            },
            RowIcon { icon: Icon::Mic }
            Words { title: "Microphone", detail, danger: false }
            span { class: if is_open { "dui-set-go on" } else { "dui-set-go" },
                IconSvg { icon: Icon::ChevronRight, size: IconSize::Small }
            }
        }
        div { class: if is_open { "dui-disclose on" } else { "dui-disclose" },
            "inert": if is_open { None } else { Some("") },
            div { role: "radiogroup", "aria-label": "Microphone",
                Pick {
                    name: DEFAULT_MIC,
                    chosen: chosen.is_none(),
                    onclick: move |()| {
                        on_choose.call(None);
                        open.set(false);
                    },
                }
                for mic in listed {
                    Pick {
                        key: "{mic.id}",
                        name: mic.name,
                        chosen: chosen.as_deref() == Some(mic.id.as_str()),
                        onclick: move |()| {
                            on_choose.call(Some(mic.id.clone()));
                            open.set(false);
                        },
                    }
                }
            }
        }
    }
}

/// One of the microphones to choose from, with a tick if it is chosen.
#[component]
fn Pick(name: String, chosen: bool, onclick: EventHandler<()>) -> Element {
    rsx! {
        button {
            r#type: "button",
            role: "radio",
            class: "dui-set-row dui-set-press dui-set-pick",
            "aria-checked": if chosen { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            span { class: "dui-set-t", "{name}" }
            if chosen {
                IconSvg { icon: Icon::Check, size: IconSize::Small }
            }
        }
    }
}

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
    /// The desktop's microphones; nothing on Android.
    microphones: Option<Microphones>,
    on_microphone: EventHandler<Option<String>>,
    platform: Platform,
) -> Element {
    let choices = vec![
        Segment::new(VoiceRecording::Keep.name(), "Keep"),
        Segment::new(VoiceRecording::TextOnly.name(), "Text only"),
    ];
    rsx! {
        Card {
            Note { text: HEARD_HERE }
            if let Some(microphones) = microphones {
                MicrophoneChoice { microphones, on_choose: on_microphone }
            }
            Row {
                icon: Icon::Music,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mic(id: &str, name: &str) -> Microphone {
        Microphone {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn the_row_names_the_chosen_microphone_or_says_it_is_gone() {
        let connected = [mic("wasapi:a", "Headset"), mic("wasapi:b", "Desk mic")];
        assert_eq!(chosen_words(None, None).as_deref(), Some(DEFAULT_MIC));
        assert_eq!(
            chosen_words(Some("wasapi:b"), Some(&connected)).as_deref(),
            Some("Desk mic")
        );
        assert_eq!(
            chosen_words(Some("wasapi:c"), Some(&connected)).as_deref(),
            Some(MIC_GONE)
        );
        // Not listed yet: nothing is said rather than a guess.
        assert_eq!(chosen_words(Some("wasapi:b"), None), None);
    }
}
