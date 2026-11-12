//! Settings → Account tab: signing in and out.
//!
//! The tab renders the session's four states. Signing in opens the system
//! browser; the session does the rest. Signing in here signs in the CLI on
//! this computer too, because the two are one device.

use dioxus::prelude::*;
use dirt_core::api::ErrorClass;
use dirt_core::session::{Session, SessionError, SessionState, SignOutError, Unsynced};
use dirt_core::util::as_sentence;

use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};
use crate::state::AppState;

/// Whether a line under the buttons reports something done or a failure.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Info,
    Error,
}

type Message = Signal<Option<(MessageKind, String)>>;

#[component]
pub(super) fn AccountSettingsTab() -> Element {
    let state = use_context::<AppState>();
    let mut busy = use_signal(|| false);
    let mut message: Message = use_signal(|| None);
    // Set when a sign-out found notes that have not reached the server: the
    // person is then offered a way to sign out anyway.
    let mut unsynced: Signal<Option<usize>> = use_signal(|| None);
    let mut local_waiting = use_signal(|| 0usize);

    let session = (state.session)();
    let session_state = (state.session_state)();
    let sync_failure = (state.sync_status)().failure;

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

    let Some(session) = session else {
        return rsx! {
            SettingRow {
                label: "Account",
                description: "Sign in to sync your notes between devices",
                div { class: "auth-hint",
                    "This build of Dirt was made without a server to sync with. Notes are kept on this device only."
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
            message.set(Some((
                MessageKind::Info,
                "Finish signing in in your browser.".to_owned(),
            )));
            spawn(async move {
                let outcome = session.sign_in_with_browser(open_browser).await;
                message.set(match outcome {
                    Ok(SessionState::Ready { .. }) | Err(SessionError::Cancelled) => None,
                    Ok(_) => Some((
                        MessageKind::Info,
                        "Signed in. This device is waiting for approval.".to_owned(),
                    )),
                    Err(error) => Some((MessageKind::Error, as_sentence(&error))),
                });
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
                    Err(error) => Some((MessageKind::Error, as_sentence(&error))),
                });
                busy.set(false);
            });
        }
    };

    let move_notes = {
        let session = session.clone();
        move |_: MouseEvent| {
            let session = session.clone();
            busy.set(true);
            spawn(async move {
                message.set(Some(match session.move_local_notes().await {
                    Ok(moved) => {
                        crate::queries::invalidate_notes_query().await;
                        local_waiting.set(0);
                        (
                            MessageKind::Info,
                            format!("Moved {moved} note(s) into your account."),
                        )
                    }
                    Err(error) => (MessageKind::Error, as_sentence(&error)),
                }));
                busy.set(false);
            });
        }
    };

    let sign_out = {
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
                                "{notes} note(s) have not been synced{why}. Signing out would delete them. Export them from the Data tab first."
                            ),
                        )));
                    }
                    Err(error) => message.set(Some((MessageKind::Error, as_sentence(&error)))),
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

    let must_sign_in_again = sync_failure
        .as_ref()
        .is_some_and(|failure| failure.class == ErrorClass::StopUntilSignedIn);

    rsx! {
        SettingRow {
            label: "Account",
            description: "Sign in to sync your notes between devices",

            div { class: "auth-panel",
                match session_state {
                    SessionState::SignedOut => rsx! {
                        div { class: "auth-hint",
                            "Not signed in. Notes are kept on this device only."
                        }
                        div { class: "auth-actions",
                            Button {
                                variant: ButtonVariant::Primary,
                                disabled: busy(),
                                onclick: sign_in,
                                "Sign in"
                            }
                        }
                    },
                    SessionState::SigningIn => rsx! {
                        div { class: "auth-hint",
                            "Waiting for you to finish signing in in your browser."
                        }
                        div { class: "auth-actions",
                            Button {
                                variant: ButtonVariant::Secondary,
                                onclick: cancel_sign_in,
                                "Cancel"
                            }
                        }
                    },
                    SessionState::WaitingForApproval { account_id, .. } => rsx! {
                        div { class: "auth-status", "Account {account_id}" }
                        div { class: "auth-hint",
                            "This device is waiting to be approved by another device of this account. Until then it cannot read the account's notes, and notes written here stay on this device."
                        }
                        div { class: "auth-actions",
                            Button {
                                variant: ButtonVariant::Secondary,
                                disabled: busy(),
                                onclick: check_approval,
                                "Check again"
                            }
                            Button {
                                variant: ButtonVariant::Ghost,
                                disabled: busy(),
                                onclick: sign_out_keeping,
                                "Sign out"
                            }
                        }
                    },
                    SessionState::Ready { account_id, .. } => rsx! {
                        div { class: "auth-status", "Signed in. Account {account_id}" }
                        if must_sign_in_again {
                            div { class: "auth-error",
                                "This device has to sign in again before it can sync."
                            }
                        }
                        if local_waiting() > 0 {
                            div { class: "auth-hint",
                                "{local_waiting()} note(s) written before signing in are on this device but not in your account."
                            }
                        }
                        div { class: "auth-actions",
                            if must_sign_in_again {
                                Button {
                                    variant: ButtonVariant::Primary,
                                    disabled: busy(),
                                    onclick: sign_in,
                                    "Sign in again"
                                }
                            }
                            if local_waiting() > 0 {
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    disabled: busy(),
                                    onclick: move_notes,
                                    "Move them into my account"
                                }
                            }
                            Button {
                                variant: ButtonVariant::Ghost,
                                disabled: busy(),
                                onclick: sign_out_keeping,
                                "Sign out"
                            }
                            if unsynced().is_some() {
                                Button {
                                    variant: ButtonVariant::Ghost,
                                    disabled: busy(),
                                    onclick: sign_out_discarding,
                                    "Sign out and delete them"
                                }
                            }
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
    }
}

/// Opens the sign-in page in the system browser.
fn open_browser(address: &str) -> Result<(), String> {
    webbrowser::open(address).map_err(|error| error.to_string())
}

/// What a signed-in session offers, for the Sync tab.
pub(super) fn session_of(state: &AppState) -> Option<Session> {
    (state.session)()
}
