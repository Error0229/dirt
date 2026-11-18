//! Find, and the list of tags that opens under it (interface design,
//! sections 6.1 and 6.3).
//!
//! Find is one line and stays one line. The tags chosen are tokens at its
//! start; when they would crowd the words they fold into one. The `#`
//! opens the list of tags, which opens in place and stays open while tags
//! are picked.
//!
//! What Find's text means is in [`crate::find`]. The field does not hold
//! the text for the app: the app hears every change, and changes the text
//! itself with [`crate::dom::set_value`], so that a key typed while the
//! app was thinking is never written over.

use dioxus::prelude::*;

use crate::appearance::Platform;
use crate::components::icon_button::{IconButton, IconButtonSize};
use crate::find::tokens_fold;
use crate::icons::{Icon, IconSize, IconSvg};

/// The ID of Find's text field in the page.
pub const FIND_INPUT_ID: &str = "dui-find";

/// The key of the one token the chosen tags fold into.
const FOLDED: &str = "folded";

/// The ID of the list of tags in the page.
pub const TAG_LIST_ID: &str = "dui-tags";

/// How wide the tokens and the words may be together on the desktop, in
/// CSS pixels: the list's 344 px less the field's margins, its icon and
/// its button.
const DESKTOP_ROOM: f64 = 236.0;

/// The field notes are found with.
#[component]
pub fn FindField(
    /// The tags chosen, in lower case and without their `#`.
    tags: Vec<String>,
    /// Whether the field holds any text, which decides how soon the tags
    /// fold.
    #[props(default)]
    has_text: bool,
    /// The list of tags is open: the `#` stays pressed.
    #[props(default)]
    tags_open: bool,
    oninput: EventHandler<String>,
    onkeydown: EventHandler<KeyboardEvent>,
    /// The cross on a token was pressed.
    on_remove_tag: EventHandler<String>,
    /// The `#` was pressed, or the folded token.
    on_tags: EventHandler<()>,
    /// The text field gained the keyboard's focus (true) or lost it.
    #[props(default)]
    on_focus: Option<EventHandler<bool>>,
    /// Android: the cross at the end of Find, which closes it.
    #[props(default)]
    on_close: Option<EventHandler<()>>,
    #[props(default = DESKTOP_ROOM)] room: f64,
    #[props(default = Platform::current())] platform: Platform,
) -> Element {
    // On Android the picked tags are the filled pills of the strip under
    // Find ([`TagStrip`]), not tokens in it (section 6.3).
    let tokens = platform == Platform::Desktop;
    let folded = tokens && tokens_fold(&tags, room, has_text);
    let all = tags
        .iter()
        .map(|tag| format!("#{tag}"))
        .collect::<Vec<_>>()
        .join(" ");
    let pressed = tags_open || !tags.is_empty();
    let hash = move || {
        rsx! {
            IconButton {
                icon: Icon::Hash,
                label: "Tags",
                shortcut: (platform == Platform::Desktop).then(|| "#".to_owned()),
                size: IconButtonSize::Small,
                pressed,
                class: "dui-find-hash",
                onclick: move |event: MouseEvent| {
                    event.stop_propagation();
                    on_tags.call(());
                },
            }
        }
    };
    rsx! {
        label {
            class: "dui-find",
            r#for: FIND_INPUT_ID,
            // A press on the field is not a press outside it.
            onclick: move |event: MouseEvent| event.stop_propagation(),
            if platform == Platform::Desktop {
                span { class: "dui-find-icon",
                    IconSvg { icon: Icon::Search }
                }
            } else {
                {hash()}
            }
            span { class: "dui-find-field",
                // The tokens are a list the script animates: one that is
                // removed shrinks away, and the rest close up.
                span {
                    class: "dui-toks",
                    "data-dui-list": "",
                    "data-dui-exit": "shrink",
                    if folded {
                        button {
                            key: "{FOLDED}",
                            r#type: "button",
                            class: "dui-tok fold",
                            "data-key": FOLDED,
                            "data-dui-enter": "none",
                            "data-tip": "{all}",
                            "aria-label": "Tags: {all}",
                            onclick: move |event: MouseEvent| {
                                event.stop_propagation();
                                event.prevent_default();
                                on_tags.call(());
                            },
                            "#{tags[0]}"
                            b { "+{tags.len() - 1}" }
                        }
                    } else if tokens {
                        for tag in tags.iter().cloned() {
                            span {
                                key: "{tag}",
                                class: "dui-tok",
                                "data-key": "{tag}",
                                "data-dui-enter": "none",
                                "#{tag}"
                                button {
                                    r#type: "button",
                                    "aria-label": "Remove #{tag}",
                                    onclick: move |event: MouseEvent| {
                                        event.stop_propagation();
                                        event.prevent_default();
                                        on_remove_tag.call(tag.clone());
                                    },
                                    IconSvg { icon: Icon::X, size: IconSize::Small }
                                }
                            }
                        }
                    }
                }
                input {
                    id: FIND_INPUT_ID,
                    class: "dui-find-input",
                    r#type: "text",
                    placeholder: "Find",
                    "aria-label": "Find a note",
                    "aria-controls": TAG_LIST_ID,
                    "aria-expanded": if tags_open { "true" } else { "false" },
                    autocomplete: "off",
                    spellcheck: "false",
                    // A pasted line break becomes a space: Find never grows.
                    "data-dui-oneline": "",
                    oninput: move |event: FormEvent| oninput.call(event.value()),
                    onkeydown: move |event| onkeydown.call(event),
                    onfocus: move |_| {
                        if let Some(on_focus) = on_focus {
                            on_focus.call(true);
                        }
                    },
                    onblur: move |_| {
                        if let Some(on_focus) = on_focus {
                            on_focus.call(false);
                        }
                    },
                }
            }
            if platform == Platform::Desktop {
                {hash()}
            }
            if let Some(on_close) = on_close {
                IconButton {
                    icon: Icon::X,
                    label: "Close",
                    size: IconButtonSize::Small,
                    class: "dui-find-close",
                    onclick: move |event: MouseEvent| {
                        event.stop_propagation();
                        event.prevent_default();
                        on_close.call(());
                    },
                }
            }
        }
    }
}

/// One tag in the list of tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagOption {
    /// The tag, in lower case and without its `#`.
    pub tag: String,
    /// How many notes there would be with this tag too. A picked tag has
    /// no number: it would only repeat how long the list is now.
    pub notes: Option<usize>,
    /// The tag is one of those chosen.
    pub picked: bool,
}

/// The list of tags, which opens in place under Find and pushes the notes
/// down, so that they stay in sight while tags are picked.
///
/// The picked tags come first, ticked; then at most six that could be
/// picked next, each with how many notes it would leave. The first of
/// those has Tab beside it on the desktop. With two or more picked, a
/// switch at the foot offers "any of them" for "all of them".
///
/// The list is always in the page, so that it can open and close; `open`
/// says which. The arrow keys move through it once the keyboard is in it,
/// and the up arrow at its top goes back to Find.
#[component]
pub fn TagList(
    open: bool,
    options: Vec<TagOption>,
    /// "Any of them" holds, not "all of them".
    #[props(default)]
    any: bool,
    on_pick: EventHandler<String>,
    /// The switch was pressed: true for "any of them".
    on_rule: EventHandler<bool>,
    #[props(default = Platform::current())] platform: Platform,
) -> Element {
    // A tag once: the app may hand one over as picked and as offered for
    // the moment between a pick and the offers read again.
    let mut seen = std::collections::HashSet::new();
    let options: Vec<TagOption> = options
        .into_iter()
        .filter(|option| seen.insert(option.tag.clone()))
        .collect();
    let first_free = options.iter().position(|option| !option.picked);
    let picked = options.iter().filter(|option| option.picked).count();
    rsx! {
        div {
            id: TAG_LIST_ID,
            class: if open { "dui-taglist on" } else { "dui-taglist" },
            role: "listbox",
            "aria-label": "Tags",
            "aria-multiselectable": "true",
            "inert": if open { None } else { Some("") },
            "data-dui-arrows": "#{FIND_INPUT_ID}",
            onclick: move |event: MouseEvent| event.stop_propagation(),
            for (index , option) in options.into_iter().enumerate() {
                button {
                    key: "{option.tag}",
                    r#type: "button",
                    class: format!(
                        "dui-tag-opt{}{}",
                        if option.picked { " on" } else { "" },
                        if Some(index) == first_free { " first" } else { "" },
                    ),
                    style: "--i:{index}",
                    role: "option",
                    "aria-selected": if option.picked { "true" } else { "false" },
                    onclick: move |_| on_pick.call(option.tag.clone()),
                    span { class: "dui-tag-tick",
                        IconSvg { icon: Icon::Check, size: IconSize::Small }
                    }
                    span { class: "dui-tag-hash", "#" }
                    "{option.tag}"
                    if let Some(notes) = option.notes {
                        span { class: "dui-tag-n", "{notes}" }
                    }
                    if Some(index) == first_free && platform == Platform::Desktop {
                        kbd { "aria-hidden": "true", "Tab" }
                    }
                }
            }
            if picked > 1 {
                div {
                    class: "dui-tag-rule",
                    role: "radiogroup",
                    "aria-label": "Notes with",
                    span { "aria-hidden": "true", "Notes with" }
                    span { class: "dui-mini-seg",
                        button {
                            r#type: "button",
                            role: "radio",
                            "aria-checked": if any { "false" } else { "true" },
                            onclick: move |_| on_rule.call(false),
                            "all of them"
                        }
                        button {
                            r#type: "button",
                            role: "radio",
                            "aria-checked": if any { "true" } else { "false" },
                            onclick: move |_| on_rule.call(true),
                            "any of them"
                        }
                    }
                }
            }
        }
    }
}

/// The tags under Find on Android (section 6.3): a strip of pills. The
/// picked ones are filled, at the front, and stay while any are picked;
/// while the `#` is pressed the rest follow, at most six. With two or more
/// picked, a last pill switches between "all of them" and "any of them".
/// There are no counts on Android: a tag that would empty the list is not
/// offered, as on the desktop.
///
/// The strip is always in the page, so that it can open and close; it is
/// open while `open` or while a tag is picked.
#[component]
pub fn TagStrip(
    open: bool,
    options: Vec<TagOption>,
    /// "Any of them" holds, not "all of them".
    #[props(default)]
    any: bool,
    on_pick: EventHandler<String>,
    /// The last pill was pressed: true for "any of them".
    on_rule: EventHandler<bool>,
) -> Element {
    let mut seen = std::collections::HashSet::new();
    let mut options: Vec<TagOption> = options
        .into_iter()
        .filter(|option| seen.insert(option.tag.clone()))
        .filter(|option| open || option.picked)
        .collect();
    // The picked ones first, in the order they were picked.
    options.sort_by_key(|option| !option.picked);
    let picked = options.iter().filter(|option| option.picked).count();
    let shown = open || picked > 0;
    let rule = if any { "any of them" } else { "all of them" };
    rsx! {
        div {
            id: TAG_LIST_ID,
            class: if shown { "dui-strip on" } else { "dui-strip" },
            role: "group",
            "aria-label": "Tags",
            "inert": if shown { None } else { Some("") },
            onclick: move |event: MouseEvent| event.stop_propagation(),
            "data-dui-list": "",
            "data-dui-exit": "fade",
            for (index , option) in options.into_iter().enumerate() {
                button {
                    key: "{option.tag}",
                    r#type: "button",
                    class: if option.picked { "dui-pill on" } else { "dui-pill" },
                    style: "--i:{index}",
                    "data-key": "{option.tag}",
                    "data-dui-enter": "none",
                    "aria-pressed": if option.picked { "true" } else { "false" },
                    onclick: move |_| on_pick.call(option.tag.clone()),
                    "#{option.tag}"
                }
            }
            if picked > 1 {
                button {
                    r#type: "button",
                    class: "dui-pill rule",
                    "data-key": "rule",
                    "data-dui-enter": "none",
                    "aria-label": "Notes with {rule}. Press for the other",
                    onclick: move |_| on_rule.call(!any),
                    "{rule}"
                    IconSvg { icon: Icon::ChevronDown, size: IconSize::Small }
                }
            }
        }
    }
}
