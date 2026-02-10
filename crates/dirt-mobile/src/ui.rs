//! Shared mobile UI primitives aligned with official Dioxus component patterns.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use dioxus::prelude::*;

/// Shared styles for mobile button/input/textarea wrappers.
pub const MOBILE_UI_STYLES: &str = r"
.ui-button {
    border-radius: 10px;
    padding: 10px 12px;
    font-size: 13px;
    font-weight: 600;
    border: 1px solid transparent;
    transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease;
}

.ui-button:disabled {
    opacity: 0.55;
}

.ui-button--block {
    width: 100%;
}

.ui-button--primary {
    background: #2563eb;
    color: #ffffff;
    border-color: #2563eb;
}

.ui-button--secondary {
    background: #111827;
    color: #ffffff;
    border-color: #111827;
}

.ui-button--outline {
    background: #ffffff;
    color: #374151;
    border-color: #d1d5db;
}

.ui-button--ghost {
    background: transparent;
    color: #374151;
    border-color: transparent;
}

.ui-button--danger {
    background: #dc2626;
    color: #ffffff;
    border-color: #dc2626;
}

.ui-input {
    width: 100%;
    border: 1px solid #d1d5db;
    border-radius: 10px;
    padding: 10px 12px;
    font-size: 13px;
    background: #ffffff;
    color: #111827;
}

.ui-textarea {
    width: 100%;
    border: 1px solid #d1d5db;
    border-radius: 10px;
    padding: 10px 12px;
    font-size: 13px;
    background: #ffffff;
    color: #111827;
    resize: none;
}
";

/// Button variant mapping.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Outline,
    Ghost,
    Danger,
}

impl ButtonVariant {
    const fn class(self) -> &'static str {
        match self {
            Self::Primary => "ui-button--primary",
            Self::Secondary => "ui-button--secondary",
            Self::Outline => "ui-button--outline",
            Self::Ghost => "ui-button--ghost",
            Self::Danger => "ui-button--danger",
        }
    }
}

#[component]
pub fn UiButton(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] block: bool,
    #[props(default)] disabled: bool,
    onclick: Option<EventHandler<MouseEvent>>,
    #[props(extends = GlobalAttributes)]
    #[props(extends = button)]
    attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let mut class_name = format!("ui-button {}", variant.class());
    if block {
        class_name.push_str(" ui-button--block");
    }

    rsx! {
        button {
            class: "{class_name}",
            disabled,
            onclick: move |event| {
                if let Some(handler) = &onclick {
                    handler.call(event);
                }
            },
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn UiInput(
    oninput: Option<EventHandler<FormEvent>>,
    onchange: Option<EventHandler<FormEvent>>,
    #[props(extends = GlobalAttributes)]
    #[props(extends = input)]
    attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        input {
            class: "ui-input",
            oninput: move |event| _ = oninput.map(|handler| handler(event)),
            onchange: move |event| _ = onchange.map(|handler| handler(event)),
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn UiTextarea(
    oninput: Option<EventHandler<FormEvent>>,
    onchange: Option<EventHandler<FormEvent>>,
    #[props(extends = GlobalAttributes)]
    #[props(extends = textarea)]
    attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        textarea {
            class: "ui-textarea",
            oninput: move |event| _ = oninput.map(|handler| handler(event)),
            onchange: move |event| _ = onchange.map(|handler| handler(event)),
            ..attributes,
            {children}
        }
    }
}
