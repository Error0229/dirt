//! Mobile Devices view: more than one device on an account.
//!
//! On a device that is waiting, this is where it gets let in: by another
//! device, by comparing a code, or by the recovery phrase. On an approved
//! device it lists the account's devices, lets a waiting one in, makes the
//! recovery phrase, and resets or deletes the account. The desktop app has
//! the same screen in Settings → Devices.
//!
//! The approval panel is the one place that asks the server repeatedly,
//! every two seconds and only while it is on screen.

use std::rc::Rc;

use dioxus::prelude::*;
use dirt_core::api::{DeviceInfo, DeviceStatus};
use dirt_core::enrolment::CancelReason;
use dirt_core::session::{
    Answer, Exchange, ExchangeStep, ExchangeView, MyDevice, Pace, ResetOutcome, Session,
    SessionError, SessionState,
};
use dirt_core::util::as_sentence;

use crate::state::{AppState, View};
use crate::ui::{ButtonVariant, UiButton, UiTextarea};

/// A line shown above everything else: what just happened. `true` marks a
/// failure.
type Notice = Signal<Option<(bool, String)>>;

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

/// How long until a time, in words.
fn wait_until(at_ms: i64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        });
    let hours = (at_ms - now_ms).max(0) / 3_600_000;
    match hours {
        0 => "now".to_owned(),
        1..=47 => format!("in about {hours} hour(s)"),
        _ => format!("in about {} days", hours / 24),
    }
}

fn session_of(state: &AppState) -> Option<Session> {
    state.session.peek().clone()
}

#[component]
pub fn Devices() -> Element {
    let state = use_context::<AppState>();
    let mut view_signal = state.view;
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
            div { style: "{PANEL}",
                p { style: "{TEXT}", "There is no account on this device to add devices to." }
            }
        }
    } else {
        match session_state {
            SessionState::SignedOut | SessionState::SigningIn => rsx! {
                div { style: "{PANEL}",
                    p { style: "{TEXT}", "Sign in from Settings first." }
                }
            },
            SessionState::WaitingForApproval { .. } => rsx! { WaitingDevice { notice } },
            SessionState::Ready { .. } => rsx! { ApprovedDevice { notice } },
        }
    };

    rsx! {
        div {
            style: "padding: 12px 16px; display: flex; flex-direction: column; gap: 12px;",
            div {
                style: "display: flex; align-items: center; gap: 8px;",
                UiButton {
                    r#type: "button",
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| view_signal.set(View::Settings),
                    "← Back"
                }
                h2 {
                    style: "margin: 0; font-size: 18px; font-weight: 600;",
                    "Devices"
                }
            }
            if let Some((failed, text)) = notice() {
                p { style: if failed { WARNING } else { TEXT }, "{text}" }
            }
            {body}
        }
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
            p { style: "{TEXT}", "Both devices should show this code:" }
            p {
                style: "margin: 0; font-size: 30px; font-weight: 700; letter-spacing: 2px; font-family: monospace; text-align: center;",
                "{code}"
            }
            p { style: "{TEXT}",
                "Compare it with the other device. If they differ, someone may be between the two."
            }
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Primary,
                onclick: move |_| answer(Answer::Match),
                "They match"
            }
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Secondary,
                onclick: move |_| answer(Answer::Mismatch),
                "They differ"
            }
        },
        Some(ExchangeView::Step(ExchangeStep::Finishing)) => rsx! {
            p { style: "{TEXT}", "Confirmed here. Waiting for the other device to confirm too." }
        },
        Some(_) => rsx! {
            p { style: "{TEXT}", "{waiting_words}" }
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Secondary,
                onclick: move |_| exchange.set(None),
                "Stop"
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
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Danger,
                disabled,
                onclick: move |_| {
                    armed.set(false);
                    onconfirm.call(());
                },
                "Yes, {label}"
            }
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Secondary,
                onclick: move |_| armed.set(false),
                "Cancel"
            }
        } else {
            UiButton {
                r#type: "button",
                block: true,
                variant: ButtonVariant::Secondary,
                disabled,
                onclick: move |_| armed.set(true),
                "{standing_alone}"
            }
        }
    }
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
                        "The account holds notes, so this has to wait. Its other devices are told and can refuse. Press the button again {}.",
                        wait_until(effective_at_ms)
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
        div { style: "{PANEL}",
            p { style: "{HEADING}", "Let this device in" }
            if exchange().is_some() {
                ExchangePanel {
                    exchange,
                    notice,
                    joining: true,
                    waiting_words: "On a device that is already signed in, open Devices and let this one in.",
                    on_over: move |()| {},
                }
            } else {
                p { style: "{TEXT}",
                    "This device is signed in but cannot read the account's notes yet."
                }
                if info.is_some() && approvers == 0 {
                    p { style: "{TEXT}", "No other device of this account is signed in and approved." }
                }
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Primary,
                    disabled: busy(),
                    onclick: start,
                    "Ask another device"
                }
            }
        }

        if has_phrase {
            div { style: "{PANEL}",
                p { style: "{HEADING}", "Recovery phrase" }
                p { style: "{TEXT}",
                    "Type the account's 24 words to let this device in without another device."
                }
                UiTextarea {
                    rows: "4",
                    value: "{phrase}",
                    oninput: move |event: FormEvent| phrase.set(event.value()),
                }
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Secondary,
                    disabled: busy() || phrase().trim().is_empty(),
                    onclick: recover,
                    "Use the phrase"
                }
            }
        }

        div { style: "{PANEL}",
            p { style: "{HEADING}", "Start the account again" }
            p { style: "{TEXT}",
                "For when no device is left that could let this one in and there is no recovery phrase. The notes on the server cannot be read by anyone and are deleted."
            }
            if let Some(at) = reset_at {
                p { style: "{TEXT}", "Asked for. It can go ahead {wait_until(at)}." }
            }
            ConfirmedButton {
                label: "start the account again",
                disabled: busy(),
                onconfirm: reset,
            }
        }
    }
}

// ---- A device that is approved ----

/// A request made from a button, boxed so that one helper can run any of
/// them.
type Request = Box<
    dyn FnOnce(
        Session,
    )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), SessionError>>>>,
>;

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

    // The list is read when the view opens, after each change made here,
    // and when a sync says a device is waiting.
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
    let mut run = move |done: &'static str, request: Request| {
        let Some(session) = session_of(&state) else {
            return;
        };
        busy.set(true);
        spawn(async move {
            notice.set(Some(match request(session).await {
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
    // The phrase stops working only if there was one.
    let key_changed_words = if has_phrase {
        "The account's key was changed. The recovery phrase no longer works: make a new one."
    } else {
        "The account's key was changed."
    };
    // What is left of a change of the account's key, as of the last sync.
    let key_change =
        dirt_core::sync::overview::SyncOverview::from(&(state.sync_status)()).key_change();
    let reset_at = (state.sync_status)()
        .reset_effective_at_ms
        .or_else(|| info.as_ref().and_then(|me| me.reset_effective_at_ms));
    let now_listed = devices();

    rsx! {
        if let Some(at) = reset_at {
            div { style: "{PANEL}",
                p { style: "{HEADING}", "Someone asked to start this account again" }
                p { style: "{WARNING}",
                    "A device that is signed in but not approved asked for the account's notes to be deleted. It takes effect {wait_until(at)}. If that was not you, refuse it."
                }
                UiButton {
                    r#type: "button",
                    block: true,
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

        div { style: "{PANEL}",
            p { style: "{HEADING}", "Devices" }
            if exchange().is_some() {
                ExchangePanel {
                    exchange,
                    notice,
                    joining: false,
                    waiting_words: "On the new device, open Devices and choose to ask another device.",
                    on_over: move |()| reload += 1,
                }
            } else {
                p { style: "{TEXT}",
                    "A new device signs in and then has to be let in here."
                }
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

        div { style: "{PANEL}",
            p { style: "{HEADING}", "Recovery phrase" }
            if let Some(shown) = words() {
                p {
                    style: "margin: 0; font-family: monospace; font-size: 14px; line-height: 1.7; padding: 10px; border: 1px solid #d1d5db; border-radius: 10px; user-select: text;",
                    "{shown}"
                }
                p { style: "{WARNING}",
                    "Write it down now. It is stored nowhere and will not be shown again. Anyone who has it and can sign in as you can read your notes."
                }
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Secondary,
                    onclick: move |_| words.set(None),
                    "I have written it down"
                }
            } else {
                p { style: "{TEXT}",
                    if has_phrase {
                        "This account has a recovery phrase: 24 words that let a new device in when no other device is left."
                    } else {
                        "This account has no recovery phrase. If every device is lost, so are the notes."
                    }
                }
                UiButton {
                    r#type: "button",
                    block: true,
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

        div { style: "{PANEL}",
            p { style: "{HEADING}", "The account's key" }
            p { style: "{TEXT}",
                "A device that was removed keeps the key it had. Changing the key means nothing written afterwards can be read with it. The recovery phrase stops working and has to be made again."
            }
            if let Some(text) = key_change {
                p { style: "{TEXT}", "{text}" }
            }
            ConfirmedButton {
                label: "change the account's key",
                disabled: busy(),
                onconfirm: move |()| run(
                    key_changed_words,
                    Box::new(move |session| Box::pin(async move {
                                session.change_account_key().await?;
                                // A phrase still on screen was made for
                                // the key just replaced. It opens nothing
                                // now, and must not be written down.
                                let mut words = words;
                                words.set(None);
                                Ok(())
                            })),
                ),
            }
        }

        div { style: "{PANEL}",
            p { style: "{HEADING}", "Start again or delete" }
            p { style: "{TEXT}",
                "Starting again deletes the account's notes on the server, its recovery phrase and every other device; this device keeps its notes and sends them again. Deleting removes the account for every device; the notes stay on this device."
            }
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
                    "The account was deleted. Its notes are still on this device.",
                    Box::new(|session| Box::pin(async move { session.delete_account().await })),
                ),
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
    let waiting = device.status == DeviceStatus::Pending;
    let detail = if device.current {
        "this device"
    } else if waiting {
        "waiting to be let in"
    } else {
        "let in"
    };
    let for_approve = device.clone();
    let for_remove = device.clone();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 6px; padding: 8px 0; border-top: 1px solid #f3f4f6;",
            p { style: "margin: 0; font-size: 14px; color: #111827;", "{device.name}" }
            p { style: "{TEXT}", "{detail}" }
            if waiting {
                UiButton {
                    r#type: "button",
                    block: true,
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
