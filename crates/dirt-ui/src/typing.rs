//! What is typed into a note, heard by the app in order, however long the
//! note is (interface design 6.4).
//!
//! Dioxus hands an event to Rust as one synchronous request from the web
//! view, with everything it carries base64-encoded into a request header
//! (`dioxus-interpreter-js`, `handleVirtualdomEventSync`). An `input`
//! event carries its field's whole text. Past a few hundred kilobytes the
//! encoding fails in the script (`String.fromCharCode.apply` is handed one
//! argument per byte), the request is never made, and the app never hears
//! the text: a note that long was never made or saved (found in the plan's
//! step 5, on both apps). And a note of a megabyte would send a megabyte
//! with every key.
//!
//! So a note's field has no `oninput` of its own. The script hears it
//! (`data-dui-typed`) and tells Rust what changed: where, how much went,
//! and what came, through a hidden field beside it, the carrier, whose
//! `input` event Dioxus delivers by the same synchronous request as every
//! other. A change larger than [`PIECE`] characters, a long paste, goes in
//! pieces. Rust keeps the field's text and rebuilds it from the changes.
//!
//! The order is the point: a change comes by the same road as the keys and
//! the presses, so the app has heard every character typed before it hears
//! the Esc or the ⌄ that leaves the page, exactly as when the text came
//! whole. A change the app made itself (the text of a note opened, words
//! merged in, the page emptied by Esc in the script) is told too, so that
//! Rust's copy stays the field's; it is not told to the app as typing.
//!
//! Each change says the field's length once it is made. A copy that does
//! not agree (it should never happen) asks the script for the whole text
//! again, and the app hears it as typed.

use dioxus::prelude::*;
use serde::Deserialize;

/// The most characters one piece of a change carries.
///
/// They are UTF-16 units, as the page counts them: a long paste is told in
/// pieces of this size, each well under what the road carries.
pub const PIECE: usize = 16_384;

/// One piece of a change to the field, as the script tells it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Piece {
    /// The field is new to the script: Rust's copy starts again from empty.
    #[serde(default)]
    r: bool,
    /// Where the change is, in UTF-16 units from the start.
    a: usize,
    /// How many UTF-16 units go from there.
    d: usize,
    /// What comes in their place.
    i: String,
    /// More pieces of this change follow.
    #[serde(default)]
    m: bool,
    /// The person typed it (or pasted, or cut): the app is told. Otherwise
    /// the app itself made the change, and knows it.
    #[serde(default)]
    t: bool,
    /// The field's length in UTF-16 units, once the whole change is made.
    n: usize,
}

/// Rust's copy of a field's text.
#[derive(Debug, Default)]
struct Held {
    text: String,
    /// A change in pieces is under way, and was typed.
    typed: bool,
}

/// The byte where the `units`th UTF-16 unit of `text` begins, counting
/// from byte `from`. `None` past the end, or in the middle of a character
/// written as two units.
fn byte_at(text: &str, from: usize, units: usize) -> Option<usize> {
    let mut counted = 0;
    for (at, character) in text[from..].char_indices() {
        if counted == units {
            return Some(from + at);
        }
        counted += character.len_utf16();
        if counted > units {
            return None;
        }
    }
    (counted == units).then_some(text.len())
}

impl Held {
    /// Makes one piece of a change. `Some` with whether it was typed once
    /// the change is whole; `Err` if the copy and the field disagree.
    fn take(&mut self, piece: &Piece) -> Result<Option<bool>, ()> {
        if piece.r {
            self.text.clear();
        }
        let start = byte_at(&self.text, 0, piece.a).ok_or(())?;
        let end = byte_at(&self.text, start, piece.d).ok_or(())?;
        self.text.replace_range(start..end, &piece.i);
        self.typed |= piece.t;
        if piece.m {
            return Ok(None);
        }
        if self.text.encode_utf16().count() != piece.n {
            return Err(());
        }
        Ok(Some(std::mem::take(&mut self.typed)))
    }
}

/// A note's text field, heard through its carrier. Give the field
/// `data-dui-typed` with [`Typing::id`], and render [`Typing::carrier`]
/// beside it.
#[derive(Clone, Copy, PartialEq)]
pub struct Typing {
    id: CopyValue<String>,
    copy: CopyValue<Held>,
    on_text: Callback<String>,
}

/// Hears a note's field: `on_text` is given its whole text each time the
/// person changes it, in order with every other event of the page.
pub fn use_typing(mut on_text: impl FnMut(String) + 'static) -> Typing {
    let on_text = use_callback(move |text: String| on_text(text));
    let id = use_hook(|| {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        CopyValue::new(format!("dui-typed-{serial}"))
    });
    let copy = use_hook(|| CopyValue::new(Held::default()));
    Typing { id, copy, on_text }
}

impl Typing {
    /// What the field's `data-dui-typed` is.
    #[must_use]
    pub fn id(self) -> String {
        self.id.peek().clone()
    }

    /// The hidden field the script tells the changes through. Rendered
    /// beside the note's field, and gone with it.
    pub fn carrier(self) -> Element {
        let id = self.id();
        rsx! {
            input {
                r#type: "text",
                hidden: true,
                tabindex: "-1",
                "aria-hidden": "true",
                "data-dui-carrier": "{id}",
                oninput: move |event: FormEvent| self.heard(&event.value()),
            }
        }
    }

    /// The text as the app last heard it.
    #[must_use]
    pub fn text(self) -> String {
        self.copy.peek().text.clone()
    }

    fn heard(mut self, message: &str) {
        let Ok(piece) = serde_json::from_str::<Piece>(message) else {
            tracing::error!("The page told of a change to a note that could not be read");
            return;
        };
        let taken = self.copy.write().take(&piece);
        match taken {
            Ok(None | Some(false)) => {}
            Ok(Some(true)) => {
                let text = self.copy.peek().text.clone();
                self.on_text.call(text);
            }
            Err(()) => {
                tracing::error!(
                    "The app's copy of a note's text and the page's differ: asking again"
                );
                *self.copy.write() = Held::default();
                document::eval(&format!(
                    "window.DirtUi && DirtUi.typedAgain({});",
                    serde_json::Value::String(self.id())
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(a: usize, d: usize, i: &str, n: usize) -> Piece {
        Piece {
            r: false,
            a,
            d,
            i: i.to_owned(),
            m: false,
            t: true,
            n,
        }
    }

    #[test]
    fn a_change_is_made_where_the_page_counts_it() {
        let mut copy = Held::default();
        assert_eq!(copy.take(&piece(0, 0, "héllo", 5)), Ok(Some(true)));
        // After an emoji, written in UTF-16 as two units.
        assert_eq!(copy.take(&piece(5, 0, " 🌱", 8)), Ok(Some(true)));
        assert_eq!(copy.take(&piece(8, 0, "!", 9)), Ok(Some(true)));
        assert_eq!(copy.text, "héllo 🌱!");
        // The emoji replaced: two units go.
        assert_eq!(copy.take(&piece(6, 2, "x", 8)), Ok(Some(true)));
        assert_eq!(copy.text, "héllo x!");
    }

    #[test]
    fn a_long_paste_comes_in_pieces_and_is_told_once_whole() {
        let mut copy = Held::default();
        let long = "a".repeat(PIECE * 3 + 5);
        let parts: Vec<&str> = [
            &long[..PIECE],
            &long[PIECE..PIECE * 2],
            &long[PIECE * 2..PIECE * 3],
            &long[PIECE * 3..],
        ]
        .to_vec();
        let mut at = 0;
        for (index, part) in parts.iter().enumerate() {
            let last = index + 1 == parts.len();
            let told = copy.take(&Piece {
                r: index == 0,
                a: at,
                d: 0,
                i: (*part).to_owned(),
                m: !last,
                t: true,
                n: long.len(),
            });
            assert_eq!(told, Ok(last.then_some(true)));
            at += part.len();
        }
        assert_eq!(copy.text, long);
    }

    #[test]
    fn a_change_the_app_made_is_kept_and_not_told_as_typing() {
        let mut copy = Held::default();
        let mut set = piece(0, 0, "a note opened", 13);
        set.t = false;
        assert_eq!(copy.take(&set), Ok(Some(false)));
        assert_eq!(copy.text, "a note opened");
        let mut reset = piece(0, 0, "", 0);
        reset.r = true;
        reset.t = false;
        assert_eq!(copy.take(&reset), Ok(Some(false)));
        assert_eq!(copy.text, "");
    }

    #[test]
    fn a_copy_that_does_not_agree_with_the_page_says_so() {
        let mut copy = Held::default();
        assert_eq!(copy.take(&piece(0, 0, "abc", 3)), Ok(Some(true)));
        // Past the end.
        assert_eq!(copy.take(&piece(9, 0, "x", 4)), Err(()));
        // Into the middle of a character written as two units.
        let mut copy = Held::default();
        assert_eq!(copy.take(&piece(0, 0, "🌱", 2)), Ok(Some(true)));
        assert_eq!(copy.take(&piece(1, 0, "x", 3)), Err(()));
        // A length that differs.
        let mut copy = Held::default();
        assert_eq!(copy.take(&piece(0, 0, "abc", 4)), Err(()));
    }
}
