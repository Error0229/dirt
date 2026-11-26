use dirt_core::Store;

use crate::commands::common::{
    normalize_search_query, note_to_list_item, search_notes, NoteListItem,
};
use crate::commands::list::print_listing;
use crate::error::CliError;

pub async fn run_search(
    query: &str,
    limit: usize,
    as_json: bool,
    db: &Store,
) -> Result<(), CliError> {
    let normalized_query = normalize_search_query(query)?;
    let notes = search_notes(&normalized_query, limit, db).await?;

    if as_json {
        let json_items = notes
            .iter()
            .map(note_to_list_item)
            .collect::<Vec<NoteListItem>>();
        println!("{}", serde_json::to_string_pretty(&json_items)?);
        return Ok(());
    }
    print_listing(&notes, &format!("`dirt search {normalized_query}`"), db).await
}
