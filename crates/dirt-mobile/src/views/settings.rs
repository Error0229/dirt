//! Mobile Settings: the account and sync.
//!
//! The view renders the session's four states, the same ones the desktop
//! app and the CLI show. Signing in opens the system browser; the app shell
//! picks the sign-in up again when Android brings the person back.

use dioxus::prelude::*;
use dirt_core::api::ErrorClass;
use dirt_core::session::{SessionState, SignOutError, Unsynced};
use dirt_core::sync::worker::{SyncStatus, Trigger};

use crate::app_shell::REDIRECT_URI;
use crate::state::{AppState, View};
use crate::ui::{ButtonVariant, UiButton};

/// Whether the banner reports something done or a failure.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Info,
    Error,
}

const PANEL: &str = "
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    border: 1px solid #e5e7eb;
    border-radius: 12px;
    background: #ffffff;
";
const HEADING: &str = "margin: 0; font-weight: 600; color: #111827;";
const TEXT: &str = "margin: 0; font-size: 13px; color: #6b7280; line-height: 1.4;";
const WARNING: &str = "margin: 0; font-size: 13px; color: #b91c1c; line-height: 1.4;";

#[component]
pub fn Settings() -> Element {
    let state = use_context::<AppState>();
    let mut view_signal = state.view;
    let mut busy = use_signal(|| false);
    let mut message: Signal<Option<(MessageKind, String)>> = use_signal(|| None);
    // Set when a sign-out found notes that have not reached the server.
    let mut unsynced: Signal<Option<usize>> = use_signal(|| None);
    let mut local_waiting = use_signal(|| 0usize);

    let session = (state.session)();
    let session_state = (state.session_state)();
    let status = (state.sync_status)();

    // Count the notes written before signing in whenever the state changes.
    use_effect(move || {
        let current = (state.session)();
        let _ = (state.session_state)();
        spawn(async move {
            let waiting = match current {
                Some(session) => session.local_notes_waiting().await.unwrap_or(0),
                None => 0,
            };
            local_waiting.set(waiting);
        });
    });

    let header = rsx! {
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
    };

    let Some(session) = session else {
        return rsx! {
            div {
                style: "padding: 12px 16px; display: flex; flex-direction: column; gap: 12px;",
                {header}
                div { style: "{PANEL}",
                    p { style: "{HEADING}", "Account" }
                    p { style: "{TEXT}",
                        "Signing in is not available in this build or on this device. Notes are kept on this device only."
                    }
                }
            }
        };
    };

    let sign_in = {
        let session = session.clone();
        move |_: MouseEvent| {
            let session = session.clone();
            busy.set(true);
            unsynced.set(None);
            message.set(None);
            spawn(async move {
                // The attempt is saved by the session before the browser
                // opens: Android may stop the app while it is in front.
                let opened = match session.begin_sign_in(REDIRECT_URI).await {
                    Ok(address) => webbrowser::open(&address).map_err(|error| {
                        session.cancel_sign_in();
                        format!("Could not open the browser: {error}")
                    }),
                    Err(error) => Err(error.to_string()),
                };
                if let Err(problem) = opened {
                    message.set(Some((MessageKind::Error, problem)));
                }
                busy.set(false);
            });
        }
    };

    let cancel_sign_in = {
        let session = session.clone();
        move |_: MouseEvent| session.cancel_sign_in()
    };

    let check_approval = {
        let session = session.clone();
        move |_: MouseEvent| {
            let session = session.clone();
            busy.set(true);
            spawn(async move {
                message.set(match session.check_approval().await {
                    Ok(SessionState::Ready { .. }) => None,
                    Ok(_) => Some((MessageKind::Info, "Not approved yet.".to_owned())),
                    Err(error) => Some((MessageKind::Error, error.to_string())),
                });
                busy.set(false);
            });
        }
    };

    let move_notes = {
        let session = session.clone();
        move |_: MouseEvent| {
            let session = session.clone();
            let mut state = state;
            busy.set(true);
            spawn(async move {
                let outcome = session.move_local_notes().await;
                message.set(Some(match outcome {
                    Ok(moved) => {
                        local_waiting.set(0);
                        let open = state.store.peek().clone();
                        if let Some(open) = open {
                            if let Ok(latest) = open.list_notes().await {
                                state.notes.set(latest);
                            }
                        }
                        (
                            MessageKind::Info,
                            format!("Moved {moved} note(s) into your account."),
                        )
                    }
                    Err(error) => (MessageKind::Error, error.to_string()),
                }));
                busy.set(false);
            });
        }
    };

    let sign_out = {
        let session = session.clone();
        move |choice: Unsynced| {
            let session = session.clone();
            busy.set(true);
            message.set(None);
            spawn(async move {
                match session.sign_out(choice).await {
                    Ok(()) => {
                        unsynced.set(None);
                        message.set(Some((MessageKind::Info, "Signed out.".to_owned())));
                    }
                    Err(SignOutError::Unsynced { notes, cause }) => {
                        unsynced.set(Some(notes));
                        let why = cause.map_or_else(String::new, |cause| format!(" ({cause})"));
                        message.set(Some((
                            MessageKind::Error,
                            format!(
                                "{notes} note(s) have not been synced{why}. Signing out would delete them."
                            ),
                        )));
                    }
                    Err(error) => message.set(Some((MessageKind::Error, error.to_string()))),
                }
                busy.set(false);
            });
        }
    };
    let sign_out_keeping = {
        let mut sign_out = sign_out.clone();
        move |_: MouseEvent| sign_out(Unsynced::Refuse)
    };
    let sign_out_discarding = {
        let mut sign_out = sign_out;
        move |_: MouseEvent| sign_out(Unsynced::Discard)
    };

    let must_sign_in_again = status
        .failure
        .as_ref()
        .is_some_and(|failure| failure.class == ErrorClass::StopUntilSignedIn);
    let sync_session = session;

    rsx! {
        div {
            style: "padding: 12px 16px; display: flex; flex-direction: column; gap: 12px;",
            {header}

            div { style: "{PANEL}",
                p { style: "{HEADING}", "Account" }
                match session_state {
                    SessionState::SignedOut => rsx! {
                        p { style: "{TEXT}", "Not signed in. Notes are kept on this device only." }
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: ButtonVariant::Primary,
                            disabled: busy(),
                            onclick: sign_in,
                            "Sign in"
                        }
                    },
                    SessionState::SigningIn => rsx! {
                        p { style: "{TEXT}", "Waiting for you to finish signing in in your browser." }
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: ButtonVariant::Secondary,
                            onclick: cancel_sign_in,
                            "Cancel"
                        }
                    },
                    SessionState::WaitingForApproval { .. } => rsx! {
                        p { style: "{TEXT}",
                            "This device is waiting to be approved by another device of this account. Until then it cannot read the account's notes, and notes written here stay on this device."
                        }
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: ButtonVariant::Secondary,
                            disabled: busy(),
                            onclick: check_approval,
                            "Check again"
                        }
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: ButtonVariant::Secondary,
                            disabled: busy(),
                            onclick: sign_out_keeping,
                            "Sign out"
                        }
                    },
                    SessionState::Ready { .. } => rsx! {
                        p { style: "{TEXT}", "Signed in. Your notes sync with your other devices." }
                        if must_sign_in_again {
                            p { style: "{WARNING}", "This device has to sign in again before it can sync." }
                            UiButton {
                                r#type: "button",
                                block: true,
                                variant: ButtonVariant::Primary,
                                disabled: busy(),
                                onclick: sign_in,
                                "Sign in again"
                            }
                        }
                        if local_waiting() > 0 {
                            p { style: "{TEXT}",
                                "{local_waiting()} note(s) written before signing in are on this device but not in your account."
                            }
                            UiButton {
                                r#type: "button",
                                block: true,
                                variant: ButtonVariant::Secondary,
                                disabled: busy(),
                                onclick: move_notes,
                                "Move them into my account"
                            }
                        }
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: ButtonVariant::Secondary,
                            disabled: busy(),
                            onclick: sign_out_keeping,
                            "Sign out"
                        }
                        if unsynced().is_some() {
                            UiButton {
                                r#type: "button",
                                block: true,
                                variant: ButtonVariant::Secondary,
                                disabled: busy(),
                                onclick: sign_out_discarding,
                                "Sign out and delete them"
                            }
                        }
                    },
                }
            }

            if session_state.is_ready() {
                div { style: "{PANEL}",
                    p { style: "{HEADING}", "Sync" }
                    p { style: "{TEXT}", "{sync_summary(&status)}" }
                    if status.unsent > 0 {
                        p { style: "{TEXT}", "{status.unsent} note(s) waiting to be sent" }
                    }
                    if status.problems > 0 {
                        p { style: "{WARNING}", "{status.problems} note(s) could not be synced." }
                    }
                    if let Some(text) = sync_failure(&status) {
                        p { style: "{WARNING}", "{text}" }
                    }
                    UiButton {
                        r#type: "button",
                        block: true,
                        variant: ButtonVariant::Secondary,
                        disabled: status.syncing,
                        onclick: move |_| sync_session.notify(Trigger::Manual),
                        "Sync now"
                    }
                }
            }

            if let Some((kind, text)) = message() {
                MessageBanner { kind, text }
            }
        }
    }
}

/// When the last sync was, in words.
pub fn sync_summary(status: &SyncStatus) -> String {
    if status.syncing {
        return "Syncing...".to_owned();
    }
    let Some(at) = status.last_synced_ms else {
        return "Never synced".to_owned();
    };
    let minutes = (chrono_now_ms() - at).max(0) / 60_000;
    match minutes {
        0 => "Synced just now".to_owned(),
        1..=59 => format!("Synced {minutes}m ago"),
        60..=1439 => format!("Synced {}h ago", minutes / 60),
        _ => format!("Synced {}d ago", minutes / 1440),
    }
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Why the last sync failed, in words that say what to do.
pub fn sync_failure(status: &SyncStatus) -> Option<String> {
    let failure = status.failure.as_ref()?;
    Some(match failure.class {
        ErrorClass::TryAgainLater => {
            "The server could not be reached. Your notes are saved and will be sent later."
                .to_owned()
        }
        ErrorClass::StopUntilSignedIn => "This device has to sign in again.".to_owned(),
        ErrorClass::UpdateTheApp => {
            "This version of Dirt is too old to sync. Update the app.".to_owned()
        }
        ErrorClass::ProblemWithOneNote => failure.message.clone(),
    })
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
