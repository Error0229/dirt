//! `dirt auth` — auth-state placeholder commands.
//!
//! In Phase 1 the CLI doesn't have a login state to manage — the only
//! credential is `DIRT_CLIENT_TOKEN`, read from the env at sync time.
//! These commands exist so the surface is stable when Phase 2 lands
//! magic-link auth. Today they're a thin connectivity probe (`status`)
//! and two clear stubs (`login`, `logout`).

use std::env;

use dirt_core::sync::api_client::{ApiClient, ApiClientError};

use crate::cli::AuthCommands;
use crate::config_profiles::{normalize_text_option, CliProfilesConfig};
use crate::error::CliError;

const PHASE_TWO_NOT_APPLICABLE: &str =
    "not applicable in solo phase; Phase 2 will implement magic-link auth";

pub async fn run_auth(command: AuthCommands) -> Result<(), CliError> {
    let line = match command {
        AuthCommands::Status => status_line().await?,
        AuthCommands::Login | AuthCommands::Logout => PHASE_TWO_NOT_APPLICABLE.to_string(),
    };
    println!("{line}");
    Ok(())
}

/// Compute the user-facing status line. Returned (not printed) so tests
/// can assert exact strings.
pub async fn status_line() -> Result<String, CliError> {
    let token = normalize_text_option(env::var("DIRT_CLIENT_TOKEN").ok());
    let Some(token) = token else {
        return Ok(
            "offline: DIRT_CLIENT_TOKEN not set — local capture works, sync disabled".to_string(),
        );
    };

    let Some(api_base_url) = resolve_api_base_url()? else {
        return Ok(
            "offline: DIRT_API_BASE_URL not set — local capture works, sync disabled".to_string(),
        );
    };

    let api = match ApiClient::new(api_base_url, token) {
        Ok(api) => api,
        Err(err) => {
            return Ok(format!(
                "offline: auth test failed — invalid configuration: {err}"
            ));
        }
    };

    // The pull endpoint is the cheapest authenticated probe — it
    // doesn't mutate anything and exercises the same bearer middleware
    // production traffic does.
    Ok(match api.pull(None, Some(1)).await {
        Ok(_) => "online: authenticated as solo-user, server ok".to_string(),
        Err(err) => {
            let (cause, fix) = describe(&err);
            format!("offline: auth test failed — {cause}; {fix}")
        }
    })
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

fn describe(err: &ApiClientError) -> (String, &'static str) {
    match err {
        ApiClientError::Unauthorized(_) => (
            "401 unauthorized".to_string(),
            "rotate DIRT_CLIENT_TOKEN to match the bearer token the server is configured with",
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
            "check DIRT_API_BASE_URL and DIRT_CLIENT_TOKEN",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    #[allow(unsafe_code)]
    async fn status_offline_when_no_client_token() {
        // SAFETY: tests run on current_thread, no concurrent reader.
        unsafe {
            std::env::remove_var("DIRT_CLIENT_TOKEN");
        }
        let line = status_line().await.unwrap();
        assert_eq!(
            line,
            "offline: DIRT_CLIENT_TOKEN not set — local capture works, sync disabled"
        );
    }

    #[test]
    fn phase_two_stub_message_mentions_phase_and_magic_link() {
        assert!(PHASE_TWO_NOT_APPLICABLE.contains("Phase 2"));
        assert!(PHASE_TWO_NOT_APPLICABLE.contains("magic-link"));
    }
}
