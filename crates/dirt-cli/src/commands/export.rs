use std::path::Path;

use dirt_core::export::{render_notes_export, ExportFormat as CoreExportFormat};

use crate::cli::ExportFormat;
use crate::commands::common::list_all_notes;
use crate::error::CliError;

pub async fn run_export(
    format: ExportFormat,
    output_path: Option<&Path>,
    db_path: &Path,
) -> Result<(), CliError> {
    let notes = list_all_notes(db_path).await?;
    let core_format = match format {
        ExportFormat::Json => CoreExportFormat::Json,
        ExportFormat::Markdown => CoreExportFormat::Markdown,
    };
    let rendered = render_notes_export(&notes, core_format)?;

    if let Some(path) = output_path {
        std::fs::write(path, rendered)?;
        println!("{}", path.display());
    } else {
        println!("{rendered}");
    }

    Ok(())
}
