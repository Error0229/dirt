//! Quick capture (interface design, sections 5.5 and 7.2): a small window
//! of its own, over whatever app is in front, for a thought written from
//! anywhere. <kbd>Ctrl Alt N</kbd>, or **New Note** in the tray's menu,
//! opens it; <kbd>Esc</kbd>, <kbd>Ctrl Enter</kbd> or a click outside puts
//! it away, saved, and the keyboard goes back to the app that was in front.
//!
//! The window is made hidden when the app starts, and kept: opening it is
//! showing it, which is at once, where making a window and its web view
//! takes most of a second.
//!
//! What is typed is kept as the main screen's page keeps it
//! (`screen::writing`): the first character that is not a space makes the
//! note, the rest is saved a moment after typing pauses, and putting it
//! away hands what it holds to the app's one queue of the store's work. So
//! nothing typed is lost, and a write that fails is told where every
//! failed write is told, on the main screen, with **Copy text**.
//!
//! Put away, the box shrinks to its grain and the window carries the grain
//! to the tray icon, or lets it fall and fade where the tray's place cannot
//! be known ([`flight`]).
//!
//! The rest of the app reaches the window through [`link`]: the hotkey and
//! the tray ask it to open, quitting asks it to hand over what it holds,
//! and the main window gives it the store in use and the appearance.

mod flight;
mod foreground;

use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{
    use_wry_event_handler, window, Config, LogicalPosition, LogicalSize, WindowBuilder,
    WindowCloseBehaviour,
};
use dioxus::prelude::*;
use dirt_core::models::Grain;
use dirt_core::session::Session;
use dirt_core::Store;
use dirt_ui::components::Logo;
use dirt_ui::dom::{self, Caret};
use dirt_ui::events::{use_outcome, Outcome};
use dirt_ui::{Appearance, UiRoot};
use tokio::sync::watch;

use crate::screen::keys;
use crate::screen::writing::{Draft, Scribe, Work};
use flight::Flight;
use foreground::Foreground;

/// The text field's ID.
const TEXT_ID: &str = "dk-capture-text";
const TEXT: &str = "#dk-capture-text";

/// How wide the box is, and the room the window leaves around it for its
/// shadow, in CSS pixels. The style sheet says the same.
const BOX_WIDTH: f64 = 560.0;
const AROUND_SIDES: f64 = 48.0;
const AROUND_TOP: f64 = 24.0;
const AROUND_BOTTOM: f64 = 72.0;
/// The box with three lines in it, before it has said its own height.
const FIRST_BOX_HEIGHT: f64 = 96.0;

/// How long typing pauses before what was typed is saved: the main
/// screen's page waits the same.
const SAVE_AFTER: Duration = Duration::from_millis(400);

/// How long the window waits for the page to say that it has finished
/// leaving, before it hides all the same: a page that is not drawn runs
/// no animation.
const GONE_AT_LATEST: Duration = Duration::from_millis(2500);

/// The window's size for a box this tall, in CSS pixels.
fn window_size(box_height: f64) -> LogicalSize<f64> {
    LogicalSize::new(
        2.0_f64.mul_add(AROUND_SIDES, BOX_WIDTH),
        box_height + AROUND_TOP + AROUND_BOTTOM,
    )
}

// ---- What the rest of the app tells it ---------------------------------

/// What quick capture was last asked to do.
#[derive(Debug, Clone, Copy, Default)]
enum Asked {
    #[default]
    Nothing,
    /// Open, for a key or a menu chosen at that moment.
    Open(Instant),
    /// The app is quitting: hand over what is written, now.
    Leave,
}

/// Where a thought goes: the store in use and the session that syncs it.
#[derive(Clone, Default)]
pub struct Context {
    store: Option<Arc<Store>>,
    session: Option<Session>,
}

/// How the rest of the app reaches quick capture's window, which has a
/// virtual DOM of its own.
pub struct Link {
    asks: watch::Sender<(u64, Asked)>,
    appearance: watch::Sender<Appearance>,
    context: Mutex<Context>,
}

/// The app's one link to quick capture.
pub fn link() -> &'static Link {
    static LINK: OnceLock<Link> = OnceLock::new();
    LINK.get_or_init(|| Link {
        asks: watch::Sender::new((0, Asked::Nothing)),
        appearance: watch::Sender::new(Appearance::default()),
        context: Mutex::new(Context::default()),
    })
}

impl Link {
    fn ask(&self, asked: Asked) {
        self.asks.send_modify(|(serial, what)| {
            *serial += 1;
            *what = asked;
        });
    }

    /// Opens quick capture: the hotkey was pressed, or New Note chosen, at
    /// `at`. From any thread.
    pub fn open(&self, at: Instant) {
        self.ask(Asked::Open(at));
    }

    /// Hands what quick capture holds to the store's queue, as the app
    /// quits.
    pub fn leave(&self) {
        self.ask(Asked::Leave);
    }

    /// The store in use and the session, as the main window has them.
    pub fn set_context(&self, store: Option<Arc<Store>>, session: Option<Session>) {
        *self.context.lock().unwrap_or_else(PoisonError::into_inner) = Context { store, session };
    }

    fn context(&self) -> Context {
        self.context
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The theme, accent and text size, as the main window has them.
    pub fn set_appearance(&self, appearance: Appearance) {
        self.appearance.send_if_modified(|now| {
            let changed = *now != appearance;
            *now = appearance;
            changed
        });
    }
}

// ---- The window ---------------------------------------------------------

/// Makes quick capture's window, hidden, beside the main one. Call once,
/// from the main window.
pub fn make_window() {
    let dom = VirtualDom::new(QuickCapture);
    let builder = WindowBuilder::new()
        .with_title("Dirt quick capture")
        .with_inner_size(window_size(FIRST_BOX_HEIGHT))
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top(true)
        .with_visible(false)
        .with_focused(false);
    #[cfg(windows)]
    let builder = {
        use dioxus::desktop::tao::platform::windows::WindowBuilderExtWindows;
        // Its shadow is the page's own, around the box, not the window's.
        builder
            .with_skip_taskbar(true)
            .with_undecorated_shadow(false)
    };
    let config = crate::without_the_web_views_own_keys(
        Config::new()
            .with_window(builder)
            .with_menu(None)
            .with_background_color((0, 0, 0, 0))
            .with_close_behaviour(WindowCloseBehaviour::WindowHides),
    );
    // The window is made when the main window's event loop next turns.
    let _ = window().new_window(dom, config);
}

/// How the box is drawn: open, or leaving by one of three ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shown {
    #[default]
    Hidden,
    Open,
    Leaving(flight::How),
}

/// What the window shows. A new serial is a new box, made afresh.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct View {
    serial: u64,
    shown: Shown,
    /// The shade of the grain the box shrinks to.
    grain: Option<String>,
}

/// Why it is put away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// Esc or Ctrl Enter: the keyboard goes back to the app that was in
    /// front.
    Key,
    /// A click outside: the keyboard is already where the click put it.
    ClickedAway,
    /// The window was closed (Alt F4), or the app quits: it is not drawn
    /// leaving.
    Closed,
}

/// What the window's parts share.
#[derive(Clone, Copy)]
struct Capture {
    view: Signal<View>,
    draft: Signal<Draft>,
    /// Where the note being written goes: the store in use when it was
    /// made, whatever is in use later.
    target: CopyValue<Option<Context>>,
    /// When the key or the menu that opened it was chosen, until the page
    /// can be typed in.
    asked_at: CopyValue<Option<Instant>>,
    /// The page has had the keyboard since it opened, so a click outside
    /// can be told from the moment of opening.
    ready: CopyValue<bool>,
    /// Where the window goes as the page leaves.
    flight: CopyValue<Option<Flight>>,
    /// The window that was in front before it opened.
    before: CopyValue<Foreground>,
}

impl Capture {
    /// Opens it, or gives the keyboard to it again if it is open.
    fn open(mut self, at: Instant) {
        let desktop = window();
        if self.view.peek().shown == Shown::Open {
            if crate::app::takes_keyboard() {
                desktop.set_focus();
            }
            dom::focus(&[TEXT], Caret::End);
            return;
        }
        self.before.set(Foreground::now());
        self.asked_at.set(Some(at));
        self.ready.set(false);
        self.flight.set(None);
        let size = window_size(FIRST_BOX_HEIGHT);
        flight::place(&desktop, size.width, size.height);
        desktop.set_visible(true);
        if crate::app::takes_keyboard() {
            desktop.set_focus();
        }
        let _ = desktop.webview.focus();
        let serial = self.view.peek().serial + 1;
        self.view.set(View {
            serial,
            shown: Shown::Open,
            grain: None,
        });
    }

    /// The text field changed.
    fn typed(mut self, serial: u64, text: String) {
        let view = self.view.peek().clone();
        if view.serial != serial || view.shown != Shown::Open {
            return;
        }
        let first = self.draft.write().typed(text);
        if let Some(work) = first {
            self.target.set(Some(link().context()));
            self.ask([work]);
            return;
        }
        let version = self.draft.peek().version();
        spawn(async move {
            tokio::time::sleep(SAVE_AFTER).await;
            if self.draft.peek().version() == version {
                let work = self.draft.write().flush();
                self.ask(work);
            }
        });
    }

    /// Has the store do `work`, in the store the note is in.
    fn ask(self, work: impl IntoIterator<Item = Work>) {
        let work: Vec<Work> = work.into_iter().collect();
        if work.is_empty() {
            return;
        }
        let Some(Context {
            store: Some(store),
            session,
        }) = self.target.peek().clone()
        else {
            tracing::error!("No store is open to keep the thought in");
            return;
        };
        Scribe::global().ask(&store, session.as_ref(), work);
    }

    /// Puts it away: what it holds goes to the store's queue, and the box
    /// leaves.
    fn put_away(mut self, why: Why) {
        if self.view.peek().shown != Shown::Open {
            return;
        }
        let settles = self.draft.peek().settles();
        let left = self.draft.write().leave();
        self.ask(left.work);
        self.target.set(None);
        let desktop = window();
        // The keyboard goes back at once, so that what is typed while the
        // grain flies reaches the app it is meant for. Where it cannot be
        // given back, the window goes at once and the system gives it.
        let handed_back = match why {
            Why::Key => !crate::app::takes_keyboard() || self.before.peek().give_focus_back(),
            Why::ClickedAway => true,
            Why::Closed => false,
        };
        if !handed_back {
            self.hide();
            return;
        }
        let how = if settles.is_some() {
            let flight = Flight::from(&desktop);
            let how = flight.how;
            self.flight.set(Some(flight));
            how
        } else {
            flight::How::Fade
        };
        let serial = self.view.peek().serial;
        self.view.set(View {
            serial,
            shown: Shown::Leaving(how),
            grain: settles.map(|note| dirt_ui::pile::shade(&Grain::of(&note))),
        });
        spawn(async move {
            tokio::time::sleep(GONE_AT_LATEST).await;
            if self.view.peek().serial == serial {
                self.gone();
            }
        });
    }

    /// The page has begun to shrink to its grain: the window carries it.
    fn away(self) {
        let Some(flight) = *self.flight.peek() else {
            return;
        };
        let serial = self.view.peek().serial;
        let view = self.view;
        spawn(async move {
            flight
                .carry(&window(), move || {
                    view.try_peek().is_ok_and(|now| now.serial == serial)
                })
                .await;
        });
    }

    /// The page has finished leaving: the window hides until next time.
    /// A box that has been replaced by a new one, opened while it left,
    /// may still say so: that is not this box's news.
    fn gone(self) {
        if matches!(self.view.peek().shown, Shown::Leaving(_)) {
            self.hide();
        }
    }

    /// The window hides until next time.
    fn hide(mut self) {
        window().set_visible(false);
        self.flight.set(None);
        let serial = self.view.peek().serial;
        self.view.set(View {
            serial,
            shown: Shown::Hidden,
            grain: None,
        });
    }

    /// The box is `height` CSS pixels tall: the window fits it, growing
    /// downward.
    fn fit(self, height: f64) {
        if self.view.peek().shown == Shown::Open {
            window().set_inner_size(window_size(height));
        }
    }

    /// The text field has the keyboard.
    fn focused(mut self) {
        self.ready.set(true);
        if let Some(at) = self.asked_at.take() {
            tracing::info!(
                "Quick capture ready to type into {} ms after it was asked for",
                at.elapsed().as_millis()
            );
        }
    }
}

/// Quick capture's window: the root of its own virtual DOM.
#[component]
fn QuickCapture() -> Element {
    let capture = Capture {
        view: use_signal(View::default),
        draft: use_signal(Draft::default),
        target: use_hook(|| CopyValue::new(None)),
        asked_at: use_hook(|| CopyValue::new(None)),
        ready: use_hook(|| CopyValue::new(false)),
        flight: use_hook(|| CopyValue::new(None)),
        before: use_hook(|| CopyValue::new(Foreground::default())),
    };
    let mut appearance = use_signal(|| *link().appearance.borrow());

    // The web view shows a new window once its page has loaded, whatever
    // it was made with. It is kept off every screen until then, and hidden
    // as soon as it has been shown, unless it was asked for meanwhile.
    use_hook(|| {
        window().set_outer_position(LogicalPosition::new(-32000.0, -32000.0));
        spawn(async move {
            tokio::task::yield_now().await;
            if capture.view.peek().shown == Shown::Hidden {
                window().set_visible(false);
            }
        });
    });

    use_future(move || async move {
        let mut asks = link().asks.subscribe();
        loop {
            let (serial, asked) = *asks.borrow_and_update();
            if serial > 0 {
                match asked {
                    Asked::Open(at) => capture.open(at),
                    Asked::Leave => capture.put_away(Why::Closed),
                    Asked::Nothing => {}
                }
            }
            if asks.changed().await.is_err() {
                break;
            }
        }
    });
    use_future(move || async move {
        let mut looks = link().appearance.subscribe();
        while looks.changed().await.is_ok() {
            let now = *looks.borrow_and_update();
            appearance.set(now);
        }
    });

    use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent { event, .. } = event {
            match event {
                WindowEvent::Focused(false) if *capture.ready.peek() => {
                    capture.put_away(Why::ClickedAway);
                }
                WindowEvent::CloseRequested => capture.put_away(Why::Closed),
                _ => {}
            }
        }
    });

    use_context_provider(|| capture);

    rsx! {
        UiRoot { appearance: appearance(),
            style { {include_str!("capture.css")} }
            div { class: "dk-capture", CaptureBox {} }
        }
    }
}

/// The box, inside the root, where the script's news is heard.
#[component]
fn CaptureBox() -> Element {
    let capture = use_context::<Capture>();
    let outcome = use_outcome(move |outcome| match outcome {
        Outcome::Away => capture.away(),
        Outcome::Gone => capture.gone(),
        _ => {}
    });

    let view = capture.view.read().clone();
    let serial = view.serial;
    let how = match view.shown {
        Shown::Hidden => return rsx! {},
        Shown::Open => "open",
        Shown::Leaving(how) => how.name(),
    };

    rsx! {
        div {
            key: "{serial}",
            class: "dk-capture-box",
            "data-dui-capture": how,
            "data-dui-grain": view.grain.clone(),
            "data-dui-id": outcome,
            onresize: move |event: ResizeEvent| {
                if let Ok(size) = event.get_border_box_size() {
                    capture.fit(size.height);
                }
            },
            Logo { size: 22, class: "dk-capture-logo" }
            textarea {
                id: TEXT_ID,
                class: "dk-capture-text",
                "aria-label": "Quick capture",
                placeholder: "What\u{2019}s on your mind?",
                spellcheck: "false",
                readonly: view.shown != Shown::Open,
                oninput: move |event: FormEvent| capture.typed(serial, event.value()),
                onkeydown: move |event: KeyboardEvent| {
                    if puts_away(&event) {
                        event.prevent_default();
                        capture.put_away(Why::Key);
                    }
                },
                onfocus: move |_| capture.focused(),
                onmounted: move |_| dom::focus(&[TEXT], Caret::End),
            }
        }
    }
}

/// Esc with no modifier, or the command key with Enter: put it away
/// (7.2). Enter alone is a new line. A key pressed while an input method
/// is composing belongs to the input method (7.4).
fn puts_away(event: &KeyboardEvent) -> bool {
    if event.is_composing() {
        return false;
    }
    let modifiers = event.modifiers();
    match event.key() {
        Key::Escape => modifiers.is_empty(),
        Key::Enter => {
            keys::command(event) && !modifiers.intersects(Modifiers::ALT | Modifiers::SHIFT)
        }
        _ => false,
    }
}
