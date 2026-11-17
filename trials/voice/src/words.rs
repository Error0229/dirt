//! Comparing words the way a reader would: case and punctuation aside.

/// A word as it is compared: lower case, letters, digits and apostrophes only.
pub fn plain(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .flat_map(char::to_lowercase)
        .collect()
}

/// How many words two hypotheses share from the start.
pub fn common_prefix(a: &[String], b: &[String]) -> usize {
    a.iter()
        .zip(b)
        .take_while(|(x, y)| plain(x) == plain(y))
        .count()
}
