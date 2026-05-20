//! Settings → Account tab: magic-code sign-in + sign-out.
//!
//! Sync is an opt-in feature; this tab is the only entry point that
//! arms it. The flow mirrors `dirt-cli auth login`:
//!
//!   1. User enters email → `POST /v1/auth/request` returns a
//!      `request_id` and emails a 6-digit code.
//!   2. User enters the code → `POST /v1/auth/verify` swaps it for a
//!      `StoredToken` which we persist via the injected `TokenStore`.
//!   3. On success we hydrate a `SessionApiClient` and spawn the sync
//!      worker so background pull/push kicks off immediately — the
//!      user doesn't need to restart the app.
//!
//! Sign-out reverses the steps: revoke the bearer server-side, clear
//! the keyring slot, shut the worker down, and reset the sync status
//! signals to Offline.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::auth::{AuthError, StoredToken, TokenStoreError};
use dirt_core::sync::session_client::SessionApiClient;

use super::row::SettingRow;
use crate::app::spawn_session_worker;
use crate::components::button::{Button, ButtonVariant};
use crate::components::input::Input;
use crate::services::SyncWorkerHandle;
use crate::state::{AppState, AuthDeps, SyncStatus};

/// Discriminator for the inline status banner.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Info,
    Error,
}

#[component]
pub(super) fn AccountSettingsTab() -> Element {
    let state = use_context::<AppState>();
    let auth_deps_signal = use_context::<Signal<AuthDeps>>();
    // Read the current snapshot once per render. The signal re-renders
    // the component when bootstrap_config resolves and the snapshot
    // gains an `AuthClient` / `api_base_url`, so this never goes stale
    // for longer than one render frame.
    let auth_deps_snapshot = auth_deps_signal.read().clone();

    let mut email_input = use_signal(String::new);
    let mut code_input = use_signal(String::new);
    let mut request_id: Signal<Option<String>> = use_signal(|| None);
    let mut busy = use_signal(|| false);
    let mut message: Signal<Option<(MessageKind, String)>> = use_signal(|| None);

    let signed_in_value = (state.signed_in)();

    // Capture the snapshot Signal (Copy) in closures; each handler
    // re-reads it so a bootstrap-config update that lands an
    // `AuthClient` mid-session is picked up without a manual refresh.
    let deps_signal = auth_deps_signal;

    rsx! {
        match &signed_in_value {
            Some(token) => rsx! {
                SignedInRow {
                    email: token.email.clone(),
                    busy: busy(),
                    on_logout: {
                        let stored = token.clone();
                        move |_| {
                            let stored_for_task = stored.clone();
                            let deps_for_task = deps_signal.read().clone();
                            busy.set(true);
                            message.set(None);
                            spawn(async move {
                                let outcome = perform_logout(&deps_for_task, &stored_for_task).await;
                                apply_logout_outcome(
                                    outcome,
                                    state,
                                    &mut message,
                                );
                                busy.set(false);
                            });
                        }
                    },
                }
            },
            None => rsx! {
                SignedOutRow {
                    auth_available: auth_deps_snapshot.auth_client.is_some(),
                    api_base_url: auth_deps_snapshot.api_base_url,
                    email_input: email_input(),
                    on_email_input: move |value: String| email_input.set(value),
                    code_input: code_input(),
                    on_code_input: move |value: String| {
                        // Keep only digits so paste-with-spaces still
                        // produces a clean OTP. Non-digits get dropped
                        // silently — they're never valid in a code.
                        let cleaned: String = value.chars().filter(char::is_ascii_digit).take(6).collect();
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
                                &deps_for_task,
                                &mut email_input,
                                &mut code_input,
                                &mut request_id,
                                &mut message,
                            );
                            busy.set(false);
                        });
                    },
                    busy: busy(),
                }
            },
        }

        if let Some((kind, text)) = message() {
            div {
                class: match kind {
                    MessageKind::Info => "auth-message",
                    MessageKind::Error => "auth-error",
                },
                "{text}"
            }
        }
    }
}

#[component]
fn SignedInRow(email: String, busy: bool, on_logout: EventHandler<MouseEvent>) -> Element {
    rsx! {
        SettingRow {
            label: "Account",
            description: "Sign out to stop syncing this device and revoke the session.",

            div {
                class: "auth-panel",
                div { class: "auth-hint", "Signed in as {email}" }
                div {
                    class: "auth-actions",
                    Button {
                        variant: ButtonVariant::Secondary,
                        disabled: busy,
                        onclick: move |event| on_logout.call(event),
                        if busy { "Signing out..." } else { "Sign out" }
                    }
                }
            }
        }
    }
}

#[component]
fn SignedOutRow(
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
            SettingRow {
                label: "Account",
                description: "Sign in to enable cloud sync across devices.",
                div {
                    class: "auth-panel",
                    div {
                        class: "auth-error",
                        "Sign-in is unavailable — DIRT_API_BASE_URL is not configured. \
                         Set it in the desktop bootstrap config and restart the app."
                    }
                }
            }
        };
    }

    let server_hint = api_base_url
        .as_deref()
        .map(|url| format!("Backend: {url}"))
        .unwrap_or_default();

    rsx! {
        SettingRow {
            label: "Email",
            description: "We'll send a one-time 6-digit code.",
            div {
                class: "auth-panel",
                Input {
                    class: "auth-input",
                    r#type: "email",
                    placeholder: "you@example.com",
                    value: "{email_input}",
                    disabled: busy || has_request_id,
                    oninput: move |event: FormEvent| on_email_input.call(event.value()),
                }
                div {
                    class: "auth-actions",
                    Button {
                        variant: ButtonVariant::Secondary,
                        disabled: busy,
                        onclick: move |event| on_send_code.call(event),
                        if has_request_id { "Resend code" } else { "Send code" }
                    }
                }
                if !server_hint.is_empty() {
                    div { class: "auth-hint", "{server_hint}" }
                }
            }
        }

        if has_request_id {
            SettingRow {
                label: "Verification code",
                description: "Enter the 6-digit code we just emailed.",
                div {
                    class: "auth-panel",
                    Input {
                        class: "auth-input",
                        r#type: "text",
                        inputmode: "numeric",
                        placeholder: "123456",
                        value: "{code_input}",
                        disabled: busy,
                        oninput: move |event: FormEvent| on_code_input.call(event.value()),
                    }
                    div {
                        class: "auth-actions",
                        Button {
                            variant: ButtonVariant::Secondary,
                            disabled: busy || code_input.len() != 6,
                            onclick: move |event| on_verify_code.call(event),
                            if busy { "Verifying..." } else { "Verify" }
                        }
                    }
                }
            }
        }
    }
}

// ---- Login / logout flow helpers (pure-ish, testable). ----

/// Outcome of `verify_magic_code` + downstream persistence + worker
/// startup. Splits success into "what got persisted" so the apply
/// helper can update reactive signals without a second store load.
enum LoginOutcome {
    Success(StoredToken, Arc<SessionApiClient>),
    Failure(String),
}

enum LogoutOutcome {
    Success,
    Failure(String),
}

async fn send_magic_code(deps: &AuthDeps, email: &str) -> Result<String, String> {
    let auth = deps
        .auth_client
        .as_ref()
        .ok_or_else(|| "Sign-in is unavailable: DIRT_API_BASE_URL not configured.".to_string())?;
    match auth.request_magic_code(email).await {
        Ok(resp) => Ok(resp.request_id),
        Err(err) => Err(describe_auth_error(&err)),
    }
}

async fn perform_verify(deps: &AuthDeps, request_id: &str, code: &str) -> LoginOutcome {
    let Some(auth) = deps.auth_client.as_ref() else {
        return LoginOutcome::Failure(
            "Sign-in is unavailable: DIRT_API_BASE_URL not configured.".into(),
        );
    };
    let resp = match auth.verify_magic_code(request_id, code).await {
        Ok(resp) => resp,
        Err(err) => return LoginOutcome::Failure(describe_auth_error(&err)),
    };
    let stored: StoredToken = resp.into();
    if let Err(err) = deps.token_store.save(&stored) {
        return LoginOutcome::Failure(describe_store_error(&err));
    }
    let Some(base_url) = deps.api_base_url.clone() else {
        return LoginOutcome::Failure(
            "Sign-in succeeded but the API base URL is missing — sync cannot start.".into(),
        );
    };
    match SessionApiClient::from_store(base_url, (**auth).clone(), deps.token_store.clone()) {
        Ok(Some(session)) => LoginOutcome::Success(stored, Arc::new(session)),
        Ok(None) => LoginOutcome::Failure(
            "Token was saved but the keyring read back empty. Try signing in again.".into(),
        ),
        Err(err) => LoginOutcome::Failure(format!("Could not build sync client: {err}")),
    }
}

async fn perform_logout(deps: &AuthDeps, stored: &StoredToken) -> LogoutOutcome {
    // Revoke server-side first. If the server is reachable but rejects
    // the call (network blip, 5xx) we surface that and keep the local
    // session intact so the user can retry — clearing the keyring on a
    // failed revoke would leave a token live on the server with no
    // local handle to revoke it later.
    if let Some(auth) = deps.auth_client.as_ref() {
        match auth.logout_session(&stored.session_token).await {
            Ok(()) | Err(AuthError::SessionExpired(_)) => {}
            Err(other) => {
                return LogoutOutcome::Failure(describe_auth_error(&other));
            }
        }
    }
    if let Err(err) = deps.token_store.clear() {
        return LogoutOutcome::Failure(format!(
            "Server revoke succeeded but local clear failed: {}. \
             Retry once the keyring is reachable.",
            describe_store_error(&err)
        ));
    }
    LogoutOutcome::Success
}

fn apply_login_outcome(
    outcome: LoginOutcome,
    mut state: AppState,
    deps: &AuthDeps,
    email_input: &mut Signal<String>,
    code_input: &mut Signal<String>,
    request_id: &mut Signal<Option<String>>,
    message: &mut Signal<Option<(MessageKind, String)>>,
) {
    match outcome {
        LoginOutcome::Success(token, session) => {
            // Tear down the previous worker (if any — happens during
            // re-login after a permanent failure) before starting a
            // fresh one, so two workers never race on the same DB.
            shutdown_existing_worker(state.sync_worker);

            state.signed_in.set(Some(token));
            state.session_client.set(Some(session.clone()));

            let db = state.db_service.read().clone();
            if let Some(db) = db {
                spawn_session_worker(
                    db,
                    session,
                    state.sync_worker,
                    state.sync_status,
                    state.sync_issue,
                    state.last_sync_at,
                );
            } else {
                // Database isn't ready yet — extremely unlikely (the
                // settings panel can't open before the DB hydrates) but
                // worth a loud error rather than silently skipping
                // worker startup.
                state.sync_status.set(SyncStatus::Error);
                state.sync_issue.set(Some(
                    "Database is not ready; sync worker not started.".into(),
                ));
            }

            email_input.set(String::new());
            code_input.set(String::new());
            request_id.set(None);
            message.set(Some((MessageKind::Info, "Signed in.".into())));
            let _ = deps; // reserved for future post-login work
        }
        LoginOutcome::Failure(reason) => {
            message.set(Some((MessageKind::Error, reason)));
        }
    }
}

fn apply_logout_outcome(
    outcome: LogoutOutcome,
    mut state: AppState,
    message: &mut Signal<Option<(MessageKind, String)>>,
) {
    match outcome {
        LogoutOutcome::Success => {
            shutdown_existing_worker(state.sync_worker);
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

fn shutdown_existing_worker(mut sync_worker: Signal<Option<SyncWorkerHandle>>) {
    if let Some(handle) = sync_worker.read().as_ref() {
        handle.shutdown();
    }
    sync_worker.set(None);
}

/// Sanity-check an email before hitting the server. We don't try to
/// fully parse RFC 5322 — the server validates definitively; this just
/// catches the trivial empty / no-`@` cases inline so the user sees the
/// error without a network round trip.
fn validate_email(email: &str) -> Result<(), String> {
    if email.is_empty() {
        return Err("Enter an email address.".into());
    }
    if !email.contains('@') {
        return Err("Email must contain '@'.".into());
    }
    Ok(())
}

fn validate_code(code: &str) -> Result<(), String> {
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err("Code must be exactly 6 digits.".into());
    }
    Ok(())
}

fn describe_auth_error(err: &AuthError) -> String {
    match err {
        AuthError::InvalidConfiguration(msg) => format!("Configuration error: {msg}"),
        AuthError::Network(msg) => format!("Network error: {msg}. Check your connection."),
        AuthError::InvalidEmail(msg) => format!("Invalid email: {msg}"),
        AuthError::InvalidCode(msg) => {
            format!("Invalid code: {msg}. Request a new code if it has expired.")
        }
        AuthError::SessionExpired(msg) => format!("Session expired: {msg}. Sign in again."),
        AuthError::RateLimited {
            message,
            retry_after_secs,
        } => retry_after_secs.as_ref().map_or_else(
            || format!("Rate limited ({message}). Try again in a moment."),
            |secs| format!("Rate limited ({message}). Retry in {secs}s."),
        ),
        AuthError::BadRequest { code, message } => {
            format!("Request rejected ({code}): {message}")
        }
        AuthError::ServerUnavailable(msg) => format!("Server unavailable: {msg}"),
        AuthError::ServerError { status, message } => {
            format!("Server error ({status}): {message}")
        }
        AuthError::Decode(msg) => format!("Server response was not understood: {msg}"),
    }
}

fn describe_store_error(err: &TokenStoreError) -> String {
    match err {
        TokenStoreError::Backend(msg) => format!("keyring backend error: {msg}"),
        TokenStoreError::Serialize(msg) => format!("stored token serialize error: {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_email_rejects_empty() {
        assert!(validate_email("").is_err());
    }

    #[test]
    fn validate_email_rejects_missing_at() {
        let err = validate_email("user.example.com").unwrap_err();
        assert!(err.contains("'@'"));
    }

    #[test]
    fn validate_email_accepts_basic_address() {
        validate_email("user@example.com").unwrap();
    }

    #[test]
    fn validate_code_requires_six_digits() {
        for bad in ["", "12345", "1234567", "12345a", "abcdef"] {
            assert!(validate_code(bad).is_err(), "{bad} should be rejected");
        }
        validate_code("000000").unwrap();
        validate_code("123456").unwrap();
    }

    #[test]
    fn describe_auth_error_includes_retry_hint_when_present() {
        let err = AuthError::RateLimited {
            message: "slow down".into(),
            retry_after_secs: Some(45),
        };
        let described = describe_auth_error(&err);
        assert!(described.contains("Retry in 45s"));
    }

    #[test]
    fn describe_auth_error_falls_back_when_no_retry_hint() {
        let err = AuthError::RateLimited {
            message: "slow down".into(),
            retry_after_secs: None,
        };
        let described = describe_auth_error(&err);
        assert!(described.contains("Try again in a moment"));
    }

    #[test]
    fn describe_store_error_distinguishes_variants() {
        assert!(
            describe_store_error(&TokenStoreError::Backend("dbus offline".into()))
                .contains("keyring backend")
        );
        assert!(
            describe_store_error(&TokenStoreError::Serialize("bad json".into()))
                .contains("serialize")
        );
    }
}
