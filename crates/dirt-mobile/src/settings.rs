//! Settings on Android (interface design, sections 5.5, 6.9 and 8.2): the
//! shared page of `dirt_ui::settings` in a sheet from the bottom, which a
//! drag down, a tap outside it or Back closes, and the banner for a device
//! that waits to be let in.
//!
//! What only Android can do is here: signing in in a browser tab over the
//! app, saving an export as one zip file where the person chooses on
//! Android's own screen, and the clipboard. Voice is shown where this build
//! has it (6.18, 8.9).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::export::ExportFormat;
use dirt_core::session::{Session, SessionError, SessionState};
use dirt_core::util::now_ms;
use dirt_ui::components::Sheet;
use dirt_ui::settings::{
    settings_nav, ExportTo, Exported, JoinBanner, Linked, SavePhrase, SettingsPage, SignIn, Task,
    PHRASE_FILE_NAME,
};
use dirt_ui::Appearance;

use crate::app_shell::REDIRECT_URI;
use crate::data::MobileNoteStore;
use crate::export::{export_folder, make_archive, Archive, MIME_TYPE};
use crate::services::activity::{self, SaveOutcome};
use crate::state::AppState;

/// What Settings is given of the app's session.
const fn linked(app: &AppState) -> Linked {
    Linked {
        session: app.session,
        state: app.session_state,
        status: app.sync_status,
    }
}

/// Opens the sign-in page in a browser tab over the app, and ends when the
/// session has an answer. The browser brings the person back through
/// `REDIRECT_URI`, and the app's shell finishes the sign-in from there,
/// also after Android has stopped the app meanwhile; this waits for that.
async fn sign_in(session: Session) -> Result<SessionState, SessionError> {
    // The attempt is saved by the session before the browser opens.
    let address = session.begin_sign_in(REDIRECT_URI).await?;
    if !activity::open_sign_in_page(&address) {
        if let Err(error) = webbrowser::open(&address) {
            session.cancel_sign_in();
            return Err(SessionError::Browser(error.to_string()));
        }
    }
    let mut states = session.watch_state();
    loop {
        let now = *states.borrow_and_update();
        if now != SessionState::SigningIn {
            return Ok(now);
        }
        if states.changed().await.is_err() {
            return Ok(session.state());
        }
    }
}

/// Makes the export as one zip file, hands it to Android's screen for
/// choosing where it goes, and says how that went. Nothing when the person
/// backed out.
async fn export_to_zip(
    store: Option<Arc<MobileNoteStore>>,
    session: Option<Session>,
    format: ExportFormat,
) -> Option<Result<Exported, String>> {
    let Some(store) = store else {
        return Some(Err("The notes are not open yet.".to_owned()));
    };
    let made = make_archive(
        store.core(),
        session.as_ref(),
        format,
        &export_folder(),
        now_ms(),
    )
    .await;
    let archive = match made {
        Ok(Some(archive)) => archive,
        Ok(None) => {
            return Some(Ok(Exported {
                said: "There are no notes to export".to_owned(),
                left_out: Vec::new(),
            }))
        }
        Err(error) => return Some(Err(format!("The export could not be made: {error}."))),
    };
    let saved = hand_over(&archive).await;
    // Android has its own copy now, or the person backed out.
    archive.discard();
    match saved {
        SaveOutcome::Saved => Some(Ok(Exported {
            said: format!("Saved {} as {}", archive.counts, archive.name),
            left_out: archive.left_out.clone(),
        })),
        SaveOutcome::Cancelled => None,
        SaveOutcome::Failed(why) => Some(Err(format!("The file could not be saved: {why}."))),
    }
}

/// Asks the person where the archive goes and waits for Android to copy it
/// there.
async fn hand_over(archive: &Archive) -> SaveOutcome {
    save_through_android(&archive.name, MIME_TYPE, &archive.path).await
}

/// Asks the person where a copy of the file at `path`, inside the app,
/// goes, on Android's own screen for that, and waits for Android to write
/// it there.
async fn save_through_android(name: &str, mime_type: &str, path: &Path) -> SaveOutcome {
    // This save's own name: its outcome is not another's.
    let request = activity::new_request();
    if !activity::save_copy(&request, name, mime_type, &path.to_string_lossy()) {
        return SaveOutcome::Failed(
            "Android's screen for saving a file could not be opened".to_owned(),
        );
    }
    // The person is choosing where to save. The activity says when they
    // have, or have backed out.
    loop {
        if let Some(outcome) = activity::take_save_outcome(&request) {
            return outcome;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// Saves the recovery phrase as a text file where the person chooses. The
/// file is written in the app's own storage for Android to copy, and
/// removed as soon as Android has, or the person has backed out
/// (SECURITY.md).
async fn save_phrase_file(text: String) -> Option<Result<String, String>> {
    let folder = export_folder();
    let path = folder.join(PHRASE_FILE_NAME);
    let written = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, text));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&path);
        return Some(Err(dirt_core::util::as_sentence(&error)));
    }
    let saved = save_through_android(PHRASE_FILE_NAME, "text/plain", &path).await;
    let _ = std::fs::remove_file(&path);
    match saved {
        SaveOutcome::Saved => Some(Ok(PHRASE_FILE_NAME.to_owned())),
        SaveOutcome::Cancelled => None,
        SaveOutcome::Failed(why) => Some(Err(format!("The file could not be saved: {why}."))),
    }
}

/// Settings, over the main screen.
#[component]
pub fn SettingsSheet() -> Element {
    let app = use_context::<AppState>();
    let nav = settings_nav();
    let linked = linked(&app);
    let appearance = Appearance::from_settings(&app.settings.read());
    let voice = crate::voice::offered().then_some(app.settings.read().voice_recording);

    let sign_in: SignIn =
        use_callback(move |session: Session| Box::pin(sign_in(session)) as Task<_>);
    let save_phrase: SavePhrase =
        use_callback(move |text: String| Box::pin(save_phrase_file(text)) as Task<_>);
    let export: ExportTo = use_callback(move |format: ExportFormat| {
        let store = app.store.peek().clone();
        let session = app.session.peek().clone();
        Box::pin(export_to_zip(store, session, format)) as Task<_>
    });
    let change_appearance = move |chosen: Appearance| {
        let mut settings = app.settings;
        let mut next = settings.peek().clone();
        chosen.write_to(&mut next);
        settings.set(next);
        app.save_settings();
    };
    let change_voice = move |kept| {
        let mut settings = app.settings;
        settings.write().voice_recording = kept;
        app.save_settings();
    };
    // Marked sensitive, so that Android 13 and later show none of it as
    // they say it was copied, and keyboards keep it out of their
    // suggestions (SECURITY.md).
    let copy = move |text: String| {
        if !activity::copy_text(&text) {
            tracing::error!("Could not copy to the clipboard");
        }
    };
    let open_note = move |id| {
        let mut request = app.open_request;
        request.set(Some(id));
    };

    rsx! {
        JoinBanner { linked }
        Sheet {
            open: nav.is_open(),
            title: "Settings",
            on_close: move |()| nav.close(),
            SettingsPage {
                linked,
                appearance,
                on_appearance: change_appearance,
                voice,
                on_voice: change_voice,
                sign_in,
                export,
                copy,
                save_phrase,
                notes_brought: move |()| app.reread(),
                open_note,
            }
        }
    }
}
