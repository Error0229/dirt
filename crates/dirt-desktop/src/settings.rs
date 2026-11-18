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
    settings_nav, ExportTo, Exported, JoinBanner, Linked, SettingsPage, SettingsRail, SignIn,
};
use dirt_ui::Appearance;
use rfd::AsyncFileDialog;

use crate::services::export_into;
use crate::state::AppState;

/// Opens the sign-in page in the system browser.
fn open_browser(address: &str) -> Result<(), String> {
    webbrowser::open(address).map_err(|error| error.to_string())
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
    let chosen = AsyncFileDialog::new()
        .set_title("Choose a folder to export into")
        .pick_folder()
        .await?;
    let exported = export_into(
        store.as_ref(),
        session.as_ref(),
        format,
        chosen.path(),
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
        if let Err(error) = arboard::Clipboard::new().and_then(|mut board| board.set_text(text)) {
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
                    open_note,
                }
            }
        }
    }
}
