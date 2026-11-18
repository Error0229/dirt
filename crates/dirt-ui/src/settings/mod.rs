//! Settings, the same on both apps (interface design, sections 6.9 to
//! 6.13 and 9): one scrolling page of sections, and the banner for a
//! device that waits to be let in.
//!
//! The page is drawn here; each app puts it in its own container, the
//! panel on the desktop (with the rail, [`SettingsRail`]) and the sheet on
//! Android. What only an app can do it is given as callbacks: open the
//! browser to sign in, choose where an export goes, put words on the
//! clipboard, open a note. Everything else is the session's, which this
//! module drives; it has no SQL, HTTP or cryptography of its own, and its
//! words about sync come from `dirt_core::sync::overview`.
//!
//! Voice, the sixth section of 6.9, comes with voice (the plan's step 9).
//! Until then the page has five: a section with nothing in it would be a
//! heading over nothing.

mod account;
mod code;
mod devices;
mod export;
mod join_banner;
mod look;
mod rows;
mod sync;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::export::ExportFormat;
use dirt_core::models::FocusStyle;
use dirt_core::session::{Session, SessionError, SessionState};
use dirt_core::sync::worker::SyncStatus;
use dirt_core::NoteId;

use crate::appearance::{Appearance, Platform};
use crate::components::ScrollArea;
use crate::icons::{Icon, IconSize, IconSvg};

pub use code::CodeComparison;
pub use devices::RecoveryWords;
pub use join_banner::JoinBanner;

/// Work an app does for Settings that takes a while: signing in in the
/// browser, writing an export.
pub type Task<T> = Pin<Box<dyn Future<Output = T>>>;

/// One section of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Account,
    Devices,
    Sync,
    Appearance,
    Export,
}

impl Section {
    /// The sections, in the order the page has them.
    pub const ALL: [Self; 5] = [
        Self::Account,
        Self::Devices,
        Self::Sync,
        Self::Appearance,
        Self::Export,
    ];

    /// The name the page and the script know it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Devices => "devices",
            Self::Sync => "sync",
            Self::Appearance => "appearance",
            Self::Export => "export",
        }
    }

    /// Its heading.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::Devices => "Devices",
            Self::Sync => "Sync",
            Self::Appearance => "Appearance",
            Self::Export => "Export",
        }
    }

    /// Its icon on the rail.
    #[must_use]
    pub const fn icon(self) -> Icon {
        match self {
            Self::Account => Icon::User,
            Self::Devices => Icon::Laptop,
            Self::Sync => Icon::Refresh,
            Self::Appearance => Icon::Sun,
            Self::Export => Icon::Download,
        }
    }
}

/// Whether Settings is open, and where it should be: what the screen, the
/// banner and the sync status ask of it. An app makes one with
/// [`use_settings_nav`] and every part of it reads it from the context.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SettingsNav {
    open: Signal<bool>,
    /// The section to bring into sight, numbered so that asking for the
    /// same one twice brings it twice.
    jump: Signal<(u64, Option<Section>)>,
    /// A device the banner's **Review** asked to compare.
    review: Signal<Option<dirt_core::api::DeviceInfo>>,
}

impl SettingsNav {
    /// Whether Settings is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        *self.open.read()
    }

    /// Opens Settings where it was.
    pub fn open(mut self) {
        self.open.set(true);
    }

    /// Opens Settings at `section`.
    pub fn open_at(mut self, section: Section) {
        let serial = self.jump.peek().0 + 1;
        self.jump.set((serial, Some(section)));
        self.open.set(true);
    }

    /// Closes Settings.
    pub fn close(mut self) {
        self.open.set(false);
    }

    /// Opens or closes it: <kbd>Ctrl ,</kbd>.
    pub fn toggle(self) {
        if *self.open.peek() {
            self.close();
        } else {
            self.open();
        }
    }

    /// Opens Settings at Devices, comparing the code with `device`.
    pub fn review(mut self, device: dirt_core::api::DeviceInfo) {
        self.review.set(Some(device));
        self.open_at(Section::Devices);
    }

    fn take_review(mut self) -> Option<dirt_core::api::DeviceInfo> {
        self.review.take()
    }
}

/// Makes the app's [`SettingsNav`] and gives it to everything under the
/// component that calls this.
pub fn use_settings_nav() -> SettingsNav {
    use_context_provider(|| SettingsNav {
        open: Signal::new(false),
        jump: Signal::new((0, None)),
        review: Signal::new(None),
    })
}

/// The app's [`SettingsNav`], made by [`use_settings_nav`] above.
#[must_use]
pub fn settings_nav() -> SettingsNav {
    use_context::<SettingsNav>()
}

/// What the server says about this device and its account, read while
/// Settings is open, for the sections that need it.
#[derive(Clone, Copy)]
struct Facts {
    /// This device, how many could let it in, whether there is a recovery
    /// phrase, and whether a reset waits.
    me: Signal<Option<dirt_core::session::MyDevice>>,
    /// The account's devices, on a device that is in.
    devices: Signal<Vec<dirt_core::api::DeviceInfo>>,
    /// Raised to read them again, after a change made here.
    reload: Signal<u64>,
}

fn facts() -> Facts {
    use_context::<Facts>()
}

/// Reads [`Facts`] when Settings opens, after each change made in it, and
/// when a sync says that a device waits.
fn use_facts(linked: Linked) -> Facts {
    let nav = settings_nav();
    let facts = use_context_provider(|| Facts {
        me: Signal::new(None),
        devices: Signal::new(Vec::new()),
        reload: Signal::new(0),
    });
    let _read = use_resource(move || {
        let open = nav.is_open();
        let _ = facts.reload.read();
        let state = *linked.state.read();
        let pending = linked.status.read().pending_devices;
        let session = linked.session.read().clone();
        let (mut me, mut devices) = (facts.me, facts.devices);
        async move {
            let _ = pending;
            let signed_in = matches!(
                state,
                SessionState::Ready { .. } | SessionState::WaitingForApproval { .. }
            );
            let Some(session) = session.filter(|_| open && signed_in) else {
                if !signed_in {
                    me.set(None);
                    devices.set(Vec::new());
                }
                return;
            };
            match session.my_device().await {
                Ok(read) => me.set(Some(read)),
                Err(error) => tracing::warn!("Could not ask about this device: {error}"),
            }
            if state.is_ready() {
                match session.devices().await {
                    Ok(read) => devices.set(read),
                    Err(error) => tracing::warn!("Could not list the devices: {error}"),
                }
            }
        }
    });
    facts
}

/// What the app knows about who is signed in and how sync goes, as it
/// follows the session.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Linked {
    /// The session, when the build has a server to sync with.
    pub session: Signal<Option<Session>>,
    /// Who is signed in.
    pub state: Signal<SessionState>,
    /// How sync goes.
    pub status: Signal<SyncStatus>,
}

impl Linked {
    fn session(&self) -> Option<Session> {
        self.session.peek().clone()
    }
}

/// An export that was written, as the app tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// For the toast: "Exported 12 notes and 1 file to dirt-export-…".
    pub said: String,
    /// The files that could not be had, with why, one line each. Shown in
    /// the toast's place, as a short list (design 9.6).
    pub left_out: Vec<String>,
}

/// What an app does when the person signs in: open the browser at the
/// sign-in page, and end when the session has an answer.
pub type SignIn = Callback<Session, Task<Result<SessionState, SessionError>>>;

/// What an app does when the person exports: ask where, and write it
/// there. Nothing when the person chose nowhere; the reason when it
/// failed.
pub type ExportTo = Callback<ExportFormat, Task<Option<Result<Exported, String>>>>;

/// The page of Settings, for the app's panel or sheet.
#[component]
pub fn SettingsPage(
    linked: Linked,
    appearance: Appearance,
    on_appearance: EventHandler<Appearance>,
    /// Focus while writing, on the desktop; nothing on Android, which has
    /// no such thing.
    #[props(default)]
    focus_style: Option<FocusStyle>,
    #[props(default)] on_focus_style: EventHandler<FocusStyle>,
    sign_in: SignIn,
    export: ExportTo,
    /// Puts the recovery phrase on the clipboard, kept out of the
    /// system's clipboard history where the system allows it.
    copy: EventHandler<String>,
    /// Opens a note, with Settings closed: one that cannot sync.
    open_note: EventHandler<NoteId>,
    #[props(default = Platform::current())] platform: Platform,
) -> Element {
    let nav = settings_nav();
    use_facts(linked);
    // A section asked for is brought into sight once the page has been
    // drawn for it.
    use_effect(move || {
        let (_, section) = *nav.jump.read();
        let open = *nav.open.read();
        if let (true, Some(section)) = (open, section) {
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                jump(section, true);
            });
        }
    });
    rsx! {
        ScrollArea { class: "dui-set-wrap", inner_class: "dui-set",
            for part in Section::ALL {
                section {
                    key: "{part.name()}",
                    class: "dui-set-sec",
                    "data-dui-section": part.name(),
                    "data-dui-rise": "",
                    "aria-labelledby": "dui-set-h-{part.name()}",
                    h3 { id: "dui-set-h-{part.name()}", "{part.label()}" }
                    match part {
                        Section::Account => rsx! { account::Account { linked, sign_in } },
                        Section::Devices => rsx! { devices::Devices { linked, copy } },
                        Section::Sync => rsx! { sync::Sync { linked, open_note, platform } },
                        Section::Appearance => rsx! {
                            look::Look { appearance, on_appearance, focus_style, on_focus_style, platform }
                        },
                        Section::Export => rsx! { export::ExportSection { export, platform } },
                    }
                }
            }
        }
    }
}

/// Brings `section` into sight in whichever Settings is open.
fn jump(section: Section, instant: bool) {
    document::eval(&format!(
        "window.DirtUi && DirtUi.jump('{}', {instant});",
        section.name()
    ));
}

/// The rail beside the page on the desktop: a button for each section,
/// which brings it into sight, and a highlight that follows the scroll.
/// The script moves the highlight; Rust only draws the buttons.
#[component]
pub fn SettingsRail() -> Element {
    rsx! {
        nav { class: "dui-rail", "aria-label": "Sections", "data-dui-rail": "",
            div { class: "dui-rail-t", "Settings" }
            div { class: "dui-rail-b",
                span { class: "dui-rail-ind", "aria-hidden": "true" }
                for part in Section::ALL {
                    button {
                        key: "{part.name()}",
                        r#type: "button",
                        "data-dui-jump": part.name(),
                        onclick: move |_| jump(part, false),
                        IconSvg { icon: part.icon(), size: IconSize::Small }
                        "{part.label()}"
                    }
                }
            }
        }
    }
}
