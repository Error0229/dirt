use crate::commands::common::{
    capture_editor_input_with_initial, normalize_note_identifier, open_database,
    resolve_note_for_edit, DbScope,
};
use crate::error::CliError;

pub async fn run_edit(id: &str, scope: &DbScope) -> Result<(), CliError> {
    let normalized_id = normalize_note_identifier(id)?;
    let db = open_database(scope).await?;
    let note = resolve_note_for_edit(&normalized_id, &db).await?;

    let Some(edited_content) = capture_editor_input_with_initial(&note.content)? else {
        return Err(CliError::EmptyEditedContent);
    };

    if edited_content == note.content {
        println!("{}", note.id);
        return Ok(());
    }

    let updated = db.update_note(&note.id, &edited_content).await?;
    println!("{}", updated.id);
    Ok(())
}
