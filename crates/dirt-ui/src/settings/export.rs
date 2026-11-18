//! Settings → Export (interface design, section 9.6): JSON or Markdown,
//! **Export**, and where. The button fills as it works, and a toast says
//! how many notes went where. Files that could not be had are listed in
//! the toast's place, under the button, with why.

use dioxus::prelude::*;
use dirt_core::export::ExportFormat;

use super::rows::Btn;
use super::ExportTo;
use crate::appearance::Platform;
use crate::components::{use_toasts, Segment, Segmented, Toast};
use crate::icons::Icon;

/// The list of files left out, brought into sight when it appears.
const LEFT_OUT_ID: &str = "dui-set-left-out";

#[component]
pub(super) fn ExportSection(export: ExportTo, platform: Platform) -> Element {
    let toasts = use_toasts();
    let mut format = use_signal(|| ExportFormat::Json);
    let mut busy = use_signal(|| false);
    let mut left_out = use_signal(Vec::<String>::new);
    let run = move |_: MouseEvent| {
        if *busy.peek() {
            return;
        }
        busy.set(true);
        left_out.set(Vec::new());
        let chosen = *format.peek();
        spawn(async move {
            match export.call(chosen).await {
                None => {}
                Some(Ok(done)) => {
                    toasts.show(Toast::new(done.said).icon(Icon::Check));
                    let some_left_out = !done.left_out.is_empty();
                    left_out.set(done.left_out);
                    if some_left_out {
                        // Once it has been drawn.
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        crate::dom::reveal(LEFT_OUT_ID);
                    }
                }
                Some(Err(reason)) => {
                    toasts.show_error(Toast::new("Nothing was exported").detail(reason));
                }
            }
            busy.set(false);
        });
    };
    let value = match format() {
        ExportFormat::Json => "json",
        ExportFormat::Markdown => "markdown",
    };
    rsx! {
        div { class: "dui-set-card",
            div { class: "dui-set-row",
                div { class: "dui-set-words",
                    div { class: "dui-set-t", "Every note and file" }
                    div { class: "dui-set-s",
                        if platform == Platform::Android { "As one zip file" } else { "Into a new folder" }
                    }
                }
                Segmented {
                    label: "Export as",
                    small: true,
                    segments: vec![Segment::new("json", "JSON"), Segment::new("markdown", "Markdown")],
                    value,
                    onchange: move |value: String| {
                        format.set(if value == "markdown" { ExportFormat::Markdown } else { ExportFormat::Json });
                    },
                }
                span { class: if busy() { "dui-fill on" } else { "dui-fill" },
                    Btn { icon: Icon::Download, disabled: busy(), onclick: run,
                        if busy() { "Exporting" } else { "Export" }
                    }
                }
            }
            if !left_out.read().is_empty() {
                ul { id: LEFT_OUT_ID, class: "dui-set-list", role: "status",
                    for (index , line) in left_out.read().iter().enumerate() {
                        li { key: "{index}", "{line}" }
                    }
                }
            }
        }
    }
}
