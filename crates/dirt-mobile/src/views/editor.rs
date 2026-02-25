                div {
                    style: "
                        padding: 10px 12px;
                        display: flex;
                        gap: 8px;
                        background: #ffffff;
                    ",
                    UiButton {
                        type: "button",
                        variant: ButtonVariant::Outline,
                        onclick: on_back_to_list,
                        "Back"
                    }
                    if saving() {
                        p {
                            style: "margin: 0; align-self: center; font-size: 12px; color: #6b7280;",
                            "Saving..."
                        }
                    }
                    if selected_note_id().is_some() {
                        UiButton {
                            type: "button",
                            variant: ButtonVariant::Danger,
                            style: "margin-left: auto;",
                            disabled: deleting(),
                            onclick: on_delete_note,
                            if deleting() { "Deleting..." } else { "Delete" }
                        }
                    }
                }

                Separator {
                    decorative: true,
                    style: "height: 1px; background: #e5e7eb;",
                }

                UiTextarea {
                    style: "
                        flex: 1;
                        margin: 12px;
                        border-radius: 12px;
                        padding: 14px;
                        line-height: 1.5;
                        font-size: 15px;
                    ",
                    value: "{draft_content}",
                    placeholder: "Write your note...",
                    oninput: move |event: Event<FormData>| {
                        draft_content.set(event.value());
                        draft_dirty.set(true);
                        draft_edit_version.set(draft_edit_version().saturating_add(1));
                    },
                }

                div {
                    style: "
                        margin: 0 12px 12px 12px;
                        padding: 10px;
                        border: 1px solid #e5e7eb;
                        border-radius: 10px;
                        background: #ffffff;
                        display: flex;
                        flex-direction: column;
                        gap: 6px;
                    ",
                    p {
                        style: "
                            margin: 0;
                            font-size: 12px;
                            font-weight: 700;
                            color: #6b7280;
                            text-transform: uppercase;
                            letter-spacing: 0.04em;
                        ",
                        "Attachments"
                    }
                    if selected_note_id().is_some() {
                        div {
                            style: "display: flex; flex-direction: column; gap: 8px;",
                            UiInput {
                                id: "attachment-file-input",
                                r#type: "file",
                                disabled: attachment_uploading()
                                    || voice_memo_state_value == VoiceMemoRecorderState::Stopping,
                                onchange: on_pick_attachment,
                            }
                            div {
                                style: "display: flex; gap: 8px; flex-wrap: wrap;",
                                if voice_memo_state_value == VoiceMemoRecorderState::Idle {
                                    UiButton {
                                        type: "button",
                                        variant: ButtonVariant::Outline,
                                        style: "padding: 6px 10px; font-size: 12px;",
                                        onclick: on_start_voice_memo,
                                        disabled: attachment_uploading(),
                                        "Record voice memo"
                                    }
                                } else if voice_memo_state_value == VoiceMemoRecorderState::Starting {
                                    UiButton {
                                        type: "button",
                                        variant: ButtonVariant::Outline,
                                        style: "padding: 6px 10px; font-size: 12px;",
                                        disabled: true,
                                        "Starting..."
                                    }
                                } else if voice_memo_state_value == VoiceMemoRecorderState::Recording {
                                    UiButton {
                                        type: "button",
                                        variant: ButtonVariant::Secondary,
                                        style: "padding: 6px 10px; font-size: 12px;",
                                        onclick: on_stop_voice_memo,
                                        disabled: attachment_uploading(),
                                        "Stop & attach"
                                    }
                                    UiButton {
                                        type: "button",
                                        variant: ButtonVariant::Outline,
                                        style: "padding: 6px 10px; font-size: 12px;",
                                        onclick: on_discard_voice_memo,
                                        disabled: attachment_uploading(),
                                        "Discard"
                                    }
                                } else {
                                    UiButton {
                                        type: "button",
                                        variant: ButtonVariant::Outline,
                                        style: "padding: 6px 10px; font-size: 12px;",
                                        disabled: true,
                                        "Attaching..."
                                    }
                                }
                            }
                        }
                    }

                    if let Some(status) = &voice_memo_status {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "{status}"
                        }
                    }

                    if attachment_uploading() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Uploading attachment..."
                        }
                    }
                    if let Some(error) = attachment_upload_error() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #b91c1c;",
                            "{error}"
                        }
                    }

                    if selected_note_id().is_none() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Save this note to view attachments."
                        }
                    } else if attachments_loading() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Loading attachments..."
                        }
                    } else if let Some(error) = attachments_error() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #b91c1c;",
                            "{error}"
                        }
                    } else if note_attachments().is_empty() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "No attachments yet."
                        }
                    } else {
                        for attachment in note_attachments() {
                            div {
                                key: "{attachment.id}",
                                style: "
                                    display: flex;
                                    flex-direction: column;
                                    gap: 8px;
                                    font-size: 12px;
                                    padding: 8px;
                                    border: 1px solid #e5e7eb;
                                    border-radius: 8px;
                                ",
                                div {
                                    style: "display: flex; justify-content: space-between; align-items: center; gap: 8px;",
                                    p {
                                        style: "
                                            margin: 0;
                                            color: #111827;
                                            min-width: 0;
                                            flex: 1;
                                            overflow: hidden;
                                            text-overflow: ellipsis;
                                            white-space: nowrap;
                                        ",
                                        "{attachment.filename}"
                                    }
                                    p {
                                        style: "margin: 0; color: #6b7280; white-space: nowrap;",
                                        "{attachment_kind_label(&attachment.filename, &attachment.mime_type)}"
                                    }
                                    p {
                                        style: "margin: 0; color: #6b7280; white-space: nowrap;",
                                        "{format_attachment_size(attachment.size_bytes)}"
                                    }
                                }
                                div {
                                    style: "display: flex; gap: 8px;",
                                    {
                                        let attachment_id = attachment.id;
                                        let attachment_for_preview = attachment.clone();
                                        let attachment_for_delete = attachment.clone();
                                        let deleting_now = deleting_attachment_id() == Some(attachment_id);

                                        rsx! {
                                            UiButton {
                                                type: "button",
                                                variant: ButtonVariant::Outline,
                                                style: "padding: 6px 10px; font-size: 12px;",
                                                disabled: deleting_now,
                                                onclick: move |_| {
                                                    let attachment_for_preview =
                                                        attachment_for_preview.clone();
                                                    attachment_preview_open.set(true);
                                                    attachment_preview_loading.set(true);
                                                    attachment_preview_error.set(None);
                                                    attachment_preview_content.set(AttachmentPreview::None);
                                                    attachment_preview_title.set(attachment_for_preview.filename.clone());

                                                    let media_api = media_api_client.read().clone();
                                                    let auth_session_value = auth_session();
                                                    spawn(async move {
                                                        match load_attachment_preview_from_r2(
                                                            &attachment_for_preview,
                                                            media_api,
                                                            auth_session_value,
                                                        )
                                                        .await {
                                                            Ok(preview) => attachment_preview_content.set(preview),
                                                            Err(error) => attachment_preview_error.set(Some(error)),
                                                        }
                                                        attachment_preview_loading.set(false);
                                                    });
                                                },
                                                "Open"
                                            }
                                            UiButton {
                                                type: "button",
                                                variant: ButtonVariant::Danger,
                                                style: "padding: 6px 10px; font-size: 12px;",
                                                disabled: deleting_now,
                                                onclick: move |_| {
                                                    let attachment_for_delete =
                                                        attachment_for_delete.clone();
                                                    let Some(note_store) = store.read().clone() else {
                                                        attachments_error.set(Some(
                                                            "Still initializing your notes. Please try again in a moment."
                                                                .to_string(),
                                                        ));
                                                        return;
                                                    };

                                                    deleting_attachment_id.set(Some(attachment_id));
                                                    attachments_error.set(None);

                                                    let media_api = media_api_client.read().clone();
                                                    let auth_session_value = auth_session();
                                                    spawn(async move {
                                                        match note_store.delete_attachment(&attachment_id).await {
                                                            Ok(()) => {
                                                                enqueue_pending_sync_change(
                                                                    attachment_for_delete.note_id,
                                                                    &mut pending_sync_note_ids,
                                                                    &mut pending_sync_count,
                                                                );
                                                                if let Err(error) = delete_attachment_object_from_r2(
                                                                    &attachment_for_delete.r2_key,
                                                                    media_api,
                                                                    auth_session_value,
                                                                )
                                                                .await {
                                                                    attachments_error.set(Some(format!(
                                                                        "Attachment removed, but failed to delete remote object: {error}"
                                                                    )));
                                                                }
                                                                attachment_refresh_version.set(attachment_refresh_version() + 1);
                                                                status_message.set(Some("Attachment deleted.".to_string()));
                                                            }
                                                            Err(error) => {
                                                                attachments_error.set(Some(format!(
                                                                    "Failed to delete attachment: {error}"
                                                                )));
                                                            }
                                                        }
                                                        deleting_attachment_id.set(None);
                                                    });
                                                },
                                                if deleting_now { "Deleting..." } else { "Delete" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if attachment_preview_open() {
                    div {
                        style: "
                            position: fixed;
                            inset: 0;
                            background: rgba(17, 24, 39, 0.55);
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            padding: 16px;
                            z-index: 9998;
                        ",
                        div {
                            style: "
                                width: 100%;
                                max-width: 520px;
                                max-height: 80vh;
                                background: #ffffff;
                                border-radius: 12px;
                                border: 1px solid #e5e7eb;
                                display: flex;
                                flex-direction: column;
                            ",
                            div {
                                style: "display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 12px; border-bottom: 1px solid #e5e7eb;",
                                p {
                                    style: "margin: 0; font-size: 14px; font-weight: 600; color: #111827; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    "{attachment_preview_title()}"
                                }
                                UiButton {
                                    type: "button",
                                    variant: ButtonVariant::Outline,
                                    style: "padding: 6px 10px; font-size: 12px;",
                                    onclick: on_close_attachment_preview,
                                    "Close"
                                }
                            }
                            div {
                                style: "padding: 12px; overflow: auto;",
                                if attachment_preview_loading() {
                                    p {
                                        style: "margin: 0; font-size: 12px; color: #6b7280;",
                                        "Loading preview..."
                                    }
                                } else if let Some(error) = attachment_preview_error() {
                                    p {
                                        style: "margin: 0; font-size: 12px; color: #b91c1c;",
                                        "{error}"
                                    }
                                } else {
                                    {render_attachment_preview(attachment_preview_content(), &attachment_preview_title())}
                                }
                            }
                        }
                    }
                }
