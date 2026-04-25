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

    /// CLI profile name for managed auth/sync configuration
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
    /// Sync local replica with remote Turso database
    Sync,
    /// Configure CLI managed profiles
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
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
