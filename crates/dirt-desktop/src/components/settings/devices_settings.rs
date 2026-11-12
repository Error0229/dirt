//! Settings → Devices tab: more than one device on an account.
//!
//! On a device that is waiting, this is where it gets let in: by another
//! device, by comparing a code, or by the recovery phrase. On an approved
//! device it lists the account's devices, lets a waiting one in, makes the
//! recovery phrase, and resets or deletes the account.
//!
//! The approval screen is the one place that asks the server repeatedly,
//! every two seconds and only while it is on screen.

use std::rc::Rc;

use dioxus::prelude::*;
use dirt_core::api::{DeviceInfo, DeviceStatus};
use dirt_core::enrolment::CancelReason;
use dirt_core::session::{
    Answer, Exchange, ExchangeStep, ExchangeView, MyDevice, Pace, ResetOutcome, Session,
    SessionState,
};
use dirt_core::util::as_sentence;

use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};
use crate::state::AppState;
use crate::time_format::format_short_time;

/// A line the tab shows above everything else: what just happened. `true`
/// marks a failure.
type Notice = Signal<Option<(bool, String)>>;

const MISMATCH_WARNING: &str = "The codes did not match, so the device was not let in. \
     Someone may be interfering with the connection between your devices. \
     Try again on a network you trust.";

/// What to tell the person when an exchange is over, and whether it is bad
/// news.
fn closing_words(view: &ExchangeView, joining: bool) -> Option<(bool, String)> {
    let words = match view {
        ExchangeView::Step(ExchangeStep::Done) if joining => {
            (false, "This device was let in.".to_owned())
        }
        ExchangeView::Step(ExchangeStep::Done) => (false, "The device was let in.".to_owned()),
        ExchangeView::Step(ExchangeStep::Cancelled(CancelReason::Mismatch)) => {
            (true, MISMATCH_WARNING.to_owned())
        }
        ExchangeView::Step(ExchangeStep::Cancelled(CancelReason::Declined)) => (
            true,
            "It was called off. The device was not let in.".to_owned(),
        ),
        ExchangeView::Step(ExchangeStep::HandledElsewhere) => (
            false,
            "Another of your devices is dealing with that device.".to_owned(),
        ),
        ExchangeView::Failed(reason) => (true, reason.clone()),
        ExchangeView::TimedOut => (
            true,
            "The other device did not answer. Nothing was changed; try again.".to_owned(),
        ),
        ExchangeView::Step(_) => return None,
    };
    Some(words)
}

fn when(at_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at_ms).map_or_else(
        || "later".to_owned(),
        |at| at.format("%Y-%m-%d %H:%M UTC").to_string(),
    )
}

#[component]
pub(super) fn DevicesSettingsTab() -> Element {
    let state = use_context::<AppState>();
    let mut notice: Notice = use_signal(|| None);
    let session_state = (state.session_state)();

    // The panel that was running the exchange goes away the moment this
    // device is let in, so the news is given here.
    let mut was_waiting = use_signal(|| false);
    use_effect(move || {
        let now = (state.session_state)();
        let waiting = matches!(now, SessionState::WaitingForApproval { .. });
        if *was_waiting.peek() && now.is_ready() {
            notice.set(Some((false, "This device was let in.".to_owned())));
        }
        if *was_waiting.peek() != waiting {
            was_waiting.set(waiting);
        }
    });

    let body = if (state.session)().is_none() {
        rsx! {
            div { class: "auth-hint",
                "This build of Dirt was made without a server to sync with, so there is no account to add devices to."
            }
        }
    } else {
        match session_state {
            SessionState::SignedOut | SessionState::SigningIn => rsx! {
                div { class: "auth-hint", "Sign in on the Account tab first." }
            },
            SessionState::WaitingForApproval { .. } => rsx! { WaitingDevice { notice } },
            SessionState::Ready { .. } => rsx! { ApprovedDevice { notice } },
        }
    };

    rsx! {
        if let Some((failed, text)) = notice() {
            div { class: "settings-row",
                div { class: if failed { "auth-error" } else { "auth-message" }, "{text}" }
            }
        }
        {body}
    }
}

/// One side of an approval exchange, on screen.
#[component]
fn ExchangePanel(
    exchange: Signal<Option<Rc<Exchange>>>,
    notice: Notice,
    joining: bool,
    waiting_words: String,
    on_over: EventHandler<()>,
) -> Element {
    let mut exchange = exchange;
    let mut notice = notice;
    let mut view = use_signal(|| None::<ExchangeView>);

    // Follow the exchange for as long as it is on screen. Taking it off the
    // screen drops it, which stops the asking.
    let _follow = use_resource(move || {
        let current = exchange();
        async move {
            let Some(current) = current else {
                view.set(None);
                return;
            };
            let mut views = current.watch();
            loop {
                let latest = views.borrow_and_update().clone();
                if let Some(words) = closing_words(&latest, joining) {
                    notice.set(Some(words));
                    exchange.set(None);
                    on_over.call(());
                    return;
                }
                view.set(Some(latest));
                if views.changed().await.is_err() {
                    return;
                }
            }
        }
    });

    let answer = move |answer: Answer| {
        if let Some(current) = exchange.peek().as_ref() {
            current.answer(answer);
        }
    };

    match view() {
        Some(ExchangeView::Step(ExchangeStep::Compare { code })) => rsx! {
            div { class: "auth-panel",
                div { class: "auth-hint", "Both devices should show this code:" }
                div {
                    style: "font-size: 26px; font-weight: 600; letter-spacing: 2px; font-family: monospace; padding: 6px 0;",
                    "{code}"
                }
                div { class: "auth-hint",
                    "Compare it with the other device. If they differ, someone may be between the two."
                }
                div { class: "auth-actions",
                    Button {
                        variant: ButtonVariant::Primary,
                        onclick: move |_| answer(Answer::Match),
                        "They match"
                    }
                    Button {
                        variant: ButtonVariant::Secondary,
                        onclick: move |_| answer(Answer::Mismatch),
                        "They differ"
                    }
                }
            }
        },
        Some(ExchangeView::Step(ExchangeStep::Finishing)) => rsx! {
            div { class: "auth-hint", "Confirmed here. Waiting for the other device to confirm too." }
        },
        Some(_) => rsx! {
            div { class: "auth-panel",
                div { class: "auth-hint", "{waiting_words}" }
                div { class: "auth-actions",
                    Button {
                        variant: ButtonVariant::Ghost,
                        onclick: move |_| exchange.set(None),
                        "Stop"
                    }
                }
            }
        },
        None => rsx! {},
    }
}

/// A button that asks "are you sure" by needing a second press.
#[component]
fn ConfirmedButton(label: String, disabled: bool, onconfirm: EventHandler<()>) -> Element {
    let mut armed = use_signal(|| false);
    // The label is written in lower case so that it reads on after "Yes, ".
    // On its own it starts with a capital.
    let mut letters = label.chars();
    let standing_alone: String = letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default();
    rsx! {
        if armed() {
            Button {
                variant: ButtonVariant::Primary,
                disabled,
                onclick: move |_| {
                    armed.set(false);
                    onconfirm.call(());
                },
                "Yes, {label}"
            }
            Button {
                variant: ButtonVariant::Ghost,
                onclick: move |_| armed.set(false),
                "Cancel"
            }
        } else {
            Button {
                variant: ButtonVariant::Ghost,
                disabled,
                onclick: move |_| armed.set(true),
                "{standing_alone}"
            }
        }
    }
}

fn session_of(state: &AppState) -> Option<Session> {
    state.session.peek().clone()
}

// ---- A device that is waiting to be let in ----

#[component]
fn WaitingDevice(notice: Notice) -> Element {
    let mut notice = notice;
    let state = use_context::<AppState>();
    let mut exchange: Signal<Option<Rc<Exchange>>> = use_signal(|| None);
    let mut busy = use_signal(|| false);
    let mut phrase = use_signal(String::new);
    let mut account: Signal<Option<MyDevice>> = use_signal(|| None);

    // What the person can do depends on what the account has.
    let _load = use_resource(move || async move {
        if let Some(session) = session_of(&state) {
            account.set(session.my_device().await.ok());
        }
    });

    let start = move |_: MouseEvent| {
        let Some(session) = session_of(&state) else {
            return;
        };
        busy.set(true);
        notice.set(None);
        spawn(async move {
            match session.begin_join().await {
                Ok(join) => exchange.set(Some(Rc::new(Exchange::join(join, Pace::default())))),
                Err(error) => notice.set(Some((true, as_sentence(&error)))),
            }
            busy.set(false);
        });
    };

    let recover = move |_: MouseEvent| {
        let Some(session) = session_of(&state) else {
            return;
        };
        busy.set(true);
        notice.set(None);
        spawn(async move {
            let typed = phrase.peek().clone();
            match session.recover_with_phrase(&typed).await {
                Ok(_) => {
                    phrase.set(String::new());
                    notice.set(Some((false, "This device was let in.".to_owned())));
                }
                Err(error) => notice.set(Some((true, as_sentence(&error)))),
            }
            busy.set(false);
        });
    };

    let reset = move |()| {
        let Some(session) = session_of(&state) else {
            return;
        };
        busy.set(true);
        spawn(async move {
            notice.set(Some(match session.reset_account().await {
                Ok(ResetOutcome::Done) => (
                    false,
                    "The account was started again. This is now its only device.".to_owned(),
                ),
                Ok(ResetOutcome::Scheduled { effective_at_ms }) => (
                    false,
                    format!(
                        "The account holds notes, so this has to wait. Its other devices are told and can refuse. Press the button again after {}.",
                        when(effective_at_ms)
                    ),
                ),
                Err(error) => (true, as_sentence(&error)),
            }));
            if let Some(session) = session_of(&state) {
                account.set(session.my_device().await.ok());
            }
            busy.set(false);
        });
    };

    let info = account();
    let approvers = info.as_ref().map_or(0, |me| me.approvers);
    let has_phrase = info.as_ref().is_some_and(|me| me.recovery_available);
    let reset_at = info.as_ref().and_then(|me| me.reset_effective_at_ms);

    rsx! {
        SettingRow {
            label: "Let this device in",
            description: "This device is signed in but cannot read the account's notes yet",

            if exchange().is_some() {
                ExchangePanel {
                    exchange,
                    notice,
                    joining: true,
                    waiting_words: "On a device that is already signed in, open Settings → Devices and approve this one.",
                    on_over: move |()| {},
                }
            } else {
                div { class: "auth-panel",
                    if info.is_some() && approvers == 0 {
                        div { class: "auth-hint",
                            "No other device of this account is signed in and approved."
                        }
                    }
                    div { class: "auth-actions",
                        Button {
                            variant: ButtonVariant::Primary,
                            disabled: busy(),
                            onclick: start,
                            "Ask another device"
                        }
                    }
                }
            }
        }

        if has_phrase {
            SettingRow {
                label: "Recovery phrase",
                description: "Type the account's 24 words to let this device in without another device",

                div { class: "auth-panel",
                    textarea {
                        rows: "3",
                        style: "width: 100%; box-sizing: border-box; font-family: monospace; font-size: 13px; padding: 8px; border-radius: 6px;",
                        value: "{phrase}",
                        oninput: move |event| phrase.set(event.value()),
                    }
                    div { class: "auth-actions",
                        Button {
                            variant: ButtonVariant::Secondary,
                            disabled: busy() || phrase().trim().is_empty(),
                            onclick: recover,
                            "Use the phrase"
                        }
                    }
                }
            }
        }

        SettingRow {
            label: "Start the account again",
            description: "For when no device is left that could let this one in and there is no recovery phrase. The notes on the server cannot be read by anyone and are deleted.",

            div { class: "auth-panel",
                if let Some(at) = reset_at {
                    div { class: "auth-hint", "Asked for. It can go ahead after {when(at)}." }
                }
                div { class: "auth-actions",
                    ConfirmedButton {
                        label: "start the account again",
                        disabled: busy(),
                        onconfirm: reset,
                    }
                }
            }
        }
    }
}

// ---- A device that is approved ----

#[component]
fn ApprovedDevice(notice: Notice) -> Element {
    let mut notice = notice;
    let state = use_context::<AppState>();
    let mut exchange: Signal<Option<Rc<Exchange>>> = use_signal(|| None);
    let mut busy = use_signal(|| false);
    let mut devices: Signal<Vec<DeviceInfo>> = use_signal(Vec::new);
    let mut account: Signal<Option<MyDevice>> = use_signal(|| None);
    let mut words: Signal<Option<String>> = use_signal(|| None);
    let mut reload = use_signal(|| 0u32);

    // The list is read when the tab opens, after each change made here, and
    // when a sync says a device is waiting.
    let _load = use_resource(move || async move {
        let _ = reload();
        let _ = (state.sync_status)().pending_devices;
        if let Some(session) = session_of(&state) {
            if let Ok(list) = session.devices().await {
                devices.set(list);
            }
            account.set(session.my_device().await.ok());
        }
    });

    let mut approve = move |device: DeviceInfo| {
        let Some(session) = session_of(&state) else {
            return;
        };
        notice.set(None);
        match session.begin_approval(device) {
            Ok(approval) => {
                exchange.set(Some(Rc::new(Exchange::approve(approval, Pace::default()))));
            }
            Err(error) => notice.set(Some((true, as_sentence(&error)))),
        }
    };

    // Runs one request against the session and says how it went.
    let mut run = move |done: &'static str,
                        action: Box<
        dyn FnOnce(
            Session,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), dirt_core::session::SessionError>>>,
        >,
    >| {
        let Some(session) = session_of(&state) else {
            return;
        };
        busy.set(true);
        spawn(async move {
            notice.set(Some(match action(session).await {
                Ok(()) => (false, done.to_owned()),
                Err(error) => (true, as_sentence(&error)),
            }));
            reload += 1;
            busy.set(false);
        });
    };

    let new_phrase = move |_: MouseEvent| {
        let Some(session) = session_of(&state) else {
            return;
        };
        busy.set(true);
        notice.set(None);
        spawn(async move {
            match session.create_recovery_phrase().await {
                Ok(phrase) => words.set(Some(phrase.words().to_string())),
                Err(error) => notice.set(Some((true, as_sentence(&error)))),
            }
            reload += 1;
            busy.set(false);
        });
    };

    let info = account();
    let has_phrase = info.as_ref().is_some_and(|me| me.recovery_available);
    // What is left of a change of the account's key, as of the last sync.
    let key_change =
        dirt_core::sync::overview::SyncOverview::from(&(state.sync_status)()).key_change();
    let reset_at = (state.sync_status)()
        .reset_effective_at_ms
        .or_else(|| info.as_ref().and_then(|me| me.reset_effective_at_ms));
    let now_listed = devices();

    rsx! {
        if let Some(at) = reset_at {
            SettingRow {
                label: "Someone asked to start this account again",
                description: "A device that is signed in but not approved asked for the account's notes to be deleted. If that was not you, refuse it.",

                div { class: "auth-panel",
                    div { class: "auth-error", "It takes effect after {when(at)}." }
                    div { class: "auth-actions",
                        Button {
                            variant: ButtonVariant::Primary,
                            disabled: busy(),
                            onclick: move |_| run(
                                "The request was refused. Nothing was changed.",
                                Box::new(|session| Box::pin(async move { session.cancel_reset().await })),
                            ),
                            "Refuse"
                        }
                    }
                }
            }
        }

        SettingRow {
            label: "Devices",
            description: "The devices of this account. A new device signs in and then has to be let in here",

            if exchange().is_some() {
                ExchangePanel {
                    exchange,
                    notice,
                    joining: false,
                    waiting_words: "On the new device, open Settings → Devices and choose to ask another device.",
                    on_over: move |()| reload += 1,
                }
            } else {
                div { class: "auth-panel",
                    for device in now_listed.into_iter().filter(|device| device.status != DeviceStatus::Revoked) {
                        DeviceRow {
                            key: "{device.id}",
                            device,
                            busy: busy(),
                            on_approve: move |device: DeviceInfo| approve(device),
                            on_remove: move |device: DeviceInfo| run(
                                "The device was removed and can no longer reach the server. It still has the account's key: change the key below so that it can read nothing written from now on.",
                                Box::new(move |session| Box::pin(async move { session.remove_device(device.id).await })),
                            ),
                        }
                    }
                }
            }
        }

        SettingRow {
            label: "Recovery phrase",
            description: "Twenty-four words that let a new device in when no other device is left. Optional",

            div { class: "auth-panel",
                if let Some(shown) = words() {
                    div {
                        style: "font-family: monospace; font-size: 13px; line-height: 1.6; padding: 10px; border-radius: 6px; border: 1px solid var(--primary-color-6); user-select: text;",
                        "{shown}"
                    }
                    div { class: "auth-error",
                        "Write it down now. It is stored nowhere and will not be shown again. Anyone who has it and can sign in as you can read your notes."
                    }
                    div { class: "auth-actions",
                        Button {
                            variant: ButtonVariant::Secondary,
                            onclick: move |_| words.set(None),
                            "I have written it down"
                        }
                    }
                } else {
                    div { class: "auth-hint",
                        if has_phrase {
                            "This account has a recovery phrase."
                        } else {
                            "This account has no recovery phrase. If every device is lost, so are the notes."
                        }
                    }
                    div { class: "auth-actions",
                        Button {
                            variant: ButtonVariant::Secondary,
                            disabled: busy(),
                            onclick: new_phrase,
                            if has_phrase { "Make a new one" } else { "Make a recovery phrase" }
                        }
                        if has_phrase {
                            ConfirmedButton {
                                label: "remove the phrase",
                                disabled: busy(),
                                onconfirm: move |()| run(
                                    "The recovery phrase was removed.",
                                    Box::new(|session| Box::pin(async move { session.remove_recovery_phrase().await })),
                                ),
                            }
                        }
                    }
                }
            }
        }

        SettingRow {
            label: "The account's key",
            description: "A device that was removed keeps the key it had. Changing the key means nothing written afterwards can be read with it. The recovery phrase stops working and has to be made again",

            div { class: "auth-panel",
                if let Some(text) = key_change {
                    div { class: "auth-hint", "{text}" }
                }
                div { class: "auth-actions",
                    ConfirmedButton {
                        label: "change the account's key",
                        disabled: busy(),
                        onconfirm: move |()| run(
                            "The account's key was changed. The recovery phrase no longer works: make a new one above.",
                            Box::new(|session| Box::pin(async move { session.change_account_key().await.map(|_| ()) })),
                        ),
                    }
                }
            }
        }

        SettingRow {
            label: "Start again or delete",
            description: "Starting again deletes the account's notes on the server, its recovery phrase and every other device; this device keeps its notes and sends them again. Deleting removes the account for every device; the notes stay on this computer.",

            div { class: "auth-panel",
                div { class: "auth-actions",
                    ConfirmedButton {
                        label: "start the account again",
                        disabled: busy(),
                        onconfirm: move |()| run(
                            "The account was started again. This is now its only device.",
                            Box::new(|session| Box::pin(async move { session.reset_account().await.map(|_| ()) })),
                        ),
                    }
                    ConfirmedButton {
                        label: "delete the account",
                        disabled: busy(),
                        onconfirm: move |()| run(
                            "The account was deleted. Its notes are still on this computer.",
                            Box::new(|session| Box::pin(async move { session.delete_account().await })),
                        ),
                    }
                }
            }
        }
    }
}

#[component]
fn DeviceRow(
    device: DeviceInfo,
    busy: bool,
    on_approve: EventHandler<DeviceInfo>,
    on_remove: EventHandler<DeviceInfo>,
) -> Element {
    let seen = match format_short_time(device.last_seen_ms).as_str() {
        "now" => "seen just now".to_owned(),
        ago => format!("seen {ago} ago"),
    };
    let waiting = device.status == DeviceStatus::Pending;
    let detail = if device.current {
        "this device".to_owned()
    } else if waiting {
        "waiting to be let in".to_owned()
    } else {
        seen
    };
    let for_approve = device.clone();
    let for_remove = device.clone();

    rsx! {
        div {
            style: "display: flex; align-items: center; justify-content: space-between; gap: 8px;",
            div {
                div { style: "font-size: 13px;", "{device.name}" }
                div { class: "auth-hint", "{detail}" }
            }
            div { class: "auth-actions",
                if waiting {
                    Button {
                        variant: ButtonVariant::Primary,
                        disabled: busy,
                        onclick: move |_| on_approve.call(for_approve.clone()),
                        "Let it in"
                    }
                }
                if !device.current {
                    ConfirmedButton {
                        label: "remove",
                        disabled: busy,
                        onconfirm: move |()| on_remove.call(for_remove.clone()),
                    }
                }
            }
        }
    }
}
