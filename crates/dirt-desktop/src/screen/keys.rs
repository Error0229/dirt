//! The screen's keys (interface design, section 7.4), and the panel that
//! shows them (Ctrl /).
//!
//! The keys that belong to one place (Find, the list) are taken there and
//! go no further. What is left reaches [`on_key`], which every key pressed
//! on the screen passes through.

use dioxus::prelude::*;
use dirt_ui::components::Panel;
use dirt_ui::dom::Caret;

use super::{Screen, Where};
use crate::state::AppState;

/// Ctrl, or Cmd on a Mac.
pub fn command(event: &KeyboardEvent) -> bool {
    let modifiers = event.modifiers();
    if cfg!(target_os = "macos") {
        modifiers.meta()
    } else {
        modifiers.ctrl()
    }
}

/// The key as a lower-case character, when it is one.
pub fn letter(event: &KeyboardEvent) -> Option<String> {
    match event.key() {
        Key::Character(character) => Some(character.to_lowercase()),
        _ => None,
    }
}

/// Del, or Cmd ⌫ on a Mac, which has no Del key.
pub fn is_delete(event: &KeyboardEvent) -> bool {
    event.key() == Key::Delete
        || (cfg!(target_os = "macos") && event.key() == Key::Backspace && event.modifiers().meta())
}

/// Every key pressed on the screen that nothing nearer took.
pub fn on_key(screen: &Screen, app: &AppState, event: &KeyboardEvent) {
    let (screen, app) = (*screen, *app);
    // A key pressed while an input method is composing is the input
    // method's (7.4).
    if event.is_composing() {
        return;
    }
    let at = *screen.focus.peek();
    let shift = event.modifiers().shift();
    let mut taken = true;
    if command(event) {
        match (letter(event).as_deref(), event.key()) {
            (Some("n"), _) => screen.done(),
            (Some("f" | "k"), _) => screen.focus_find(),
            (Some("r"), _) => screen.sync_now(),
            (Some(","), _) => screen.open_settings(),
            (Some("+" | "="), _) => screen.text_size(true),
            (Some("-" | "_"), _) => screen.text_size(false),
            (Some("/"), _) => {
                let mut open = screen.keys_open;
                let now = *open.peek();
                open.set(!now);
            }
            (Some("a"), _) if shift => screen.choose_files(),
            (Some("c"), _) if shift => screen.copy_note(),
            // Outside the text: the text's own undo is the field's, and
            // Find's is always Find's. A blank page has nothing of its own
            // to undo, and is where the cursor is after a note is archived
            // from it.
            (Some("z"), _)
                if !shift && at != Where::Find && (at != Where::Page || screen.page_is_blank()) =>
            {
                screen.undo(None);
            }
            (_, Key::Enter) if at == Where::Page => screen.done(),
            (_, _) if is_delete(event) && at == Where::Page => {
                if let Some(open) = screen.open_note() {
                    if shift {
                        screen.delete(open);
                    } else {
                        screen.archive_or_delete(open);
                    }
                }
            }
            _ => taken = false,
        }
    } else if event.key() == Key::Escape {
        if *app.settings_open.peek() {
            let mut open = app.settings_open;
            open.set(false);
        } else if at == Where::Page {
            screen.done();
        } else {
            taken = false;
        }
    } else if letter(event).as_deref() == Some("/")
        && !event.modifiers().alt()
        && !matches!(at, Where::Page | Where::Find)
    {
        screen.focus_find();
    } else {
        taken = false;
    }
    if taken {
        event.prevent_default();
    }
}

/// One row of the panel: the keys, and what they do.
const KEYS: &[(&str, &str)] = &[
    ("Ctrl Alt N", "Quick capture, from any app"),
    ("Esc", "Done with the note: a blank page"),
    ("Ctrl Enter", "Done, from the note"),
    ("Ctrl N", "Done, from anywhere"),
    ("Ctrl F", "Find"),
    ("Ctrl K", "Find"),
    ("/", "Find"),
    ("Enter", "In Find: open the first note found"),
    ("#", "In Find: the tags. Tab picks the first"),
    ("Backspace", "In an empty Find: take back the last tag"),
    ("\u{2193}", "From Find into the list"),
    ("\u{2191} \u{2193}", "In the list: move, showing each note"),
    ("Enter", "In the list: edit the note"),
    ("Del", "In the list: archive (delete, if archived)"),
    ("Shift Del", "In the list: delete for good"),
    ("Ctrl Del", "In a note: archive (delete, if archived)"),
    ("Ctrl Shift Del", "In a note: delete for good"),
    ("Ctrl Z", "Undo the archive or the delete"),
    ("Ctrl Shift A", "Attach a file"),
    ("Ctrl Shift C", "Copy the note's text"),
    ("Ctrl R", "Sync now"),
    ("Ctrl ,", "Settings"),
    ("Ctrl +", "Larger text"),
    ("Ctrl \u{2212}", "Smaller text"),
    ("Ctrl /", "These keys"),
];

/// The keys, on a Mac as a Mac has them.
fn for_this_computer(keys: &str) -> String {
    if cfg!(target_os = "macos") {
        keys.replace("Ctrl", "\u{2318}").replace("Del", "\u{232b}")
    } else {
        keys.to_owned()
    }
}

/// The panel that shows every key: Ctrl /.
#[component]
pub fn KeysPanel() -> Element {
    let screen = use_context::<Screen>();
    let mut open = screen.keys_open;
    let close = move |()| {
        open.set(false);
        screen.focus_page(Caret::Kept);
    };
    rsx! {
        Panel { open: open(), label: "Keys", on_close: close,
            div { class: "dk-keys dui-scroll", "data-dui-scroll": "",
                h2 { "data-dui-rise": "", "Keys" }
                table { "data-dui-rise": "",
                    tbody {
                        for (index , (keys , does)) in KEYS.iter().enumerate() {
                            tr { key: "{index}",
                                td {
                                    for key in for_this_computer(keys).split(' ').map(str::to_owned) {
                                        kbd { key: "{key}", "{key}" }
                                    }
                                }
                                td { "{does}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
