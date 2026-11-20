//! Dirt CLI - Command-line interface for capturing fleeting thoughts
//!
//! Quick capture from the terminal with minimal friction.

mod cli;
mod commands;
mod context;
mod error;
#[cfg(test)]
mod tests;

use clap::{CommandFactory, Parser};

use crate::cli::{Cli, Commands};
use crate::context::{Context, WAITING_TO_BE_LET_IN};
use crate::error::CliError;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), CliError> {
    // The log goes where errors go. What a command prints on standard
    // output is its answer, which a script may be reading: the ID of a new
    // note, a list as JSON. Only errors are logged unless `RUST_LOG` asks
    // for more: what a person needs to know, the CLI says in words, once
    // (issue #314).
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
        )
        .init();

    let cli = Cli::parse();

    // These need no note store, so they work even when it cannot be opened.
    let command = match cli.command {
        Some(Commands::Completions { shell, output }) => {
            return commands::completions::run_completions(shell, output.as_deref());
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
    // A device that waits to be let in reads none of the account's notes,
    // and a list of nothing would not say why. Said where warnings go, so
    // that a script reading the answer is not misled by it. `dirt sync`
    // says it as its error.
    let about_the_account = matches!(
        command,
        Some(Commands::Auth { .. } | Commands::Devices { .. } | Commands::Sync { .. })
    );
    if !about_the_account && context.waiting_to_be_let_in() {
        eprintln!("{WAITING_TO_BE_LET_IN}");
    }
    match command {
        Some(Commands::Add { content }) => commands::add::run_add(&content, &context).await?,
        Some(Commands::List {
            limit,
            tag,
            archived,
            json,
        }) => {
            commands::list::run_list(limit, tag.as_deref(), archived, json, store).await?;
        }
        Some(Commands::Search { query, limit, json }) => {
            commands::search::run_search(&query, limit, json, store).await?;
        }
        Some(Commands::Edit { id }) => commands::edit::run_edit(&id, &context).await?,
        Some(Commands::Delete { id }) => commands::delete::run_delete(&id, &context).await?,
        Some(Commands::Restore { id }) => commands::restore::run_restore(&id, &context).await?,
        Some(Commands::Archive { id }) => {
            commands::archive::run_archive(&id, &context).await?;
        }
        Some(Commands::Unarchive { id }) => {
            commands::archive::run_unarchive(&id, &context).await?;
        }
        Some(Commands::Attach {
            note,
            file,
            name,
            media_type,
        }) => {
            commands::files::run_attach(
                &note,
                &file,
                name.as_deref(),
                media_type.as_deref(),
                &context,
            )
            .await?;
        }
        Some(Commands::Files { note, json }) => {
            commands::files::run_files(&note, json, store).await?;
        }
        Some(Commands::Save {
            note,
            file,
            path,
            force,
        }) => {
            commands::files::run_save(&note, &file, path.as_deref(), force, &context).await?;
        }
        Some(Commands::Detach { note, file }) => {
            commands::files::run_detach(&note, &file, &context).await?;
        }
        Some(Commands::Export {
            format,
            output,
            folder,
        }) => {
            commands::export::run_export(format, output.as_deref(), folder.as_deref(), &context)
                .await?;
        }
        Some(Commands::Sync { verbose }) => commands::sync::run_sync(&context, verbose).await?,
        Some(Commands::Auth { command }) => {
            commands::account::run_auth(command, context).await?;
        }
        Some(Commands::Devices { command }) => {
            commands::devices::run_devices(command, &context).await?;
        }
        Some(Commands::Completions { .. } | Commands::Tui) => {
            unreachable!("handled before the store was opened")
        }
        None => commands::add::run_add(&cli.note, &context).await?,
    }

    Ok(())
}
