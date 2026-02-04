//! Dirt CLI - Command-line interface for capturing fleeting thoughts
//!
//! Quick capture from the terminal with minimal friction.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dirt")]
#[command(about = "Capture fleeting thoughts from the command line")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Quick capture: dirt "my thought here"
    #[arg(trailing_var_arg = true)]
    note: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new note
    New {
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

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::New { content }) => {
            let note_content = content.join(" ");
            println!("Creating note: {note_content}");
            // TODO: Implement note creation
        }
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
                println!("Usage: dirt <note> or dirt --help");
            } else {
                let note_content = cli.note.join(" ");
                println!("Quick capture: {note_content}");
                // TODO: Implement quick capture
            }
        }
    }
}
