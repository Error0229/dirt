                if store.read().is_none() {
                    div {
                        style: "
                            flex: 1;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            padding: 20px;
                        ",
                        div {
                            style: "
                                width: 100%;
                                max-width: 360px;
                                background: #ffffff;
                                border: 1px solid #e5e7eb;
                                border-radius: 12px;
                                padding: 16px;
                                display: flex;
                                flex-direction: column;
                                gap: 10px;
                                color: #374151;
                            ",
                            p {
                                style: "margin: 0; font-size: 14px; font-weight: 600; color: #111827;",
                                "Database initialization failed"
                            }
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "Retry initialization to continue."
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Primary,
                                onclick: on_retry_db_init,
                                disabled: loading(),
                                "Retry"
                            }
                        }
                    }
                } else {
                    div {
                        style: "padding: 12px 16px; display: flex; gap: 8px;",
                        UiButton {
                            type: "button",
                            block: true,
                            variant: ButtonVariant::Secondary,
                            style: "font-size: 14px; padding: 12px;",
                            onclick: on_new_note,
                            "New note"
                        }
                    }

                    div {
                        style: "padding: 0 16px 12px 16px; display: flex; flex-direction: column; gap: 8px;",
                        Label {
                            html_for: "note-search",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Search"
                        }
                        UiInput {
                            id: "note-search",
                            r#type: "search",
                            placeholder: "Search notes...",
                            value: "{search_query_value}",
                            oninput: move |event: Event<FormData>| {
                                search_query.set(event.value());
                            },
                        }

                        if !available_tags.is_empty() {
                            div {
                                style: "display: flex; gap: 6px; flex-wrap: wrap;",
                                UiButton {
                                    type: "button",
                                    variant: if active_tag_filter_value.is_none() {
                                        ButtonVariant::Secondary
                                    } else {
                                        ButtonVariant::Outline
                                    },
                                    style: "padding: 6px 10px; font-size: 12px;",
                                    onclick: move |_| active_tag_filter.set(None),
                                    "All tags"
                                }
                                for tag in available_tags {
                                    {
                                        let tag_label = format!("#{tag}");
                                        let tag_value = tag.clone();
                                        let is_active =
                                            active_tag_filter_value.as_deref() == Some(tag.as_str());

                                        rsx! {
                                            UiButton {
                                                key: "{tag}",
                                                type: "button",
                                                variant: if is_active {
                                                    ButtonVariant::Secondary
                                                } else {
                                                    ButtonVariant::Outline
                                                },
                                                style: "padding: 6px 10px; font-size: 12px;",
                                                onclick: move |_| active_tag_filter.set(Some(tag_value.clone())),
                                                "{tag_label}"
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if has_active_note_filters {
                            div {
                                style: "display: flex; align-items: center; justify-content: space-between; gap: 8px;",
                                p {
                                    style: "margin: 0; font-size: 12px; color: #6b7280;",
                                    "Showing {filtered_note_count} of {total_note_count} notes"
                                }
                                UiButton {
                                    type: "button",
                                    variant: ButtonVariant::Outline,
                                    style: "padding: 6px 10px; font-size: 12px;",
                                    onclick: move |_| {
                                        search_query.set(String::new());
                                        active_tag_filter.set(None);
                                    },
                                    "Clear filters"
                                }
                            }
                        }
                    }

                    ScrollArea {
                        direction: ScrollDirection::Vertical,
                        scroll_type: ScrollType::Auto,
                        tabindex: "0",
                        style: "flex: 1; padding: 0 12px 16px 12px;",

                        if all_notes.is_empty() {
                            div {
                                style: "
                                    margin-top: 24px;
                                    padding: 20px;
                                    background: #ffffff;
                                    border: 1px solid #e5e7eb;
                                    border-radius: 12px;
                                    text-align: center;
                                    color: #6b7280;
                                ",
                                "No notes yet. Create your first note."
                            }
                        } else if filtered_notes.is_empty() {
                            div {
                                style: "
                                    margin-top: 24px;
                                    padding: 20px;
                                    background: #ffffff;
                                    border: 1px solid #e5e7eb;
                                    border-radius: 12px;
                                    text-align: center;
                                    color: #6b7280;
                                ",
                                "No notes match the current filters."
                            }
                        } else {
                            for note in filtered_notes {
                                {
                                    let note_id = note.id;
                                    let note_content = note.content.clone();
                                    let title = note_title(&note);
                                    let preview = note_preview(&note);
                                    let updated = relative_time(note.updated_at);
                                    let selected = selected_note_id() == Some(note_id);
                                    let border_color = if selected { "#2563eb" } else { "#e5e7eb" };
                                    let card_style = format!(
                                        "margin-bottom: 10px;\
                                         width: 100%;\
                                         border: 1px solid {border_color};\
                                         background: #ffffff;\
                                         border-radius: 12px;\
                                         padding: 12px;\
                                         text-align: left;"
                                    );

                                    rsx! {
                                        UiButton {
                                            key: "{note_id}",
                                            type: "button",
                                            variant: ButtonVariant::Ghost,
                                            style: "{card_style}",
                                            onclick: move |_| {
                                                selected_note_id.set(Some(note_id));
                                                draft_content.set(note_content.clone());
                                                draft_dirty.set(false);
                                                status_message.set(None);
                                                attachment_upload_error.set(None);
                                                attachment_preview_open.set(false);
                                                view.set(MobileView::Editor);
                                            },

                                            p {
                                                style: "
                                                    margin: 0 0 6px 0;
                                                    font-size: 15px;
                                                    font-weight: 600;
                                                    color: #111827;
                                                ",
                                                "{title}"
                                            }
                                            p {
                                                style: "
                                                    margin: 0 0 6px 0;
                                                    font-size: 13px;
                                                    color: #6b7280;
                                                ",
                                                "{preview}"
                                            }
                                            p {
                                                style: "margin: 0; font-size: 12px; color: #9ca3af;",
                                                "Updated {updated}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
