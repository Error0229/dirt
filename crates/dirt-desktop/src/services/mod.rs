//! Application services
//!
//! Opening the local note store, exporting notes, and handing a note's
//! file to another program.

mod database;
mod export;
mod files;

pub use database::{data_folder, open_local_store};
pub use export::{export_notes_to_path, suggested_export_file_name, NotesExportFormat};
pub use files::{clear_opened_copies, copy_for_opening, open_with_system};
