//! Dirt CLI - Command-line interface for capturing fleeting thoughts
//!
//! Quick capture from the terminal with minimal friction.

mod cli;
mod commands;
mod config_profiles;
mod error;
#[cfg(test)]
mod tests;

use std::env;

use clap::{CommandFactory, Parser};

use crate::cli::{Cli, Commands};
use crate::error::CliError;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), CliError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=info".parse().expect("valid directive")),
        )
        .init();

    let cli = Cli::parse();
    let scope = commands::common::resolve_db_scope(cli.db_path).await?;
    let global_profile = config_profiles::normalize_profile_name(cli.profile.as_deref());
    if let Some(profile) = &global_profile {
        env::set_var("DIRT_PROFILE", profile);
    }

    match cli.command {
        Some(Commands::Add { content }) => commands::add::run_add(&content, &scope).await?,
        Some(Commands::List { limit, tag, json }) => {
            commands::list::run_list(limit, tag.as_deref(), json, &scope).await?;
        }
        Some(Commands::Search { query, limit, json }) => {
            commands::search::run_search(&query, limit, json, &scope).await?;
        }
        Some(Commands::Edit { id }) => commands::edit::run_edit(&id, &scope).await?,
        Some(Commands::Delete { id }) => commands::delete::run_delete(&id, &scope).await?,
        Some(Commands::Export { format, output }) => {
            commands::export::run_export(format, output.as_deref(), &scope).await?;
        }
        Some(Commands::Completions { shell, output }) => {
            commands::completions::run_completions(shell, output.as_deref())?;
        }
        Some(Commands::Sync) => commands::sync::run_sync(&scope).await?,
        Some(Commands::Config { command }) => {
            commands::config::run_config(command, global_profile.as_deref())?;
        }
        Some(Commands::Auth { command }) => {
            commands::auth_cmd::run_auth(command).await?;
        }
        Some(Commands::Tui) => {
            println!("Opening TUI...");
        }
        None => {
            if cli.note.is_empty() {
                Cli::command().print_help().map_err(CliError::Io)?;
                println!();
            } else {
                commands::add::run_add(&cli.note, &scope).await?;
            }
        }
    }

    Ok(())
}
