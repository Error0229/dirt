//! Launch argument parsing for mobile quick-capture and share-intent flows.

const QUICK_CAPTURE_FLAG: &str = "--quick-capture";
const SHARE_TEXT_FLAG: &str = "--share-text";

#[cfg(target_os = "android")]
const QUICK_CAPTURE_ENV_ENABLED: &str = "DIRT_QUICK_CAPTURE";
#[cfg(target_os = "android")]
const QUICK_CAPTURE_ENV_CONTENT: &str = "DIRT_QUICK_CAPTURE_CONTENT";
#[cfg(target_os = "android")]
const SHARE_TEXT_ENV_CONTENT: &str = "DIRT_SHARE_TEXT";

/// Parsed quick-capture launch state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickCaptureLaunch {
    /// Whether quick-capture mode should be opened.
    pub enabled: bool,
    /// Optional seeded content to prefill capture text.
    pub seed_text: Option<String>,
}

/// Parsed launch intent state for the mobile app.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchIntent {
    /// Quick-capture mode metadata.
    pub quick_capture: QuickCaptureLaunch,
    /// Shared text payload from share-intent style launches.
    pub share_text: Option<String>,
}

/// Detect launch intent settings from process arguments and environment.
#[cfg(target_os = "android")]
pub fn detect_launch_intent_from_runtime() -> LaunchIntent {
    let args: Vec<String> = std::env::args().collect();
    let env_quick_content = std::env::var(QUICK_CAPTURE_ENV_CONTENT).ok();
    let env_quick_enabled = std::env::var(QUICK_CAPTURE_ENV_ENABLED).ok();
    let env_share_text = std::env::var(SHARE_TEXT_ENV_CONTENT).ok();
    parse_launch_intent(
        args.iter().map(std::string::String::as_str),
        env_quick_content.as_deref(),
        env_quick_enabled.as_deref(),
        env_share_text.as_deref(),
    )
}

/// Parse full launch intent state from explicit args/env inputs.
pub fn parse_launch_intent<'a>(
    args: impl IntoIterator<Item = &'a str>,
    env_quick_content: Option<&str>,
    env_quick_enabled: Option<&str>,
    env_share_text: Option<&str>,
) -> LaunchIntent {
    let args: Vec<&str> = args.into_iter().collect();

    let quick_capture =
        parse_quick_capture_launch(args.iter().copied(), env_quick_content, env_quick_enabled);
    let share_text = parse_share_text(args.iter().copied(), env_share_text);

    LaunchIntent {
        quick_capture,
        share_text,
    }
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

fn parse_share_text<'a>(
    args: impl IntoIterator<Item = &'a str>,
    env_content: Option<&str>,
) -> Option<String> {
    let mut from_args: Option<String> = None;

    let mut iter = args.into_iter().peekable();
    // Skip executable path.
    _ = iter.next();

    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--share-text=") {
            from_args = normalize_text(value);
            continue;
        }

        if arg == SHARE_TEXT_FLAG {
            let next = iter.peek().copied().unwrap_or_default();
            if !next.is_empty() && !next.starts_with("--") {
                _ = iter.next();
                from_args = normalize_text(next);
            }
        }
    }

    from_args.or_else(|| env_content.and_then(normalize_text))
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
    fn parse_quick_capture_with_next_argument() {
        let parsed = parse_quick_capture_launch(
            ["dirt-mobile", "--quick-capture", "capture this"],
            None,
            None,
        );

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text.as_deref(), Some("capture this"));
    }

    #[test]
    fn parse_quick_capture_with_equals_argument() {
        let parsed =
            parse_quick_capture_launch(["dirt-mobile", "--quick-capture=seed note"], None, None);

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text.as_deref(), Some("seed note"));
    }

    #[test]
    fn parse_quick_capture_without_payload_still_enables_mode() {
        let parsed = parse_quick_capture_launch(["dirt-mobile", "--quick-capture"], None, None);

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text, None);
    }

    #[test]
    fn parse_quick_capture_uses_env_payload_as_fallback() {
        let parsed = parse_quick_capture_launch(["dirt-mobile"], Some(" from env "), None);

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text.as_deref(), Some("from env"));
    }

    #[test]
    fn parse_quick_capture_env_enabled_without_payload() {
        let parsed = parse_quick_capture_launch(["dirt-mobile"], None, Some("true"));

        assert!(parsed.enabled);
        assert_eq!(parsed.seed_text, None);
    }

    #[test]
    fn parse_share_text_with_flag_argument() {
        let parsed = parse_launch_intent(
            ["dirt-mobile", "--share-text", "shared content"],
            None,
            None,
            None,
        );

        assert_eq!(parsed.share_text.as_deref(), Some("shared content"));
    }

    #[test]
    fn parse_share_text_with_equals_argument() {
        let parsed = parse_launch_intent(
            ["dirt-mobile", "--share-text=shared content"],
            None,
            None,
            None,
        );

        assert_eq!(parsed.share_text.as_deref(), Some("shared content"));
    }

    #[test]
    fn parse_share_text_uses_env_payload_as_fallback() {
        let parsed = parse_launch_intent(["dirt-mobile"], None, None, Some(" from share env "));

        assert_eq!(parsed.share_text.as_deref(), Some("from share env"));
    }

    #[test]
    fn parse_launch_intent_includes_both_share_and_quick_capture() {
        let parsed = parse_launch_intent(
            [
                "dirt-mobile",
                "--quick-capture",
                "quick text",
                "--share-text",
                "shared text",
            ],
            None,
            None,
            None,
        );

        assert!(parsed.quick_capture.enabled);
        assert_eq!(
            parsed.quick_capture.seed_text.as_deref(),
            Some("quick text")
        );
        assert_eq!(parsed.share_text.as_deref(), Some("shared text"));
    }
}
