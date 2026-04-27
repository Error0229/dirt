//! `dirt sync` — push local mutations and pull remote changes once.
//!
//! Resolution order for the API base URL:
//!   1. `DIRT_API_BASE_URL` env var (overrides everything for ad-hoc runs).
//!   2. Active CLI profile's `dirt_api_base_url`.
//!
//! The bearer token is read from `DIRT_CLIENT_TOKEN` only — it never
//! lands in the CLI profile JSON because that file is plaintext.

use std::env;
use std::path::Path;

use dirt_core::sync::api_client::ApiClient;
use dirt_core::sync::engine::{SyncEngine, SyncReport};
use dirt_core::SOLO_USER_ID;

use crate::commands::common::open_database;
use crate::config_profiles::{normalize_text_option, CliProfilesConfig};
use crate::error::CliError;

pub async fn run_sync(db_path: &Path) -> Result<(), CliError> {
    let api_base_url = resolve_api_base_url()?;
    let token = require_env("DIRT_CLIENT_TOKEN")?;

    let api = ApiClient::new(api_base_url, token)
        .map_err(|err| CliError::Config(format!("invalid sync configuration: {err}")))?;
    let db = open_database(db_path).await?;
    let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);

    let report = engine
        .run_once()
        .await
        .map_err(|err| CliError::Config(format!("sync failed: {err}")))?;

    print_report(&report);
    Ok(())
}

fn resolve_api_base_url() -> Result<String, CliError> {
    if let Some(url) = normalize_text_option(env::var("DIRT_API_BASE_URL").ok()) {
        return Ok(url);
    }

    let config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(None);
    let profile = config
        .profile(&profile_name)
        .ok_or(CliError::SyncNotConfigured)?;
    profile
        .dirt_api_base_url()
        .ok_or(CliError::SyncNotConfigured)
}

fn require_env(key: &str) -> Result<String, CliError> {
    normalize_text_option(env::var(key).ok()).ok_or_else(|| {
        CliError::Config(format!(
            "{key} must be set in the environment for `dirt sync`"
        ))
    })
}

fn print_report(report: &SyncReport) {
    println!(
        "Sync complete — pulled {} (skipped {}), pushed {}",
        report.pulled_applied, report.pulled_skipped, report.pushed
    );
}
