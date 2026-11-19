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

/// The command key's name, as the page tells the script
/// (`data-dui-page`): Cmd on a Mac, Ctrl elsewhere.
pub const COMMAND_KEY: &str = if cfg!(target_os = "macos") {
    "meta"
} else {
    "ctrl"
};

/// Whether a key pressed in the page is done and empties it: Esc with no
/// modifier, or the command key with Enter or N and no other modifier.
///
/// The script empties the page on the same keys as they are pressed
/// (`data-dui-page` in `dirt-ui`'s script), and this is the one place the
/// rule is written for Rust. The two must agree on every key: if the script
/// empties the page and the app does not take the key as done, the next
/// character would be saved over the note; if the app takes it as done
/// and the script has not emptied the page, the old text would become a
/// second note.
fn empties_page(key: &Key, modifiers: Modifiers, mac: bool) -> bool {
    let (command, other) = if mac {
        (Modifiers::META, Modifiers::CONTROL)
    } else {
        (Modifiers::CONTROL, Modifiers::META)
    };
    if modifiers.intersects(Modifiers::ALT | Modifiers::SHIFT | other) {
        return false;
    }
    if modifiers.contains(command) {
        match key {
            Key::Enter => true,
            Key::Character(character) => character.to_lowercase() == "n",
            _ => false,
        }
    } else {
        *key == Key::Escape
    }
}

/// [`empties_page`] for a key pressed here.
fn empties_the_page(event: &KeyboardEvent) -> bool {
    empties_page(&event.key(), event.modifiers(), cfg!(target_os = "macos"))
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
    if at == Where::Page {
        screen.key_on_page();
    }
    // The keys that empty the page have emptied it already, in the script,
    // unless Settings is open over it (the page then has no
    // `data-dui-page`). Done by any other key gives a fresh page instead.
    let emptied = at == Where::Page && !app.nav.is_open() && empties_the_page(event);
    let mut taken = true;
    if command(event) {
        match (letter(event).as_deref(), event.key()) {
            (Some("n"), _) => screen.done(emptied),
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
            (_, Key::Enter) if at == Where::Page => screen.done(emptied),
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
        if app.nav.is_open() {
            app.nav.close();
        } else if at == Where::Page {
            screen.done(emptied);
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
    (
        "Space",
        "On a file: preview it. \u{2190} \u{2192} for the others, Esc or Space to close",
    ),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn character(text: &str) -> Key {
        Key::Character(text.to_owned())
    }

    /// The keys that are done, on Windows and Linux and on a Mac. The
    /// script's rule in `dirt-ui.js` is the same, with the command key the
    /// page names.
    #[test]
    fn the_page_is_emptied_by_the_done_keys_alone() {
        for mac in [false, true] {
            let command = if mac {
                Modifiers::META
            } else {
                Modifiers::CONTROL
            };
            assert!(empties_page(&Key::Escape, Modifiers::empty(), mac));
            assert!(empties_page(&Key::Enter, command, mac));
            assert!(empties_page(&character("n"), command, mac));
            assert!(empties_page(&character("N"), command, mac));
            // Enter alone is a new line, and a modifier added to a done
            // key makes it another key.
            assert!(!empties_page(&Key::Enter, Modifiers::empty(), mac));
            assert!(!empties_page(&Key::Escape, Modifiers::SHIFT, mac));
            assert!(!empties_page(&Key::Escape, Modifiers::ALT, mac));
            assert!(!empties_page(&Key::Enter, command | Modifiers::SHIFT, mac));
            assert!(!empties_page(
                &character("n"),
                command | Modifiers::ALT,
                mac
            ));
            assert!(!empties_page(&character("m"), command, mac));
        }
        // Windows and Linux: Win or Super is not the command key, and with
        // Esc it is not done either.
        assert!(!empties_page(&Key::Escape, Modifiers::META, false));
        assert!(!empties_page(&Key::Enter, Modifiers::META, false));
        assert!(!empties_page(&Key::Escape, Modifiers::CONTROL, false));
        // A Mac: Ctrl is not the command key.
        assert!(!empties_page(&Key::Enter, Modifiers::CONTROL, true));
        assert!(!empties_page(&character("n"), Modifiers::CONTROL, true));
        assert!(!empties_page(&Key::Escape, Modifiers::CONTROL, true));
        assert!(!empties_page(&Key::Escape, Modifiers::META, true));
    }
}
