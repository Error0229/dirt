//! Dirt CLI - Command-line interface for capturing fleeting thoughts
//!
//! Quick capture from the terminal with minimal friction.

mod cli;
mod commands;
mod config_profiles;
mod context;
mod error;
#[cfg(test)]
mod tests;

use std::env;

use clap::{CommandFactory, Parser};

use crate::cli::{Cli, Commands};
use crate::context::Context;
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

    // These need no note store, so they work even when it cannot be opened.
    let command = match cli.command {
        Some(Commands::Completions { shell, output }) => {
            return commands::completions::run_completions(shell, output.as_deref());
        }
        Some(Commands::Config { command }) => {
            return commands::config::run_config(command, global_profile.as_deref());
        }
        Some(Commands::Tui) => {
            println!("Opening TUI...");
            return Ok(());
        }
        None if cli.note.is_empty() => {
            Cli::command().print_help().map_err(CliError::Io)?;
            println!();
            return Ok(());
        }
        other => other,
    };

    let context = Context::open(cli.db_path)?;
    let store = &context.store;
    match command {
        Some(Commands::Add { content }) => commands::add::run_add(&content, &context).await?,
        Some(Commands::List { limit, tag, json }) => {
            commands::list::run_list(limit, tag.as_deref(), json, store).await?;
        }
        Some(Commands::Search { query, limit, json }) => {
            commands::search::run_search(&query, limit, json, store).await?;
        }
        Some(Commands::Edit { id }) => commands::edit::run_edit(&id, &context).await?,
        Some(Commands::Delete { id }) => commands::delete::run_delete(&id, &context).await?,
        Some(Commands::Export { format, output }) => {
            commands::export::run_export(format, output.as_deref(), store).await?;
        }
        Some(Commands::Sync) => commands::sync::run_sync(&context).await?,
        Some(Commands::Auth { command }) => {
            commands::account::run_auth(command, context).await?;
        }
        Some(Commands::Devices { command }) => {
            commands::devices::run_devices(command, &context).await?;
        }
        Some(Commands::Completions { .. } | Commands::Config { .. } | Commands::Tui) => {
            unreachable!("handled before the store was opened")
        }
        None => commands::add::run_add(&cli.note, &context).await?,
    }

    Ok(())
}
