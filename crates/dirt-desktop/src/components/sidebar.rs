//! Sidebar component with tag list

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::state::AppState;

/// Sidebar showing tags and filters
#[component]
pub fn Sidebar() -> Element {
    let mut state = use_context::<AppState>();
    let colors = (state.theme)().palette();

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

    rsx! {
        aside {
            class: "sidebar",
            style: "
                width: 200px;
                background: {colors.bg_secondary};
                border-right: 1px solid {colors.border};
                padding: 16px;
                overflow-y: auto;
            ",

            h2 {
                style: "
                    font-size: 14px;
                    font-weight: 600;
                    margin-bottom: 12px;
                    color: {colors.text_secondary};
                ",
                "Tags"
            }

            // All notes filter
            TagItem {
                label: "All Notes",
                count: total_notes,
                is_active: active_tag.is_none(),
                onclick: move |_| {
                    state.active_tag_filter.set(None);
                },
            }

            // Tag list
            for (tag, count) in sorted_tags {
                {
                    let tag_clone = tag.clone();
                    let is_active = active_tag.as_ref() == Some(&tag);
                    rsx! {
                        TagItem {
                            label: "#{tag}",
                            count: count,
                            is_active: is_active,
                            onclick: move |_| {
                                state.active_tag_filter.set(Some(tag_clone.clone()));
                            },
                        }
                    }
                }
            }
        }
    }
}

/// Tag item in the sidebar
#[component]
fn TagItem(
    label: String,
    count: usize,
    is_active: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let state = use_context::<AppState>();
    let colors = (state.theme)().palette();

    let bg = if is_active {
        colors.accent
    } else {
        "transparent"
    };
    let text_color = if is_active {
        colors.accent_text
    } else {
        colors.text_primary
    };
    let count_color = if is_active {
        colors.accent_text
    } else {
        colors.text_muted
    };

    rsx! {
        div {
            style: "
                padding: 8px 10px;
                border-radius: 6px;
                cursor: pointer;
                margin-bottom: 4px;
                background: {bg};
                color: {text_color};
                display: flex;
                justify-content: space-between;
                align-items: center;
                transition: background 0.15s;
            ",
            onclick: onclick,
            span { "{label}" }
            span {
                style: "color: {count_color}; font-size: 12px;",
                "{count}"
            }
        }
    }
}
