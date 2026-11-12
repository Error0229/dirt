//! Turns "old text, new text" into the smallest set of inserts and deletes.
//!
//! The editors hand over the whole text of a note. Writing that into the CRDT
//! as "delete everything, insert everything" would make two concurrent edits
//! show up twice after a merge, so only the changed spans are applied.
//!
//! Lengths are UTF-8 byte counts, because that is how yrs addresses text by
//! default. The diff itself runs over characters, so a multi-byte character is
//! never split.
//!
//! The search for the smallest script is given a time limit. Two large texts
//! with little in common would otherwise take minutes, and saving a note
//! must never wait. Past the limit the rest of the text is replaced whole:
//! a longer script that still gives the right text.

use std::time::Duration;

use similar::{ChangeTag, TextDiff};

/// How long the search for the smallest script may take. A note of ordinary
/// size is done in far less.
const SEARCH_LIMIT: Duration = Duration::from_millis(100);

/// One step of an edit script, applied left to right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Leave this many bytes untouched.
    Keep(u32),
    /// Remove this many bytes at the current position.
    Delete(u32),
    /// Insert this text at the current position.
    Insert(String),
}

/// Computes the edit script that turns `old` into `new`.
///
/// Adjacent steps of the same kind are merged, so typing a word produces one
/// `Insert` and not one per character.
#[must_use]
pub fn diff(old: &str, new: &str) -> Vec<Op> {
    let mut ops: Vec<Op> = Vec::new();
    let found = TextDiff::configure()
        .timeout(SEARCH_LIMIT)
        .diff_chars(old, new);
    for change in found.iter_all_changes() {
        let text = change.value();
        let len = u32::try_from(text.len()).expect("note text fits in u32");
        match (change.tag(), ops.last_mut()) {
            (ChangeTag::Equal, Some(Op::Keep(n))) | (ChangeTag::Delete, Some(Op::Delete(n))) => {
                *n += len;
            }
            (ChangeTag::Insert, Some(Op::Insert(s))) => s.push_str(text),
            (ChangeTag::Equal, _) => ops.push(Op::Keep(len)),
            (ChangeTag::Delete, _) => ops.push(Op::Delete(len)),
            (ChangeTag::Insert, _) => ops.push(Op::Insert(text.to_owned())),
        }
    }
    // A trailing Keep changes nothing.
    if matches!(ops.last(), Some(Op::Keep(_))) {
        ops.pop();
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Applies an edit script to a plain string, to check the script itself.
    fn apply(old: &str, ops: &[Op]) -> String {
        let mut out = String::new();
        let mut pos = 0usize;
        for op in ops {
            match op {
                Op::Keep(n) => {
                    out.push_str(&old[pos..pos + *n as usize]);
                    pos += *n as usize;
                }
                Op::Delete(n) => pos += *n as usize,
                Op::Insert(s) => out.push_str(s),
            }
        }
        out.push_str(&old[pos..]);
        out
    }

    /// Replacing the whole of a very large note must not stall the app.
    /// Left to run, a character diff of two large texts with little in
    /// common takes minutes.
    #[test]
    fn replacing_a_very_large_text_is_quick_and_still_right() {
        let large = "x".repeat(1_100_000);
        let other = format!("y{}", "z".repeat(900_000));
        for (old, new) in [
            (large.as_str(), "a note of ordinary size"),
            ("a note of ordinary size", large.as_str()),
            (large.as_str(), other.as_str()),
        ] {
            let began = std::time::Instant::now();
            let ops = diff(old, new);
            assert!(
                began.elapsed() < std::time::Duration::from_secs(10),
                "took {:?}",
                began.elapsed()
            );
            assert!(apply(old, &ops) == new, "the script gives another text");
        }
    }

    #[test]
    fn identical_text_needs_no_ops() {
        assert!(diff("same", "same").is_empty());
    }

    #[test]
    fn one_word_change_touches_only_that_word() {
        let ops = diff("buy milk today", "buy bread today");
        assert_eq!(apply("buy milk today", &ops), "buy bread today");
        // The untouched prefix is kept, not rewritten.
        assert_eq!(ops.first(), Some(&Op::Keep(4)));
    }

    #[test]
    fn two_separate_changes_keep_the_text_between_them() {
        let old = "alpha beta gamma delta";
        let new = "ALPHA beta gamma DELTA";
        let ops = diff(old, new);
        assert_eq!(apply(old, &ops), new);
        let kept = u32::try_from(" beta gamma ".len()).unwrap();
        assert!(ops.contains(&Op::Keep(kept)));
    }

    #[test]
    fn multi_byte_characters_are_counted_in_bytes_and_never_split() {
        let old = "買牛奶 🥛 today";
        let new = "買麵包 🍞 today";
        let ops = diff(old, new);
        assert_eq!(apply(old, &ops), new);
        // "買" is three bytes in UTF-8 and must be kept whole.
        assert_eq!(ops.first(), Some(&Op::Keep(3)));
    }

    #[test]
    fn empty_to_text_and_back() {
        assert_eq!(diff("", "note"), vec![Op::Insert("note".into())]);
        assert_eq!(diff("note", ""), vec![Op::Delete(4)]);
    }
}
