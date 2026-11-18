//! Settings → Devices (interface design, sections 6.12, 6.13, 9.2 and
//! 9.3): the account's devices, letting a new one in by comparing the
//! code, and the recovery phrase.
//!
//! On a device that waits to be let in, this is where it waits: the
//! exchange begins by itself while Settings is open, and the recovery
//! phrase is offered beside it. Comparing the code is the one place Dirt
//! asks the server over and over, every two seconds, so it happens only
//! while Settings is open and stops when it closes.

use std::rc::Rc;
use std::time::{Duration, Instant};

use dioxus::prelude::*;
use dirt_core::api::{DeviceInfo, DeviceStatus};
use dirt_core::enrolment::CancelReason;
use dirt_core::session::{Answer, Exchange, ExchangeStep, ExchangeView, Pace, SessionState};
use dirt_core::util::as_sentence;

use super::code::{CodeComparison, Outcome, GATHER_MS};
use super::rows::{Btn, Card, Note, Row, RowIcon, Spinner};
use super::{facts, settings_nav, Linked};
use crate::components::{use_toasts, ConfirmInPlace, IconButton, IconButtonSize, Toast};
use crate::icons::{Icon, IconSize, IconSvg};
use crate::time::{edited_short, Clock};

/// How often "Add a device" looks for a device that has signed in.
const LOOK_EVERY: Duration = Duration::from_secs(2);

/// How long it looks before it gives up, as an exchange does.
const LOOK_FOR: Duration = Duration::from_secs(300);

/// How long the drawn tick stays before the list takes its place.
const TICK_STAYS: Duration = Duration::from_millis(1700);

/// What the person is told when the pictures differed (SECURITY.md,
/// "Approving a device").
const DIFFERED: &str = "The pictures differed, so someone may be between your devices. \
     Start again from the new device, on a network you trust.";

/// The icon for a device, by its name: Android names itself so.
fn device_icon(name: &str) -> Icon {
    let name = name.to_lowercase();
    if name.contains("android") || name.contains("phone") {
        Icon::Phone
    } else {
        Icon::Laptop
    }
}

/// When a device was last seen, in the list's own words.
fn seen(device: &DeviceInfo, clock: Clock) -> String {
    let when = edited_short(device.last_seen_ms, clock);
    let ago = when
        .strip_suffix(['m', 'h'])
        .is_some_and(|count| count.parse::<u32>().is_ok());
    match when.as_str() {
        "now" => "Seen just now".to_owned(),
        _ if ago => format!("Seen {when} ago"),
        _ => format!("Seen {when}"),
    }
}

#[component]
pub(super) fn Devices(linked: Linked, copy: EventHandler<String>) -> Element {
    let state = *linked.state.read();
    if linked.session.read().is_none() {
        return rsx! {
            Card { Note { text: "This build of Dirt has no server, so there is no account to add devices to." } }
        };
    }
    match state {
        SessionState::SignedOut | SessionState::SigningIn => rsx! {
            Card { Note { text: "Sign in to use Dirt on more than one device." } }
        },
        SessionState::WaitingForApproval { .. } => rsx! { JoinHere { linked } },
        SessionState::Ready { .. } => rsx! { DeviceList { linked, copy } },
    }
}

/// Follows an exchange for as long as it is on screen and shows where it
/// stands. When it is over, `on_end` hears how.
#[component]
fn Pairing(
    exchange: Signal<Option<Rc<Exchange>>>,
    /// The other device's name, when it is known: on the device that lets
    /// another in.
    other: Option<String>,
    on_end: EventHandler<ExchangeView>,
    #[props(default)] on_cancel: Option<EventHandler<()>>,
) -> Element {
    let mut view = use_signal(|| None::<ExchangeView>);
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
                if latest.is_over() {
                    on_end.call(latest);
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
    let name = other
        .clone()
        .unwrap_or_else(|| "your other device".to_owned());
    match view() {
        Some(ExchangeView::Step(ExchangeStep::Compare { code })) => rsx! {
            CodeComparison {
                code,
                question: other.as_ref().map_or_else(
                    || "Does your other device show these?".to_owned(),
                    |other| format!("Does {other} show these?"),
                ),
                on_match: move |()| {
                    // The pictures gather first (the script plays it).
                    spawn(async move {
                        tokio::time::sleep(Duration::from_millis(GATHER_MS)).await;
                        answer(Answer::Match);
                    });
                },
                on_differ: move |()| answer(Answer::Mismatch),
            }
        },
        Some(ExchangeView::Step(ExchangeStep::Finishing)) => rsx! {
            div { class: "dui-pair", role: "status",
                Spinner {}
                div { class: "dui-pair-t", "Waiting for {name} to say they match too" }
            }
        },
        _ if other.is_none() => rsx! {
            div { class: "dui-pair", role: "status",
                Spinner {}
                div { class: "dui-pair-t", "Open Dirt on a device that's already in" }
                div { class: "dui-pair-s", "It shows the same pictures there. Keep this open." }
            }
        },
        _ => rsx! {
            div { class: "dui-pair", role: "status",
                Spinner {}
                div { class: "dui-pair-t", "Waiting for {name}" }
                div { class: "dui-pair-s", "Keep Dirt open on it." }
                if let Some(on_cancel) = on_cancel {
                    Btn { kind: "ghost", onclick: move |_| on_cancel.call(()), "Cancel" }
                }
            }
        },
    }
}

/// A device that has signed in and waits to be let in.
#[component]
fn JoinHere(linked: Linked) -> Element {
    let nav = settings_nav();
    let facts = facts();
    let toasts = use_toasts();
    let mut exchange = use_signal(|| None::<Rc<Exchange>>);
    // How the last exchange ended, while that is on screen: "Try again".
    let mut ended = use_signal(|| None::<(String, String)>);
    let mut starting = use_signal(|| false);
    let mut with_phrase = use_signal(|| false);
    let mut phrase = use_signal(String::new);
    let mut busy = use_signal(|| false);

    // The exchange runs while Settings is open, and begins by itself.
    use_effect(move || {
        let open = nav.is_open();
        let typing = with_phrase();
        if !open || typing {
            exchange.set(None);
            if !open {
                ended.set(None);
                phrase.set(String::new());
            }
            return;
        }
        if exchange.peek().is_some() || ended.read().is_some() || *starting.peek() {
            return;
        }
        let Some(session) = linked.session() else {
            return;
        };
        starting.set(true);
        spawn(async move {
            match session.begin_join().await {
                Ok(join) => {
                    if nav.is_open() && !*with_phrase.peek() {
                        exchange.set(Some(Rc::new(Exchange::join(join, Pace::default()))));
                    }
                }
                Err(error) => ended.set(Some(("Could not ask".to_owned(), as_sentence(&error)))),
            }
            starting.set(false);
        });
    });

    let on_end = move |view: ExchangeView| {
        exchange.set(None);
        match view {
            ExchangeView::Step(ExchangeStep::Done) => {
                toasts.show(
                    Toast::new("This device is in. It can read your notes now").icon(Icon::Check),
                );
            }
            ExchangeView::Step(ExchangeStep::Cancelled(reason)) => {
                // The new device has been removed from the account, and
                // is signed out: this section is about to go.
                let why = match reason {
                    CancelReason::Mismatch => DIFFERED.to_owned(),
                    CancelReason::Declined => {
                        "The other device called it off. Sign in again to try again.".to_owned()
                    }
                };
                toasts.show_error(Toast::new("Nothing was shared").detail(why));
            }
            ExchangeView::Failed(reason) => {
                ended.set(Some(("Nothing was shared".to_owned(), reason)));
            }
            ExchangeView::TimedOut => ended.set(Some((
                "No device answered".to_owned(),
                "Nothing was changed.".to_owned(),
            ))),
            ExchangeView::Step(_) => {}
        }
    };

    let recover = move |_: MouseEvent| {
        let Some(session) = linked.session() else {
            return;
        };
        busy.set(true);
        spawn(async move {
            let typed = phrase.peek().clone();
            match session.recover_with_phrase(&typed).await {
                Ok(_) => {
                    phrase.set(String::new());
                    with_phrase.set(false);
                    toasts.show(
                        Toast::new("This device is in. It can read your notes now")
                            .icon(Icon::Check),
                    );
                }
                Err(error) => toasts.show_error(
                    Toast::new("The phrase did not open the account").detail(as_sentence(&error)),
                ),
            }
            busy.set(false);
        });
    };

    let me = facts.me.read().clone();
    let has_phrase = me.as_ref().is_some_and(|me| me.recovery_available);
    let no_way_in = me
        .as_ref()
        .is_some_and(|me| me.approvers == 0 && !me.recovery_available);

    rsx! {
        Card {
            if with_phrase() {
                div { class: "dui-pair",
                    div { class: "dui-pair-t", "Your recovery phrase" }
                    div { class: "dui-pair-s", "The 24 words, in order, with spaces between." }
                    textarea {
                        class: "dui-phrase-in",
                        rows: "4",
                        "aria-label": "Recovery phrase",
                        autocomplete: "off",
                        spellcheck: "false",
                        value: "{phrase}",
                        oninput: move |event| phrase.set(event.value()),
                    }
                    div { class: "dui-pair-a",
                        Btn { kind: "ghost", onclick: move |_| with_phrase.set(false), "Back" }
                        Btn {
                            kind: "primary",
                            spinning: busy(),
                            disabled: busy() || phrase().split_whitespace().count() < 24,
                            onclick: recover,
                            "Use the phrase"
                        }
                    }
                }
            } else if let Some((title, detail)) = ended() {
                Outcome { good: false, title, detail,
                    Btn { onclick: move |_| ended.set(None), "Try again" }
                }
            } else if no_way_in {
                Note { text: "No device of this account can let this one in, and it has no recovery phrase. To use it here, start the account again under Account actions." }
            } else {
                Pairing { exchange, other: None, on_end }
            }
        }
        if has_phrase && !with_phrase() {
            button {
                r#type: "button",
                class: "dui-set-link",
                onclick: move |_| with_phrase.set(true),
                "No other device? Use your recovery phrase."
            }
        }
    }
}

/// How letting a device in ended, while it is on screen.
#[derive(Debug, Clone, PartialEq)]
struct Ending {
    good: bool,
    title: String,
    detail: String,
}

impl Ending {
    fn of(view: &ExchangeView, name: &str) -> Option<Self> {
        let (good, title, detail) = match view {
            ExchangeView::Step(ExchangeStep::Done) => (
                true,
                format!("{name} is in"),
                "It can read your notes now.".to_owned(),
            ),
            ExchangeView::Step(ExchangeStep::HandledElsewhere) => (
                true,
                "Another device is letting it in".to_owned(),
                "Nothing needs doing here.".to_owned(),
            ),
            ExchangeView::Step(ExchangeStep::Cancelled(CancelReason::Mismatch)) => {
                (false, "Nothing was shared".to_owned(), DIFFERED.to_owned())
            }
            ExchangeView::Step(ExchangeStep::Cancelled(CancelReason::Declined)) => (
                false,
                "Nothing was shared".to_owned(),
                "It was called off. Start again from the new device.".to_owned(),
            ),
            ExchangeView::Failed(reason) => {
                (false, "Nothing was shared".to_owned(), reason.clone())
            }
            ExchangeView::TimedOut => (
                false,
                "No answer".to_owned(),
                "Nothing was changed. Start again from the new device.".to_owned(),
            ),
            ExchangeView::Step(_) => return None,
        };
        Some(Self {
            good,
            title,
            detail,
        })
    }
}

/// A device that is in: the account's devices, letting another in, and
/// the recovery phrase.
#[component]
fn DeviceList(linked: Linked, copy: EventHandler<String>) -> Element {
    let nav = settings_nav();
    let facts = facts();
    let toasts = use_toasts();
    let mut exchange = use_signal(|| None::<Rc<Exchange>>);
    let mut other = use_signal(|| None::<String>);
    let mut ending = use_signal(|| None::<Ending>);
    // "Add a device": looking for one that has signed in.
    let mut looking = use_signal(|| None::<Instant>);
    let mut removing = use_signal(|| None::<uuid::Uuid>);
    let mut words = use_signal(|| None::<Vec<String>>);
    let mut asking_phrase = use_signal(|| false);
    let mut busy = use_signal(|| false);

    let mut approve = move |device: DeviceInfo| {
        let Some(session) = linked.session() else {
            return;
        };
        looking.set(None);
        ending.set(None);
        let name = device.name.clone();
        match session.begin_approval(device) {
            Ok(approval) => {
                other.set(Some(name));
                exchange.set(Some(Rc::new(Exchange::approve(approval, Pace::default()))));
            }
            Err(error) => {
                toasts.show_error(Toast::new("Could not compare").detail(as_sentence(&error)));
            }
        }
    };

    // Settings closing stops everything here; what was on screen goes.
    use_effect(move || {
        if !nav.is_open() {
            exchange.set(None);
            looking.set(None);
            ending.set(None);
            words.set(None);
            removing.set(None);
            asking_phrase.set(false);
        }
    });
    // The banner's Review: compare with that device. Taking it writes the
    // signal read here, so it is taken only when there is one: taking
    // nothing would write it again, and run this again, for ever.
    use_effect(move || {
        if nav.review.read().is_some() {
            if let Some(device) = nav.take_review() {
                approve(device);
            }
        }
    });
    // Looking for a new device, every two seconds while asked to.
    let _look = use_resource(move || {
        let since = looking();
        let session = linked.session();
        async move {
            let (Some(since), Some(session)) = (since, session) else {
                return;
            };
            while since.elapsed() < LOOK_FOR {
                if let Ok(waiting) = session.waiting_devices().await {
                    if let Some(device) = waiting.into_iter().next() {
                        if *looking.peek() == Some(since) {
                            approve(device);
                        }
                        return;
                    }
                }
                tokio::time::sleep(LOOK_EVERY).await;
                if *looking.peek() != Some(since) {
                    return;
                }
            }
            looking.set(None);
            ending.set(Some(Ending {
                good: false,
                title: "No device signed in".to_owned(),
                detail: "Nothing was changed.".to_owned(),
            }));
        }
    });

    let on_end = move |view: ExchangeView| {
        exchange.set(None);
        let name = other.peek().clone().unwrap_or_default();
        let shown = Ending::of(&view, &name);
        let good = shown.as_ref().is_some_and(|shown| shown.good);
        ending.set(shown.clone());
        let mut reload = facts.reload;
        reload += 1;
        if good {
            spawn(async move {
                tokio::time::sleep(TICK_STAYS).await;
                if *ending.peek() == shown {
                    ending.set(None);
                }
            });
        }
    };

    let mut remove = move |device: DeviceInfo| {
        let Some(session) = linked.session() else {
            return;
        };
        busy.set(true);
        spawn(async move {
            match session.remove_device(device.id).await {
                Ok(()) => toasts.show(Toast::new(format!("{} removed", device.name))),
                Err(error) => toasts.show_error(
                    Toast::new(format!("{} was not removed", device.name))
                        .detail(as_sentence(&error)),
                ),
            }
            removing.set(None);
            let mut reload = facts.reload;
            reload += 1;
            busy.set(false);
        });
    };

    let make_phrase = move |_: MouseEvent| {
        let Some(session) = linked.session() else {
            return;
        };
        busy.set(true);
        spawn(async move {
            match session.create_recovery_phrase().await {
                Ok(phrase) => words.set(Some(
                    phrase
                        .words()
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                )),
                Err(error) => {
                    toasts.show_error(Toast::new("No phrase was made").detail(as_sentence(&error)));
                }
            }
            let mut reload = facts.reload;
            reload += 1;
            busy.set(false);
        });
    };

    let remove_phrase = move |()| {
        let Some(session) = linked.session() else {
            return;
        };
        asking_phrase.set(false);
        spawn(async move {
            match session.remove_recovery_phrase().await {
                Ok(()) => toasts.show(Toast::new("Recovery phrase removed")),
                Err(error) => toasts.show_error(
                    Toast::new("The phrase was not removed").detail(as_sentence(&error)),
                ),
            }
            let mut reload = facts.reload;
            reload += 1;
        });
    };

    let clock = Clock::now();
    let all = facts.devices.read().clone();
    let listed = all
        .into_iter()
        .filter(|device| device.status != DeviceStatus::Revoked);
    let has_phrase = facts
        .me
        .read()
        .as_ref()
        .is_some_and(|me| me.recovery_available);
    let comparing = exchange.read().is_some();

    rsx! {
        Card {
            for device in listed {
                ConfirmInPlace {
                    key: "{device.id}",
                    asking: removing() == Some(device.id),
                    question: format!("Remove {}? It stops getting notes.", device.name),
                    confirm: "Remove",
                    on_confirm: {
                        let device = device.clone();
                        move |()| remove(device.clone())
                    },
                    on_cancel: move |()| removing.set(None),
                    div { class: "dui-set-row",
                        RowIcon { icon: device_icon(&device.name) }
                        div { class: "dui-set-words",
                            div { class: "dui-set-t", "{device.name}" }
                            div { class: "dui-set-s",
                                if device.current {
                                    "This device"
                                } else if device.status == DeviceStatus::Pending {
                                    "Waiting to be let in"
                                } else {
                                    {seen(&device, clock)}
                                }
                            }
                        }
                        if device.status == DeviceStatus::Pending && !comparing {
                            Btn {
                                onclick: move |_| approve(device.clone()),
                                "Let it in"
                            }
                        } else if !device.current && device.status != DeviceStatus::Pending {
                            IconButton {
                                icon: Icon::X,
                                label: "Remove",
                                size: IconButtonSize::Small,
                                danger: true,
                                disabled: busy(),
                                onclick: {
                                    let id = device.id;
                                    move |_| removing.set(Some(id))
                                },
                            }
                        }
                    }
                }
            }
            if comparing {
                Pairing {
                    exchange,
                    other: other(),
                    on_end,
                    on_cancel: move |()| exchange.set(None),
                }
            } else if let Some(shown) = ending() {
                Outcome { good: shown.good, title: shown.title, detail: shown.detail,
                    if !shown.good {
                        Btn { kind: "ghost", onclick: move |_| ending.set(None), "OK" }
                    }
                }
            } else if looking().is_some() {
                div { class: "dui-pair", role: "status",
                    Spinner {}
                    div { class: "dui-pair-t", "Sign in to Dirt on the new device" }
                    div { class: "dui-pair-s", "It shows up here by itself. Keep this open." }
                    Btn { kind: "ghost", onclick: move |_| looking.set(None), "Cancel" }
                }
            } else {
                button {
                    r#type: "button",
                    class: "dui-set-row dui-set-press dui-set-add",
                    onclick: move |_| looking.set(Some(Instant::now())),
                    span { class: "dui-set-ic", "aria-hidden": "true", IconSvg { icon: Icon::Plus, size: IconSize::Small } }
                    "Add a device"
                }
            }
            if let Some(shown) = words() {
                RecoveryWords {
                    words: shown,
                    on_copy: move |text: String| {
                        copy.call(text);
                        toasts.show(Toast::new("Copied. Clear it from the clipboard after").icon(Icon::Copy));
                    },
                    on_done: move |()| words.set(None),
                }
            } else {
                ConfirmInPlace {
                    asking: asking_phrase(),
                    question: "Remove the recovery phrase? If every device is lost, so are the notes.",
                    confirm: "Remove",
                    on_confirm: remove_phrase,
                    on_cancel: move |()| asking_phrase.set(false),
                    Row {
                        icon: Icon::Key,
                        title: "Recovery phrase",
                        detail: if has_phrase { "You have one" } else { "Not made yet" },
                        if has_phrase {
                            Btn { kind: "ghost", disabled: busy(), onclick: move |_| asking_phrase.set(true), "Remove" }
                        } else {
                            Btn { disabled: busy(), spinning: busy(), onclick: make_phrase, "Make one" }
                        }
                    }
                }
            }
        }
    }
}

/// The recovery phrase, once, as it is made: the 24 words, numbered, in
/// four columns, with **Copy** and "I've written it down".
#[component]
pub fn RecoveryWords(
    words: Vec<String>,
    on_copy: EventHandler<String>,
    on_done: EventHandler<()>,
) -> Element {
    let all = words.join(" ");
    rsx! {
        div { class: "dui-pair",
            div { class: "dui-pair-t", "Your recovery phrase" }
            div { class: "dui-pair-s", "Write it down. It is the only way in if every device is lost." }
            ol { class: "dui-words",
                for (index , word) in words.iter().enumerate() {
                    li { key: "{index}", style: "--i:{index}",
                        b { "{index + 1}" }
                        "{word}"
                    }
                }
            }
            div { class: "dui-pair-a",
                Btn { kind: "ghost", icon: Icon::Copy, onclick: move |_| on_copy.call(all.clone()), "Copy" }
                Btn { kind: "primary", onclick: move |_| on_done.call(()), "I've written it down" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_is_told_from_a_computer_by_its_name() {
        assert_eq!(device_icon("Android device"), Icon::Phone);
        assert_eq!(device_icon("DESKTOP-4F2"), Icon::Laptop);
    }
}
