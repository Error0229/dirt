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
pub enum SyncCommands {
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
pub enum ConfigCommands {
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
pub enum AuthCommands {
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
