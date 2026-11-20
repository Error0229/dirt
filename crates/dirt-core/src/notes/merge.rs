//! Merging two changes made to the same text from one starting point.
//!
//! The page saves a note's whole text, and between the page reading the
//! note and its save reaching the store something else may have changed the
//! note: the words of what was said put at its start (interface design
//! 6.18), or an edit from another device. Saving the page's text as it is
//! would undo that change. So the page says what text it started from, its
//! base, and its changes from the base are made to what the store holds
//! now: [`merge3`].
//!
//! The changes are found as the note's edits are ([`super::diff`]), and the
//! page's are carried over to the store's text by where they fall in the
//! base. Where both changed the same stretch, both changes are kept, the
//! page's after the store's: nothing either side wrote is lost.

use super::diff::{diff, Op};

/// Where position `at` of the base is in the other text: before anything
/// the other side inserted there, and after it.
fn place(ops: &[Op], at: usize) -> (usize, usize) {
    let (mut base, mut other) = (0usize, 0usize);
    let mut before = None;
    for op in ops {
        match op {
            Op::Insert(text) => {
                if base == at && before.is_none() {
                    before = Some(other);
                }
                other += text.len();
            }
            Op::Keep(n) => {
                let n = *n as usize;
                if at < base + n {
                    let here = other + (at - base);
                    return (before.unwrap_or(here), here);
                }
                base += n;
                other += n;
            }
            Op::Delete(n) => {
                let n = *n as usize;
                if at < base + n {
                    return (before.unwrap_or(other), other);
                }
                base += n;
            }
        }
    }
    // The script leaves out the text that ends both unchanged.
    let here = other + at.saturating_sub(base);
    (before.unwrap_or(here), here)
}

/// One change of one side: the stretch `start..end` of the base, in bytes,
/// becomes `text`.
struct Change {
    start: usize,
    end: usize,
    text: String,
}

/// The changes that turn `base` into `new`, from the first to the last.
fn changes(base: &str, new: &str) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let mut at = 0usize;
    let mut open: Option<Change> = None;
    for op in diff(base, new) {
        match op {
            Op::Keep(n) => {
                if let Some(change) = open.take() {
                    out.push(change);
                }
                at += n as usize;
            }
            Op::Delete(n) => {
                let change = open.get_or_insert_with(|| Change {
                    start: at,
                    end: at,
                    text: String::new(),
                });
                at += n as usize;
                change.end = at;
            }
            Op::Insert(text) => {
                open.get_or_insert_with(|| Change {
                    start: at,
                    end: at,
                    text: String::new(),
                })
                .text
                .push_str(&text);
            }
        }
    }
    out.extend(open);
    out
}

/// The changes that turn `old` into `new`, from the first to the last.
///
/// Each is the stretch `start..end` of `old`, in bytes, and what takes its
/// place. For a field that is shown the new text: only these are written
/// into it, and what is between them stays as it is.
#[must_use]
pub fn text_changes(old: &str, new: &str) -> Vec<(usize, usize, String)> {
    changes(old, new)
        .into_iter()
        .map(|change| (change.start, change.end, change.text))
        .collect()
}

/// `ours`' changes from `base`, made to `theirs`.
///
/// With `theirs` the same as `base` this is `ours`, and with `ours` the
/// same as `base` it is `theirs`. A change of ours is placed where its
/// stretch of the base is in `theirs`; what they inserted at the edge of
/// that stretch stays, and what they removed of it stays removed.
#[must_use]
pub fn merge3(base: &str, ours: &str, theirs: &str) -> String {
    if theirs == base {
        return ours.to_owned();
    }
    if ours == base {
        return theirs.to_owned();
    }
    // Two texts written apart from nothing: both are kept, theirs first,
    // a blank line between, as the words of what was said go before what
    // was typed.
    if base.trim().is_empty() {
        return format!("{}\n\n{}", theirs.trim_end(), ours.trim_start());
    }
    let theirs_ops = diff(base, theirs);
    let mut merged = theirs.to_owned();
    // From the last to the first, so that each is placed in text that the
    // ones before it have not moved.
    for change in changes(base, ours).into_iter().rev() {
        // Where both inserted at one point of the base, ours goes after
        // theirs: words put at the start of a note come before what is
        // typed there. At the very end of the base it goes before: what is
        // typed at the end goes on from the text it was typed after, and
        // words put at the end of a note (a recording added to it) come
        // after it.
        let at = place(&theirs_ops, change.start);
        let start = if change.start == base.len() {
            at.0
        } else {
            at.1
        };
        let end = if change.end > change.start {
            place(&theirs_ops, change.end).0.max(start)
        } else {
            start
        };
        merged.replace_range(start..end, &change.text);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_one_side_unchanged_the_other_side_is_taken() {
        assert_eq!(merge3("a b", "a b c", "a b"), "a b c");
        assert_eq!(merge3("a b", "a b", "z a b"), "z a b");
        assert_eq!(merge3("", "typed", ""), "typed");
    }

    #[test]
    fn words_put_before_what_was_typed_survive_what_is_typed_next() {
        let base = "and #milk";
        let theirs = "Buy oat milk and stamps.\n\nand #milk";
        assert_eq!(
            merge3(base, "and #milk, eggs", theirs),
            "Buy oat milk and stamps.\n\nand #milk, eggs"
        );
        // Typed at the very start: it goes after the words, which were put
        // before everything the page had.
        assert_eq!(
            merge3(base, "Also and #milk", theirs),
            "Buy oat milk and stamps.\n\nAlso and #milk"
        );
        // Everything typed taken away: the words stay.
        assert_eq!(merge3(base, "", theirs), "Buy oat milk and stamps.\n\n");
        // Words into a page that had nothing when they began.
        assert_eq!(
            merge3("", "typed", "Call the dentist"),
            "Call the dentist\n\ntyped"
        );
    }

    #[test]
    fn words_put_at_the_end_come_after_what_is_typed_at_the_end() {
        let base = "Buy milk, eg";
        let theirs = "Buy milk, eg\n\ncall mum";
        // Typed on at the end while the words were being made.
        assert_eq!(
            merge3(base, "Buy milk, eggs", theirs),
            "Buy milk, eggs\n\ncall mum"
        );
        // A new line typed at the end, and more.
        assert_eq!(
            merge3(base, "Buy milk, eggs\n- bread", theirs),
            "Buy milk, eggs\n- bread\n\ncall mum"
        );
        // The end typed over: the words stay after it.
        assert_eq!(
            merge3(base, "Buy milk, bread", theirs),
            "Buy milk, bread\n\ncall mum"
        );
        // Typed elsewhere: the words at the end are untouched.
        assert_eq!(
            merge3(base, "Buy oat milk, eg", theirs),
            "Buy oat milk, eg\n\ncall mum"
        );
    }

    #[test]
    fn changes_in_different_places_are_both_kept() {
        let base = "one two three";
        assert_eq!(merge3(base, "one 2 three", "one two 3"), "one 2 3");
        assert_eq!(
            merge3(base, "zero one two three", "one two three four"),
            "zero one two three four"
        );
    }

    #[test]
    fn both_changing_one_stretch_keeps_both_writings() {
        let merged = merge3("cat", "dog", "cow");
        assert!(merged.contains("dog"), "{merged}");
    }

    #[test]
    fn characters_of_several_bytes_are_never_split() {
        assert_eq!(merge3("café", "café au lait", "un café"), "un café au lait");
        assert_eq!(merge3("日本", "日本語", "大日本"), "大日本語");
    }
}
