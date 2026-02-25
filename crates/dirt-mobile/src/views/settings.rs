                ScrollArea {
                    direction: ScrollDirection::Vertical,
                    scroll_type: ScrollType::Auto,
                    tabindex: "0",
                    style: "flex: 1; padding: 12px;",

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 6px;
                            margin-bottom: 10px;
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
                            "Sync"
                        }
                        p {
                            style: "margin: 0; font-size: 14px; color: #111827;",
                            "{sync_state_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Last successful sync: {last_sync_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Scheduler: {sync_scheduler_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Last scheduler attempt: {last_sync_attempt_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Consecutive sync failures: {consecutive_sync_failures}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Pending local changes: {pending_sync_count_value}"
                        }
                        if pending_sync_count_value > 0 {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "Pending note IDs: {pending_sync_preview}"
                            }
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Provisioning: {provisioning.sync_status}"
                        }
                        if let Some(sync_action) = provisioning.sync_action.as_ref() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "{sync_action}"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
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
                            "API keys"
                        }
                        Label {
                            html_for: "openai-api-key",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "OpenAI API key"
                        }
                        UiInput {
                            id: "openai-api-key",
                            r#type: "password",
                            placeholder: "sk-...",
                            value: "{openai_api_key_input}",
                            oninput: move |event: Event<FormData>| {
                                openai_api_key_input.set(event.value());
                            },
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            if openai_api_key_configured() {
                                "OpenAI API key is stored securely on this device."
                            } else {
                                "No OpenAI API key is currently stored."
                            }
                        }
                        div {
                            style: "display: flex; gap: 8px;",
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Primary,
                                onclick: on_save_openai_api_key,
                                "Save key"
                            }
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Outline,
                                onclick: on_clear_openai_api_key,
                                "Clear key"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
                        ",
                        div {
                            style: "display: flex; align-items: center; justify-content: space-between; gap: 8px;",
                            p {
                                style: "
                                    margin: 0;
                                    font-size: 12px;
                                    font-weight: 700;
                                    color: #6b7280;
                                    text-transform: uppercase;
                                    letter-spacing: 0.04em;
                                ",
                                "Sync conflicts"
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Outline,
                                style: "padding: 6px 10px; font-size: 12px;",
                                disabled: sync_conflicts_loading(),
                                onclick: on_refresh_sync_conflicts,
                                "Refresh"
                            }
                        }

                        if sync_conflicts_loading() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "Loading recent conflicts..."
                            }
                        } else if let Some(error) = sync_conflicts_error() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #b91c1c;",
                                "{error}"
                            }
                        } else if sync_conflicts().is_empty() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "No sync conflicts recorded yet."
                            }
                        } else {
                            div {
                                style: "display: flex; flex-direction: column; gap: 8px;",
                                for conflict in sync_conflicts() {
                                    div {
                                        key: "{conflict.id}",
                                        style: "padding: 8px; border: 1px solid #e5e7eb; border-radius: 8px; display: flex; flex-direction: column; gap: 3px;",
                                        p {
                                            style: "margin: 0; font-size: 12px; color: #111827;",
                                            "Note {conflict.note_id}"
                                        }
                                        p {
                                            style: "margin: 0; font-size: 11px; color: #6b7280;",
                                            "Resolved: {format_sync_conflict_time(conflict.resolved_at)}"
                                        }
                                        p {
                                            style: "margin: 0; font-size: 11px; color: #6b7280;",
                                            "Local ts: {conflict.local_updated_at}, incoming ts: {conflict.incoming_updated_at}, strategy: {conflict.strategy}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
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
                            "Authentication"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Session: {auth_session_summary}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Auth config: {auth_config_summary_text}"
                        }
                        Label {
                            html_for: "auth-email",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Email"
                        }
                        UiInput {
                            id: "auth-email",
                            r#type: "email",
                            placeholder: "Email",
                            value: "{auth_email_input}",
                            oninput: move |event: Event<FormData>| {
                                auth_email_input.set(event.value());
                            },
                        }
                        Label {
                            html_for: "auth-password",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Password"
                        }
                        UiInput {
                            id: "auth-password",
                            r#type: "password",
                            placeholder: "Password",
                            value: "{auth_password_input}",
                            oninput: move |event: Event<FormData>| {
                                auth_password_input.set(event.value());
                            },
                        }
                        div {
                            style: "display: flex; gap: 8px; flex-wrap: wrap;",
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Primary,
                                style: "flex: 1; min-width: 100px;",
                                disabled: auth_loading(),
                                onclick: on_auth_sign_in,
                                if auth_loading() { "Working..." } else { "Sign in" }
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Outline,
                                style: "flex: 1; min-width: 100px;",
                                disabled: auth_loading(),
                                onclick: on_auth_sign_up,
                                "Sign up"
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Outline,
                                style: "flex: 1; min-width: 100px;",
                                disabled: auth_loading() || auth_session().is_none(),
                                onclick: on_auth_sign_out,
                                "Sign out"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
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
                            "Export"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Destination: {export_directory_text}"
                        }
                        div {
                            style: "display: flex; gap: 8px;",
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Outline,
                                disabled: export_busy(),
                                onclick: on_export_json,
                                if export_busy() { "Exporting..." } else { "Export JSON" }
                            }
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Outline,
                                disabled: export_busy(),
                                onclick: on_export_markdown,
                                "Export Markdown"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
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
                            "Turso sync settings"
                        }
                        Label {
                            html_for: "turso-url",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Turso URL"
                        }
                        UiInput {
                            id: "turso-url",
                            r#type: "text",
                            placeholder: "libsql://your-db.region.turso.io",
                            value: "{turso_database_url_input}",
                            oninput: move |event: Event<FormData>| {
                                turso_database_url_input.set(event.value());
                            },
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Managed mode uses signed-in Supabase session + bootstrap sync endpoint to fetch short-lived sync credentials."
                        }
                        div {
                            style: "display: flex; gap: 8px;",
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Primary,
                                onclick: on_save_sync_settings,
                                "Save sync config"
                            }
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Outline,
                                onclick: on_clear_sync_settings,
                                "Clear"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 6px;
                            margin-bottom: 10px;
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
                            "Build"
                        }
                        p {
                            style: "margin: 0; font-size: 13px; color: #111827;",
                            "{package_name} v{app_version}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Target: {std::env::consts::ARCH}/{std::env::consts::OS}"
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
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
                            "Provisioning status"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Auth: {provisioning.auth_status}"
                        }
                        if let Some(auth_action) = provisioning.auth_action.as_ref() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "{auth_action}"
                            }
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Sync: {provisioning.sync_status}"
                        }
                        if let Some(sync_action) = provisioning.sync_action.as_ref() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "{sync_action}"
                            }
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Media: {provisioning.media_status}"
                        }
                        if let Some(media_action) = provisioning.media_action.as_ref() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "{media_action}"
                            }
                        }
                        if cfg!(debug_assertions) {
                            div {
                                style: "margin-top: 8px; padding-top: 8px; border-top: 1px dashed #d1d5db; display: flex; flex-direction: column; gap: 6px;",
                                p {
                                    style: "margin: 0; font-size: 11px; color: #6b7280; font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em;",
                                    "Developer diagnostics (debug)"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Turso runtime endpoint: {diagnostics.turso_runtime_endpoint}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Turso runtime token: {diagnostics.turso_runtime_token_status}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Managed token endpoint: {diagnostics.turso_managed_auth_endpoint}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Config source: {diagnostics.turso_active_source}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Supabase URL: {diagnostics.supabase_url}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Supabase anon key: {diagnostics.supabase_anon_key_status}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Supabase auth config: {diagnostics.supabase_auth_status}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Media bucket status: {diagnostics.r2_bucket}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Media endpoint: {diagnostics.r2_endpoint}"
                                }
                                p {
                                    style: "margin: 0; font-size: 12px; color: #374151;",
                                    "Media credentials: {diagnostics.r2_credentials_status}"
                                }
                            }
                        }
                    }
                }
