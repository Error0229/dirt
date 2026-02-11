//! Dirt CLI - Command-line interface for capturing fleeting thoughts
//!
//! Quick capture from the terminal with minimal friction.

mod auth;
mod bootstrap_manifest;
mod config_profiles;
mod managed_sync;

use std::env;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::aot::Generator;
use clap_complete::{generate, shells};
use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository, SyncConfig};
use dirt_core::export::{render_json_export, render_markdown_export};
use dirt_core::{Note, NoteId, SyncConflict};
use serde::Serialize;
use thiserror::Error;

use crate::auth::{clear_stored_session, load_stored_session, SupabaseAuthService};
use crate::bootstrap_manifest::fetch_bootstrap_manifest;
use crate::config_profiles::{
    is_http_url, normalize_profile_name, normalize_text_option, CliProfilesConfig,
};
use crate::managed_sync::ManagedSyncAuthClient;

#[derive(Parser)]
#[command(name = "dirt")]
#[command(about = "Capture fleeting thoughts from the command line")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Optional path to local database file
    #[arg(long, value_name = "PATH")]
    db_path: Option<PathBuf>,

    /// CLI profile name for managed auth/sync configuration
    #[arg(long, global = true, value_name = "NAME")]
    profile: Option<String>,

    /// Quick capture: dirt "my thought here"
    #[arg(trailing_var_arg = true)]
    note: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new note
    #[command(alias = "new")]
    Add {
        /// Note content
        content: Vec<String>,
    },
    /// List recent notes
    List {
        /// Number of notes to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Filter notes by tag name
        #[arg(long)]
        tag: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search notes
    Search {
        /// Search query
        query: String,
        /// Number of notes to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit an existing note
    Edit {
        /// Note ID or unique ID prefix
        id: String,
    },
    /// Delete an existing note
    Delete {
        /// Note ID or unique ID prefix
        id: String,
    },
    /// Export notes
    Export {
        /// Export format
        #[arg(long, value_enum, default_value_t = ExportFormat::Json)]
        format: ExportFormat,
        /// Optional output path (stdout when omitted)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
    /// Generate shell completion scripts
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: CompletionShell,
        /// Optional output path (stdout when omitted)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
    /// Sync local replica with remote Turso database
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommands>,
    },
    /// Configure CLI managed profiles
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Authenticate CLI profile with Supabase
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Open TUI interface
    Tui,
}

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Core(#[from] dirt_core::Error),
    #[error(transparent)]
    LibSql(#[from] libsql::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error("No note content provided")]
    EmptyContent,
    #[error("Edited note content cannot be empty")]
    EmptyEditedContent,
    #[error("Note ID cannot be empty")]
    EmptyNoteId,
    #[error("Search query cannot be empty")]
    EmptySearchQuery,
    #[error("Note not found for id/prefix: {0}")]
    NoteNotFound(String),
    #[error("{0}")]
    AmbiguousNoteId(String),
    #[error("Editor command failed: {0}")]
    EditorFailed(String),
    #[error("Database initialization failed: {0}")]
    DatabaseInit(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Managed sync error: {0}")]
    ManagedSync(String),
    #[error(
        "Sync is not configured. Run `dirt config init` + `dirt auth login`, or set TURSO_DATABASE_URL and TURSO_AUTH_TOKEN for advanced env mode."
    )]
    SyncNotConfigured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ExportFormat {
    Json,
    Markdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand)]
enum SyncCommands {
    /// List recently resolved sync conflicts
    Conflicts {
        /// Number of conflicts to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Initialize or update profile config
    Init {
        /// Profile name to initialize
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Supabase project URL
        #[arg(long, value_name = "URL")]
        supabase_url: Option<String>,
        /// Supabase anon/public key
        #[arg(long, value_name = "KEY")]
        supabase_anon_key: Option<String>,
        /// Backend sync token exchange endpoint
        #[arg(long, value_name = "URL")]
        sync_token_endpoint: Option<String>,
        /// Optional managed API base URL
        #[arg(long, value_name = "URL")]
        api_base_url: Option<String>,
        /// Optional bootstrap manifest URL (e.g. <https://api.example.com/v1/bootstrap>)
        #[arg(long, value_name = "URL")]
        bootstrap_url: Option<String>,
        /// Keep current active profile instead of activating this one
        #[arg(long)]
        no_activate: bool,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Login with Supabase email/password and store session in keychain
    Login {
        /// Optional profile override
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Supabase account email
        #[arg(long, value_name = "EMAIL")]
        email: String,
        /// Supabase account password
        #[arg(long, value_name = "PASSWORD")]
        password: String,
    },
    /// Show auth status for profile
    Status {
        /// Optional profile override
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
    /// Logout profile and clear stored session
    Logout {
        /// Optional profile override
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), CliError> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();
    let db_path = resolve_db_path(cli.db_path);
    let global_profile = normalize_profile_name(cli.profile.as_deref());
    if let Some(profile) = &global_profile {
        env::set_var("DIRT_PROFILE", profile);
    }

    match cli.command {
        Some(Commands::Add { content }) => run_add(&content, &db_path).await?,
        Some(Commands::List { limit, tag, json }) => {
            run_list(limit, tag.as_deref(), json, &db_path).await?;
        }
        Some(Commands::Search { query, limit, json }) => {
            run_search(&query, limit, json, &db_path).await?;
        }
        Some(Commands::Edit { id }) => run_edit(&id, &db_path).await?,
        Some(Commands::Delete { id }) => run_delete(&id, &db_path).await?,
        Some(Commands::Export { format, output }) => {
            run_export(format, output.as_deref(), &db_path).await?;
        }
        Some(Commands::Completions { shell, output }) => {
            run_completions(shell, output.as_deref())?;
        }
        Some(Commands::Sync { command }) => match command {
            Some(SyncCommands::Conflicts { limit, json }) => {
                run_sync_conflicts(limit, json, &db_path).await?;
            }
            None => run_sync(&db_path).await?,
        },
        Some(Commands::Config { command }) => {
            run_config(command, global_profile.as_deref()).await?;
        }
        Some(Commands::Auth { command }) => {
            run_auth(command, global_profile.as_deref()).await?;
        }
        Some(Commands::Tui) => {
            println!("Opening TUI...");
            // TODO: Implement TUI with ratatui
        }
        None => {
            // Quick capture mode: dirt "my thought"
            if cli.note.is_empty() {
                Cli::command().print_help().map_err(CliError::Io)?;
                println!();
            } else {
                run_add(&cli.note, &db_path).await?;
            }
        }
    }

    Ok(())
}

async fn run_config(command: ConfigCommands, global_profile: Option<&str>) -> Result<(), CliError> {
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
async fn run_config_init(
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
                Err(error) if explicit_bootstrap_url.is_some() => {
                    return Err(CliError::Config(format!(
                        "Failed to load bootstrap manifest from {url}: {error}"
                    )));
                }
                Err(error) => {
                    tracing::warn!(
                        "Bootstrap manifest fetch failed from {}: {}. Falling back to existing/env values.",
                        url,
                        error
                    );
                    None
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

async fn run_auth(command: AuthCommands, global_profile: Option<&str>) -> Result<(), CliError> {
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

fn resolve_bootstrap_url(
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

fn normalize_bootstrap_url(url: String) -> Result<String, CliError> {
    let normalized = normalize_text_option(Some(url))
        .ok_or_else(|| CliError::Config("bootstrap_url must not be empty".to_string()))?;
    if !is_http_url(&normalized) {
        return Err(CliError::Config(
            "bootstrap_url must include http:// or https://".to_string(),
        ));
    }
    Ok(normalized.trim_end_matches('/').to_string())
}

fn validate_profile_urls(profile: &crate::config_profiles::CliProfile) -> Result<(), CliError> {
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

async fn run_add(content_parts: &[String], db_path: &Path) -> Result<(), CliError> {
    let content = resolve_note_content(content_parts)?;

    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    let note = repo.create(&content).await?;

    println!("{}", note.id);
    Ok(())
}

#[derive(Debug, Serialize)]
struct NoteListItem {
    id: String,
    preview: String,
    content: String,
    created_at: i64,
    updated_at: i64,
    relative_time: String,
    tags: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SyncConflictItem {
    id: i64,
    note_id: String,
    local_updated_at: i64,
    incoming_updated_at: i64,
    resolved_at: i64,
    resolved_at_iso: String,
    strategy: String,
}

async fn run_list(
    limit: usize,
    tag: Option<&str>,
    as_json: bool,
    db_path: &Path,
) -> Result<(), CliError> {
    let notes = list_notes(limit, tag, db_path).await?;

    if as_json {
        let json_items = notes
            .iter()
            .map(note_to_list_item)
            .collect::<Vec<NoteListItem>>();
        println!("{}", serde_json::to_string_pretty(&json_items)?);
    } else {
        for line in format_note_lines(&notes) {
            println!("{line}");
        }
    }

    Ok(())
}

async fn run_search(
    query: &str,
    limit: usize,
    as_json: bool,
    db_path: &Path,
) -> Result<(), CliError> {
    let normalized_query = normalize_search_query(query)?;
    let notes = search_notes(&normalized_query, limit, db_path).await?;

    if as_json {
        let json_items = notes
            .iter()
            .map(note_to_list_item)
            .collect::<Vec<NoteListItem>>();
        println!("{}", serde_json::to_string_pretty(&json_items)?);
    } else {
        for line in format_note_lines(&notes) {
            println!("{line}");
        }
    }

    Ok(())
}

async fn run_edit(id: &str, db_path: &Path) -> Result<(), CliError> {
    let normalized_id = normalize_note_identifier(id)?;
    let db = open_database(db_path).await?;
    let note = resolve_note_for_edit(&normalized_id, &db).await?;

    let Some(edited_content) = capture_editor_input_with_initial(&note.content)? else {
        return Err(CliError::EmptyEditedContent);
    };

    if edited_content == note.content {
        println!("{}", note.id);
        return Ok(());
    }

    let repo = LibSqlNoteRepository::new(db.connection());
    let updated = repo.update(&note.id, &edited_content).await?;
    println!("{}", updated.id);
    Ok(())
}

async fn run_delete(id: &str, db_path: &Path) -> Result<(), CliError> {
    let normalized_id = normalize_note_identifier(id)?;
    let db = open_database(db_path).await?;
    let note = resolve_note_for_edit(&normalized_id, &db).await?;

    let repo = LibSqlNoteRepository::new(db.connection());
    repo.delete(&note.id).await?;
    println!("{}", note.id);
    Ok(())
}

async fn run_sync(db_path: &Path) -> Result<(), CliError> {
    let db = open_database_with_mode(db_path, OpenDatabaseMode::RequireSync).await?;
    if !db.is_sync_enabled() {
        return Err(CliError::SyncNotConfigured);
    }

    db.sync().await?;
    println!("Sync completed");
    Ok(())
}

async fn run_sync_conflicts(limit: usize, as_json: bool, db_path: &Path) -> Result<(), CliError> {
    let conflicts = list_sync_conflicts(limit, db_path).await?;

    if as_json {
        let items = conflicts
            .iter()
            .map(sync_conflict_to_item)
            .collect::<Vec<SyncConflictItem>>();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if conflicts.is_empty() {
        println!("No sync conflicts recorded.");
    } else {
        for line in format_sync_conflict_lines(&conflicts) {
            println!("{line}");
        }
    }

    Ok(())
}

fn run_completions(shell: CompletionShell, output_path: Option<&Path>) -> Result<(), CliError> {
    let mut command = Cli::command();
    let mut buffer = Vec::new();

    match shell {
        CompletionShell::Bash => generate_for_shell(shells::Bash, &mut command, &mut buffer),
        CompletionShell::Zsh => generate_for_shell(shells::Zsh, &mut command, &mut buffer),
        CompletionShell::Fish => generate_for_shell(shells::Fish, &mut command, &mut buffer),
    }

    if let Some(path) = output_path {
        std::fs::write(path, &buffer)?;
        println!("{}", path.display());
    } else {
        io::stdout().write_all(&buffer)?;
    }

    Ok(())
}

fn generate_for_shell<G: Generator>(
    generator: G,
    command: &mut clap::Command,
    buffer: &mut Vec<u8>,
) {
    generate(generator, command, "dirt", buffer);
}

async fn run_export(
    format: ExportFormat,
    output_path: Option<&Path>,
    db_path: &Path,
) -> Result<(), CliError> {
    let notes = list_all_notes(db_path).await?;
    let rendered = match format {
        ExportFormat::Json => render_json_export(&notes)?,
        ExportFormat::Markdown => render_markdown_export(&notes),
    };

    if let Some(path) = output_path {
        std::fs::write(path, rendered)?;
        println!("{}", path.display());
    } else {
        println!("{rendered}");
    }

    Ok(())
}

async fn list_notes(
    limit: usize,
    tag: Option<&str>,
    db_path: &Path,
) -> Result<Vec<Note>, CliError> {
    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());

    if let Some(tag_name) = tag {
        Ok(repo.list_by_tag(tag_name, limit, 0).await?)
    } else {
        Ok(repo.list(limit, 0).await?)
    }
}

async fn list_all_notes(db_path: &Path) -> Result<Vec<Note>, CliError> {
    const PAGE_SIZE: usize = 500;

    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());

    let mut notes = Vec::new();
    let mut offset = 0usize;

    loop {
        let batch = repo.list(PAGE_SIZE, offset).await?;
        let count = batch.len();
        notes.extend(batch);

        if count < PAGE_SIZE {
            break;
        }
        offset += count;
    }

    Ok(notes)
}

async fn search_notes(query: &str, limit: usize, db_path: &Path) -> Result<Vec<Note>, CliError> {
    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    Ok(repo.search(query, limit).await?)
}

async fn list_sync_conflicts(limit: usize, db_path: &Path) -> Result<Vec<SyncConflict>, CliError> {
    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    Ok(repo.list_conflicts(limit).await?)
}

async fn resolve_note_for_edit(note_query: &str, db: &Database) -> Result<Note, CliError> {
    let repo = LibSqlNoteRepository::new(db.connection());

    if let Ok(note_id) = note_query.parse::<NoteId>() {
        if let Some(note) = repo.get(&note_id).await? {
            return Ok(note);
        }
    }

    let mut rows = db
        .connection()
        .query(
            "SELECT id
             FROM notes
             WHERE is_deleted = 0 AND id LIKE ?
             ORDER BY updated_at DESC
             LIMIT ?",
            libsql::params![format!("{note_query}%"), 3i64],
        )
        .await?;

    let mut matching_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        matching_ids.push(id);
    }

    match matching_ids.len() {
        0 => Err(CliError::NoteNotFound(note_query.to_string())),
        1 => {
            let resolved_id = matching_ids[0]
                .parse::<NoteId>()
                .map_err(|_| CliError::NoteNotFound(note_query.to_string()))?;
            repo.get(&resolved_id)
                .await?
                .ok_or_else(|| CliError::NoteNotFound(note_query.to_string()))
        }
        _ => {
            let options = matching_ids
                .iter()
                .take(3)
                .map(|id| id.chars().take(13).collect::<String>())
                .collect::<Vec<_>>()
                .join(", ");
            Err(CliError::AmbiguousNoteId(format!(
                "ID prefix '{note_query}' is ambiguous; matches: {options}"
            )))
        }
    }
}

fn format_note_lines(notes: &[Note]) -> Vec<String> {
    let now_ms = Utc::now().timestamp_millis();
    notes
        .iter()
        .map(|note| {
            let id = note.id.to_string();
            let short_id = id.chars().take(13).collect::<String>();
            let preview = note_preview(note, 40);
            let relative_time = format_relative_time(note.updated_at, now_ms);
            let tags = render_tags(note);

            if tags.is_empty() {
                format!("{short_id:<13}  {preview:<40}  {relative_time}")
            } else {
                format!("{short_id:<13}  {preview:<40}  {relative_time:<10}  {tags}")
            }
        })
        .collect()
}

fn note_to_list_item(note: &Note) -> NoteListItem {
    let now_ms = Utc::now().timestamp_millis();
    let mut tags = note.tags();
    tags.sort();

    NoteListItem {
        id: note.id.to_string(),
        preview: note_preview(note, 80),
        content: note.content.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
        relative_time: format_relative_time(note.updated_at, now_ms),
        tags,
    }
}

fn sync_conflict_to_item(conflict: &SyncConflict) -> SyncConflictItem {
    SyncConflictItem {
        id: conflict.id,
        note_id: conflict.note_id.clone(),
        local_updated_at: conflict.local_updated_at,
        incoming_updated_at: conflict.incoming_updated_at,
        resolved_at: conflict.resolved_at,
        resolved_at_iso: format_sync_timestamp(conflict.resolved_at),
        strategy: conflict.strategy.clone(),
    }
}

fn note_preview(note: &Note, max_chars: usize) -> String {
    let first_line = note.content.lines().next().unwrap_or("").trim();
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");

    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        let take_len = max_chars.saturating_sub(3);
        let mut truncated = collapsed.chars().take(take_len).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

fn render_tags(note: &Note) -> String {
    let mut tags = note.tags();
    tags.sort();
    tags.into_iter()
        .map(|tag| format!("#{tag}"))
        .collect::<Vec<String>>()
        .join(" ")
}

fn format_sync_conflict_lines(conflicts: &[SyncConflict]) -> Vec<String> {
    conflicts
        .iter()
        .map(|conflict| {
            format!(
                "{}  {:<4}  note={}  local={} incoming={}",
                format_sync_timestamp(conflict.resolved_at),
                conflict.strategy,
                conflict.note_id,
                conflict.local_updated_at,
                conflict.incoming_updated_at
            )
        })
        .collect()
}

fn format_sync_timestamp(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms).map_or_else(
        || timestamp_ms.to_string(),
        |dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

fn format_relative_time(timestamp_ms: i64, now_ms: i64) -> String {
    let diff = now_ms.saturating_sub(timestamp_ms);
    let minute = 60_000;
    let hour = 60 * minute;
    let day = 24 * hour;
    let week = 7 * day;
    let month = 30 * day;
    let year = 365 * day;

    if diff < minute {
        "just now".to_string()
    } else if diff < hour {
        format!("{}m ago", diff / minute)
    } else if diff < day {
        format!("{}h ago", diff / hour)
    } else if diff < week {
        format!("{}d ago", diff / day)
    } else if diff < month {
        format!("{}w ago", diff / week)
    } else if diff < year {
        format!("{}mo ago", diff / month)
    } else {
        format!("{}y ago", diff / year)
    }
}

fn resolve_note_content(content_parts: &[String]) -> Result<String, CliError> {
    if let Some(content) = normalize_content(&content_parts.join(" ")) {
        return Ok(content);
    }

    if let Some(content) = read_piped_stdin()? {
        return Ok(content);
    }

    if let Some(content) = capture_editor_input()? {
        return Ok(content);
    }

    Err(CliError::EmptyContent)
}

fn normalize_content(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_search_query(query: &str) -> Result<String, CliError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        Err(CliError::EmptySearchQuery)
    } else {
        Ok(trimmed.to_string())
    }
}

fn normalize_note_identifier(id: &str) -> Result<String, CliError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        Err(CliError::EmptyNoteId)
    } else {
        Ok(trimmed.to_string())
    }
}

fn read_piped_stdin() -> Result<Option<String>, CliError> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }

    let mut buffer = String::new();
    stdin.lock().read_to_string(&mut buffer)?;
    Ok(normalize_content(&buffer))
}

fn capture_editor_input() -> Result<Option<String>, CliError> {
    capture_editor_input_with_initial("")
}

fn capture_editor_input_with_initial(initial_content: &str) -> Result<Option<String>, CliError> {
    let editor = preferred_editor();
    let temp_file = create_temp_note_file_path();
    std::fs::write(&temp_file, initial_content)?;

    let launch_result = launch_editor(&editor, &temp_file);
    let note_content = std::fs::read_to_string(&temp_file)?;
    let _ = std::fs::remove_file(&temp_file);

    launch_result?;
    Ok(normalize_content(&note_content))
}

fn launch_editor(editor: &str, file_path: &Path) -> Result<(), CliError> {
    match Command::new(editor).arg(file_path).status() {
        Ok(status) => {
            if status.success() {
                Ok(())
            } else {
                Err(CliError::EditorFailed(format!(
                    "`{editor}` exited with status {status}"
                )))
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            // Fallback for editor commands with args, e.g. "code --wait"
            let mut parts = editor.split_whitespace();
            let Some(program) = parts.next() else {
                return Err(CliError::EditorFailed("empty EDITOR command".into()));
            };

            let mut command = Command::new(program);
            command.args(parts).arg(file_path);

            let status = command.status()?;
            if status.success() {
                Ok(())
            } else {
                Err(CliError::EditorFailed(format!(
                    "`{editor}` exited with status {status}"
                )))
            }
        }
        Err(err) => Err(CliError::Io(err)),
    }
}

fn preferred_editor() -> String {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| default_editor().to_string())
}

const fn default_editor() -> &'static str {
    if cfg!(windows) {
        "notepad"
    } else {
        "vi"
    }
}

fn create_temp_note_file_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    env::temp_dir().join(format!("dirt-note-{}-{now}.md", std::process::id()))
}

fn resolve_db_path(cli_db_path: Option<PathBuf>) -> PathBuf {
    cli_db_path
        .or_else(|| env::var_os("DIRT_DB_PATH").map(PathBuf::from))
        .unwrap_or_else(default_db_path)
}

fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dirt")
        .join("dirt.db")
}

fn sync_config_from_env() -> Option<SyncConfig> {
    let url = env::var("TURSO_DATABASE_URL").ok()?;
    let auth_token = env::var("TURSO_AUTH_TOKEN").ok()?;

    if url.is_empty() || auth_token.is_empty() {
        return None;
    }

    Some(SyncConfig::new(url, auth_token))
}

#[derive(Clone, Copy)]
enum OpenDatabaseMode {
    Standard,
    RequireSync,
}

impl OpenDatabaseMode {
    const fn requires_sync(self) -> bool {
        matches!(self, Self::RequireSync)
    }
}

async fn open_database(path: &Path) -> Result<Database, CliError> {
    open_database_with_mode(path, OpenDatabaseMode::Standard).await
}

async fn open_database_with_mode(
    path: &Path,
    mode: OpenDatabaseMode,
) -> Result<Database, CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let sync_config = if let Some(env_sync_config) = sync_config_from_env() {
        tracing::info!("Sync enabled with Turso");
        Some(env_sync_config)
    } else {
        sync_config_from_profile(mode).await?
    };

    if let Some(sync_config) = sync_config {
        let path_buf = path.to_path_buf();
        let db = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| dirt_core::Error::Database(error.to_string()))?;
                runtime.block_on(Database::open_with_sync(&path_buf, sync_config))
            })
            .map_err(|error| CliError::DatabaseInit(error.to_string()))?
            .join()
            .map_err(|_| CliError::DatabaseInit("sync initialization thread panicked".into()))??;

        Ok(db)
    } else {
        Ok(Database::open(path).await?)
    }
}

async fn sync_config_from_profile(mode: OpenDatabaseMode) -> Result<Option<SyncConfig>, CliError> {
    let config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(None);
    let Some(profile) = config.profile(&profile_name) else {
        return Ok(None);
    };
    let Some(endpoint) = profile.managed_sync_endpoint() else {
        return Ok(None);
    };

    let maybe_auth_service = SupabaseAuthService::new_for_profile(&profile_name, profile)
        .map_err(|error| CliError::Auth(error.to_string()))?;
    let mut session = if let Some(service) = maybe_auth_service.as_ref() {
        service
            .restore_session()
            .await
            .map_err(|error| CliError::Auth(error.to_string()))?
    } else {
        load_stored_session(&profile_name).map_err(|error| CliError::Auth(error.to_string()))?
    };

    if let Some(stored) = session.as_ref() {
        if stored.is_expired() {
            if let Some(service) = maybe_auth_service {
                session = service
                    .refresh_session(&stored.refresh_token)
                    .await
                    .map(Some)
                    .map_err(|error| CliError::Auth(error.to_string()))?;
            } else {
                clear_stored_session(&profile_name)
                    .map_err(|error| CliError::Auth(error.to_string()))?;
                session = None;
            }
        }
    }

    let Some(session) = session else {
        if mode.requires_sync() {
            return Err(CliError::SyncNotConfigured);
        }
        return Ok(None);
    };

    let sync_auth_client = ManagedSyncAuthClient::new(endpoint)
        .map_err(|error| CliError::ManagedSync(error.to_string()))?;
    let managed_token = match sync_auth_client.exchange_token(&session.access_token).await {
        Ok(token) => token,
        Err(error) => {
            if mode.requires_sync() {
                return Err(CliError::ManagedSync(error.to_string()));
            }
            tracing::warn!(
                "Managed sync token exchange failed for profile '{}': {}",
                profile_name,
                error
            );
            return Ok(None);
        }
    };

    tracing::info!("Managed sync enabled via profile '{}'", profile_name);
    Ok(Some(SyncConfig::new(
        managed_token.database_url,
        managed_token.auth_token,
    )))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository};
    use dirt_core::{Note, SyncConflict};
    use tokio::time::sleep;

    use super::{
        default_editor, format_relative_time, format_sync_conflict_lines, format_sync_timestamp,
        list_notes, normalize_bootstrap_url, normalize_content, normalize_note_identifier,
        normalize_search_query, note_preview, render_markdown_export, resolve_bootstrap_url,
        resolve_note_for_edit, run_completions, run_delete, run_export, run_sync, search_notes,
        CliError, CompletionShell, ExportFormat,
    };

    #[test]
    fn normalize_content_trims_and_rejects_empty() {
        assert_eq!(normalize_content("  hello  "), Some("hello".to_string()));
        assert_eq!(normalize_content(" \n\t "), None);
    }

    #[test]
    fn normalize_content_keeps_multiline_text() {
        assert_eq!(
            normalize_content("line 1\nline 2\n"),
            Some("line 1\nline 2".to_string())
        );
    }

    #[test]
    fn default_editor_is_defined() {
        assert!(!default_editor().is_empty());
    }

    #[test]
    fn normalize_bootstrap_url_requires_http_scheme() {
        assert!(
            normalize_bootstrap_url("https://api.example.com/v1/bootstrap".to_string()).is_ok()
        );
        assert!(normalize_bootstrap_url("api.example.com/v1/bootstrap".to_string()).is_err());
    }

    #[test]
    fn resolve_bootstrap_url_prefers_explicit_manifest_url() {
        let resolved = resolve_bootstrap_url(
            Some("https://api.example.com/v1/bootstrap".to_string()),
            Some("https://ignored.example.com".to_string()),
            Some("https://also-ignored.example.com".to_string()),
        )
        .unwrap();
        assert_eq!(
            resolved.as_deref(),
            Some("https://api.example.com/v1/bootstrap")
        );
    }

    #[test]
    fn resolve_bootstrap_url_derives_from_api_base() {
        let resolved =
            resolve_bootstrap_url(None, Some("https://api.example.com/".to_string()), None)
                .unwrap();
        assert_eq!(
            resolved.as_deref(),
            Some("https://api.example.com/v1/bootstrap")
        );
    }

    #[test]
    fn format_relative_time_units() {
        let now = 10_000_000;
        assert_eq!(format_relative_time(now - 30_000, now), "just now");
        assert_eq!(format_relative_time(now - 120_000, now), "2m ago");
        assert_eq!(format_relative_time(now - 2 * 60 * 60_000, now), "2h ago");
    }

    #[test]
    fn note_preview_truncates_with_ellipsis() {
        let note = dirt_core::Note::new("This is a very long sentence that should be shortened");
        let preview = note_preview(&note, 20);
        assert_eq!(preview, "This is a very lo...");
    }

    #[test]
    fn format_sync_timestamp_returns_utc_label() {
        assert_eq!(format_sync_timestamp(0), "1970-01-01 00:00:00 UTC");
    }

    #[test]
    fn format_sync_conflict_lines_include_key_fields() {
        let conflicts = vec![SyncConflict {
            id: 1,
            note_id: "11111111-1111-7111-8111-111111111111".to_string(),
            local_updated_at: 200,
            incoming_updated_at: 100,
            resolved_at: 300,
            strategy: "lww".to_string(),
        }];

        let rendered = format_sync_conflict_lines(&conflicts);
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].contains("lww"));
        assert!(rendered[0].contains("note=11111111-1111-7111-8111-111111111111"));
        assert!(rendered[0].contains("local=200"));
        assert!(rendered[0].contains("incoming=100"));
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn list_notes_respects_limit_and_tag_filter() {
        let db_path = unique_test_db_path();
        {
            let db = Database::open(&db_path).await.unwrap();
            let repo = LibSqlNoteRepository::new(db.connection());

            repo.create("First #work").await.unwrap();
            sleep(Duration::from_millis(2)).await;
            repo.create("Second #personal").await.unwrap();
            sleep(Duration::from_millis(2)).await;
            repo.create("Third #work").await.unwrap();
        }

        let recent = list_notes(2, None, &db_path).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].content, "Third #work");
        assert_eq!(recent[1].content, "Second #personal");

        let work_only = list_notes(10, Some("work"), &db_path).await.unwrap();
        assert_eq!(work_only.len(), 2);
        assert!(work_only.iter().all(|note| note.content.contains("#work")));

        cleanup_db_files(&db_path);
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn search_notes_finds_matches_with_limit() {
        let db_path = unique_test_db_path();
        {
            let db = Database::open(&db_path).await.unwrap();
            let repo = LibSqlNoteRepository::new(db.connection());

            repo.create("Milk and eggs").await.unwrap();
            sleep(Duration::from_millis(2)).await;
            repo.create("Milkshake recipe").await.unwrap();
            sleep(Duration::from_millis(2)).await;
            repo.create("Unrelated note").await.unwrap();
        }

        let matches = search_notes("milk", 1, &db_path).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].content.to_lowercase().contains("milk"));

        cleanup_db_files(&db_path);
    }

    #[test]
    fn normalize_search_query_rejects_empty() {
        assert!(normalize_search_query(" \n\t ").is_err());
        assert_eq!(
            normalize_search_query("  exact phrase  ").unwrap(),
            "exact phrase"
        );
    }

    #[test]
    fn normalize_note_identifier_rejects_empty() {
        assert!(matches!(
            normalize_note_identifier(" \n "),
            Err(CliError::EmptyNoteId)
        ));
        assert_eq!(
            normalize_note_identifier("  abc123  ").unwrap(),
            "abc123".to_string()
        );
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn resolve_note_for_edit_supports_exact_and_prefix_id() {
        let db_path = unique_test_db_path();
        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());

        let note_a = Note {
            id: "11111111-1111-7111-8111-111111111111".parse().unwrap(),
            content: "Note A".to_string(),
            created_at: 1000,
            updated_at: 1000,
            is_deleted: false,
        };
        let note_b = Note {
            id: "11111111-1111-7111-8111-222222222222".parse().unwrap(),
            content: "Note B".to_string(),
            created_at: 1001,
            updated_at: 1001,
            is_deleted: false,
        };
        repo.create_with_note(&note_a).await.unwrap();
        repo.create_with_note(&note_b).await.unwrap();

        let by_exact = resolve_note_for_edit("11111111-1111-7111-8111-111111111111", &db)
            .await
            .unwrap();
        assert_eq!(by_exact.content, "Note A");

        let by_prefix = resolve_note_for_edit("11111111-1111-7111-8111-2", &db)
            .await
            .unwrap();
        assert_eq!(by_prefix.content, "Note B");

        cleanup_db_files(&db_path);
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn resolve_note_for_edit_rejects_ambiguous_prefix() {
        let db_path = unique_test_db_path();
        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());

        let note_a = Note {
            id: "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa".parse().unwrap(),
            content: "Left".to_string(),
            created_at: 1000,
            updated_at: 1000,
            is_deleted: false,
        };
        let note_b = Note {
            id: "aaaaaaaa-aaaa-7aaa-8aaa-bbbbbbbbbbbb".parse().unwrap(),
            content: "Right".to_string(),
            created_at: 1001,
            updated_at: 1001,
            is_deleted: false,
        };
        repo.create_with_note(&note_a).await.unwrap();
        repo.create_with_note(&note_b).await.unwrap();

        let error = resolve_note_for_edit("aaaaaaaa-aaaa-7aaa-8aaa", &db)
            .await
            .unwrap_err();
        assert!(matches!(error, CliError::AmbiguousNoteId(_)));

        cleanup_db_files(&db_path);
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn resolve_note_for_edit_rejects_missing_note() {
        let db_path = unique_test_db_path();
        let db = Database::open(&db_path).await.unwrap();

        let error = resolve_note_for_edit("does-not-exist", &db)
            .await
            .unwrap_err();
        assert!(matches!(error, CliError::NoteNotFound(_)));

        cleanup_db_files(&db_path);
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn run_delete_soft_deletes_note_by_exact_and_prefix_id() {
        let db_path = unique_test_db_path();
        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());

        let note_a = Note {
            id: "bbbbbbbb-bbbb-7bbb-8bbb-111111111111".parse().unwrap(),
            content: "Keep me".to_string(),
            created_at: 1000,
            updated_at: 1000,
            is_deleted: false,
        };
        let note_b = Note {
            id: "bbbbbbbb-bbbb-7bbb-8bbb-222222222222".parse().unwrap(),
            content: "Delete me".to_string(),
            created_at: 1001,
            updated_at: 1001,
            is_deleted: false,
        };
        repo.create_with_note(&note_a).await.unwrap();
        repo.create_with_note(&note_b).await.unwrap();
        drop(db);

        run_delete("bbbbbbbb-bbbb-7bbb-8bbb-2", &db_path)
            .await
            .unwrap();

        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());
        assert!(repo.get(&note_b.id).await.unwrap().is_none());
        assert!(repo.get(&note_a.id).await.unwrap().is_some());
        drop(db);

        run_delete("bbbbbbbb-bbbb-7bbb-8bbb-111111111111", &db_path)
            .await
            .unwrap();

        let db = Database::open(&db_path).await.unwrap();
        let repo = LibSqlNoteRepository::new(db.connection());
        assert!(repo.get(&note_a.id).await.unwrap().is_none());

        cleanup_db_files(&db_path);
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn run_sync_requires_sync_configuration() {
        let db_path = unique_test_db_path();

        let error = run_sync(&db_path).await.unwrap_err();
        assert!(matches!(error, CliError::SyncNotConfigured));

        cleanup_db_files(&db_path);
    }

    #[test]
    fn note_to_export_item_sorts_tags() {
        let note = Note::new("#zeta test #alpha #beta");
        let export = dirt_core::export::note_to_export_item(&note);

        assert_eq!(export.tags, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn render_markdown_export_includes_frontmatter_and_content() {
        let note = Note {
            id: "cccccccc-cccc-7ccc-8ccc-111111111111".parse().unwrap(),
            content: "Hello export #tag".to_string(),
            created_at: 123,
            updated_at: 456,
            is_deleted: false,
        };

        let rendered = render_markdown_export(&[note]);
        assert!(rendered.contains("id: cccccccc-cccc-7ccc-8ccc-111111111111"));
        assert!(rendered.contains("created_at: 123"));
        assert!(rendered.contains("updated_at: 456"));
        assert!(rendered.contains("tags:\n  - tag"));
        assert!(rendered.contains("Hello export #tag"));
    }

    #[cfg_attr(windows, ignore = "libsql integration is flaky on windows CI")]
    #[tokio::test(flavor = "current_thread")]
    async fn run_export_writes_json_file() {
        let db_path = unique_test_db_path();
        {
            let db = Database::open(&db_path).await.unwrap();
            let repo = LibSqlNoteRepository::new(db.connection());
            repo.create("Export me #one").await.unwrap();
        }

        let output_path = std::env::temp_dir().join(format!(
            "dirt-export-test-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));

        run_export(ExportFormat::Json, Some(&output_path), &db_path)
            .await
            .unwrap();

        let exported = std::fs::read_to_string(&output_path).unwrap();
        assert!(exported.contains("\"content\": \"Export me #one\""));
        assert!(exported.contains("\"tags\": [\n      \"one\"\n    ]"));

        let _ = std::fs::remove_file(output_path);
        cleanup_db_files(&db_path);
    }

    #[test]
    fn run_completions_writes_bash_script_file() {
        let output_path = std::env::temp_dir().join(format!(
            "dirt-completions-test-{}.bash",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));

        run_completions(CompletionShell::Bash, Some(&output_path)).unwrap();

        let script = std::fs::read_to_string(&output_path).unwrap();
        assert!(script.contains("_dirt()"));
        assert!(script.contains("complete -F _dirt"));
        assert!(script.contains(" default dirt"));

        let _ = std::fs::remove_file(output_path);
    }

    fn unique_test_db_path() -> PathBuf {
        static NEXT_TEST_DB_ID: AtomicU64 = AtomicU64::new(0);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let sequence = NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("dirt-cli-list-test-{timestamp}-{sequence}.db"))
    }

    fn cleanup_db_files(path: &PathBuf) {
        // On Windows, libsql can keep file handles alive briefly after drop.
        // Removing test DB files eagerly can trigger intermittent access violations.
        if cfg!(windows) {
            return;
        }

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }
}
