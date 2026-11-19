//! Settings on the desktop (interface design, sections 5.5 and 6.9): the
//! shared page of `dirt_ui::settings` in a panel that grows out of the
//! gear, with the rail beside it, and the banner for a device that waits
//! to be let in.
//!
//! What only the desktop can do is here: signing in through the system
//! browser, choosing the folder an export goes into, and the clipboard.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::export::ExportFormat;
use dirt_core::models::FocusStyle;
use dirt_core::session::Session;
use dirt_core::Store;
use dirt_ui::components::Panel;
use dirt_ui::settings::{
    settings_nav, ExportTo, Exported, JoinBanner, Linked, SavePhrase, SettingsPage, SettingsRail,
    SignIn, PHRASE_FILE_NAME,
};
use dirt_ui::Appearance;
use rfd::AsyncFileDialog;

use crate::services::export_into;
use crate::state::AppState;

/// In a development build, `DIRT_SIGN_IN_TO_FILE` names a file the
/// sign-in address is written to instead of opening the browser, and
/// `DIRT_EXPORT_TO` a folder an export, or the recovery phrase's file,
/// goes into without asking: for
/// trying the flows on a computer whose browser and dialogs belong to
/// someone else. A release build ignores both.
fn test_hook(name: &str) -> Option<std::path::PathBuf> {
    if cfg!(debug_assertions) {
        std::env::var_os(name).map(std::path::PathBuf::from)
    } else {
        None
    }
}

/// Opens the sign-in page in the system browser.
fn open_browser(address: &str) -> Result<(), String> {
    if let Some(file) = test_hook("DIRT_SIGN_IN_TO_FILE") {
        return std::fs::write(file, address).map_err(|error| error.to_string());
    }
    webbrowser::open(address).map_err(|error| error.to_string())
}

/// Puts a secret, the recovery phrase, on the clipboard, marked so that
/// the system keeps it out of what it remembers of the clipboard: Windows'
/// clipboard history and its cloud clipboard, and the clipboard managers
/// that follow the platform's convention on macOS (nspasteboard.org's
/// concealed type) and Linux (`x-kde-passwordManagerHint`). It is on the
/// clipboard itself, for any program to read, until something else is
/// copied (`SECURITY.md`).
fn copy_secret(text: String) -> Result<(), arboard::Error> {
    let mut board = arboard::Clipboard::new()?;
    let set = board.set();
    #[cfg(windows)]
    let set = {
        use arboard::SetExtWindows;
        set.exclude_from_history().exclude_from_cloud()
    };
    #[cfg(target_os = "macos")]
    let set = {
        use arboard::SetExtApple;
        set.exclude_from_history()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let set = {
        use arboard::SetExtLinux;
        set.exclude_from_history()
    };
    set.text(text)
}

/// Saves the recovery phrase as a text file where the person chooses, in
/// the system's save dialog. Nothing chosen is nothing written.
async fn save_phrase_file(text: String) -> Option<Result<String, String>> {
    let path = match test_hook("DIRT_EXPORT_TO") {
        Some(folder) => folder.join(PHRASE_FILE_NAME),
        None => AsyncFileDialog::new()
            .set_title("Save the recovery phrase")
            .set_file_name(PHRASE_FILE_NAME)
            .add_filter("Text", &["txt"])
            .save_file()
            .await?
            .path()
            .to_path_buf(),
    };
    let name = path.file_name().map_or_else(
        || PHRASE_FILE_NAME.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    // A few hundred bytes: written at once.
    Some(
        std::fs::write(&path, text)
            .map(|()| name)
            .map_err(|error| dirt_core::util::as_sentence(&error)),
    )
}

/// What Settings is given of the app's session.
const fn linked(app: &AppState) -> Linked {
    Linked {
        session: app.session,
        state: app.session_state,
        status: app.sync_status,
    }
}

/// Exports every note into a new folder inside the one the person
/// chooses. Nothing chosen is nothing done.
async fn export_to_folder(
    store: Option<Arc<Store>>,
    session: Option<Session>,
    format: ExportFormat,
) -> Option<Result<Exported, String>> {
    let store = store?;
    let chosen = match test_hook("DIRT_EXPORT_TO") {
        Some(folder) => folder,
        None => AsyncFileDialog::new()
            .set_title("Choose a folder to export into")
            .pick_folder()
            .await?
            .path()
            .to_path_buf(),
    };
    let exported = export_into(
        store.as_ref(),
        session.as_ref(),
        format,
        &chosen,
        chrono::Utc::now().timestamp_millis(),
    )
    .await;
    Some(
        exported
            .map(|exported| Exported {
                said: exported.said,
                left_out: exported.left_out,
            })
            .map_err(|error| dirt_core::util::as_sentence(&error)),
    )
}

/// Settings, over the main screen.
#[component]
pub fn SettingsPanel() -> Element {
    let app = use_context::<AppState>();
    let nav = settings_nav();
    let linked = linked(&app);
    let settings = app.settings.read().clone();
    let appearance = Appearance::from_settings(&settings);
    let focus_style = settings.focus_style;

    let sign_in: SignIn = use_callback(move |session: Session| {
        Box::pin(async move { session.sign_in_with_browser(open_browser).await })
            as dirt_ui::settings::Task<_>
    });
    let save_phrase: SavePhrase = use_callback(move |text: String| {
        Box::pin(save_phrase_file(text)) as dirt_ui::settings::Task<_>
    });
    let export: ExportTo = use_callback(move |format: ExportFormat| {
        let store = app.db_service.peek().clone();
        let session = app.session.peek().clone();
        Box::pin(export_to_folder(store, session, format)) as dirt_ui::settings::Task<_>
    });
    let change_appearance = move |chosen: Appearance| {
        let mut settings = app.settings;
        let mut next = settings.peek().clone();
        chosen.write_to(&mut next);
        settings.set(next);
        app.save_settings();
    };
    let change_focus = move |style: FocusStyle| {
        let mut settings = app.settings;
        settings.write().focus_style = style;
        app.save_settings();
    };
    let copy = move |text: String| {
        if let Err(error) = copy_secret(text) {
            tracing::error!("Could not copy: {error}");
        }
    };
    let open_note = move |id| {
        let mut request = app.open_request;
        request.set(Some(id));
    };

    rsx! {
        JoinBanner { linked }
        Panel {
            open: nav.is_open(),
            label: "Settings",
            on_close: move |()| nav.close(),
            div {
                class: "dui-set-panel",
                // Ctrl , closes it as it opened it.
                onkeydown: move |event| {
                    let command = event.modifiers().ctrl() || event.modifiers().meta();
                    if command && event.key() == Key::Character(",".to_owned()) {
                        event.prevent_default();
                        nav.close();
                    }
                },
                SettingsRail {}
                SettingsPage {
                    linked,
                    appearance,
                    on_appearance: change_appearance,
                    focus_style,
                    on_focus_style: change_focus,
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
}
