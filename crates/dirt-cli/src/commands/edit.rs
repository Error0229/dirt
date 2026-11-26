//! `dirt edit`: a note in the person's editor.

use std::io::IsTerminal;

use dirt_core::Note;

use crate::commands::common::{
    edit_in_editor, note_file_name, preferred_editor, report, Editor, RETURNED_AT_ONCE,
};
use crate::context::Context;
use crate::error::CliError;
use crate::naming::note_named_or_latest;

/// What an edit came to.
#[derive(Debug)]
pub enum EditOutcome {
    /// The note, with what the editor left.
    Saved(Note),
    /// The editor left the text as it was, and nothing was written.
    Unchanged(Note),
}

/// Opens the note named, or the latest, in the person's editor, and saves
/// what the editor leaves if it differs. Says "Saved" or "Unchanged" with
/// the note's first words; to a script, the note's ID either way, and
/// "Unchanged" on standard error.
pub async fn run_edit(note: &[String], context: &Context) -> Result<(), CliError> {
    match edit_with(&preferred_editor(), note, context).await? {
        EditOutcome::Saved(note) => {
            report("Saved", &note, None);
            context.sync_after_write().await;
        }
        EditOutcome::Unchanged(note) => {
            report("Unchanged", &note, None);
            if !std::io::stdout().is_terminal() {
                eprintln!("Unchanged");
            }
        }
    }
    Ok(())
}

/// [`run_edit`] with the editor given, and nothing said.
pub async fn edit_with(
    editor: &Editor,
    note: &[String],
    context: &Context,
) -> Result<EditOutcome, CliError> {
    let note = note_named_or_latest(note, context).await?;

    let edited = edit_in_editor(editor, &note.content, &note_file_name(Some(&note)))?;
    let Some(edited_content) = edited.text else {
        return Err(CliError::EmptyEditedContent);
    };

    // Space around the text is not kept, so it is not a change either.
    if edited_content == note.content.trim() {
        if edited.at_once {
            eprintln!("{RETURNED_AT_ONCE}");
        }
        return Ok(EditOutcome::Unchanged(note));
    }

    // The changes from the text the editor was given, made to the text the
    // note has now: what another device or the desktop app wrote while the
    // editor was open is kept.
    let updated = context
        .store
        .update_note_from(&note.id, &note.content, &edited_content)
        .await?;
    Ok(EditOutcome::Saved(updated))
}
