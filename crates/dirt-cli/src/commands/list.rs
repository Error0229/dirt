use dirt_core::{Note, Store};

use crate::commands::common::{list_notes, listing_lines, note_to_list_item, NoteListItem};
use crate::error::CliError;
use crate::naming::remember_list;

pub async fn run_list(
    limit: usize,
    tag: Option<&str>,
    archived: bool,
    as_json: bool,
    db: &Store,
) -> Result<(), CliError> {
    let notes = list_notes(limit, tag, archived, db).await?;

    if as_json {
        let json_items = notes
            .iter()
            .map(note_to_list_item)
            .collect::<Vec<NoteListItem>>();
        println!("{}", serde_json::to_string_pretty(&json_items)?);
        return Ok(());
    }
    print_listing(&notes, &listing_name(tag, archived), db).await
}

/// What a list is called when a number from it is not there.
pub fn listing_name(tag: Option<&str>, archived: bool) -> String {
    let mut name = "`dirt list".to_owned();
    if archived {
        name.push_str(" --archived");
    }
    if let Some(tag) = tag {
        name.push_str(" --tag ");
        name.push_str(tag);
    }
    name.push('`');
    name
}

/// Prints a numbered list and keeps it as the last list, so that the next
/// command can name a note by its number. An empty list is said where
/// errors go, so that standard output stays empty.
pub async fn print_listing(notes: &[Note], what: &str, db: &Store) -> Result<(), CliError> {
    remember_list(db, what, notes);
    if notes.is_empty() {
        eprintln!("No notes.");
    }
    for line in listing_lines(notes, db).await? {
        println!("{line}");
    }
    Ok(())
}
