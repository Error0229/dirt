//! Application services
//!
//! Opening the local note store and exporting notes.

mod database;
mod export;

pub use database::{data_folder, open_local_store};
pub use export::{export_notes_to_path, suggested_export_file_name, NotesExportFormat};
