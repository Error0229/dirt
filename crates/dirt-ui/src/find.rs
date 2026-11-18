//! What Find's text means (interface design, sections 6.1 and 6.3).
//!
//! Find holds words, and at its end perhaps the start of a tag: `milk #sh`.
//! The words narrow the list. What follows the `#` narrows the list of
//! tags, which is open for as long as the text ends so. Chosen tags are
//! not in the text: they are tokens in front of it.

/// Whether a tag's name may hold this character. A name starts with a
/// letter; here it is still being typed, so a digit may come first for a
/// moment and simply match no tag.
const fn of_a_tag(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-'
}

/// Where the `#` of a tag being typed is: at the very end of the text or
/// followed only by what a tag's name can hold, and at the start of the
/// text or after a space.
fn tag_start(text: &str) -> Option<usize> {
    let hash = text.rfind('#')?;
    let before_is_space = text[..hash]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    (before_is_space && text[hash + 1..].chars().all(of_a_tag)).then_some(hash)
}

/// What was typed after a `#` at the end of Find's text: `"milk #sh"`
/// gives `"sh"`, and `"milk #"` gives the empty word. Nothing when the
/// text does not end in a tag being typed.
#[must_use]
pub fn tag_word(text: &str) -> Option<&str> {
    tag_start(text).map(|hash| &text[hash + 1..])
}

/// Find's text without a tag being typed at its end: the words the list
/// is narrowed by, and what is left in Find once the tag is picked.
#[must_use]
pub fn words(text: &str) -> &str {
    tag_start(text).map_or(text, |hash| &text[..hash])
}

/// Whether the text is one tag being typed and nothing else: `"#sh"`.
/// Enter then picks the first tag offered instead of opening a note.
#[must_use]
pub fn is_lone_tag(text: &str) -> bool {
    tag_word(text).is_some() && words(text).trim().is_empty()
}

/// Whether the chosen tags fold into one token, `#work +2`, so that Find
/// stays one line with room for words (section 6.1).
///
/// `room` is how wide the tokens and the words may be together, in CSS
/// pixels. A token is as wide as its name in 13 px type and the cross
/// beside it; the words want more room once there are some.
#[must_use]
pub fn tokens_fold(tags: &[String], room: f64, has_words: bool) -> bool {
    // The same measure the mockup takes: 30 px of padding and cross, 7.6 px
    // a character, 6 px between two tokens.
    #[allow(clippy::cast_precision_loss)]
    let needed: f64 = tags
        .iter()
        .map(|tag| (tag.chars().count() as f64).mul_add(7.6, 36.0))
        .sum();
    let for_words = if has_words { 110.0 } else { 52.0 };
    tags.len() > 1 && needed > room - for_words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_being_typed_is_at_the_end_after_a_space() {
        assert_eq!(tag_word("#"), Some(""));
        assert_eq!(tag_word("milk #sh"), Some("sh"));
        assert_eq!(tag_word("milk #q3-plan"), Some("q3-plan"));
        assert_eq!(tag_word("milk"), None);
        assert_eq!(tag_word(""), None);
        // A `#` inside a word is part of the word.
        assert_eq!(tag_word("c#"), None);
        assert_eq!(tag_word("issue#12"), None);
        // Once a space follows, the tag is no longer being typed.
        assert_eq!(tag_word("#work milk"), None);
        // A character a tag cannot hold ends it too.
        assert_eq!(tag_word("#caf\u{e9}"), None);
    }

    #[test]
    fn the_words_are_what_comes_before_it() {
        assert_eq!(words("milk #sh"), "milk ");
        assert_eq!(words("#sh"), "");
        assert_eq!(words("milk and #honey more"), "milk and #honey more");
        assert!(is_lone_tag("#sh"));
        assert!(is_lone_tag(" #"));
        assert!(!is_lone_tag("milk #sh"));
        assert!(!is_lone_tag("milk"));
    }

    #[test]
    fn tags_fold_when_they_would_crowd_the_words() {
        let tags = |names: &[&str]| {
            names
                .iter()
                .map(|&name| name.to_owned())
                .collect::<Vec<_>>()
        };
        // One tag never folds: there is nothing to fold it into.
        assert!(!tokens_fold(
            &tags(&["a-very-long-tag-name-indeed"]),
            100.0,
            true
        ));
        assert!(!tokens_fold(&tags(&["work", "idea"]), 236.0, false));
        assert!(tokens_fold(
            &tags(&["work", "idea", "shopping"]),
            236.0,
            false
        ));
        // The same two fit beside an empty field and not beside words.
        assert!(!tokens_fold(&tags(&["cooking", "idea"]), 236.0, false));
        assert!(tokens_fold(&tags(&["cooking", "idea"]), 236.0, true));
    }
}
