//! Dirt CLI - Command-line interface for capturing fleeting thoughts
//!
//! Quick capture from the terminal with minimal friction.

use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{CommandFactory, Parser, Subcommand};
use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository, SyncConfig};
use thiserror::Error;

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
    },
    /// Search notes
    Search {
        /// Search query
        query: String,
    },
    /// Open TUI interface
    Tui,
}

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Core(#[from] dirt_core::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("No note content provided")]
    EmptyContent,
    #[error("Editor command failed: {0}")]
    EditorFailed(String),
    #[error("Database initialization failed: {0}")]
    DatabaseInit(String),
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

    match cli.command {
        Some(Commands::Add { content }) => run_add(&content, &db_path).await?,
        Some(Commands::List { limit }) => {
            println!("Listing {limit} recent notes...");
            // TODO: Implement note listing
        }
        Some(Commands::Search { query }) => {
            println!("Searching for: {query}");
            // TODO: Implement search
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

async fn run_add(content_parts: &[String], db_path: &Path) -> Result<(), CliError> {
    let content = resolve_note_content(content_parts)?;

    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    let note = repo.create(&content).await?;

    println!("{}", note.id);
    Ok(())
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
    let editor = preferred_editor();
    let temp_file = create_temp_note_file_path();
    std::fs::write(&temp_file, "")?;

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

fn default_editor() -> &'static str {
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

async fn open_database(path: &Path) -> Result<Database, CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if let Some(sync_config) = sync_config_from_env() {
        tracing::info!("Sync enabled with Turso");
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

#[cfg(test)]
mod tests {
    use super::{default_editor, normalize_content};

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
}
