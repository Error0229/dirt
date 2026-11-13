//! Mobile Settings: the account and sync.
//!
//! The view renders the session's four states, the same ones the desktop
//! app and the CLI show. Signing in opens a browser tab over the app; the
//! app shell picks the sign-in up again when Android brings the person
//! back.
//!
//! What is shown about sync, and in which words, comes from `dirt-core`'s
//! sync overview, which the desktop app and `dirt sync --verbose` show too.

use dioxus::prelude::*;
use dirt_core::api::ErrorClass;
use dirt_core::export::ExportFormat;
use dirt_core::session::{SessionState, SignOutError, Unsynced};
use dirt_core::store::{FileProblem, NoteProblem};
use dirt_core::sync::overview::SyncOverview;
use dirt_core::sync::worker::Trigger;
use dirt_core::util::{as_sentence, now_ms};

use crate::app_shell::REDIRECT_URI;
use crate::services::activity::{self, SaveOutcome};
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
    let mut overview = use_signal(|| None::<SyncOverview>);

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

    // The counts and the notes that could not be synced are in the store;
    // read them again whenever sync reports something new.
    use_effect(move || {
        let current = (state.session)();
        let _ = (state.session_state)();
        let _ = (state.sync_status)();
        spawn(async move {
            let read = match current {
                Some(session) => session.sync_overview().await.ok(),
                None => None,
            };
            overview.set(read);
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
                ExportPanel {}
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
                    // A tab laid over the app, or else the browser itself.
                    Ok(address) if activity::open_sign_in_page(&address) => Ok(()),
                    Ok(address) => webbrowser::open(&address).map_err(|error| {
                        session.cancel_sign_in();
                        format!("Could not open the browser: {error}")
                    }),
                    Err(error) => Err(as_sentence(&error)),
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
            let mut state = state;
            busy.set(true);
            spawn(async move {
                let outcome = session.move_local_notes().await;
                message.set(Some(match outcome {
                    Ok(moved) => {
                        local_waiting.set(0);
                        let open = state.store.peek().clone();
                        if let Some(open) = open {
                            let listed = *state.listed_notes.peek();
                            if let Ok(latest) = open.list_notes(listed).await {
                                state.notes.set(latest);
                            }
                        }
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
                    Err(SignOutError::Unsynced {
                        notes,
                        files,
                        cause,
                    }) => {
                        unsynced.set(Some(notes + files));
                        message.set(Some((
                            MessageKind::Error,
                            dirt_core::sync::overview::sign_out_would_lose(
                                notes,
                                files,
                                cause.as_deref(),
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

    let must_sign_in_again = status
        .failure
        .as_ref()
        .is_some_and(|failure| failure.class == ErrorClass::StopUntilSignedIn);
    let sync_session = session;
    // Until the store has been read, the worker's own figures stand in.
    let shown = overview().unwrap_or_else(|| SyncOverview::from(&status));
    let last_synced = shown.last_synced(now_ms());
    let waiting = shown.waiting();
    let sync_failure = shown
        .failure
        .as_ref()
        .map(dirt_core::sync::worker::SyncFailure::advice);
    let problem_count = shown.problem_count();
    let file_problem_count = shown.file_problem_count();
    let key_change = shown.key_change();

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
                            "This device is signed in but has not been let in yet. Until then it cannot read the account's notes, and notes written here stay on this device."
                        }
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: ButtonVariant::Primary,
                            onclick: move |_| view_signal.set(View::Devices),
                            "Let this device in"
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
                        if must_sign_in_again {
                            p { style: "{TEXT}", "Signed in, but this device can no longer sync." }
                        } else {
                            p { style: "{TEXT}", "Signed in. Your notes sync with your other devices." }
                        }
                        if status.pending_devices > 0 {
                            p { style: "{TEXT}", "A device is waiting to be let into this account." }
                        }
                        if status.reset_effective_at_ms.is_some() {
                            p { style: "{WARNING}", "Someone asked to start this account again." }
                        }
                        UiButton {
                            r#type: "button",
                            block: true,
                            variant: if status.pending_devices > 0 || status.reset_effective_at_ms.is_some() {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Secondary
                            },
                            onclick: move |_| view_signal.set(View::Devices),
                            "Devices and recovery phrase"
                        }
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
                    p { style: "{TEXT}", "{last_synced}" }
                    if let Some(text) = waiting {
                        p { style: "{TEXT}", "{text}" }
                    }
                    if let Some(text) = sync_failure {
                        p { style: "{WARNING}", "{text}" }
                    }
                    if let Some(text) = key_change {
                        p { style: "{TEXT}", "{text}" }
                    }
                    if let Some(text) = problem_count {
                        p { style: "{WARNING}", "{text}" }
                    }
                    for problem in shown.problems {
                        ProblemNote { key: "{problem.note_id}", problem }
                    }
                    if let Some(text) = file_problem_count {
                        p { style: "{WARNING}", "{text}" }
                    }
                    for problem in shown.file_problems {
                        ProblemFile { key: "{problem.file_id}-{problem.note_id}", problem }
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

            ExportPanel {}

            if let Some((kind, text)) = message() {
                MessageBanner { kind, text }
            }
        }
    }
}

/// Saves every note and its files as one zip file, in a place the person
/// chooses on Android's own screen for that.
#[component]
fn ExportPanel() -> Element {
    let state = use_context::<AppState>();
    let mut busy = use_signal(|| false);
    let mut message: Signal<Option<(MessageKind, String)>> = use_signal(|| None);

    let mut export = move |format: ExportFormat| {
        busy.set(true);
        message.set(None);
        spawn(async move {
            let session = state.session.peek().clone();
            let said = save_export((state.store)(), session, format).await;
            message.set(Some(said));
            busy.set(false);
        });
    };

    rsx! {
        div { style: "{PANEL}",
            p { style: "{HEADING}", "Export" }
            p {
                style: "{TEXT}",
                "Save every note and its files as one zip file. You choose where it goes."
            }
            div { style: "display: flex; gap: 8px;",
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Secondary,
                    disabled: busy(),
                    onclick: move |_| export(ExportFormat::Json),
                    "Export JSON"
                }
                UiButton {
                    r#type: "button",
                    block: true,
                    variant: ButtonVariant::Secondary,
                    disabled: busy(),
                    onclick: move |_| export(ExportFormat::Markdown),
                    "Export Markdown"
                }
            }
            if busy() {
                // Files this device does not hold are fetched first, which
                // takes a while for many.
                p { style: "{TEXT}", "Exporting..." }
            }
            if let Some((kind, text)) = message() {
                MessageBanner { kind, text }
            }
        }
    }
}

/// Makes the export, hands it to Android to save, and says how it went.
async fn save_export(
    store: Option<std::sync::Arc<crate::data::MobileNoteStore>>,
    session: Option<dirt_core::session::Session>,
    format: ExportFormat,
) -> (MessageKind, String) {
    let Some(store) = store else {
        return (MessageKind::Error, "The notes are not open yet.".to_owned());
    };
    let made = crate::export::make_archive(
        store.core(),
        session.as_ref(),
        format,
        &crate::export::export_folder(),
        now_ms(),
    )
    .await;
    let archive = match made {
        Ok(Some(archive)) => archive,
        Ok(None) => {
            return (
                MessageKind::Info,
                "There are no notes to export.".to_owned(),
            )
        }
        Err(error) => {
            return (
                MessageKind::Error,
                format!("The export could not be made: {error}."),
            )
        }
    };
    let said = hand_over(&archive).await;
    // Android has its own copy now, or the person backed out.
    archive.discard();
    said
}

/// Asks the person where the archive should go, waits for Android to copy
/// it there, and says how that went.
async fn hand_over(archive: &crate::export::Archive) -> (MessageKind, String) {
    // This save's own name: its outcome is not another's.
    let request = activity::new_request();
    if !activity::save_copy(
        &request,
        &archive.name,
        crate::export::MIME_TYPE,
        &archive.path.to_string_lossy(),
    ) {
        return (
            MessageKind::Error,
            "Android's screen for saving a file could not be opened.".to_owned(),
        );
    }
    // The person is choosing where to save. The activity says when they
    // have, or have backed out.
    let outcome = loop {
        if let Some(outcome) = activity::take_save_outcome(&request) {
            break outcome;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    };
    match outcome {
        SaveOutcome::Saved if archive.left_out.is_empty() => {
            (MessageKind::Info, format!("Saved {}.", archive.counts))
        }
        // Saved, and not whole: said as loudly as a failure, since the
        // person may keep this file as their copy of the notes.
        SaveOutcome::Saved => (
            MessageKind::Error,
            format!("Saved {}.\n{}", archive.counts, archive.left_out.join("\n")),
        ),
        SaveOutcome::Cancelled => (MessageKind::Info, "Nothing was saved.".to_owned()),
        SaveOutcome::Failed(why) => (
            MessageKind::Error,
            format!("The file could not be saved: {why}."),
        ),
    }
}

/// One file that could not be sent: which it is, why, and what becomes of
/// it meanwhile.
#[component]
fn ProblemFile(problem: FileProblem) -> Element {
    let title = problem.title();
    let reason = as_sentence(&problem.reason);
    let release = problem.release();

    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                gap: 4px;
                padding: 8px 10px;
                border-left: 3px solid #b91c1c;
            ",
            p { style: "margin: 0; font-size: 14px; color: #111827;", "{title}" }
            p { style: "{TEXT}", "{reason}" }
            p { style: "{TEXT}", "{release}" }
        }
    }
}

/// One note that could not be synced: which it is, why, and what releases
/// it. A note that is on this device opens when it is pressed.
#[component]
fn ProblemNote(problem: NoteProblem) -> Element {
    let mut state = use_context::<AppState>();
    let id = problem.note_id;
    let title = problem.title();
    let reason = as_sentence(&problem.reason);
    let release = problem.release();
    // Only a note in the list can be opened: not one this device never
    // held, and not a deleted one.
    let on_this_device = (state.notes)().iter().any(|note| note.id == id);
    let name_style = if on_this_device {
        "margin: 0; font-size: 14px; color: #111827; text-decoration: underline;"
    } else {
        "margin: 0; font-size: 14px; color: #111827;"
    };

    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                gap: 4px;
                padding: 8px 10px;
                border-left: 3px solid #b91c1c;
            ",
            onclick: move |_| {
                if on_this_device {
                    state.selected_note_id.set(Some(id));
                    state.view.set(View::Editor);
                }
            },
            p { style: "{name_style}", "{title}" }
            p { style: "{TEXT}", "{reason}" }
            p { style: "{TEXT}", "{release}" }
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
                white-space: pre-line;
            ",
            "{text}"
        }
    }
}
