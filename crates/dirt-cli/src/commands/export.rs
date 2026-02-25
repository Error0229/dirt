use std::path::Path;

use dirt_core::export::{render_json_export, render_markdown_export};

use crate::cli::ExportFormat;
use crate::commands::common::list_all_notes;
use crate::error::CliError;

pub async fn run_export(
    format: ExportFormat,
    output_path: Option<&Path>,
    db_path: &Path,
) -> Result<(), CliError> {
    let notes = list_all_notes(db_path).await?;
    let rendered = match format {
        ExportFormat::Json => render_json_export(&notes)?,
        ExportFormat::Markdown => render_markdown_export(&notes),
    };

    if let Some(path) = output_path {
        std::fs::write(path, rendered)?;
        println!("{}", path.display());
    } else {
        println!("{rendered}");
    }

    Ok(())
}
