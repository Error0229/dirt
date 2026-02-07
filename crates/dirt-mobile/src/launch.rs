//! Launch argument parsing for widget-style quick capture entry.

const QUICK_CAPTURE_FLAG: &str = "--quick-capture";
#[cfg(target_os = "android")]
const QUICK_CAPTURE_ENV_ENABLED: &str = "DIRT_QUICK_CAPTURE";
#[cfg(target_os = "android")]
const QUICK_CAPTURE_ENV_CONTENT: &str = "DIRT_QUICK_CAPTURE_CONTENT";

/// Parsed quick-capture launch state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickCaptureLaunch {
    /// Whether quick-capture mode should be opened.
    pub enabled: bool,
    /// Optional seeded content to prefill capture text.
    pub seed_text: Option<String>,
}

/// Detect quick-capture launch settings from process arguments and environment.
#[cfg(target_os = "android")]
pub fn detect_quick_capture_launch_from_runtime() -> QuickCaptureLaunch {
    let args: Vec<String> = std::env::args().collect();
    let env_content = std::env::var(QUICK_CAPTURE_ENV_CONTENT).ok();
    let env_enabled = std::env::var(QUICK_CAPTURE_ENV_ENABLED).ok();
    parse_quick_capture_launch(
        args.iter().map(std::string::String::as_str),
        env_content.as_deref(),
        env_enabled.as_deref(),
    )
}

/// Parse quick-capture launch state from explicit args/env inputs.
pub fn parse_quick_capture_launch<'a>(
    args: impl IntoIterator<Item = &'a str>,
    env_content: Option<&str>,
    env_enabled: Option<&str>,
) -> QuickCaptureLaunch {
    let mut enabled = false;
    let mut from_args: Option<String> = None;

    let mut iter = args.into_iter().peekable();
    // Skip executable path.
    _ = iter.next();

    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--quick-capture=") {
            enabled = true;
            from_args = normalize_text(value);
            continue;
        }

        if arg == QUICK_CAPTURE_FLAG {
            enabled = true;
            let next = iter.peek().copied().unwrap_or_default();
            if !next.is_empty() && !next.starts_with("--") {
                _ = iter.next();
                from_args = normalize_text(next);
            }
        }
    }

    let env_enabled = env_enabled
        .map(str::trim)
        .is_some_and(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "YES"));
    let from_env = env_content.and_then(normalize_text);

    QuickCaptureLaunch {
        enabled: enabled || env_enabled || from_env.is_some(),
        seed_text: from_args.or(from_env),
    }
}

fn normalize_text(input: &str) -> Option<String> {
    let normalized = input.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flag_with_next_argument() {
        let parsed = parse_quick_capture_launch(
            ["dirt-mobile", "--quick-capture", "capture this"],
            None,
            None,
        );

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text.as_deref(), Some("capture this"));
    }

    #[test]
    fn parse_flag_with_equals_argument() {
        let parsed =
            parse_quick_capture_launch(["dirt-mobile", "--quick-capture=seed note"], None, None);

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text.as_deref(), Some("seed note"));
    }

    #[test]
    fn parse_flag_without_payload_still_enables_mode() {
        let parsed = parse_quick_capture_launch(["dirt-mobile", "--quick-capture"], None, None);

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text, None);
    }

    #[test]
    fn parse_uses_env_payload_as_fallback() {
        let parsed = parse_quick_capture_launch(["dirt-mobile"], Some(" from env "), None);

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text.as_deref(), Some("from env"));
    }

    #[test]
    fn parse_env_enabled_without_payload() {
        let parsed = parse_quick_capture_launch(["dirt-mobile"], None, Some("true"));

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text, None);
    }
}
