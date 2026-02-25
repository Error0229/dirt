use crate::auth::{clear_stored_session, load_stored_session, SupabaseAuthService};
use crate::cli::AuthCommands;
use crate::config_profiles::CliProfilesConfig;
use crate::error::CliError;

pub async fn run_auth(command: AuthCommands, global_profile: Option<&str>) -> Result<(), CliError> {
    match command {
        AuthCommands::Login {
            profile,
            email,
            password,
        } => {
            let config = CliProfilesConfig::load().map_err(CliError::Config)?;
            let profile_name = config.resolve_profile_name(profile.as_deref().or(global_profile));
            let profile_config = config.profiles.get(&profile_name).ok_or_else(|| {
                CliError::Config(format!(
                    "Profile '{profile_name}' is not configured. Run `dirt config init --profile {profile_name}` first."
                ))
            })?;
            let auth_service = SupabaseAuthService::new_for_profile(&profile_name, profile_config)
                .map_err(|error| CliError::Auth(error.to_string()))?
                .ok_or_else(|| {
                    CliError::Config(format!(
                        "Profile '{profile_name}' missing Supabase auth config. Set SUPABASE_URL and SUPABASE_ANON_KEY via `dirt config init`."
                    ))
                })?;
            let session = auth_service
                .sign_in(&email, &password)
                .await
                .map_err(|error| CliError::Auth(error.to_string()))?;
            let email_label = session.user.email.as_deref().unwrap_or("(no email)");
            println!("Signed in profile '{profile_name}' as {email_label}");
            Ok(())
        }
        AuthCommands::Status { profile } => {
            let config = CliProfilesConfig::load().map_err(CliError::Config)?;
            let profile_name = config.resolve_profile_name(profile.as_deref().or(global_profile));
            let maybe_profile = config.profiles.get(&profile_name);
            if maybe_profile.is_none() {
                println!("Profile '{profile_name}' is not configured.");
                return Ok(());
            }

            let profile = maybe_profile.expect("checked is_some");
            let maybe_auth_service = SupabaseAuthService::new_for_profile(&profile_name, profile)
                .map_err(|error| CliError::Auth(error.to_string()))?;
            let session = if let Some(service) = maybe_auth_service {
                service
                    .restore_session()
                    .await
                    .map_err(|error| CliError::Auth(error.to_string()))?
            } else {
                load_stored_session(&profile_name)
                    .map_err(|error| CliError::Auth(error.to_string()))?
            };

            if let Some(session) = session {
                let email_label = session.user.email.as_deref().unwrap_or("(no email)");
                println!(
                    "Profile '{}' is signed in as {} (expires_at={})",
                    profile_name, email_label, session.expires_at
                );
            } else {
                println!("Profile '{profile_name}' is not signed in.");
            }
            Ok(())
        }
        AuthCommands::Logout { profile } => {
            let config = CliProfilesConfig::load().map_err(CliError::Config)?;
            let profile_name = config.resolve_profile_name(profile.as_deref().or(global_profile));
            let maybe_profile = config.profiles.get(&profile_name);

            let stored_session = load_stored_session(&profile_name)
                .map_err(|error| CliError::Auth(error.to_string()))?;

            if let Some(profile) = maybe_profile {
                let maybe_auth_service =
                    SupabaseAuthService::new_for_profile(&profile_name, profile)
                        .map_err(|error| CliError::Auth(error.to_string()))?;
                if let (Some(service), Some(session)) = (maybe_auth_service, stored_session) {
                    service
                        .sign_out(&session.access_token)
                        .await
                        .map_err(|error| CliError::Auth(error.to_string()))?;
                } else {
                    clear_stored_session(&profile_name)
                        .map_err(|error| CliError::Auth(error.to_string()))?;
                }
            } else {
                clear_stored_session(&profile_name)
                    .map_err(|error| CliError::Auth(error.to_string()))?;
            }

            println!("Signed out profile '{profile_name}'");
            Ok(())
        }
    }
}
