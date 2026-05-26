//! Mobile Settings → Account view.
//!
//! Account-only for Phase 2.7. Theme / Sync / Window tabs are explicitly
//! out of scope; mobile inherits the OS theme and there's no window
//! geometry to manage on Android. The flow mirrors desktop's Account
//! row exactly so a future shared UI library can collapse them into one
//! component once the styling is unified:
//!
//!   1. User enters email → `POST /v1/auth/request` returns a
//!      `request_id` and emails a 6-digit code.
//!   2. User enters the code → `POST /v1/auth/verify` swaps it for a
//!      `StoredToken` which we persist via the injected `TokenStore`.
//!   3. On success we hydrate a `SessionApiClient` and spawn the sync
//!      worker so background pull / push kicks off immediately — the
//!      user doesn't need to restart the app.
//!
//! Sign-out reverses the steps: revoke the bearer server-side, clear
//! the stored slot, shut the worker down, and reset the sync status
//! signals to Offline. The shutdown is awaited (see
//! `shutdown_existing_worker`) so a freshly spawned worker after
//! re-login cannot race the previous one's in-flight `sync_once`.

use std::sync::Arc;

use dioxus::prelude::*;

use crate::app_shell::spawn_session_worker;
use crate::services::auth_flow::{
    perform_logout, perform_verify, send_magic_code, validate_code, validate_email, LoginOutcome,
    LogoutOutcome,
};
use crate::services::SyncWorkerHandle;
use crate::state::{AppState, AuthDeps, SyncStatus, View};
use crate::ui::{ButtonVariant, UiButton};

/// Discriminator for the inline status banner.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Info,
    Error,
}

#[component]
pub fn Settings() -> Element {
    let state = use_context::<AppState>();
    let auth_deps_signal = use_context::<Signal<AuthDeps>>();
    // Snapshot once per render. `auth_deps_signal` re-renders the
    // component when its inner value changes — same pattern desktop
    // uses to keep the snapshot fresh after bootstrap resolves.
    let auth_deps_snapshot = auth_deps_signal.read().clone();

    let mut email_input = use_signal(String::new);
    let mut code_input = use_signal(String::new);
    let mut request_id: Signal<Option<String>> = use_signal(|| None);
    let mut busy = use_signal(|| false);
    let mut message: Signal<Option<(MessageKind, String)>> = use_signal(|| None);

    let signed_in_value = (state.signed_in)();
    let mut view_signal = state.view;
    let deps_signal = auth_deps_signal;

    rsx! {
        div {
            style: "padding: 12px 16px; display: flex; flex-direction: column; gap: 12px;",

            // Header with a back button so a single-screen mobile shell
            // can return to the list without a hardware back button being
            // the only escape hatch (Android does have one, but the
            // affordance shouldn't depend on it).
            div {
                style: "display: flex; align-items: center; gap: 8px;",
                UiButton {
                    r#type: "button",
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| view_signal.set(View::List),
                    "← Back"
                }
                h2 {
                    style: "margin: 0; font-size: 18px; font-weight: 600;",
                    "Settings"
                }
            }

            match &signed_in_value {
                Some(token) => rsx! {
                    SignedInPanel {
                        email: token.email.clone(),
                        busy: busy(),
                        on_logout: move |_| {
                            let deps_for_task = deps_signal.read().clone();
                            busy.set(true);
                            message.set(None);
                            spawn(async move {
                                // Shut down (and await) the worker *before*
                                // talking to the server. Otherwise a parallel
                                // `sync_once` that hits a 401 can call
                                // `session.refresh()` mid-logout and persist
                                // a fresh token to the store after
                                // `perform_logout` has cleared it — leaving
                                // the device "signed in" while the UI claims
                                // it's signed out.
                                shutdown_existing_worker(state.sync_worker).await;
                                let outcome = perform_logout(&deps_for_task).await;
                                apply_logout_outcome(outcome, state, &mut message).await;
                                busy.set(false);
                            });
                        },
                    }
                },
                None => rsx! {
                    SignedOutPanel {
                        auth_available: auth_deps_snapshot.auth_client.is_some(),
                        api_base_url: auth_deps_snapshot.api_base_url,
                        email_input: email_input(),
                        on_email_input: move |value: String| email_input.set(value),
                        code_input: code_input(),
                        on_code_input: move |value: String| {
                            // Drop non-digits + cap to 6 so paste-with-spaces
                            // produces a clean OTP. Non-digits get dropped
                            // silently — they're never valid in a code.
                            let cleaned: String = value
                                .chars()
                                .filter(char::is_ascii_digit)
                                .take(6)
                                .collect();
                            code_input.set(cleaned);
                        },
                        has_request_id: request_id().is_some(),
                        on_send_code: move |_| {
                            let email = email_input().trim().to_string();
                            if let Err(reason) = validate_email(&email) {
                                message.set(Some((MessageKind::Error, reason)));
                                return;
                            }
                            let deps_for_task = deps_signal.read().clone();
                            busy.set(true);
                            message.set(None);
                            spawn(async move {
                                match send_magic_code(&deps_for_task, &email).await {
                                    Ok(req_id) => {
                                        request_id.set(Some(req_id));
                                        message.set(Some((
                                            MessageKind::Info,
                                            format!("Code sent to {email}. Check your inbox."),
                                        )));
                                    }
                                    Err(err) => {
                                        request_id.set(None);
                                        message.set(Some((MessageKind::Error, err)));
                                    }
                                }
                                busy.set(false);
                            });
                        },
                        on_verify_code: move |_| {
                            let Some(req_id) = request_id() else {
                                message.set(Some((
                                    MessageKind::Error,
                                    "Send a code first.".into(),
                                )));
                                return;
                            };
                            let code = code_input();
                            if let Err(reason) = validate_code(&code) {
                                message.set(Some((MessageKind::Error, reason)));
                                return;
                            }
                            let deps_for_task = deps_signal.read().clone();
                            busy.set(true);
                            message.set(None);
                            spawn(async move {
                                let outcome = perform_verify(&deps_for_task, &req_id, &code).await;
                                apply_login_outcome(
                                    outcome,
                                    state,
                                    &mut email_input,
                                    &mut code_input,
                                    &mut request_id,
                                    &mut message,
                                )
                                .await;
                                busy.set(false);
                            });
                        },
                        busy: busy(),
                    }
                },
            }

            if let Some((kind, text)) = message() {
                MessageBanner { kind, text }
            }
        }
    }
}

#[component]
fn SignedInPanel(email: String, busy: bool, on_logout: EventHandler<MouseEvent>) -> Element {
    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                gap: 10px;
                padding: 14px;
                border: 1px solid #e5e7eb;
                border-radius: 12px;
                background: #ffffff;
            ",
            p {
                style: "margin: 0; font-weight: 600; color: #111827;",
                "Account"
            }
            p {
                style: "margin: 0; font-size: 13px; color: #6b7280;",
                "Signed in as {email}"
            }
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Secondary,
                disabled: busy,
                onclick: move |event| on_logout.call(event),
                if busy { "Signing out..." } else { "Sign out" }
            }
            p {
                style: "margin: 0; font-size: 12px; color: #9ca3af; line-height: 1.4;",
                "Sign out to stop syncing this device and revoke the session."
            }
        }
    }
}

#[component]
fn SignedOutPanel(
    auth_available: bool,
    api_base_url: Option<String>,
    email_input: String,
    on_email_input: EventHandler<String>,
    code_input: String,
    on_code_input: EventHandler<String>,
    has_request_id: bool,
    on_send_code: EventHandler<MouseEvent>,
    on_verify_code: EventHandler<MouseEvent>,
    busy: bool,
) -> Element {
    if !auth_available {
        return rsx! {
            div {
                style: "
                    padding: 14px;
                    border: 1px solid #fecaca;
                    border-radius: 12px;
                    background: #fef2f2;
                    color: #b91c1c;
                    font-size: 13px;
                    line-height: 1.4;
                ",
                "Sign-in is unavailable — DIRT_API_BASE_URL is not configured. \
                 Set it in the mobile bootstrap config (.env.client at build \
                 time) and rebuild the app."
            }
        };
    }

    let server_hint = api_base_url
        .as_deref()
        .map(|url| format!("Backend: {url}"))
        .unwrap_or_default();

    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                gap: 10px;
                padding: 14px;
                border: 1px solid #e5e7eb;
                border-radius: 12px;
                background: #ffffff;
            ",
            p {
                style: "margin: 0; font-weight: 600; color: #111827;",
                "Email"
            }
            p {
                style: "margin: 0; font-size: 12px; color: #6b7280; line-height: 1.4;",
                "We'll send a one-time 6-digit code."
            }
            input {
                r#type: "email",
                inputmode: "email",
                placeholder: "you@example.com",
                value: "{email_input}",
                disabled: busy || has_request_id,
                style: "
                    width: 100%;
                    padding: 10px 12px;
                    border: 1px solid #d1d5db;
                    border-radius: 10px;
                    font-size: 15px;
                    box-sizing: border-box;
                ",
                oninput: move |event| on_email_input.call(event.value()),
            }
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Secondary,
                disabled: busy,
                onclick: move |event| on_send_code.call(event),
                if has_request_id { "Resend code" } else { "Send code" }
            }
            if !server_hint.is_empty() {
                p {
                    style: "margin: 0; font-size: 11px; color: #9ca3af;",
                    "{server_hint}"
                }
            }
        }

        if has_request_id {
            div {
                style: "
                    display: flex;
                    flex-direction: column;
                    gap: 10px;
                    padding: 14px;
                    border: 1px solid #e5e7eb;
                    border-radius: 12px;
                    background: #ffffff;
                ",
                p {
                    style: "margin: 0; font-weight: 600; color: #111827;",
                    "Verification code"
                }
                p {
                    style: "margin: 0; font-size: 12px; color: #6b7280; line-height: 1.4;",
                    "Enter the 6-digit code we just emailed."
                }
                input {
                    r#type: "text",
                    inputmode: "numeric",
                    placeholder: "123456",
                    value: "{code_input}",
                    disabled: busy,
                    style: "
                        width: 100%;
                        padding: 10px 12px;
                        border: 1px solid #d1d5db;
                        border-radius: 10px;
                        font-size: 18px;
                        letter-spacing: 4px;
                        text-align: center;
                        font-family: monospace;
                        box-sizing: border-box;
                    ",
                    oninput: move |event| on_code_input.call(event.value()),
                }
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Primary,
                    disabled: busy || code_input.len() != 6,
                    onclick: move |event| on_verify_code.call(event),
                    if busy { "Verifying..." } else { "Verify" }
                }
            }
        }
    }
}

#[component]
fn MessageBanner(kind: MessageKind, text: String) -> Element {
    let (bg, fg) = match kind {
        MessageKind::Info => ("#dbeafe", "#1d4ed8"),
        MessageKind::Error => ("#fee2e2", "#b91c1c"),
    };
    rsx! {
        div {
            style: "
                padding: 10px 12px;
                background: {bg};
                color: {fg};
                border-radius: 10px;
                font-size: 13px;
                line-height: 1.4;
            ",
            "{text}"
        }
    }
}

// `AppState` is a `Copy` struct of `Signal` handles; passing by value
// matches the rest of the mobile component tree and avoids fighting
// the borrow checker against the `Signal::set` `&mut self` receiver
// when called on subfields. The lint flags the raw byte count without
// noticing that every field is a cheap `Arc`-backed handle.
#[allow(clippy::large_types_passed_by_value)]
async fn apply_login_outcome(
    outcome: LoginOutcome,
    mut state: AppState,
    email_input: &mut Signal<String>,
    code_input: &mut Signal<String>,
    request_id: &mut Signal<Option<String>>,
    message: &mut Signal<Option<(MessageKind, String)>>,
) {
    match outcome {
        LoginOutcome::Success(token, session) => {
            // Tear down the previous worker (if any — happens during
            // re-login after a permanent failure) before starting a
            // fresh one. The await joins the old task so two workers
            // never run a `sync_once` against the same DB concurrently.
            shutdown_existing_worker(state.sync_worker).await;

            // Per-user DB swap (issue #234). When the just-authenticated
            // user differs from the DB currently in `state.store`,
            // migrate, rewrite state.json, and reopen against the new
            // user. Same-user logins skip the swap.
            let current_user = state
                .store
                .read()
                .as_ref()
                .map(|store| store.user_id().to_string());
            let user_changed = current_user.as_deref() != Some(token.user_id.as_str());

            let store_for_worker = if user_changed {
                let data_dir = crate::config::default_mobile_data_directory();
                if let Err(err) = dirt_core::services::db_paths::migrate_solo_db_to_user(
                    &data_dir,
                    &token.user_id,
                )
                .await
                {
                    state.sync_status.set(SyncStatus::Error);
                    state
                        .sync_issue
                        .set(Some(format!("Could not migrate local data: {err}")));
                    message.set(Some((
                        MessageKind::Error,
                        format!(
                            "Sign-in succeeded but local data migration failed: {err}. \
                             Re-run sign-in after resolving."
                        ),
                    )));
                    return;
                }
                if let Err(err) =
                    dirt_core::services::db_paths::write_active_user(&data_dir, &token.user_id)
                        .await
                {
                    state.sync_status.set(SyncStatus::Error);
                    state
                        .sync_issue
                        .set(Some(format!("Could not persist active user: {err}")));
                    message.set(Some((
                        MessageKind::Error,
                        format!("Sign-in succeeded but persistence failed: {err}"),
                    )));
                    return;
                }

                state.store.set(None);
                match crate::data::MobileNoteStore::open_for_user(&token.user_id).await {
                    Ok(new_store) => {
                        let new_store = Arc::new(new_store);
                        state.store.set(Some(new_store.clone()));
                        Some(new_store)
                    }
                    Err(err) => {
                        state.sync_status.set(SyncStatus::Error);
                        state
                            .sync_issue
                            .set(Some(format!("Could not open per-user DB: {err}")));
                        message.set(Some((
                            MessageKind::Error,
                            format!("Sign-in succeeded but the new DB would not open: {err}"),
                        )));
                        return;
                    }
                }
            } else {
                state.store.read().clone()
            };

            state.signed_in.set(Some(token));
            state.session_client.set(Some(session.clone()));

            let events_tx = state.events_tx.read().clone();
            match (store_for_worker, events_tx) {
                (Some(store), Some(events_tx)) => {
                    spawn_session_worker(store, session, events_tx, state.sync_worker);
                }
                // Either the local DB or the long-lived event bridge is
                // missing. Both should be present by the time the
                // Settings view is reachable (they're hydrated in
                // `AppShell`'s startup `use_resource`), so this is a
                // hard error — surface loudly rather than silently
                // skipping worker startup.
                _ => {
                    state.sync_status.set(SyncStatus::Error);
                    state.sync_issue.set(Some(
                        "Local database / sync bridge not ready; sync worker not started.".into(),
                    ));
                }
            }

            email_input.set(String::new());
            code_input.set(String::new());
            request_id.set(None);
            message.set(Some((MessageKind::Info, "Signed in.".into())));
        }
        LoginOutcome::Failure(reason) => {
            message.set(Some((MessageKind::Error, reason)));
        }
    }
}

// See the rationale on [`apply_login_outcome`].
//
// Logout shuts the worker down *before* this runs (in the on_logout
// handler) — so this function does not call `shutdown_existing_worker`.
// Calling it again here would be a no-op (the handle has been removed
// from the signal already), but documenting the ordering up front keeps
// future refactors honest: the shutdown must happen ahead of
// `perform_logout` to close the refresh-after-revoke race.
#[allow(clippy::large_types_passed_by_value)]
async fn apply_logout_outcome(
    outcome: LogoutOutcome,
    mut state: AppState,
    message: &mut Signal<Option<(MessageKind, String)>>,
) {
    match outcome {
        LogoutOutcome::Success => {
            state.signed_in.set(None);
            state.session_client.set(None);
            state.sync_status.set(SyncStatus::Offline);
            state.sync_issue.set(None);
            message.set(Some((MessageKind::Info, "Signed out.".into())));
        }
        LogoutOutcome::Failure(reason) => {
            message.set(Some((MessageKind::Error, reason)));
        }
    }
}

/// Drop the currently-tracked worker handle and wait for the
/// underlying tokio task to finish its in-flight `sync_once`. The
/// await is critical — without it a fresh worker spawned right after
/// can run a startup sync concurrently with the old worker's
/// not-yet-cancelled push cycle, producing two simultaneous bearer
/// tokens against the same server endpoint.
async fn shutdown_existing_worker(mut sync_worker: Signal<Option<SyncWorkerHandle>>) {
    let handle = sync_worker.read().clone();
    sync_worker.set(None);
    if let Some(handle) = handle {
        handle.shutdown().await;
    }
}
