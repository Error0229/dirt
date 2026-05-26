use crate::commands::common::{
    format_note_lines, normalize_search_query, note_to_list_item, search_notes, DbScope,
    NoteListItem,
};
use crate::error::CliError;

pub async fn run_search(
    query: &str,
    limit: usize,
    as_json: bool,
    scope: &DbScope,
) -> Result<(), CliError> {
    let normalized_query = normalize_search_query(query)?;
    let notes = search_notes(&normalized_query, limit, scope).await?;

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
