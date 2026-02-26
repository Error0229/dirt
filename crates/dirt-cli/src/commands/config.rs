use std::env;

use crate::bootstrap_manifest::fetch_bootstrap_manifest;
use crate::cli::ConfigCommands;
use crate::config_profiles::{is_http_url, normalize_text_option, CliProfile, CliProfilesConfig};
use crate::error::CliError;

pub async fn run_config(
    command: ConfigCommands,
    global_profile: Option<&str>,
) -> Result<(), CliError> {
    match command {
        ConfigCommands::Init {
            profile,
            supabase_url,
            supabase_anon_key,
            sync_token_endpoint,
            api_base_url,
            bootstrap_url,
            no_activate,
        } => {
            run_config_init(
                profile.as_deref().or(global_profile),
                supabase_url,
                supabase_anon_key,
                sync_token_endpoint,
                api_base_url,
                bootstrap_url,
                no_activate,
            )
            .await
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
pub async fn run_config_init(
    profile_name: Option<&str>,
    supabase_url: Option<String>,
    supabase_anon_key: Option<String>,
    sync_token_endpoint: Option<String>,
    api_base_url: Option<String>,
    bootstrap_url: Option<String>,
    no_activate: bool,
) -> Result<(), CliError> {
    let mut config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(profile_name);
    let existing_profile = config.profile(&profile_name).cloned().unwrap_or_default();

    let explicit_supabase_url = normalize_text_option(supabase_url);
    let explicit_supabase_anon_key = normalize_text_option(supabase_anon_key);
    let explicit_sync_token_endpoint = normalize_text_option(sync_token_endpoint);
    let explicit_api_base_url = normalize_text_option(api_base_url);
    let explicit_bootstrap_url = normalize_text_option(bootstrap_url);

    let bootstrap_url = resolve_bootstrap_url(
        explicit_bootstrap_url.clone(),
        explicit_api_base_url.clone(),
        existing_profile.dirt_api_base_url.clone(),
    )?;

    let should_fetch_bootstrap = explicit_bootstrap_url.is_some()
        || explicit_supabase_url.is_none()
        || explicit_supabase_anon_key.is_none()
        || explicit_sync_token_endpoint.is_none()
        || explicit_api_base_url.is_none();
    let bootstrap_profile = if should_fetch_bootstrap {
        if let Some(url) = bootstrap_url.clone() {
            match fetch_bootstrap_manifest(&url).await {
                Ok(profile) => {
                    println!("Loaded managed bootstrap manifest from {url}");
                    Some(profile)
                }
                Err(error) => {
                    return Err(CliError::Config(format!(
                        "Failed to load bootstrap manifest from {url}: {error}"
                    )));
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let merged_supabase_url = explicit_supabase_url
        .or_else(|| {
            bootstrap_profile
                .as_ref()
                .map(|manifest| manifest.supabase_url.clone())
        })
        .or_else(|| normalize_text_option(env::var("SUPABASE_URL").ok()))
        .or_else(|| existing_profile.supabase_url());
    let merged_supabase_anon_key = explicit_supabase_anon_key
        .or_else(|| {
            bootstrap_profile
                .as_ref()
                .map(|manifest| manifest.supabase_anon_key.clone())
        })
        .or_else(|| normalize_text_option(env::var("SUPABASE_ANON_KEY").ok()))
        .or_else(|| existing_profile.supabase_anon_key());
    let merged_sync_token_endpoint = explicit_sync_token_endpoint
        .or_else(|| {
            bootstrap_profile
                .as_ref()
                .and_then(|manifest| manifest.sync_token_endpoint.clone())
        })
        .or_else(|| normalize_text_option(env::var("TURSO_SYNC_TOKEN_ENDPOINT").ok()))
        .or_else(|| existing_profile.managed_sync_endpoint());
    let merged_api_base_url = explicit_api_base_url
        .or_else(|| {
            bootstrap_profile
                .as_ref()
                .map(|manifest| manifest.api_base_url.clone())
        })
        .or_else(|| normalize_text_option(env::var("DIRT_API_BASE_URL").ok()))
        .or_else(|| normalize_text_option(existing_profile.dirt_api_base_url.clone()));

    let profile = config.profile_mut_or_default(&profile_name);
    if let Some(value) = merged_supabase_url {
        profile.supabase_url = Some(value);
    }
    if let Some(value) = merged_supabase_anon_key {
        profile.supabase_anon_key = Some(value);
    }
    if let Some(value) = merged_sync_token_endpoint {
        profile.turso_sync_token_endpoint = Some(value);
    }
    if let Some(value) = merged_api_base_url {
        profile.dirt_api_base_url = Some(value);
    }

    validate_profile_urls(profile)?;

    if !no_activate {
        config.active_profile = Some(profile_name.clone());
    }

    let path = config.save().map_err(CliError::Config)?;
    println!(
        "Profile '{}' initialized at {}",
        profile_name,
        path.display()
    );

    let profile = config
        .profiles
        .get(&profile_name)
        .ok_or_else(|| CliError::Config("Failed to persist profile".to_string()))?;
    let mut missing_fields = Vec::new();
    if profile.supabase_url().is_none() {
        missing_fields.push("supabase_url");
    }
    if profile.supabase_anon_key().is_none() {
        missing_fields.push("supabase_anon_key");
    }
    if profile.managed_sync_endpoint().is_none() {
        missing_fields.push("sync_token_endpoint");
    }
    if missing_fields.is_empty() {
        println!(
            "Managed sync profile '{profile_name}' is ready. Run `dirt auth login --email <email> --password <password>`."
        );
    } else {
        println!(
            "Profile '{}' is missing: {}",
            profile_name,
            missing_fields.join(", ")
        );
    }

    Ok(())
}

pub fn resolve_bootstrap_url(
    explicit_bootstrap_url: Option<String>,
    explicit_api_base_url: Option<String>,
    existing_api_base_url: Option<String>,
) -> Result<Option<String>, CliError> {
    if let Some(url) = explicit_bootstrap_url {
        return normalize_bootstrap_url(url).map(Some);
    }

    if let Some(url) = normalize_text_option(env::var("DIRT_BOOTSTRAP_URL").ok()) {
        return normalize_bootstrap_url(url).map(Some);
    }

    let api_base_url = explicit_api_base_url
        .or_else(|| normalize_text_option(env::var("DIRT_API_BASE_URL").ok()))
        .or_else(|| normalize_text_option(existing_api_base_url));
    Ok(api_base_url.map(|base| format!("{}/v1/bootstrap", base.trim_end_matches('/'))))
}

pub fn normalize_bootstrap_url(url: String) -> Result<String, CliError> {
    let normalized = normalize_text_option(Some(url))
        .ok_or_else(|| CliError::Config("bootstrap_url must not be empty".to_string()))?;
    if !is_http_url(&normalized) {
        return Err(CliError::Config(
            "bootstrap_url must include http:// or https://".to_string(),
        ));
    }
    Ok(normalized.trim_end_matches('/').to_string())
}

fn validate_profile_urls(profile: &CliProfile) -> Result<(), CliError> {
    if let Some(url) = profile.supabase_url() {
        if !is_http_url(&url) {
            return Err(CliError::Config(
                "supabase_url must include http:// or https://".to_string(),
            ));
        }
    }
    if let Some(url) = profile.managed_sync_endpoint() {
        if !is_http_url(&url) {
            return Err(CliError::Config(
                "sync_token_endpoint must include http:// or https://".to_string(),
            ));
        }
    }
    if let Some(url) = normalize_text_option(profile.dirt_api_base_url.clone()) {
        if !is_http_url(&url) {
            return Err(CliError::Config(
                "api_base_url must include http:// or https://".to_string(),
            ));
        }
    }
    Ok(())
}
