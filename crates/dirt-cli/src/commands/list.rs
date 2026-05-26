use crate::commands::common::{
    format_note_lines, list_notes, note_to_list_item, DbScope, NoteListItem,
};
use crate::error::CliError;

pub async fn run_list(
    limit: usize,
    tag: Option<&str>,
    as_json: bool,
    scope: &DbScope,
) -> Result<(), CliError> {
    let notes = list_notes(limit, tag, scope).await?;

    if as_json {
        let json_items = notes
            .iter()
            .map(note_to_list_item)
            .collect::<Vec<NoteListItem>>();
        println!("{}", serde_json::to_string_pretty(&json_items)?);
    } else {
        for line in format_note_lines(&notes) {
            println!("{line}");
        }
    }

    Ok(())
}
