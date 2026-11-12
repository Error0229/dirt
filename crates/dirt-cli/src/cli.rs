use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "dirt")]
#[command(about = "Capture fleeting thoughts from the command line")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Optional path to local database file
    #[arg(long, value_name = "PATH")]
    pub db_path: Option<PathBuf>,

    /// CLI profile name, which selects the server to sync with
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// Quick capture: dirt "my thought here"
    #[arg(trailing_var_arg = true)]
    pub note: Vec<String>,
}

#[derive(Subcommand)]
pub enum Commands {
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
    /// Sync notes with the account's other devices now
    Sync,
    /// Configure CLI managed profiles
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Sign in, sign out, or show the account's state
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Open TUI interface
    Tui,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ExportFormat {
    Json,
    Markdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Show who is signed in and how sync is going
    Status,
    /// Sign in through the browser. The desktop app on this computer is
    /// signed in by this too
    Login {
        /// Move notes written before signing in into the account without
        /// asking
        #[arg(long)]
        move_local_notes: bool,
    },
    /// Sign out and remove the account's notes and keys from this computer
    Logout {
        /// Sign out even if some notes have not reached the server; they
        /// are deleted
        #[arg(long)]
        discard_unsynced: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Initialize or update profile config
    Init {
        /// Profile name to initialize
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Backend API base URL (e.g. <https://dirt-api.vercel.app>)
        #[arg(long, value_name = "URL")]
        api_base_url: Option<String>,
        /// Keep current active profile instead of activating this one
        #[arg(long)]
        no_activate: bool,
    },
}
