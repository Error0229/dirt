//! Shared utility functions used across multiple modules.

/// Normalize optional text by trimming whitespace and removing empties.
///
/// Returns `None` when the input is `None` or the trimmed value is empty.
pub fn normalize_text_option(value: Option<String>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Check if a string starts with `http://` or `https://`.
pub fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

/// An error's text as a sentence to show a person: its first letter in
/// upper case and a full stop at the end. Error messages are written in
/// lower case with no full stop, as Rust's own are.
pub fn as_sentence(error: &impl std::fmt::Display) -> String {
    let text = error.to_string();
    let mut characters = text.trim().chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    let mut sentence: String = first.to_uppercase().chain(characters).collect();
    if !sentence.ends_with(['.', '!', '?']) {
        sentence.push('.');
    }
    sentence
}

/// Current Unix timestamp in seconds.
pub fn unix_timestamp_now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_is_shown_as_a_sentence() {
        assert_eq!(
            as_sentence(&"sign-in was cancelled"),
            "Sign-in was cancelled."
        );
        assert_eq!(as_sentence(&"Already a sentence."), "Already a sentence.");
        assert_eq!(as_sentence(&"  really?  "), "Really?");
        assert_eq!(as_sentence(&""), "");
    }

    #[test]
    fn normalize_text_option_rejects_empty() {
        assert_eq!(normalize_text_option(None), None);
        assert_eq!(normalize_text_option(Some("   ".to_string())), None);
    }

    #[test]
    fn normalize_text_option_trims_value() {
        assert_eq!(
            normalize_text_option(Some(" https://example.com ".to_string())),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn is_http_url_accepts_valid_schemes() {
        assert!(is_http_url("http://localhost"));
        assert!(is_http_url("https://example.com"));
        assert!(!is_http_url("ftp://example.com"));
        assert!(!is_http_url("example.com"));
    }
}
