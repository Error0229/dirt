//! Sidebar component with tag list

use dioxus::prelude::*;

use crate::state::AppState;

/// Sidebar showing tags and filters
#[component]
pub fn Sidebar() -> Element {
    let mut state = use_context::<AppState>();

    // Collect all unique tags from notes
    let all_tags: Vec<String> = {
        let notes = (state.notes)();
        let mut tags: Vec<String> = notes.iter().flat_map(dirt_core::Note::tags).collect();
        tags.sort();
        tags.dedup();
        tags
    };

    let active_tag = (state.active_tag_filter)();

    rsx! {
        aside {
            class: "sidebar",
            style: "width: 200px; background: var(--sidebar-bg, #f5f5f5); border-right: 1px solid var(--border-color, #e0e0e0); padding: 16px; overflow-y: auto;",

            h2 {
                style: "font-size: 14px; font-weight: 600; margin-bottom: 12px; color: var(--text-secondary, #666);",
                "Tags"
            }

            // All notes filter
            div {
                class: if active_tag.is_none() { "tag-item active" } else { "tag-item" },
                style: "padding: 6px 10px; border-radius: 4px; cursor: pointer; margin-bottom: 4px;",
                onclick: move |_| {
                    state.active_tag_filter.set(None);
                },
                "All Notes"
            }

            // Tag list
            for tag in all_tags {
                {
                    let tag_clone = tag.clone();
                    let is_active = active_tag.as_ref() == Some(&tag);
                    rsx! {
                        div {
                            class: if is_active { "tag-item active" } else { "tag-item" },
                            style: "padding: 6px 10px; border-radius: 4px; cursor: pointer; margin-bottom: 4px;",
                            onclick: move |_| {
                                state.active_tag_filter.set(Some(tag_clone.clone()));
                            },
                            "#{tag}"
                        }
                    }
                }
            }
        }
    }
}
