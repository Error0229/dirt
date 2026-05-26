//! Mobile note list filtering helpers (search + tag filtering).

use std::collections::BTreeSet;

use dirt_core::Note;

/// Return a sorted, deduplicated tag list discovered across notes.
#[must_use]
pub fn collect_note_tags(notes: &[Note]) -> Vec<String> {
    let mut tags = BTreeSet::new();
    for note in notes {
        for tag in note.tags() {
            tags.insert(tag);
        }
    }
    tags.into_iter().collect()
}

/// Filter notes by case-insensitive text query and optional exact tag filter.
#[must_use]
pub fn filter_notes(notes: &[Note], search_query: &str, tag_filter: Option<&str>) -> Vec<Note> {
    let normalized_query = normalize_query(search_query);
    let normalized_tag_filter = tag_filter
        .map(normalize_query)
        .filter(|value| !value.is_empty());

    notes
        .iter()
        .filter(|note| note_matches_query(note, &normalized_query))
        .filter(|note| note_matches_tag_filter(note, normalized_tag_filter.as_deref()))
        .cloned()
        .collect()
}

fn normalize_query(raw: &str) -> String {
    raw.trim().to_lowercase()
}

fn note_matches_query(note: &Note, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    note.content.to_lowercase().contains(query)
}

fn note_matches_tag_filter(note: &Note, tag_filter: Option<&str>) -> bool {
    let Some(tag_filter) = tag_filter else {
        return true;
    };
    note.tags().iter().any(|tag| tag == tag_filter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dirt_core::SOLO_USER_ID;

    fn note(content: &str) -> Note {
        Note::new_for_user(content, SOLO_USER_ID).expect("test note fixture must build")
    }

    #[test]
    fn collects_sorted_unique_tags() {
        let notes = vec![
            note("Ship #work and #urgent updates"),
            note("Go hiking #personal #urgent"),
            note("No tag note"),
        ];

        assert_eq!(
            collect_note_tags(&notes),
            vec![
                "personal".to_string(),
                "urgent".to_string(),
                "work".to_string()
            ]
        );
    }

    #[test]
    fn filters_notes_with_search_and_tag_together() {
        let notes = vec![
            note("Project kickoff tomorrow #work"),
            note("Project movie night #personal"),
            note("Standup notes #work"),
        ];

        let filtered = filter_notes(&notes, "project", Some("work"));
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].content.contains("kickoff"));
    }

    #[test]
    fn treats_query_and_tag_case_insensitively() {
        let notes = vec![
            note("Debug release blocker #Work"),
            note("Read a book #personal"),
        ];

        let filtered = filter_notes(&notes, "DEBUG", Some("WORK"));
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].content.contains("Debug"));
    }
}
