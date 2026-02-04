//! Sidebar component with tag list

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::state::AppState;

/// Sidebar showing tags and filters
#[component]
pub fn Sidebar() -> Element {
    let mut state = use_context::<AppState>();

    // Collect all unique tags with counts from notes
    let tag_counts: HashMap<String, usize> = {
        let notes = (state.notes)();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for note in notes.iter().filter(|n| !n.is_deleted) {
            for tag in note.tags() {
                *counts.entry(tag).or_insert(0) += 1;
            }
        }
        counts
    };

    // Sort tags by count (descending), then alphabetically
    let mut sorted_tags: Vec<(String, usize)> = tag_counts.into_iter().collect();
    sorted_tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let active_tag = (state.active_tag_filter)();
    let total_notes = (state.notes)().iter().filter(|n| !n.is_deleted).count();

    let active_style = "padding: 6px 10px; border-radius: 4px; cursor: pointer; margin-bottom: 4px; background: #e0e7ff; color: #3730a3;";
    let inactive_style =
        "padding: 6px 10px; border-radius: 4px; cursor: pointer; margin-bottom: 4px;";

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
                style: if active_tag.is_none() { active_style } else { inactive_style },
                onclick: move |_| {
                    state.active_tag_filter.set(None);
                },
                "All Notes "
                span {
                    style: "color: #666; font-size: 12px;",
                    "({total_notes})"
                }
            }

            // Tag list
            for (tag, count) in sorted_tags {
                {
                    let tag_clone = tag.clone();
                    let is_active = active_tag.as_ref() == Some(&tag);
                    rsx! {
                        div {
                            style: if is_active { active_style } else { inactive_style },
                            onclick: move |_| {
                                state.active_tag_filter.set(Some(tag_clone.clone()));
                            },
                            "#{tag} "
                            span {
                                style: "color: #666; font-size: 12px;",
                                "({count})"
                            }
                        }
                    }
                }
            }
        }
    }
}
