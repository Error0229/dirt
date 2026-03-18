//! Note list component with integrated tag chips

use std::collections::HashMap;
use std::time::Duration;

use dioxus::prelude::*;

use super::NoteCard;
use crate::state::AppState;

/// List of notes with tag filter chips
#[component]
pub fn NoteList() -> Element {
    let mut state = use_context::<AppState>();
    let mut timestamp_tick = use_signal(|| 0_u64);

    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            timestamp_tick.set(timestamp_tick().wrapping_add(1));
        }
    });

    // Force periodic rerender so relative timestamps stay fresh.
    _ = timestamp_tick();

    let note_list_visible = (state.note_list_visible)();
    if !note_list_visible {
        return rsx! {};
    }

    let colors = (state.theme)().palette();
    let current_id = (state.current_note_id)();
    let active_tag = (state.active_tag_filter)();
    let query = (state.search_query)().to_lowercase();

    // Single pass: compute tag counts, total, and filtered notes from one read.
    let all_notes = (state.notes)();

    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    let mut total_notes = 0usize;
    let mut filtered_notes = Vec::new();

    for note in &all_notes {
        if note.is_deleted {
            continue;
        }
        total_notes += 1;

        let tags = note.tags();
        for tag in &tags {
            *tag_counts.entry(tag.clone()).or_insert(0) += 1;
        }

        let matches_query = query.is_empty() || note.content.to_lowercase().contains(&query);
        let matches_tag = active_tag
            .as_ref()
            .map_or(true, |filter_tag| tags.iter().any(|t| t == filter_tag));

        if matches_query && matches_tag {
            filtered_notes.push(note.clone());
        }
    }

    let mut sorted_tags: Vec<(String, usize)> = tag_counts.into_iter().collect();
    sorted_tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    rsx! {
        div {
            class: "note-list-panel",
            style: "
                width: 240px;
                min-width: 240px;
                display: flex;
                flex-direction: column;
                border-right: 1px solid {colors.border};
                background: {colors.bg_secondary};
                overflow: hidden;
            ",

            // Tag chips row
            if !sorted_tags.is_empty() || total_notes > 0 {
                div {
                    class: "tag-chips-row",
                    style: "
                        display: flex;
                        align-items: center;
                        gap: 4px;
                        padding: 4px 6px;
                        overflow-x: auto;
                        flex-shrink: 0;
                        border-bottom: 1px solid {colors.border};
                    ",

                    // "All" chip
                    TagChip {
                        label: format!("All {total_notes}"),
                        is_active: active_tag.is_none(),
                        accent: colors.accent,
                        border_color: colors.border,
                        bg_active: colors.bg_tertiary,
                        text_active: colors.text_primary,
                        text_inactive: colors.text_muted,
                        onclick: move |_| {
                            state.active_tag_filter.set(None);
                        },
                    }

                    for (tag, count) in sorted_tags {
                        {
                            let tag_clone = tag.clone();
                            let is_active = active_tag.as_ref() == Some(&tag);
                            rsx! {
                                TagChip {
                                    label: format!("#{tag} {count}"),
                                    is_active: is_active,
                                    accent: colors.accent,
                                    border_color: colors.border,
                                    bg_active: colors.bg_tertiary,
                                    text_active: colors.text_primary,
                                    text_inactive: colors.text_muted,
                                    onclick: move |_| {
                                        state.active_tag_filter.set(Some(tag_clone.clone()));
                                    },
                                }
                            }
                        }
                    }
                }
            }

            // Note cards
            div {
                class: "note-cards",
                style: "
                    flex: 1;
                    overflow-y: auto;
                    padding: 0;
                ",

                if filtered_notes.is_empty() {
                    div {
                        style: "
                            padding: 24px 12px;
                            text-align: center;
                            color: {colors.text_muted};
                            font-size: 13px;
                            font-style: italic;
                        ",
                        "Hit + to capture a thought"
                    }
                } else {
                    for note in filtered_notes {
                        {
                            let note_id = note.id;
                            let is_selected = current_id == Some(note_id);
                            let title = note.title_preview(40);
                            let tags = note.tags();
                            let preview = if tags.is_empty() {
                                note.content.lines().nth(1).unwrap_or("").chars().take(40).collect::<String>()
                            } else {
                                tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")
                            };
                            let updated_at_ms = note.updated_at;

                            rsx! {
                                NoteCard {
                                    key: "{note_id}",
                                    note_id,
                                    title,
                                    preview,
                                    updated_at_ms,
                                    is_selected,
                                    onclick: move |_| {
                                        state.current_note_id.set(Some(note_id));
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Compact tag chip — receives colors as props to avoid redundant context lookups.
#[component]
fn TagChip(
    label: String,
    is_active: bool,
    accent: &'static str,
    border_color: &'static str,
    bg_active: &'static str,
    text_active: &'static str,
    text_inactive: &'static str,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let bg = if is_active { bg_active } else { "transparent" };
    let border = if is_active {
        format!("1px solid {accent}")
    } else {
        format!("1px solid {border_color}")
    };
    let text_color = if is_active {
        text_active
    } else {
        text_inactive
    };

    rsx! {
        button {
            style: "
                height: 22px;
                padding: 0 8px;
                border: {border};
                border-radius: 11px;
                background: {bg};
                color: {text_color};
                font-size: 11px;
                font-weight: 500;
                white-space: nowrap;
                cursor: pointer;
                transition: all 0.1s;
                flex-shrink: 0;
            ",
            onclick: move |evt| onclick.call(evt),
            "{label}"
        }
    }
}
