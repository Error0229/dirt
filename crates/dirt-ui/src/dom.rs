//! What an app asks of the page that rendering cannot say: where the
//! keyboard's focus goes, and the text of a field the app does not hold.
//!
//! A text field of the new interface keeps its own text. The app hears
//! every change and never writes the text back while the person types, so
//! a key pressed while the app was busy is not written over. When the app
//! itself changes the text (Find emptied by Esc, a note that changed on
//! another device), it does so here.
//!
//! Each of these runs in the page a moment later. An element that is
//! being rendered in the same breath may not be there yet: focus such an
//! element from its `onmounted`.

use dioxus::prelude::*;

/// Where the caret goes in a text field that is given the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Caret {
    /// Where it was the last time the field had the focus.
    #[default]
    Kept,
    /// After the last character.
    End,
    /// The whole text is selected.
    All,
}

impl Caret {
    const fn name(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::End => "end",
            Self::All => "all",
        }
    }
}

fn quoted(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

/// Gives the keyboard's focus to the first element that matches one of
/// `selectors` and can be seen, trying them in order.
pub fn focus(selectors: &[&str], caret: Caret) {
    let selectors = serde_json::json!(selectors);
    document::eval(&format!(
        "window.DirtUi && DirtUi.focus({selectors}, {});",
        quoted(caret.name())
    ));
}

/// Takes the keyboard's focus from whatever has it. On a phone that puts
/// the keyboard away.
pub fn blur() {
    document::eval(
        "document.activeElement && document.activeElement.blur && document.activeElement.blur();",
    );
}

/// Sets the text of the field that matches `selector`. Nothing is heard
/// of it: the app already knows the text it set.
pub fn set_value(selector: &str, text: &str) {
    document::eval(&format!(
        "window.DirtUi && DirtUi.setValue({}, {});",
        quoted(selector),
        quoted(text)
    ));
}

/// Adds `text` at the end of the field that matches `selector`, leaving
/// the caret where it is: the tags a new page gets.
///
/// The field then says so as it does when the person types, so the app
/// hears the whole text, with anything typed in the meantime.
pub fn append_text(selector: &str, text: &str) {
    document::eval(&format!(
        "window.DirtUi && DirtUi.appendText({}, {});",
        quoted(selector),
        quoted(text)
    ));
}

/// Puts `text` where the caret was when Paste was chosen in the app's own
/// menu, in place of what was selected. The field says so as it does when
/// the person types.
pub fn paste(text: &str) {
    document::eval(&format!("window.DirtUi && DirtUi.paste({});", quoted(text)));
}

/// Scrolls the element with this ID into sight, as little as it takes:
/// the row the keyboard has moved to.
pub fn reveal(element_id: &str) {
    document::eval(&format!(
        "(document.getElementById({}) || {{ scrollIntoView() {{}} }}).scrollIntoView({{ block: 'nearest' }});",
        quoted(element_id)
    ));
}
