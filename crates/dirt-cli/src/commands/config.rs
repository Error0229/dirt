use std::env;

use crate::cli::ConfigCommands;
use crate::config_profiles::{normalize_text_option, CliProfile, CliProfilesConfig};
use crate::error::CliError;

pub fn run_config(command: ConfigCommands, global_profile: Option<&str>) -> Result<(), CliError> {
    match command {
        ConfigCommands::Init {
            profile,
            api_base_url,
            no_activate,
        } => run_config_init(
            profile.as_deref().or(global_profile),
            api_base_url,
            no_activate,
        ),
    }
}

#[allow(clippy::needless_pass_by_value)]
pub fn run_config_init(
    profile_name: Option<&str>,
    api_base_url: Option<String>,
    no_activate: bool,
) -> Result<(), CliError> {
    let mut config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(profile_name);
    let existing_profile = config.profile(&profile_name).cloned().unwrap_or_default();

    let merged_api_base_url = normalize_text_option(api_base_url)
        .or_else(|| normalize_text_option(env::var("DIRT_API_BASE_URL").ok()))
        .or_else(|| existing_profile.dirt_api_base_url());

    let profile = config.profile_mut_or_default(&profile_name);
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
    if profile.dirt_api_base_url().is_some() {
        println!(
            "Profile '{profile_name}' is ready. Run `dirt auth login` to sign in and authorize sync."
        );
    } else {
        println!("Profile '{profile_name}' is missing: api_base_url");
    }

    Ok(())
}

fn validate_profile_urls(profile: &CliProfile) -> Result<(), CliError> {
    if let Some(url) = profile.dirt_api_base_url() {
        if !dirt_core::util::is_http_url(&url) {
            return Err(CliError::Config(
                "api_base_url must include http:// or https://".to_string(),
            ));
        }
    }
    Ok(())
}
