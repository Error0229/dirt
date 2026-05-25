//! `dirt auth` — magic-link auth commands.
//!
//! All four subcommands (`status` / `login` / `logout` and any future
//! additions) route through [`dirt_core::auth::AuthClient`] +
//! [`dirt_core::auth::KeyringTokenStore`]. Phase 2.8 finished the cutover:
//! `status` now probes with the keyring-stored session token and shares
//! the silent-refresh path with `dirt sync`, so the CLI has no remaining
//! dependency on `DIRT_CLIENT_TOKEN`.

use std::env;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

use dirt_core::auth::{AuthClient, AuthError, StoredToken, TokenStore, TokenStoreError};
// `KeyringTokenStore` is target-gated in dirt-core to non-Android
// (Android has no keyring backend; dirt-mobile uses its own AndroidKeyStore
// impl in Phase 2.7). Mirror the gate here so an Android cross-compile of
// dirt-cli fails at the `build_keyring_store` call site with a clear,
// localized error rather than a confusing "unresolved import" originating
// from inside the dirt-core auth module.
#[cfg(not(target_os = "android"))]
use dirt_core::auth::KeyringTokenStore;
use dirt_core::sync::api_client::ApiClientError;
use dirt_core::sync::session_client::{SessionApiClient, SessionRefreshError};

use crate::cli::AuthCommands;
use crate::config_profiles::{normalize_text_option, CliProfilesConfig};
use crate::error::CliError;

/// Reverse-DNS service identifier the keyring slot is filed under. Shows
/// up verbatim in Keychain Access / Credential Manager / `seahorse` so
/// a user looking for "where does dirt store its session" can find it.
///
/// Pub-in-crate so `dirt sync` and `dirt auth status` reach the same
/// slot — the three commands MUST hit identical `(service, account)`
/// keyring coordinates or a login from one command leaves the others
/// stranded.
pub const KEYRING_SERVICE: &str = "dev.dirt.session";

pub async fn run_auth(command: AuthCommands) -> Result<(), CliError> {
    match command {
        AuthCommands::Status => {
            let line = status_line().await?;
            println!("{line}");
            Ok(())
        }
        AuthCommands::Login => {
            let base_url = require_api_base_url()?;
            let client = build_auth_client(&base_url)?;
            let store = build_keyring_store()?;
            login_flow(&client, &store, stdin_read_line).await
        }
        AuthCommands::Logout => {
            let store = build_keyring_store()?;
            // Defer building the AuthClient (and the
            // `require_api_base_url` failure path) until we know the
            // store actually has a token to revoke. A user on a fresh
            // machine without a configured profile must still be able
            // to run `dirt auth logout` and get "Not signed in"
            // instead of "DIRT_API_BASE_URL not set".
            dispatch_logout(&store, || {
                let base_url = require_api_base_url()?;
                build_auth_client(&base_url)
            })
            .await
        }
    }
}

/// Compute the user-facing status line. Returned (not printed) so tests
/// can assert exact strings.
///
/// Reads the keyring-stored session token planted by `dirt auth login`
/// and probes the API with it. Shares the silent-refresh path with
/// `dirt sync` so a freshly rotated session keeps reporting `online`
/// without nudging the user.
///
/// Output forms (exact prefixes are part of the contract — scripts grep
/// for `^online:` to detect a healthy state):
///
///   - "offline: `DIRT_API_BASE_URL` not set — …"
///   - "offline: not signed in — run `dirt auth login` to sign in"
///   - "offline: session expired — run `dirt auth login` …"
///   - "offline: \<cause\>; \<fix\>"
///   - "online: signed in as \<email\>, server ok"
///   - "online: signed in as \<email\>, server ok (session rotated)"
pub async fn status_line() -> Result<String, CliError> {
    let Some(api_base_url) = resolve_api_base_url()? else {
        return Ok(
            "offline: DIRT_API_BASE_URL not set — local capture works, sync disabled".to_string(),
        );
    };

    let store: Arc<dyn TokenStore> = Arc::new(build_keyring_store()?);

    // Load once to extract the email for the diagnostic line. The
    // SessionApiClient::from_store call below performs its own
    // store.load() — two reads against the same keyring slot are
    // cheap on every platform we ship (Credential Manager / Secret
    // Service / Keychain all serve the second call from in-process
    // state once ACL has been granted in the first call).
    let stored = match store.load() {
        Ok(Some(stored)) => stored,
        Ok(None) => {
            return Ok("offline: not signed in — run `dirt auth login` to sign in".to_string())
        }
        Err(err) => {
            return Ok(format!(
                "offline: auth test failed — {}",
                describe_token_store_error(&err)
            ))
        }
    };

    Ok(probe_session_status(api_base_url, store, &stored.email).await)
}

/// Wire the session client up against `api_base_url` and probe with
/// `.pull(None, Some(1))`. Pulled out so it stays tractable to unit-test
/// against a wiremock server — the surrounding `status_line` is mostly
/// keyring-store plumbing that needs an integration test instead.
async fn probe_session_status(
    api_base_url: String,
    store: Arc<dyn TokenStore>,
    email: &str,
) -> String {
    let auth = match AuthClient::new(&api_base_url) {
        Ok(auth) => auth,
        Err(err) => {
            return format!("offline: auth test failed — invalid configuration: {err}");
        }
    };

    let session = match SessionApiClient::from_store(api_base_url, auth, store) {
        Ok(Some(session)) => session,
        // The earlier `store.load()` returned `Some`, so reaching `Ok(None)` here
        // means the keyring slot was cleared between the load and now —
        // treat it as a fresh not-signed-in.
        Ok(None) => return "offline: not signed in — run `dirt auth login` to sign in".to_string(),
        Err(err) => return format!("offline: auth test failed — {err}"),
    };

    // The pull endpoint is the cheapest authenticated probe — it
    // doesn't mutate anything and exercises the same bearer middleware
    // production traffic does.
    let api = session.current();
    match api.pull(None, Some(1)).await {
        Ok(_) => format!("online: signed in as {email}, server ok"),
        Err(ApiClientError::Unauthorized(_)) => {
            // The snapshot Arc isn't load-bearing for refresh — drop
            // early to free the stale ApiClient before pulling a fresh
            // snapshot post-refresh (mirrors desktop's pattern).
            drop(api);
            handle_status_unauthorized(&session, email).await
        }
        Err(err) => {
            let (cause, fix) = describe_api_error(&err);
            format!("offline: auth test failed — {cause}; {fix}")
        }
    }
}

/// Bearer was rejected mid-probe. Try one silent refresh and re-probe
/// so a long-running CLI session (e.g. shell prompt integration that
/// invokes `dirt auth status` every minute) doesn't flap to `offline:`
/// the first time the token rotates. Matches the policy in
/// `commands::sync::run_session_sync`.
async fn handle_status_unauthorized(session: &SessionApiClient, email: &str) -> String {
    match session.refresh().await {
        Ok(()) => {
            let api = session.current();
            match api.pull(None, Some(1)).await {
                Ok(_) => {
                    format!("online: signed in as {email}, server ok (session rotated)")
                }
                Err(err) => {
                    let (cause, fix) = describe_api_error(&err);
                    format!("offline: server rejected refreshed session — {cause}; {fix}")
                }
            }
        }
        Err(SessionRefreshError::SessionExpired(_) | SessionRefreshError::NoToken) => {
            "offline: session expired — run `dirt auth login` to sign in again".to_string()
        }
        Err(other) => format!("offline: session refresh failed — {other}; retry shortly"),
    }
}

/// Orchestrate the interactive login flow.
///
/// Lifted out of [`run_auth`] so it can be unit-tested against a
/// `MemoryTokenStore` + wiremock-backed `AuthClient` without touching
/// the OS keyring or real stdin. The `read_line` closure is the seam
/// for the prompts — production passes [`stdin_read_line`], tests pass
/// a `Vec`-backed closure.
async fn login_flow<R>(
    client: &AuthClient,
    store: &dyn TokenStore,
    mut read_line: R,
) -> Result<(), CliError>
where
    R: FnMut(&str) -> io::Result<String>,
{
    let email = read_line("Email: ").map_err(CliError::Io)?;
    if email.is_empty() {
        return Err(CliError::Auth(
            "email must not be empty; nothing was sent".to_string(),
        ));
    }

    let req = client
        .request_magic_code(&email)
        .await
        .map_err(|err| auth_error_to_cli(&err))?;
    println!("Sent a 6-digit code to {email}. Enter it below.");

    let code = read_line("Code: ").map_err(CliError::Io)?;
    if code.is_empty() {
        // The /v1/auth/request call has already succeeded by this
        // point and the user has a live code in their inbox — they
        // don't need a fresh email, they just need to re-run and
        // type the code. "resend" implied otherwise.
        return Err(CliError::Auth(
            "code must not be empty; check your email and run `dirt auth login` again to enter the code"
                .to_string(),
        ));
    }
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        // dirt-api always emits a 6-digit numeric code. Reject any
        // other shape locally before hitting /v1/auth/verify — the
        // server would reply `INVALID_CODE`, which (intentionally)
        // collapses "wrong code", "expired", "consumed", and
        // "wrong format" into one signal. A client-side check
        // separates "your fingers slipped" from "your code is dead"
        // and saves a wasted round-trip.
        return Err(CliError::Auth(
            "code must be exactly 6 digits (0-9); double-check the code from your email"
                .to_string(),
        ));
    }

    let verify = client
        .verify_magic_code(&req.request_id, &code)
        .await
        .map_err(|err| auth_error_to_cli(&err))?;

    let stored: StoredToken = verify.into();
    store
        .save(&stored)
        .map_err(|err| token_store_error_to_cli(&err))?;

    println!("Signed in as {}", stored.email);
    Ok(())
}

/// Top-level logout dispatcher. Peeks the token store before doing
/// anything else so callers that are not signed in never pay the
/// "build an `AuthClient`" cost — and, crucially, never trip the
/// `DIRT_API_BASE_URL`-not-configured error path for users who only
/// want to confirm/clear local state on a fresh machine.
///
/// `build_client` is invoked **only** when there is a token to revoke.
/// In production `run_auth` passes a closure that resolves the API
/// base URL and constructs an `AuthClient`; tests pass a closure that
/// either returns a wiremock-backed client or panics to assert the
/// no-op branch never builds one.
async fn dispatch_logout<F>(store: &dyn TokenStore, build_client: F) -> Result<(), CliError>
where
    F: FnOnce() -> Result<AuthClient, CliError>,
{
    let Some(stored) = store.load().map_err(|err| token_store_error_to_cli(&err))? else {
        println!("Not signed in");
        return Ok(());
    };
    let client = build_client()?;
    logout_flow(&client, store, stored).await
}

/// Orchestrate the logout flow given an already-loaded token.
///
/// `stored` is the `StoredToken` the dispatcher confirmed exists.
/// Threading it through avoids a second `store.load()` round-trip
/// (significant on macOS Keychain / Windows Credential Manager where
/// a locked keyring can prompt twice) and closes the TOCTOU gap where
/// a concurrent `dirt auth login` could replace the slot between
/// the dispatcher's existence check and this function's re-read.
///
/// Sequence: revoke the server-side session → clear the local slot.
/// Clearing only happens after the server-side revoke succeeds
/// (or short-circuits as `SessionExpired` inside `AuthClient`) — a
/// network failure leaves the local token intact so the user can retry
/// without orphaning a still-valid server-side session.
async fn logout_flow(
    client: &AuthClient,
    store: &dyn TokenStore,
    stored: StoredToken,
) -> Result<(), CliError> {
    // Any 401 `AuthClient::logout_session` surfaces means the server
    // considers this token invalid — clear the local slot regardless
    // of which 401 sub-code arrived:
    //
    //   * 401 with `error.code == "SESSION_EXPIRED"` → AuthClient
    //     short-circuits to `Ok(())` internally (server's preferred
    //     "already revoked" signal); caught by the Ok arm below.
    //   * 401 with any other parseable envelope (`MISSING_TOKEN`,
    //     `INVALID_TOKEN`, a future server code) → surfaces as
    //     `Err(SessionExpired(message))` and we still clear: the
    //     server could not validate the token, so it is effectively
    //     dead from this client's perspective.
    //   * 401 with an unparseable body (proxy-injected) → also
    //     surfaces as `Err(SessionExpired(body))` and same call:
    //     clear, force a fresh sign-in.
    //
    // The tradeoff this accepts: if the bearer header is missing
    // due to a code defect on our side, this clears a live
    // server-side session — but the next sign-in just rotates it,
    // and the alternative (leaving the local slot with a token the
    // server won't accept) is strictly worse.
    //
    // Real network/server failures (5xx, ServerUnavailable, Network)
    // fall through the catch-all arm and propagate, preserving the
    // local slot for retry.
    match client.logout_session(&stored.session_token).await {
        Ok(()) | Err(AuthError::SessionExpired(_)) => {}
        Err(err) => return Err(auth_error_to_cli(&err)),
    }

    if let Err(err) = store.clear() {
        // The server-side revoke just succeeded; only the local slot
        // failed to clear. Surface that explicitly so the user knows
        // the next step: re-run `dirt auth logout` once the keyring
        // is reachable — the server will reply SESSION_EXPIRED (the
        // arm above), and store.clear() will get a second chance.
        // Without this framing the user sees a generic "keyring
        // unreachable" and may worry their session is still live.
        let cause = match &err {
            TokenStoreError::Backend(msg) => format!("keyring backend error: {msg}"),
            TokenStoreError::Serialize(msg) => format!("stored token is unreadable: {msg}"),
        };
        return Err(CliError::Auth(format!(
            "server revoke succeeded but local store clear failed: {cause}. \
             Re-run `dirt auth logout` once the keyring is reachable; \
             the server will reply SESSION_EXPIRED and the local slot \
             will be cleared then."
        )));
    }
    println!("Logged out");
    Ok(())
}

/// Resolve the dirt-api base URL the same way `status` does (env var
/// wins; profile config is the fallback) but propagate "not configured"
/// as an error instead of silently degrading — login is meaningless
/// without an endpoint to talk to.
fn require_api_base_url() -> Result<String, CliError> {
    resolve_api_base_url()?.ok_or_else(|| {
        CliError::Auth(
            "DIRT_API_BASE_URL not set and no profile is configured; \
             run `dirt config init --api-base-url <URL>` first"
                .to_string(),
        )
    })
}

fn build_auth_client(base_url: &str) -> Result<AuthClient, CliError> {
    AuthClient::new(base_url).map_err(|err| auth_error_to_cli(&err))
}

#[cfg(not(target_os = "android"))]
fn build_keyring_store() -> Result<KeyringTokenStore, CliError> {
    let profile = CliProfilesConfig::load()
        .map_err(CliError::Config)?
        .resolve_profile_name(None);
    Ok(KeyringTokenStore::new(KEYRING_SERVICE, profile))
}

fn resolve_api_base_url() -> Result<Option<String>, CliError> {
    if let Some(url) = normalize_text_option(env::var("DIRT_API_BASE_URL").ok()) {
        return Ok(Some(url));
    }

    let config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(None);
    let Some(profile) = config.profile(&profile_name) else {
        return Ok(None);
    };
    Ok(profile.dirt_api_base_url())
}

fn stdin_read_line(prompt: &str) -> io::Result<String> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    drop(stdout);

    let stdin = io::stdin();
    let mut buf = String::new();
    stdin.lock().read_line(&mut buf)?;
    // `read_line` keeps the trailing newline (and CRLF on Windows);
    // strip leading/trailing whitespace before handing to AuthClient
    // so users don't get `INVALID_EMAIL` for an invisible `\r`.
    Ok(buf.trim().to_string())
}

fn auth_error_to_cli(err: &AuthError) -> CliError {
    let (cause, fix) = describe_auth_error(err);
    CliError::Auth(format!("{cause}; {fix}"))
}

fn token_store_error_to_cli(err: &TokenStoreError) -> CliError {
    CliError::Auth(describe_token_store_error(err))
}

/// Build the `cause; fix` line for a `TokenStoreError`. Pulled out of
/// `token_store_error_to_cli` so `status_line` can fold the same copy
/// into its `offline:` envelope without going through `CliError`.
fn describe_token_store_error(err: &TokenStoreError) -> String {
    let (cause, fix) = match err {
        TokenStoreError::Backend(msg) => (
            format!("keyring backend error: {msg}"),
            "ensure the OS keyring is unlocked and accessible, then retry",
        ),
        TokenStoreError::Serialize(msg) => (
            // Almost always a schema drift across dirt releases: an
            // older / newer build wrote a `StoredToken` shape this
            // build can't deserialize. "sign in again with `dirt
            // auth login`" alone is misleading because dirt-cli's
            // own load would still hit the corrupt slot — the user
            // needs to clear the keyring entry first.
            format!("stored token is unreadable (likely a schema drift): {msg}"),
            "delete the entry manually from the OS keyring \
             (service: dev.dirt.session), then run `dirt auth login` \
             to write a fresh one",
        ),
    };
    format!("{cause}; {fix}")
}

fn describe_auth_error(err: &AuthError) -> (String, &'static str) {
    match err {
        AuthError::InvalidEmail(msg) => (
            format!("invalid email: {msg}"),
            "double-check the address and try again",
        ),
        AuthError::InvalidCode(msg) => (
            format!("invalid code: {msg}"),
            "the code is wrong, expired, or already used — request a new one with `dirt auth login`",
        ),
        AuthError::SessionExpired(msg) => (
            format!("session expired: {msg}"),
            "sign in again with `dirt auth login`",
        ),
        AuthError::RateLimited {
            message,
            retry_after_secs,
        } => {
            // Use a comma inside `cause` (not `;`) so the assembled
            // user-facing string keeps the single-separator shape
            // `{cause}; {fix}` shared by every other arm — otherwise
            // a 429 surfaces as "...; retry after 42s; wait the
            // cooldown..." with two semicolons.
            let cause = retry_after_secs.as_ref().map_or_else(
                || format!("rate limited ({message})"),
                |secs| format!("rate limited ({message}), retry after {secs}s"),
            );
            (cause, "wait the cooldown period and try again")
        }
        AuthError::Network(msg) => (
            format!("network error: {msg}"),
            "check connectivity and DIRT_API_BASE_URL, then retry",
        ),
        AuthError::ServerUnavailable(msg) => (
            format!("server unavailable: {msg}"),
            "retry shortly; the dirt-api server may be restarting",
        ),
        AuthError::BadRequest { code, message } => (
            format!("bad request ({code}): {message}"),
            "this should not happen for a normal login — file a bug",
        ),
        AuthError::ServerError { status, message } => (
            format!("server error {status}: {message}"),
            "retry shortly; check server logs",
        ),
        AuthError::Decode(msg) => (
            format!("decode error: {msg}"),
            "client/server contract drift — upgrade the CLI",
        ),
        AuthError::InvalidConfiguration(msg) => (
            format!("invalid configuration: {msg}"),
            "check DIRT_API_BASE_URL and your CLI profile config",
        ),
    }
}

fn describe_api_error(err: &ApiClientError) -> (String, &'static str) {
    match err {
        // Status's 401 branch hits this *after* one silent refresh has
        // already been attempted, so the server actually rejected a
        // freshly minted bearer — surface re-login as the next step
        // rather than nudging the user at a token they cannot rotate.
        ApiClientError::Unauthorized(_) => (
            "401 unauthorized".to_string(),
            "run `dirt auth login` to sign in again; the server rejected the current session",
        ),
        ApiClientError::Network(msg) => (
            format!("network error: {msg}"),
            "check DIRT_API_BASE_URL and connectivity",
        ),
        ApiClientError::ServerUnavailable(msg) => (
            format!("server unavailable: {msg}"),
            "retry shortly; check the server's Turso status",
        ),
        ApiClientError::BadRequest { code, message } => (
            format!("bad request ({code}): {message}"),
            "this should not happen for a probe — file a bug",
        ),
        ApiClientError::ServerError { status, message } => (
            format!("server error {status}: {message}"),
            "retry shortly; check server logs",
        ),
        ApiClientError::Decode(msg) => (
            format!("decode error: {msg}"),
            "client/server contract drift — upgrade the CLI",
        ),
        ApiClientError::InvalidConfiguration(msg) => (
            format!("invalid configuration: {msg}"),
            "check DIRT_API_BASE_URL and your CLI profile config",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use dirt_core::auth::MemoryTokenStore;
    use serde_json::json;
    use wiremock::matchers::{bearer_token, body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// Closure-backed reader that pops scripted lines off a `Vec`.
    /// Mirrors the prod `stdin_read_line` shape so the orchestration
    /// can drive it without knowing whether it's wired to real stdin.
    fn scripted_reader(lines: Vec<&'static str>) -> impl FnMut(&str) -> io::Result<String> {
        let lines = RefCell::new(lines.into_iter());
        move |_prompt: &str| {
            lines
                .borrow_mut()
                .next()
                .map(str::to_string)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "scripted reader exhausted")
                })
        }
    }

    fn client_for(server: &MockServer) -> AuthClient {
        AuthClient::new(server.uri()).expect("client should build for mock server")
    }

    fn seeded_store(token: &str) -> Arc<MemoryTokenStore> {
        Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: token.into(),
            session_id: "sid-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        }))
    }

    /// Successful probe — pull endpoint returns 200, status line must
    /// be the canonical `online:` form including the stored email.
    #[tokio::test(flavor = "current_thread")]
    async fn probe_session_status_online_when_session_valid() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .and(bearer_token("tok-live"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 1_700_000_000_000_i64,
                "has_more": false,
                "next_cursor": null,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store: Arc<dyn TokenStore> = seeded_store("tok-live");
        let line = probe_session_status(server.uri(), store, "user@example.com").await;
        assert_eq!(line, "online: signed in as user@example.com, server ok");
    }

    /// 401 + refresh-rotates + re-probe succeeds → must surface the
    /// `(session rotated)` suffix so a watcher can tell that the
    /// online state involved a refresh.
    #[tokio::test(flavor = "current_thread")]
    async fn probe_session_status_refreshes_and_reprobes_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .and(bearer_token("tok-stale"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "tok-fresh",
                "session_id": "sid-new",
                "expires_at_ms": 9_999_999_999_999_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .and(bearer_token("tok-fresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 1_700_000_000_000_i64,
                "has_more": false,
                "next_cursor": null,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store: Arc<dyn TokenStore> = seeded_store("tok-stale");
        let line = probe_session_status(server.uri(), store, "user@example.com").await;
        assert_eq!(
            line,
            "online: signed in as user@example.com, server ok (session rotated)"
        );
    }

    /// 401 → refresh returns `SESSION_EXPIRED` → terminal `offline:`
    /// pointing at `dirt auth login`. The keyring slot is cleared by
    /// `SessionApiClient::refresh`; we assert that explicitly so the
    /// next status invocation lands the "not signed in" path instead
    /// of looping through refresh again.
    #[tokio::test(flavor = "current_thread")]
    async fn probe_session_status_session_expired_after_failed_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .mount(&server)
            .await;

        let store_concrete = seeded_store("tok-dead");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let line = probe_session_status(server.uri(), store, "user@example.com").await;
        assert_eq!(
            line,
            "offline: session expired — run `dirt auth login` to sign in again"
        );
        assert!(
            store_concrete.load().unwrap().is_none(),
            "keyring slot must be cleared after SESSION_EXPIRED"
        );
    }

    /// 401 on the probe + transient 5xx on refresh → must report a
    /// retry-shortly hint and keep the stored token intact so the next
    /// `dirt auth status` (or `dirt sync`) can recover when the server
    /// settles down.
    #[tokio::test(flavor = "current_thread")]
    async fn probe_session_status_transient_refresh_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(503).set_body_string("turso down"))
            .mount(&server)
            .await;

        let store_concrete = seeded_store("tok-maybe-live");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let line = probe_session_status(server.uri(), store, "user@example.com").await;
        assert!(
            line.starts_with("offline: session refresh failed"),
            "got {line}"
        );
        assert!(line.contains("retry shortly"), "got {line}");
        assert!(
            store_concrete.load().unwrap().is_some(),
            "transient refresh failure must NOT clear the keyring"
        );
    }

    /// A 5xx on the probe itself (not a 401) must NOT trigger refresh
    /// and must surface the server-side cause verbatim — separating
    /// "server is sick" from "your credentials are sick" is what makes
    /// `dirt auth status` actionable.
    #[tokio::test(flavor = "current_thread")]
    async fn probe_session_status_offline_on_server_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "error": {
                    "code": "TURSO_UNREACHABLE",
                    "message": "turso unreachable",
                    "cause": "turso unreachable",
                    "fix": "Retry shortly."
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        // /v1/auth/refresh should never be reached; mounting nothing
        // confirms that any accidental refresh would 404 here (wiremock
        // returns 404 for unmounted routes), changing the output line
        // and failing the assertion below.

        let store: Arc<dyn TokenStore> = seeded_store("tok-live");
        let line = probe_session_status(server.uri(), store, "user@example.com").await;
        assert!(line.starts_with("offline: auth test failed"), "got {line}");
        assert!(line.contains("server unavailable"), "got {line}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_persists_token_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .and(body_json(json!({ "email": "user@example.com" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-abc",
                "expires_at_ms": 1_700_000_000_000_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/verify"))
            .and(body_json(json!({
                "request_id": "req-abc",
                "code": "123456",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "tok-live",
                "session_id": "sess-1",
                "user_id": "uid-1",
                "email": "user@example.com",
                "expires_at_ms": 1_800_000_000_000_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec!["user@example.com", "123456"]);

        login_flow(&client, &store, reader).await.unwrap();

        let token = store.load().unwrap().expect("token must be persisted");
        assert_eq!(token.session_token, "tok-live");
        assert_eq!(token.session_id, "sess-1");
        assert_eq!(token.user_id, "uid-1");
        assert_eq!(token.email, "user@example.com");
        assert_eq!(token.expires_at_ms, 1_800_000_000_000_i64);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_rejects_empty_email_before_calling_server() {
        // Server mounts no mocks. wiremock returns 404 for unmatched
        // routes (it does not panic), and AuthClient maps that 404 to
        // a generic `AuthError::ServerError`. If the flow regressed
        // and contacted the server here, the resulting CliError would
        // carry "server error 404; retry shortly..." and the
        // assertion below — which requires the literal "email must
        // not be empty" — would fail.
        let server = MockServer::start().await;
        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec![""]);

        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => assert!(msg.contains("email must not be empty")),
            other => panic!("expected Auth error, got {other:?}"),
        }
        assert!(store.load().unwrap().is_none());
    }

    /// Companion to `login_flow_rejects_empty_email_before_calling_server`
    /// for the second prompt: `/v1/auth/request` succeeds but the user
    /// hits Enter on the code prompt. The flow must short-circuit
    /// without ever hitting `/v1/auth/verify` — no `/verify` mock is
    /// mounted, so a regression that contacted the server would get
    /// a 404 from `wiremock`, which `AuthClient` surfaces as
    /// `ServerError {status: 404, ...}`, which then fails the
    /// `code must not be empty` assertion below.
    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_rejects_empty_code_before_calling_verify() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-1",
                "expires_at_ms": 1_700_000_000_000_i64,
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec!["user@example.com", ""]);

        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => assert!(msg.contains("code must not be empty"), "got {msg}"),
            other => panic!("expected Auth error, got {other:?}"),
        }
        assert!(store.load().unwrap().is_none());
    }

    /// Companion to the empty-code guard: a 5-digit, 7-digit, or
    /// non-numeric input is locally rejected before /v1/auth/verify
    /// would have produced the same generic `INVALID_CODE`.
    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_rejects_non_six_digit_code_before_calling_verify() {
        for (label, bad_code) in [
            ("five digits", "12345"),
            ("seven digits", "1234567"),
            ("letters", "abc123"),
            (
                "trailing whitespace would already trim; padded letters",
                "12a345",
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/auth/request"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "request_id": "req-1",
                    "expires_at_ms": 1_700_000_000_000_i64,
                })))
                .mount(&server)
                .await;
            let client = client_for(&server);
            let store = MemoryTokenStore::new();
            let reader = scripted_reader(vec!["user@example.com", bad_code]);
            let err = login_flow(&client, &store, reader).await.unwrap_err();
            match err {
                CliError::Auth(msg) => {
                    assert!(msg.contains("exactly 6 digits"), "[{label}] got {msg}");
                }
                other => panic!("[{label}] expected Auth error, got {other:?}"),
            }
            assert!(store.load().unwrap().is_none());
        }
    }

    /// A working server response but a `TokenStore` that fails to save
    /// must surface as a `CliError::Auth` (so the user knows what
    /// happened) and leave the store empty (nothing to leak).
    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_surfaces_save_failure() {
        use std::sync::Mutex;

        use dirt_core::auth::{TokenStore, TokenStoreError};

        struct SaveFailsStore {
            inner: Mutex<Option<StoredToken>>,
        }
        impl TokenStore for SaveFailsStore {
            fn load(&self) -> Result<Option<StoredToken>, TokenStoreError> {
                Ok(self.inner.lock().unwrap().clone())
            }
            fn save(&self, _token: &StoredToken) -> Result<(), TokenStoreError> {
                Err(TokenStoreError::Backend("injected save failure".into()))
            }
            fn clear(&self) -> Result<(), TokenStoreError> {
                unreachable!("login_flow does not clear")
            }
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-abc",
                "expires_at_ms": 1_700_000_000_000_i64,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/verify"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "tok-live",
                "session_id": "sess-1",
                "user_id": "uid-1",
                "email": "user@example.com",
                "expires_at_ms": 1_800_000_000_000_i64,
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = SaveFailsStore {
            inner: Mutex::new(None),
        };
        let reader = scripted_reader(vec!["user@example.com", "123456"]);
        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => {
                assert!(msg.contains("keyring backend error"), "got {msg}");
                assert!(msg.contains("injected save failure"), "got {msg}");
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
        assert!(store.load().unwrap().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_surfaces_invalid_code_and_leaves_store_empty() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-1",
                "expires_at_ms": 1_700_000_000_000_i64,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/verify"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": "INVALID_CODE",
                    "message": "wrong code",
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec!["user@example.com", "000000"]);

        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => {
                assert!(msg.contains("invalid code"), "got {msg}");
                assert!(msg.contains("request a new one"), "got {msg}");
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
        assert!(store.load().unwrap().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_surfaces_rate_limit_with_retry_hint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": {
                    "code": "RATE_LIMITED",
                    "message": "per-email cooldown",
                    "retry_after_secs": 42,
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec!["user@example.com"]);

        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => {
                assert!(msg.contains("rate limited"), "got {msg}");
                assert!(msg.contains("42"), "retry hint missing: {msg}");
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
    }

    /// Regression for the codex P2 review: `dispatch_logout` must not
    /// evaluate the `AuthClient` builder when the store is empty. A
    /// production user on a fresh machine without `DIRT_API_BASE_URL`
    /// configured runs `dirt auth logout` and expects "Not signed in",
    /// not "`DIRT_API_BASE_URL` not set".
    #[tokio::test(flavor = "current_thread")]
    async fn dispatch_logout_does_not_invoke_client_builder_when_store_is_empty() {
        let store = MemoryTokenStore::new();
        dispatch_logout(&store, || -> Result<AuthClient, CliError> {
            panic!("AuthClient builder must not run when no token is stored");
        })
        .await
        .unwrap();
        assert!(store.load().unwrap().is_none());
    }

    /// Counterpart to the no-op test: when a token IS stored, the
    /// dispatcher must build the client, hit the server, and clear
    /// the slot — the full `logout_flow` path is exercised.
    #[tokio::test(flavor = "current_thread")]
    async fn dispatch_logout_builds_client_and_revokes_when_token_present() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .and(bearer_token("tok-live"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let store = MemoryTokenStore::with_initial(StoredToken {
            session_token: "tok-live".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        });
        let uri = server.uri();
        dispatch_logout(&store, move || -> Result<AuthClient, CliError> {
            AuthClient::new(uri).map_err(|err| auth_error_to_cli(&err))
        })
        .await
        .unwrap();
        assert!(store.load().unwrap().is_none());
    }

    // Note: there is no `logout_flow_with_empty_store_is_noop` test
    // because `logout_flow` no longer carries the empty-store branch
    // — the "not signed in" case lives in `dispatch_logout`, covered
    // by `dispatch_logout_does_not_invoke_client_builder_when_store_is_empty`.

    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_clears_store_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .and(bearer_token("tok-live"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let stored = StoredToken {
            session_token: "tok-live".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        };
        let store = MemoryTokenStore::with_initial(stored.clone());
        let client = client_for(&server);

        logout_flow(&client, &store, stored).await.unwrap();
        assert!(store.load().unwrap().is_none());
    }

    /// Exercises the `Ok(())` arm of the match: `AuthClient`
    /// short-circuits a 401-with-SESSION_EXPIRED-envelope to
    /// `Ok(())` internally, so even though the server replied 401
    /// the local clear still fires.
    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_clears_store_when_auth_client_silently_accepts_session_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "already revoked",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let stored = StoredToken {
            session_token: "tok-dead".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        };
        let store = MemoryTokenStore::with_initial(stored.clone());
        let client = client_for(&server);

        logout_flow(&client, &store, stored).await.unwrap();
        assert!(store.load().unwrap().is_none());
    }

    /// Exercises the explicit `Err(AuthError::SessionExpired)` arm in
    /// `logout_flow`. `AuthClient` surfaces a 401 with a non-SESSION_EXPIRED
    /// envelope (e.g. a proxy-injected 401, or a future server code like
    /// `MISSING_TOKEN`) as `AuthError::SessionExpired` — and the server's
    /// signal is still "this token is dead". The local slot must be
    /// cleared so the user is not left holding an unusable credential.
    ///
    /// The `.unwrap()` on `logout_flow` is load-bearing: if a future
    /// `AuthClient` refactor stops mapping non-SESSION_EXPIRED 401s to
    /// `Err(SessionExpired)`, the call will return a different error
    /// variant, the store will not be cleared, and the unwrap will
    /// panic — making the regression visible at test time instead of
    /// silently passing.
    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_clears_store_when_auth_client_surfaces_session_expired_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "MISSING_TOKEN",
                    "message": "bearer token missing",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let stored = StoredToken {
            session_token: "tok-stale".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        };
        let store = MemoryTokenStore::with_initial(stored.clone());
        let client = client_for(&server);

        logout_flow(&client, &store, stored).await.unwrap();
        assert!(store.load().unwrap().is_none());
    }

    /// `store.clear()` failures after a successful server-side revoke
    /// are a real keyring scenario (e.g. credential-manager IPC dies
    /// between read and delete). The error must convey that the
    /// server-side revoke already succeeded so the user understands
    /// that retrying `dirt auth logout` is harmless and will finish
    /// the cleanup via `SESSION_EXPIRED`.
    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_reports_revoke_succeeded_when_local_clear_fails() {
        use std::sync::Mutex;

        use dirt_core::auth::{TokenStore, TokenStoreError};

        struct LoadOkClearFailsStore {
            inner: Mutex<Option<StoredToken>>,
        }

        impl TokenStore for LoadOkClearFailsStore {
            fn load(&self) -> Result<Option<StoredToken>, TokenStoreError> {
                Ok(self.inner.lock().unwrap().clone())
            }
            fn save(&self, _token: &StoredToken) -> Result<(), TokenStoreError> {
                unreachable!("logout_flow never calls save")
            }
            fn clear(&self) -> Result<(), TokenStoreError> {
                Err(TokenStoreError::Backend(
                    "simulated keyring IPC failure".into(),
                ))
            }
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_for(&server);
        let stored = StoredToken {
            session_token: "tok-live".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        };
        let store = LoadOkClearFailsStore {
            inner: Mutex::new(Some(stored.clone())),
        };

        let err = logout_flow(&client, &store, stored).await.unwrap_err();
        match err {
            CliError::Auth(msg) => {
                assert!(
                    msg.contains("server revoke succeeded"),
                    "must surface that server revoke succeeded: {msg}"
                );
                assert!(
                    msg.contains("simulated keyring IPC failure"),
                    "must include the underlying keyring error: {msg}"
                );
                assert!(
                    msg.contains("Re-run `dirt auth logout`"),
                    "must point the user at the retry path: {msg}"
                );
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
    }

    /// A real network/server failure during logout must NOT clear the
    /// local store. The server still believes the session is alive, so
    /// dropping the local token would orphan it (and a retry could
    /// then succeed once connectivity is restored).
    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_preserves_store_on_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": {
                    "code": "INTERNAL",
                    "message": "boom",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let stored = StoredToken {
            session_token: "tok-live".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        };
        let store = MemoryTokenStore::with_initial(stored.clone());
        let client = client_for(&server);

        let err = logout_flow(&client, &store, stored).await.unwrap_err();
        match err {
            CliError::Auth(msg) => assert!(msg.contains("server error 500"), "got {msg}"),
            other => panic!("expected Auth error, got {other:?}"),
        }
        // Token must survive the failed revoke so the user can retry.
        assert!(store.load().unwrap().is_some());
    }

    #[test]
    fn describe_auth_error_includes_retry_hint_when_present() {
        let err = AuthError::RateLimited {
            message: "per-email cooldown".into(),
            retry_after_secs: Some(60),
        };
        let (cause, _fix) = describe_auth_error(&err);
        assert!(cause.contains("60"), "{cause}");
    }

    #[test]
    fn describe_auth_error_omits_retry_hint_when_absent() {
        let err = AuthError::RateLimited {
            message: "per-email cooldown".into(),
            retry_after_secs: None,
        };
        let (cause, _fix) = describe_auth_error(&err);
        assert!(!cause.contains("retry after"), "{cause}");
    }
}
