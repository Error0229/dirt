//! Outbound email for the magic-code flow.
//!
//! Two production modes:
//!
//!   - `Log` — write the magic code to `tracing::info!` and never leave
//!     the process. Used for local dev, CI, and any deploy where Resend
//!     credentials aren't configured. Picked deliberately at startup so
//!     "I'm not actually emailing this" is never an accident.
//!
//!   - `Resend` — POST to `https://api.resend.com/emails` with the
//!     `RESEND_API_KEY` Bearer. Picked when both `RESEND_API_KEY` and
//!     `RESEND_FROM_ADDRESS` are present in the environment.
//!
//! Plus a test-only `Capture` mode for the round-trip tests in
//! `routes_auth.rs`.
//!
//! Errors from `send_magic_code` propagate as `AppError::Internal` so
//! the request handler maps them onto a 500. The user can't fix a
//! Resend outage themselves — retrying `/v1/auth/request` is the right
//! prompt.

use std::time::Duration;

use serde::Serialize;

use crate::error::AppError;

#[cfg(test)]
use std::sync::{Arc, Mutex};

/// Default Resend API base URL. Lifted to a constant so tests can swap
/// it for a wiremock URL without monkey-patching DNS.
const RESEND_API_BASE: &str = "https://api.resend.com";

/// HTTP timeout for the Resend round-trip. Resend typically responds in
/// well under a second; a 10 s ceiling keeps a stuck `/v1/auth/request`
/// from hanging the whole router. Long enough that a one-off TCP retry
/// fits underneath, short enough that a wedged DNS doesn't pile up
/// in-flight requests.
const RESEND_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Default subject line for the magic-code email. Hardcoded for now —
/// when we have a real product brand and copy review, this graduates
/// to a config knob; until then a single string keeps the surface tiny.
const RESEND_SUBJECT: &str = "Your Dirt sign-in code";

/// Sender mode picked at startup. Kept private — callers go through
/// `EmailSender::send_magic_code`, not the variant directly.
enum Mode {
    /// Print the code to the server log instead of sending email.
    /// The right default for `cargo run` and tests; explicit because
    /// "I'm not actually emailing this" should be a deliberate choice.
    Log,
    /// Send via the Resend HTTPS API. `from` is the verified sender
    /// (e.g. `Dirt <noreply@catjam.dev>`); `base_url` is normally the
    /// real Resend URL but is overrideable so wiremock-based tests can
    /// point at a local mock without touching the public API.
    Resend(ResendConfig),
    /// Test-only: stash every (email, code) pair into a shared `Vec`
    /// the test can inspect. Lets the round-trip test parse the actual
    /// code that `/v1/auth/request` minted, instead of seeding a
    /// parallel one and pretending.
    #[cfg(test)]
    Capture(CapturedSends),
    /// Test-only: every send returns `AppError::Internal`. Used by the
    /// route-level test that proves `request_magic_code` rolls back the
    /// just-inserted DB row when the downstream send fails. The real
    /// production failure path is the Resend HTTPS branch; that's
    /// covered by the wiremock 4xx/5xx tests.
    #[cfg(test)]
    Failing,
}

struct ResendConfig {
    client: reqwest::Client,
    api_key: String,
    from: String,
    base_url: String,
}

#[cfg(test)]
pub type CapturedSends = Arc<Mutex<Vec<(String, String)>>>;

pub struct EmailSender {
    mode: Mode,
}

/// Wire format for the Resend POST body. Documented at
/// <https://resend.com/docs/api-reference/emails/send-email>.
#[derive(Serialize)]
struct ResendRequest<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
}

impl EmailSender {
    /// Always-log sender. The constructor every test reaches for.
    #[must_use]
    pub const fn log_only() -> Self {
        Self { mode: Mode::Log }
    }

    /// Build a Resend-backed sender. `from` is the verified sender
    /// (Resend rejects `From` addresses on unverified domains with a
    /// 403). The `reqwest::Client` is built once and reused for every
    /// send so connection-pooling actually kicks in.
    ///
    /// Returns `AppError::config` if the timeout-armed client builder
    /// can't be constructed. In practice that only happens on a host
    /// without TLS support, which would also fail the deploy long
    /// before this is called — but propagating is cleaner than panicking.
    pub fn resend(api_key: String, from: String) -> Result<Self, AppError> {
        Self::resend_with_base_url(api_key, from, RESEND_API_BASE.to_string())
    }

    /// Same as [`resend`](Self::resend) but lets callers override the API
    /// base URL. `pub(crate)` because this is a test seam — the wiremock
    /// tests need to point the sender at a local mock, and we don't
    /// want downstream crates building a production `EmailSender`
    /// aimed at an arbitrary URL.
    pub(crate) fn resend_with_base_url(
        api_key: String,
        from: String,
        base_url: String,
    ) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .timeout(RESEND_REQUEST_TIMEOUT)
            .build()
            .map_err(|err| {
                AppError::config(format!("failed to build Resend HTTP client: {err}"))
            })?;
        Ok(Self {
            mode: Mode::Resend(ResendConfig {
                client,
                api_key,
                from,
                base_url,
            }),
        })
    }

    /// Test-only: build a sender that records every send into the
    /// returned `Arc<Mutex<Vec>>`, alongside the sender itself.
    #[cfg(test)]
    pub fn capture() -> (Self, CapturedSends) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                mode: Mode::Capture(captured.clone()),
            },
            captured,
        )
    }

    /// Test-only: a sender whose every `send_magic_code` call resolves
    /// to `AppError::Internal`. Lets the route-level test cover the
    /// rollback branch in `request_magic_code` without standing up a
    /// wiremock server in the same test.
    #[cfg(test)]
    pub(crate) const fn always_failing() -> Self {
        Self {
            mode: Mode::Failing,
        }
    }

    /// Pick a mode from the process environment.
    ///
    /// Selection rules:
    ///   - Both `RESEND_API_KEY` and `RESEND_FROM_ADDRESS` set → Resend.
    ///   - Neither set → Log (the dev/CI default).
    ///   - Exactly one set → `AppError::config`. Half-configured Resend
    ///     is almost certainly a deploy mistake; silently falling back
    ///     to Log would put real magic codes into log aggregators
    ///     instead of users' inboxes, which is worse than failing fast.
    pub fn from_env() -> Result<Self, AppError> {
        let api_key = trimmed_env("RESEND_API_KEY");
        let from = trimmed_env("RESEND_FROM_ADDRESS");
        match (api_key, from) {
            (Some(api_key), Some(from)) => Self::resend(api_key, from),
            (None, None) => Ok(Self::log_only()),
            (Some(_), None) => Err(AppError::config(
                "RESEND_API_KEY is set but RESEND_FROM_ADDRESS is not — set both to enable Resend, or neither to keep Log mode",
            )),
            (None, Some(_)) => Err(AppError::config(
                "RESEND_FROM_ADDRESS is set but RESEND_API_KEY is not — set both to enable Resend, or neither to keep Log mode",
            )),
        }
    }

    /// Deliver a magic code to `email`. Errors here propagate as
    /// `AppError::Internal` so the request handler maps them onto a 500
    /// — the user can't fix a Resend outage themselves, retrying the
    /// `/v1/auth/request` is the right user-facing prompt.
    ///
    /// `expires_in_minutes` is forwarded to the rendered body so the
    /// "expires in N minutes" copy can't drift from the actual code TTL.
    /// Caller (in `routes_auth`) computes it from `CODE_TTL_MS`, which is
    /// the single source of truth for the magic-code lifetime.
    pub async fn send_magic_code(
        &self,
        email: &str,
        code: &str,
        expires_in_minutes: i64,
    ) -> Result<(), AppError> {
        match &self.mode {
            Mode::Log => {
                // The "we tried to email you" record stays at info for
                // observability; the raw code drops to debug so a
                // RUST_LOG=info deploy doesn't write live magic codes
                // into a log aggregator. RUST_LOG=debug is the explicit
                // opt-in for "I want to see codes in the dev console."
                tracing::info!(target: "dirt_api::email", "[dev email] to={email}");
                tracing::debug!(target: "dirt_api::email", "[dev email] to={email} code={code}");
                Ok(())
            }
            Mode::Resend(cfg) => send_via_resend(cfg, email, code, expires_in_minutes).await,
            #[cfg(test)]
            Mode::Capture(captured) => {
                captured
                    .lock()
                    .expect("CapturedSends mutex poisoned")
                    .push((email.to_string(), code.to_string()));
                Ok(())
            }
            #[cfg(test)]
            Mode::Failing => Err(AppError::internal(
                "test-only forced send failure for rollback coverage",
            )),
        }
    }
}

impl Default for EmailSender {
    fn default() -> Self {
        Self::log_only()
    }
}

/// Body text for the magic-code email. Centralised so the tests can
/// assert on the same string the production code produces, and so a
/// future copy edit only touches one place.
///
/// `expires_in_minutes` is threaded through from the caller (which knows
/// `CODE_TTL_MS`) instead of being hardcoded — without that, a future
/// edit to the TTL constant would silently leave the email body
/// promising the old number.
fn render_magic_code_body(code: &str, expires_in_minutes: i64) -> String {
    format!(
        "Your Dirt sign-in code is: {code}\n\n\
         It expires in {expires_in_minutes} minutes. \
         If you didn't request this code, you can ignore this email.\n"
    )
}

async fn send_via_resend(
    cfg: &ResendConfig,
    email: &str,
    code: &str,
    expires_in_minutes: i64,
) -> Result<(), AppError> {
    let body = render_magic_code_body(code, expires_in_minutes);
    let payload = ResendRequest {
        from: &cfg.from,
        to: [email],
        subject: RESEND_SUBJECT,
        text: &body,
    };
    let url = format!("{}/emails", cfg.base_url.trim_end_matches('/'));

    let response = cfg
        .client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|err| {
            // Network errors, DNS failures, timeout — none of these are
            // user-fixable. Log the full error server-side; surface a
            // generic "we couldn't send" so the response body never
            // echoes Resend's internals back to a probing client.
            tracing::warn!(target: "dirt_api::email", "Resend send failed: {err}");
            AppError::internal("failed to dispatch magic-code email")
        })?;

    let status = response.status();
    if status.is_success() {
        // The email-id is useful in logs (Resend's dashboard keys off
        // it) but never hits the response — the user already knows
        // they asked for an email.
        //
        // `response.text()` reads the whole body into memory. Resend's
        // documented response shape is `{"id": "..."}` — well under a
        // KiB — so the implicit bound is fine in practice; a hostile
        // upstream blasting a multi-MB body would already be the least
        // of our problems given Vercel's ~6 MB function-response cap.
        //
        // On a body-stream read error we keep the diagnostic in the log
        // line itself rather than collapsing to an empty `body=`, so the
        // operator can tell "Resend hung up mid-stream" from "Resend
        // genuinely returned an empty body".
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|err| format!("<body read error: {err}>"));
        tracing::info!(
            target: "dirt_api::email",
            "Resend accepted magic-code email status={status} body={body_text}"
        );
        return Ok(());
    }

    // Read what Resend told us so the server log makes the deploy
    // operator's life easier — but classify the failure on status only,
    // not on body. A 401 here is almost always a stale `RESEND_API_KEY`;
    // a 403 is an unverified `from`; a 5xx is Resend's problem.
    //
    // The client-facing string is intentionally generic and identical
    // to the network-error path above. `AppError::Internal(msg)` is
    // serialised verbatim into the JSON body's `message` and `cause`
    // fields, so embedding the Resend status here would tell a probing
    // client (a) that the backend is Resend, and (b) which kind of
    // failure (401 vs 403 vs 5xx). The detail stays in the warn-level
    // server log only.
    // Same diagnostic-on-read-error treatment as the success branch:
    // an empty `body=` would make a stream-read error indistinguishable
    // from a genuinely-empty Resend response, and the error path is
    // exactly where the body content is most useful (it's what Resend
    // told us about the 401/403/5xx).
    let body_text = response
        .text()
        .await
        .unwrap_or_else(|err| format!("<body read error: {err}>"));
    tracing::warn!(
        target: "dirt_api::email",
        "Resend rejected magic-code email status={status} body={body_text}"
    );
    Err(AppError::internal("failed to dispatch magic-code email"))
}

fn trimmed_env(key: &str) -> Option<String> {
    let raw = std::env::var(key).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use wiremock::matchers::{body_json, header, header_regex, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Serialises the `from_env_*` tests, which mutate process-wide
    /// `RESEND_*` env vars. Without this, `cargo test --all` on Linux
    /// (where CI doesn't pin `RUST_TEST_THREADS=1`) lets these tests
    /// race each other and observe inconsistent state. Held as a
    /// `MutexGuard` for the body of each env test.
    ///
    /// Poison handling: a panicking test poisons the lock, but we don't
    /// care — the next test will see a fresh `EnvGuard::clear` call
    /// regardless, and propagating the poison would just mask the real
    /// failure further down the test list. `into_inner` recovers the
    /// guard.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_mode_send_succeeds() {
        let sender = EmailSender::log_only();
        sender
            .send_magic_code("user@example.com", "123456", 15)
            .await
            .unwrap();
    }

    /// Snapshot the rendered body so a typo or format-string change is
    /// caught here, not silently in the wiremock test (which feeds the
    /// same `render_magic_code_body` output into both the mock matcher
    /// and the production payload — circular by construction).
    #[test]
    fn render_magic_code_body_snapshot() {
        let body = render_magic_code_body("123456", 15);
        assert_eq!(
            body,
            "Your Dirt sign-in code is: 123456\n\n\
             It expires in 15 minutes. \
             If you didn't request this code, you can ignore this email.\n"
        );
    }

    /// `expires_in_minutes` actually reaches the rendered body — the
    /// reason we plumbed the parameter through in the first place
    /// (see `render_magic_code_body` doc comment about TTL drift).
    #[test]
    fn render_magic_code_body_uses_ttl_argument() {
        assert!(render_magic_code_body("000000", 7).contains("expires in 7 minutes"));
        assert!(render_magic_code_body("000000", 30).contains("expires in 30 minutes"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resend_mode_posts_expected_payload_and_headers() {
        let mock = MockServer::start().await;
        let expected_body = render_magic_code_body("123456", 15);

        Mock::given(method("POST"))
            .and(path("/emails"))
            .and(header("authorization", "Bearer rs_test_key"))
            // Regex on content-type so a future reqwest release that
            // appends `; charset=utf-8` doesn't break the test without
            // any production-code change. The `body_json` matcher
            // below already proves the body is valid JSON.
            .and(header_regex("content-type", "^application/json"))
            .and(body_json(serde_json::json!({
                "from": "Dirt <noreply@catjam.dev>",
                "to": ["user@example.com"],
                "subject": RESEND_SUBJECT,
                "text": expected_body,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "email_test_123"
            })))
            .expect(1)
            .mount(&mock)
            .await;

        let sender = EmailSender::resend_with_base_url(
            "rs_test_key".into(),
            "Dirt <noreply@catjam.dev>".into(),
            mock.uri(),
        )
        .unwrap();

        sender
            .send_magic_code("user@example.com", "123456", 15)
            .await
            .unwrap();
        // wiremock's drop-time `.expect(1)` assertion fails the test if
        // the mock wasn't hit exactly once.
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resend_5xx_maps_to_internal_error() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/emails"))
            .respond_with(ResponseTemplate::new(503).set_body_string("upstream busy"))
            .mount(&mock)
            .await;

        let sender = EmailSender::resend_with_base_url(
            "rs_test_key".into(),
            "Dirt <noreply@catjam.dev>".into(),
            mock.uri(),
        )
        .unwrap();

        let err = sender
            .send_magic_code("user@example.com", "123456", 15)
            .await
            .expect_err("503 must surface as AppError");
        assert!(
            matches!(err, AppError::Internal(_)),
            "expected Internal, got {err:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resend_4xx_maps_to_internal_error() {
        // 401 (bad API key) and 403 (unverified `from`) are deploy-time
        // misconfigurations from the user's perspective they're still
        // "the email didn't go out, retry later"; we don't want to leak
        // the distinction back to the client.
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/emails"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "name": "validation_error",
                "message": "Invalid `api_key`",
            })))
            .mount(&mock)
            .await;

        let sender = EmailSender::resend_with_base_url(
            "rs_bad_key".into(),
            "Dirt <noreply@catjam.dev>".into(),
            mock.uri(),
        )
        .unwrap();

        let err = sender
            .send_magic_code("user@example.com", "123456", 15)
            .await
            .expect_err("401 must surface as AppError");
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resend_with_trailing_slash_base_url_does_not_double_up_path() {
        // `resend_with_base_url` lets a caller override the API base.
        // If they (or someone copying the URL from the Resend docs)
        // leave a trailing slash, path joining must still produce
        // `/emails`, not `//emails`. Production callers go through
        // `resend()` which uses `RESEND_API_BASE` verbatim, so this
        // only protects against the test-seam misuse — but the
        // `trim_end_matches('/')` in `send_via_resend` is the line we
        // care about, and this test is the one that catches its
        // removal.
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/emails"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "email_test_trailing"
            })))
            .expect(1)
            .mount(&mock)
            .await;

        let sender = EmailSender::resend_with_base_url(
            "rs_test_key".into(),
            "Dirt <noreply@catjam.dev>".into(),
            format!("{}/", mock.uri()),
        )
        .unwrap();

        sender
            .send_magic_code("user@example.com", "123456", 15)
            .await
            .unwrap();
    }

    /// `from_env` defaults to Log when neither var is set. Each env
    /// test grabs `lock_env()` first so concurrent tests on Linux CI
    /// (which doesn't pin `RUST_TEST_THREADS=1`) can't observe each
    /// other's set/clear sequences. `EnvGuard` then restores prior
    /// values on drop so the rest of the suite isn't disturbed.
    #[test]
    fn from_env_defaults_to_log_when_neither_var_set() {
        let _env_lock = lock_env();
        let _guard = EnvGuard::clear(&["RESEND_API_KEY", "RESEND_FROM_ADDRESS"]);
        let sender = EmailSender::from_env().unwrap();
        assert!(matches!(sender.mode, Mode::Log));
    }

    #[test]
    fn from_env_picks_resend_when_both_set() {
        let _env_lock = lock_env();
        let _guard = EnvGuard::set(&[
            ("RESEND_API_KEY", "rs_test_key"),
            ("RESEND_FROM_ADDRESS", "Dirt <noreply@catjam.dev>"),
        ]);
        let sender = EmailSender::from_env().unwrap();
        // Inspect the inner config so an accidental argument-swap (e.g.
        // `Self::resend(from, api_key)`) or trimmed_env-key swap fails
        // here instead of slipping past on `matches!(_, Mode::Resend(_))`.
        let Mode::Resend(cfg) = &sender.mode else {
            panic!("expected Mode::Resend, got something else");
        };
        assert_eq!(cfg.api_key, "rs_test_key");
        assert_eq!(cfg.from, "Dirt <noreply@catjam.dev>");
        assert_eq!(cfg.base_url, RESEND_API_BASE);
    }

    #[test]
    fn from_env_errors_when_only_api_key_set() {
        let _env_lock = lock_env();
        let _guard = EnvGuard::set(&[("RESEND_API_KEY", "rs_test_key")])
            .extend_clear(&["RESEND_FROM_ADDRESS"]);
        // `EmailSender` deliberately does not implement Debug (the
        // Resend API key would land in panic output otherwise) so we
        // can't use `.unwrap_err()` here — match on the result instead.
        match EmailSender::from_env() {
            Err(AppError::Config(_)) => {}
            Err(other) => panic!("expected AppError::Config, got {other:?}"),
            Ok(_) => panic!("expected an error, got Ok"),
        }
    }

    #[test]
    fn from_env_errors_when_only_from_set() {
        let _env_lock = lock_env();
        let _guard = EnvGuard::set(&[("RESEND_FROM_ADDRESS", "Dirt <noreply@catjam.dev>")])
            .extend_clear(&["RESEND_API_KEY"]);
        match EmailSender::from_env() {
            Err(AppError::Config(_)) => {}
            Err(other) => panic!("expected AppError::Config, got {other:?}"),
            Ok(_) => panic!("expected an error, got Ok"),
        }
    }

    /// Trimming sanity check — empty/whitespace `RESEND_API_KEY` is
    /// treated as unset, matching the `AppConfig::from_env` precedent
    /// for `TURSO_AUTH_TOKEN`.
    #[test]
    fn from_env_treats_whitespace_api_key_as_unset() {
        let _env_lock = lock_env();
        let _guard = EnvGuard::set(&[
            ("RESEND_API_KEY", "   "),
            ("RESEND_FROM_ADDRESS", "Dirt <noreply@catjam.dev>"),
        ]);
        // Whitespace key is treated as unset, so we end up in the
        // "only from is set" error arm — *not* a successful Log
        // fallback. That's the strict-no-silent-fallback contract.
        match EmailSender::from_env() {
            Err(AppError::Config(_)) => {}
            Err(other) => panic!("expected AppError::Config, got {other:?}"),
            Ok(_) => panic!("expected an error, got Ok"),
        }
    }

    /// Symmetric counterpart to `from_env_treats_whitespace_api_key_as_unset`.
    /// `trimmed_env` handles both keys identically, but without this
    /// test a future refactor could break one side without breaking
    /// CI.
    #[test]
    fn from_env_treats_whitespace_from_address_as_unset() {
        let _env_lock = lock_env();
        let _guard = EnvGuard::set(&[
            ("RESEND_API_KEY", "rs_test_key"),
            ("RESEND_FROM_ADDRESS", "   "),
        ]);
        match EmailSender::from_env() {
            Err(AppError::Config(_)) => {}
            Err(other) => panic!("expected AppError::Config, got {other:?}"),
            Ok(_) => panic!("expected an error, got Ok"),
        }
    }

    /// RAII guard for env-var mutation. Restores prior values on drop
    /// so a test's set/clear doesn't bleed into the rest of the suite.
    /// Cross-test mutual exclusion is enforced separately via
    /// `lock_env` / `ENV_LOCK`; `EnvGuard` itself is not thread-safe
    /// and assumes the caller holds the env lock.
    struct EnvGuard {
        snapshots: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn clear(keys: &[&str]) -> Self {
            let snapshots = keys
                .iter()
                .map(|k| {
                    let prior = std::env::var(k).ok();
                    std::env::remove_var(k);
                    ((*k).to_string(), prior)
                })
                .collect();
            Self { snapshots }
        }

        fn set(pairs: &[(&str, &str)]) -> Self {
            let snapshots = pairs
                .iter()
                .map(|(k, v)| {
                    let prior = std::env::var(k).ok();
                    std::env::set_var(k, v);
                    ((*k).to_string(), prior)
                })
                .collect();
            Self { snapshots }
        }

        fn extend_clear(mut self, keys: &[&str]) -> Self {
            for k in keys {
                let prior = std::env::var(k).ok();
                std::env::remove_var(k);
                self.snapshots.push(((*k).to_string(), prior));
            }
            self
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, prior) in self.snapshots.drain(..) {
                match prior {
                    Some(v) => std::env::set_var(&k, v),
                    None => std::env::remove_var(&k),
                }
            }
        }
    }
}
