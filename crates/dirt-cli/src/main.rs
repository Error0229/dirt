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
    let global_profile = config_profiles::normalize_profile_name(cli.profile.as_deref());
    if let Some(profile) = &global_profile {
        env::set_var("DIRT_PROFILE", profile);
    }

    // Per-command DB scope resolution. Commands that don't touch
    // the notes DB (`auth`, `config`, `completions`, the empty-help
    // path) intentionally skip `resolve_db_scope` so a corrupt or
    // unreadable `state.json` doesn't block recovery flows like
    // `dirt auth login`. Only DB-bearing commands pay the
    // resolution cost — and only those will surface a Config error
    // if the state file is broken.
    let cli_db_path = cli.db_path.clone();
    let resolve = || commands::common::resolve_db_scope(cli_db_path.clone());

    match cli.command {
        Some(Commands::Add { content }) => {
            let scope = resolve().await?;
            commands::add::run_add(&content, &scope).await?;
        }
        Some(Commands::List { limit, tag, json }) => {
            let scope = resolve().await?;
            commands::list::run_list(limit, tag.as_deref(), json, &scope).await?;
        }
        Some(Commands::Search { query, limit, json }) => {
            let scope = resolve().await?;
            commands::search::run_search(&query, limit, json, &scope).await?;
        }
        Some(Commands::Edit { id }) => {
            let scope = resolve().await?;
            commands::edit::run_edit(&id, &scope).await?;
        }
        Some(Commands::Delete { id }) => {
            let scope = resolve().await?;
            commands::delete::run_delete(&id, &scope).await?;
        }
        Some(Commands::Export { format, output }) => {
            let scope = resolve().await?;
            commands::export::run_export(format, output.as_deref(), &scope).await?;
        }
        Some(Commands::Completions { shell, output }) => {
            commands::completions::run_completions(shell, output.as_deref())?;
        }
        Some(Commands::Sync) => {
            let scope = resolve().await?;
            commands::sync::run_sync(&scope).await?;
        }
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
                let scope = resolve().await?;
                commands::add::run_add(&cli.note, &scope).await?;
            }
        }
    }

    Ok(())
}
