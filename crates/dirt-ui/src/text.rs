//! A note's words as a list draws them (interface design, section 6.2):
//! what Find matched is marked, and tags are in the accent.
//!
//! What is marked follows the store's search: every word typed must start
//! a word of the note. So a match is always the start of a word, and a tag
//! is marked whole when a typed word starts its name.

use dirt_core::models::tag_spans;

/// How a piece of a note's text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    /// As the text around it.
    Plain,
    /// What Find matched.
    Match,
    /// A `#tag`.
    Tag,
    /// A `#tag` that Find matched.
    TagMatch,
}

/// A run of a note's text that is drawn one way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// The run, as it is written in the note.
    pub text: String,
    /// How it is drawn.
    pub kind: PieceKind,
}

/// The words Find's text asks for, in lower case.
///
/// The store asks its index for each stretch of what was typed that has no
/// space in it, and the index reads such a stretch as the letters and
/// digits in it. So "to-do" asks for "to" and "do".
#[must_use]
pub fn find_words(typed: &str) -> Vec<String> {
    typed
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How many bytes at the start of `token` spell `word`, whatever their
/// case. `word` is in lower case.
fn starts_with(token: &str, word: &str) -> Option<usize> {
    let mut wanted = word.chars();
    let mut next = wanted.next()?;
    for (at, character) in token.char_indices() {
        for lower in character.to_lowercase() {
            if lower != next {
                return None;
            }
            match wanted.next() {
                Some(following) => next = following,
                None => return Some(at + character.len_utf8()),
            }
        }
    }
    None
}

/// The longest start of `token` that one of `words` spells.
fn matched(token: &str, words: &[String]) -> Option<usize> {
    words
        .iter()
        .filter_map(|word| starts_with(token, word))
        .max()
}

fn push(pieces: &mut Vec<Piece>, text: &str, kind: PieceKind) {
    if text.is_empty() {
        return;
    }
    match pieces.last_mut() {
        Some(last) if last.kind == kind && kind == PieceKind::Plain => last.text.push_str(text),
        _ => pieces.push(Piece {
            text: text.to_owned(),
            kind,
        }),
    }
}

/// The prose between two tags: each word that starts with a typed word is
/// marked as far as that word reaches.
fn prose(pieces: &mut Vec<Piece>, text: &str, words: &[String]) {
    let mut rest = text;
    while !rest.is_empty() {
        let word_ends = rest
            .find(|character: char| !character.is_alphanumeric())
            .unwrap_or(rest.len());
        if word_ends == 0 {
            let gap_ends = rest.find(char::is_alphanumeric).unwrap_or(rest.len());
            push(pieces, &rest[..gap_ends], PieceKind::Plain);
            rest = &rest[gap_ends..];
            continue;
        }
        let token = &rest[..word_ends];
        match matched(token, words) {
            Some(found) => {
                push(pieces, &token[..found], PieceKind::Match);
                push(pieces, &token[found..], PieceKind::Plain);
            }
            None => push(pieces, token, PieceKind::Plain),
        }
        rest = &rest[word_ends..];
    }
}

/// `text` in pieces: its tags apart, and what `words` match marked.
/// `words` are [`find_words`] of what was typed in Find; with none, only
/// the tags are told apart.
#[must_use]
pub fn pieces(text: &str, words: &[String]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut drawn = 0;
    for tag in tag_spans(text) {
        prose(&mut pieces, &text[drawn..tag.start], words);
        let name = &text[tag.start + 1..tag.end];
        let kind = if matched(name, words).is_some() {
            PieceKind::TagMatch
        } else {
            PieceKind::Tag
        };
        push(&mut pieces, &text[tag.clone()], kind);
        drawn = tag.end;
    }
    prose(&mut pieces, &text[drawn..], words);
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(text: &str, typed: &str) -> Vec<(String, PieceKind)> {
        pieces(text, &find_words(typed))
            .into_iter()
            .map(|piece| (piece.text, piece.kind))
            .collect()
    }

    fn piece(text: &str, kind: PieceKind) -> (String, PieceKind) {
        (text.to_owned(), kind)
    }

    #[test]
    fn the_words_asked_for_are_the_letters_and_digits_typed() {
        assert_eq!(find_words("  Ask  JORDAN "), ["ask", "jordan"]);
        assert_eq!(find_words("to-do #q3"), ["to", "do", "q3"]);
        assert!(find_words(" - # ").is_empty());
    }

    #[test]
    fn a_match_is_the_start_of_a_word_whatever_its_case() {
        assert_eq!(
            drawn("Ask Jordan about jam", "j"),
            [
                piece("Ask ", PieceKind::Plain),
                piece("J", PieceKind::Match),
                piece("ordan about ", PieceKind::Plain),
                piece("j", PieceKind::Match),
                piece("am", PieceKind::Plain),
            ]
        );
        // The middle of a word is not a match.
        assert_eq!(drawn("adjourn", "j"), [piece("adjourn", PieceKind::Plain)]);
    }

    #[test]
    fn the_longest_typed_word_that_fits_is_the_one_marked() {
        assert_eq!(
            drawn("sourdough", "so sour"),
            [
                piece("sour", PieceKind::Match),
                piece("dough", PieceKind::Plain),
            ]
        );
    }

    #[test]
    fn a_tag_is_apart_and_marked_whole() {
        assert_eq!(
            drawn("buy milk #Shopping today", "shop"),
            [
                piece("buy milk ", PieceKind::Plain),
                piece("#Shopping", PieceKind::TagMatch),
                piece(" today", PieceKind::Plain),
            ]
        );
        assert_eq!(drawn("#idea", ""), [piece("#idea", PieceKind::Tag)]);
    }

    #[test]
    fn text_in_other_scripts_is_cut_on_its_own_characters() {
        assert_eq!(
            drawn("Élan vital, ÉCLAIR", "é"),
            [
                piece("É", PieceKind::Match),
                piece("lan vital, ", PieceKind::Plain),
                piece("É", PieceKind::Match),
                piece("CLAIR", PieceKind::Plain),
            ]
        );
        assert_eq!(drawn("", "x"), []);
    }
}
