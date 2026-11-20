//! Settings → Account (interface design, sections 6.9, 6.12 and 9.1):
//! signing in and out, the notes from before signing in, and the
//! account's actions that cannot be undone.

use dioxus::prelude::*;
use dirt_core::api::ErrorClass;
use dirt_core::session::{
    KeyChange, ResetOutcome, SessionError, SessionState, SignOutError, Unsynced,
};
use dirt_core::sync::overview::{sign_out_would_lose, SyncOverview, SIGN_IN_AGAIN_TO_SYNC};
use dirt_core::sync::worker::Trigger;
use dirt_core::util::as_sentence;

use super::rows::{Btn, Card, Row, RowButton};
use super::{facts, settings_nav, Linked, Section, SignIn};
use crate::components::{use_toasts, ConfirmInPlace, Toast};
use crate::icons::{Icon, IconSize, IconSvg};

/// A time in the future, for "after Nov 25, 14:00".
pub(super) fn when(at_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at_ms).map_or_else(
        || "later".to_owned(),
        |at| {
            at.with_timezone(&chrono::Local)
                .format("%b %-d, %H:%M")
                .to_string()
        },
    )
}

/// One of the account's actions, while it asks to be confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asking {
    SignOut,
    ChangeKey,
    StartAgain,
    Delete,
}

/// "12 notes", "1 note".
fn notes(count: usize) -> String {
    if count == 1 {
        "1 note".to_owned()
    } else {
        format!("{count} notes")
    }
}

#[component]
pub(super) fn Account(linked: Linked, sign_in: SignIn, notes_brought: EventHandler<()>) -> Element {
    let toasts = use_toasts();
    let nav = settings_nav();
    let facts = facts();
    let mut asking = use_signal(|| None::<Asking>);
    let mut busy = use_signal(|| false);
    let mut actions_open = use_signal(|| false);
    // "Bring the 12 notes on this device into the account": checked by
    // default, and remembered until the device is let in.
    let mut bring = use_signal(|| true);
    let mut bring_when_in = use_signal(|| false);
    // What signing out would lose, once it has said so: how many notes and
    // files, and the words for it.
    let mut would_lose = use_signal(|| None::<(usize, usize, String)>);
    // A question left open is not answered later: Settings closing takes
    // every confirmation away, the word typed for Delete with it.
    use_effect(move || {
        if !nav.is_open() {
            asking.set(None);
            would_lose.set(None);
        }
    });
    // Notes in the local store: those a sign-in would bring, or those
    // written before signing in that are still not in the account.
    let local = use_resource(move || {
        // Notes are written while Settings is closed: counted as it opens.
        let _ = nav.is_open();
        let state = *linked.state.read();
        let _ = facts.reload.read();
        let session = linked.session.read().clone();
        async move {
            let Some(session) = session else { return 0 };
            match state {
                SessionState::SignedOut | SessionState::SigningIn => {
                    session.store().note_count().await.unwrap_or(0)
                }
                SessionState::Ready { .. } => session.local_notes_waiting().await.unwrap_or(0),
                SessionState::WaitingForApproval { .. } => 0,
            }
        }
    });

    // Brings the notes in, and says how many came.
    let bring_in = move || {
        let Some(session) = linked.session() else {
            return;
        };
        spawn(async move {
            match session.move_local_notes().await {
                Ok(0) => {}
                Ok(moved) => {
                    notes_brought.call(());
                    toasts.show(Toast::new(format!(
                        "Brought {} into the account",
                        notes(moved)
                    )));
                }
                Err(error) => toasts.show_error(
                    Toast::new("The notes could not be brought in").detail(as_sentence(&error)),
                ),
            }
            let mut reload = facts.reload;
            reload += 1;
        });
    };

    // A device let in after signing in brings its notes then, if it was
    // asked to.
    use_effect(move || {
        let ready = linked.state.read().is_ready();
        if ready && *bring_when_in.peek() {
            bring_when_in.set(false);
            bring_in();
        }
    });

    let state = *linked.state.read();
    let Some(session) = linked.session.read().clone() else {
        return rsx! {
            Card {
                Row {
                    icon: Icon::User,
                    title: "Not signed in",
                    detail: "This build of Dirt has no server to sync with",
                }
            }
        };
    };

    let start_sign_in = {
        let session = session.clone();
        move |_: MouseEvent| {
            let session = session.clone();
            if *linked.state.peek() == SessionState::SigningIn {
                session.cancel_sign_in();
                return;
            }
            let bring_now = *bring.peek() && local.peek().unwrap_or(0) > 0;
            spawn(async move {
                match sign_in.call(session).await {
                    Ok(SessionState::Ready { .. }) if bring_now => bring_in(),
                    Ok(SessionState::WaitingForApproval { .. }) => {
                        bring_when_in.set(bring_now);
                        nav.open_at(Section::Devices);
                    }
                    Ok(_) | Err(SessionError::Cancelled) => {}
                    Err(error) => toasts
                        .show_error(Toast::new("Could not sign in").detail(as_sentence(&error))),
                }
            });
        }
    };

    // Signs out. It is always asked first without losing anything, which
    // syncs and says what is still unsent now. Only when the person has
    // agreed to lose just that does it go ahead; anything else asks again.
    let sign_out = {
        let session = session.clone();
        move |agreed: bool| {
            if *busy.peek() {
                return;
            }
            let session = session.clone();
            let shown = would_lose
                .peek()
                .as_ref()
                .map(|(notes, files, _)| (*notes, *files));
            busy.set(true);
            spawn(async move {
                let mut outcome = session.sign_out(Unsynced::Refuse).await;
                if let Err(SignOutError::Unsynced { notes, files, .. }) = &outcome {
                    if agreed && shown == Some((*notes, *files)) {
                        outcome = session.sign_out(Unsynced::Discard).await;
                    }
                }
                match outcome {
                    Ok(()) => {
                        asking.set(None);
                        would_lose.set(None);
                        toasts.show(Toast::new("Signed out"));
                    }
                    Err(SignOutError::Unsynced {
                        notes,
                        files,
                        cause,
                    }) => {
                        let words = sign_out_would_lose(notes, files, cause.as_deref());
                        would_lose.set(Some((notes, files, words)));
                        asking.set(Some(Asking::SignOut));
                    }
                    Err(error) => {
                        asking.set(None);
                        toasts.show_error(Toast::new("Not signed out").detail(as_sentence(&error)));
                    }
                }
                busy.set(false);
            });
        }
    };

    // The three actions that cannot be undone, once confirmed.
    let run = {
        let session = session.clone();
        move |action: Asking| {
            if *busy.peek() {
                return;
            }
            let session = session.clone();
            asking.set(None);
            busy.set(true);
            let had_phrase = facts
                .me
                .peek()
                .as_ref()
                .is_some_and(|me| me.recovery_available);
            spawn(async move {
                let said: Result<String, SessionError> =
                    match action {
                        Asking::ChangeKey => session.change_account_key().await.map(|change| {
                            match change {
                            KeyChange::Changed { .. } if had_phrase => {
                                "Key changed. The recovery phrase no longer works: make a new one"
                                    .to_owned()
                            }
                            KeyChange::Changed { .. } => "Key changed".to_owned(),
                            KeyChange::ChangedByAnotherDevice { .. } => {
                                "Another device has just changed the key".to_owned()
                            }
                        }
                        }),
                        Asking::StartAgain => {
                            session.reset_account().await.map(|outcome| match outcome {
                                ResetOutcome::Done => {
                                    "The account was started again. This is its only device now"
                                        .to_owned()
                                }
                                ResetOutcome::Scheduled { effective_at_ms } => format!(
                                "Asked for. Its other devices can refuse until {}; then ask again",
                                when(effective_at_ms)
                            ),
                            })
                        }
                        Asking::Delete => session.delete_account().await.map(|()| {
                            "The account was deleted. Its notes are still on this device".to_owned()
                        }),
                        Asking::SignOut => Ok(String::new()),
                    };
                match said {
                    Ok(said) => toasts.show(Toast::new(said)),
                    Err(error) => toasts
                        .show_error(Toast::new("That did not work").detail(as_sentence(&error))),
                }
                let mut reload = facts.reload;
                reload += 1;
                busy.set(false);
            });
        }
    };

    let status = linked.status.read().clone();
    let must_sign_in_again = status
        .failure
        .as_ref()
        .is_some_and(|failure| failure.class == ErrorClass::StopUntilSignedIn);
    let me = facts.me.read().clone();
    let reset_at = status
        .reset_effective_at_ms
        .or_else(|| me.as_ref().and_then(|me| me.reset_effective_at_ms));
    let key_change = SyncOverview::from(&status).key_change();
    let signed_in = matches!(
        state,
        SessionState::Ready { .. } | SessionState::WaitingForApproval { .. }
    );
    let ready = state.is_ready();
    let waiting_in = matches!(state, SessionState::WaitingForApproval { .. });
    let count = local().unwrap_or(0);

    rsx! {
        Card {
            if signed_in {
                ConfirmInPlace {
                    asking: asking() == Some(Asking::SignOut),
                    question: format!(
                        "{} Sign out all the same?",
                        would_lose().map(|(_, _, words)| words).unwrap_or_default()
                    ),
                    confirm: "Sign out",
                    on_confirm: {
                        let mut sign_out = sign_out.clone();
                        move |()| sign_out(true)
                    },
                    on_cancel: move |()| asking.set(None),
                    div { class: "dui-set-row",
                        span { class: "dui-avatar", "aria-hidden": "true", IconSvg { icon: Icon::User } }
                        div { class: "dui-set-words",
                            div { class: "dui-set-t", "Signed in" }
                            div { class: "dui-set-s",
                                if waiting_in {
                                    "Waiting to be let in"
                                } else {
                                    span { class: "dui-set-lock", IconSvg { icon: Icon::Lock, size: IconSize::Small } }
                                    "Encrypted on your devices"
                                }
                            }
                        }
                        Btn {
                            kind: "ghost",
                            disabled: busy(),
                            onclick: {
                                let mut sign_out = sign_out;
                                move |_| sign_out(false)
                            },
                            "Sign out"
                        }
                    }
                }
                if must_sign_in_again {
                    div { class: "dui-set-row",
                        super::rows::RowIcon { icon: Icon::Alert, tone: "warn" }
                        div { class: "dui-set-words",
                            div { class: "dui-set-t", "Sign in again" }
                            div { class: "dui-set-s", "{SIGN_IN_AGAIN_TO_SYNC}" }
                        }
                        Btn { kind: "primary", onclick: start_sign_in.clone(), "Sign in" }
                    }
                }
                if ready && count > 0 {
                    Row {
                        icon: Icon::Laptop,
                        title: format!("{} from before you signed in", notes(count)),
                        detail: "On this device, not in the account",
                        Btn { onclick: move |_| bring_in(), "Bring them in" }
                    }
                }
                if let (true, Some(at)) = (ready, reset_at) {
                    Row {
                        icon: Icon::Alert,
                        title: "Someone asked to start this account again",
                        detail: format!("It happens after {} unless you refuse", when(at)),
                        Btn {
                            kind: "primary",
                            disabled: busy(),
                            onclick: {
                                let session = session;
                                move |_| {
                                    if *busy.peek() {
                                        return;
                                    }
                                    busy.set(true);
                                    let session = session.clone();
                                    spawn(async move {
                                        match session.cancel_reset().await {
                                            Ok(()) => {
                                                toasts.show(Toast::new("Refused. Nothing was changed"));
                                                // The request is in the sync's figures
                                                // until the next sync.
                                                session.notify(Trigger::Manual);
                                            }
                                            Err(error) => toasts.show_error(
                                                Toast::new("Could not refuse").detail(as_sentence(&error)),
                                            ),
                                        }
                                        let mut reload = facts.reload;
                                        reload += 1;
                                        busy.set(false);
                                    });
                                }
                            },
                            "Refuse"
                        }
                    }
                }
            } else {
                div { class: "dui-set-row",
                    span { class: "dui-avatar off", "aria-hidden": "true", IconSvg { icon: Icon::User } }
                    div { class: "dui-set-words",
                        div { class: "dui-set-t", "Not signed in" }
                        div { class: "dui-set-s", "Notes stay on this device" }
                    }
                    Btn {
                        kind: "primary",
                        spinning: state == SessionState::SigningIn,
                        tip: (state == SessionState::SigningIn).then(|| "Cancel".to_owned()),
                        onclick: start_sign_in.clone(),
                        if state == SessionState::SigningIn { "Waiting for browser" } else { "Sign in" }
                    }
                }
                if count > 0 {
                    label { class: "dui-set-row dui-check",
                        input {
                            r#type: "checkbox",
                            checked: bring(),
                            onchange: move |event| bring.set(event.checked()),
                        }
                        "Bring the {notes(count)} on this device into the account"
                    }
                }
            }
        }
        if signed_in {
            button {
                r#type: "button",
                class: if actions_open() { "dui-disc-btn on" } else { "dui-disc-btn" },
                "aria-expanded": if actions_open() { "true" } else { "false" },
                onclick: move |_| actions_open.toggle(),
                IconSvg { icon: Icon::ChevronRight, size: IconSize::Small }
                "Account actions"
            }
            div { class: if actions_open() { "dui-disclose on" } else { "dui-disclose" },
                "inert": if actions_open() { None } else { Some("") },
                div {
                    Card {
                        if ready {
                            ConfirmInPlace {
                                asking: asking() == Some(Asking::ChangeKey),
                                question: "Change the key? Your other devices get the new one when they sync.",
                                confirm: "Change",
                                on_confirm: { let mut run = run.clone(); move |()| run(Asking::ChangeKey) },
                                on_cancel: move |()| asking.set(None),
                                RowButton {
                                    icon: Icon::Key,
                                    title: "Change the account key",
                                    detail: key_change.unwrap_or_else(|| "Locks out a lost device for good".to_owned()),
                                    onclick: move |_| asking.set(Some(Asking::ChangeKey)),
                                }
                            }
                        }
                        ConfirmInPlace {
                            asking: asking() == Some(Asking::StartAgain),
                            question: if ready {
                                "Start again? The notes on the server, the other devices and the recovery phrase go. This device keeps its notes."
                            } else {
                                "Start again? The account's notes on the server are deleted, and this becomes its only device."
                            },
                            confirm: "Start again",
                            on_confirm: { let mut run = run.clone(); move |()| run(Asking::StartAgain) },
                            on_cancel: move |()| asking.set(None),
                            RowButton {
                                icon: Icon::Refresh,
                                title: "Start the account again",
                                detail: match (waiting_in, me.as_ref().and_then(|me| me.reset_effective_at_ms)) {
                                    (true, Some(at)) => format!("Asked for. It can go ahead after {}", when(at)),
                                    _ => "When every device and the phrase are gone".to_owned(),
                                },
                                onclick: move |_| asking.set(Some(Asking::StartAgain)),
                            }
                        }
                        if ready {
                            ConfirmInPlace {
                                asking: asking() == Some(Asking::Delete),
                                question: "Delete every note on the server, for every device? Type DELETE.",
                                confirm: "Delete",
                                typed: "DELETE",
                                on_confirm: { let mut run = run; move |()| run(Asking::Delete) },
                                on_cancel: move |()| asking.set(None),
                                RowButton {
                                    icon: Icon::Trash,
                                    title: "Delete the account",
                                    detail: "Every note, from every device",
                                    danger: true,
                                    onclick: move |_| asking.set(Some(Asking::Delete)),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_counted_in_words() {
        assert_eq!(notes(1), "1 note");
        assert_eq!(notes(12), "12 notes");
    }
}
