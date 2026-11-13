//! `dirt export`: every note, as one text or as a folder with the files.

use std::path::Path;

use dirt_core::export::{Export, ExportError, ExportFormat as CoreExportFormat};

use crate::cli::ExportFormat;
use crate::context::Context;
use crate::error::CliError;

/// Exports every note.
///
/// With `folder`, the notes and their files are written into it, and its
/// path is printed. Otherwise the notes' text is printed, or written to
/// `output`: it names each note's files and holds none, and a line on
/// standard error says so if there are any.
///
/// A folder that could not be given every file is still written, since
/// what it holds is good. But the command fails, having said which files
/// it lacks and why: whoever keeps the folder as their copy of the notes
/// must not be told that all is well.
pub async fn run_export(
    format: ExportFormat,
    output: Option<&Path>,
    folder: Option<&Path>,
    context: &Context,
) -> Result<(), CliError> {
    let format = match format {
        ExportFormat::Json => CoreExportFormat::Json,
        ExportFormat::Markdown => CoreExportFormat::Markdown,
    };
    let Some(folder) = folder else {
        return text_alone(format, output, context).await;
    };

    // Said now, before files are fetched for nothing. It is looked at
    // again when the folder is written.
    if folder.symlink_metadata().is_ok() {
        return Err(already_there(folder));
    }
    let export = Export::with_files(&context.store, context.session.as_ref(), format)
        .await
        .map_err(failed)?;
    export.write_folder(folder).map_err(failed)?;
    println!("{}", folder.display());
    eprintln!("Exported {}.", export.counts());
    let left_out = export.left_out_lines(usize::MAX);
    if left_out.is_empty() {
        Ok(())
    } else {
        Err(CliError::Files(left_out.join("\n")))
    }
}

async fn text_alone(
    format: CoreExportFormat,
    output: Option<&Path>,
    context: &Context,
) -> Result<(), CliError> {
    let export = Export::notes_only(&context.store, format)
        .await
        .map_err(failed)?;
    let text = export.notes_text().map_err(failed)?;
    if let Some(path) = output {
        std::fs::write(path, text)?;
        println!("{}", path.display());
    } else {
        println!("{text}");
    }
    if let Some(line) = files_not_here(export.files_not_held()) {
        eprintln!("{line}");
    }
    Ok(())
}

/// What to say when the notes name files and the text alone was asked
/// for.
pub fn files_not_here(files: usize) -> Option<String> {
    let files = match files {
        0 => return None,
        1 => "1 file attached to these notes is".to_owned(),
        many => format!("{many} files attached to these notes are"),
    };
    Some(format!(
        "{files} named here and not exported. \
         `dirt export --folder <folder>` writes the notes and their files."
    ))
}

fn already_there(path: &Path) -> CliError {
    CliError::Files(format!(
        "{} is already there. An export is written to a new folder: give a name nothing has",
        path.display()
    ))
}

fn failed(error: ExportError) -> CliError {
    match error {
        ExportError::Store(error) => CliError::Core(error),
        ExportError::Text(error) => CliError::Serialization(error),
        ExportError::Io(error) => CliError::Io(error),
        ExportError::AlreadyThere(path) => already_there(&path),
    }
}
