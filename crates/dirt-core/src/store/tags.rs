//! Narrowing by tags: several at once, under either rule, and which tag
//! could be chosen next.
//!
//! Find holds the words typed and the tags chosen (interface design 6.3).
//! The notes it shows carry every chosen tag, or with "any of them" at
//! least one. Beside each tag that could be chosen next stands the number
//! of notes there would be with that tag chosen too, and that number is
//! always the number [`super::notes::find`] then gives: both read the same
//! tables by the same rules, archived notes included.

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection};

use super::notes::full_text_query;
use crate::Result;

/// How several chosen tags narrow what is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum TagRule {
    /// A note must carry every chosen tag. With one tag chosen, or none,
    /// the two rules are the same.
    #[default]
    All,
    /// A note must carry at least one of them.
    Any,
}

/// A tag that could be chosen next, and what choosing it would show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    /// The tag, in lower case and without its `#`.
    pub tag: String,
    /// How many notes there would be with this tag chosen too: what
    /// [`super::Store::find_notes_by_tags`] gives for the same words and
    /// the chosen tags with this one added. Archived notes count, because
    /// finding keeps them.
    pub notes: usize,
}

/// A tag as the store keeps it: in lower case, without its `#` or the
/// space around it.
fn normal(tag: &str) -> String {
    tag.trim().trim_start_matches('#').to_lowercase()
}

/// The tags chosen in Find and the rule that combines them.
///
/// Its tags are the first parameters of every statement built from it:
/// `?1` to `?n`, in order. A statement's own parameters follow them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Chosen {
    tags: Vec<String>,
    rule: TagRule,
}

impl Chosen {
    /// The tags as the store keeps them, each once. A tag that is nothing
    /// but its `#` or spaces is left out, as Find leaves an empty tag out.
    pub(super) fn new<T: AsRef<str>>(tags: &[T], rule: TagRule) -> Self {
        let mut kept: Vec<String> = Vec::with_capacity(tags.len());
        for tag in tags {
            let tag = normal(tag.as_ref());
            if !tag.is_empty() && !kept.contains(&tag) {
                kept.push(tag);
            }
        }
        Self { tags: kept, rule }
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    pub(super) const fn len(&self) -> usize {
        self.tags.len()
    }

    /// The rule in force for the notes these tags find. With fewer than
    /// two tags the rules do not differ, and the simpler statements of
    /// [`TagRule::All`] serve.
    const fn rule(&self) -> TagRule {
        if self.tags.len() < 2 {
            TagRule::All
        } else {
            self.rule
        }
    }

    /// The rule in force once one more tag is chosen: what a count of
    /// the notes "with that tag too" goes by. One chosen tag finds the
    /// same notes under either rule, and a second does not.
    const fn rule_with_one_more(&self) -> TagRule {
        if self.tags.is_empty() {
            TagRule::All
        } else {
            self.rule
        }
    }

    /// `?1, ?2, ...`: the chosen tags, as a statement names them.
    fn placeholders(&self) -> String {
        (1..=self.tags.len())
            .map(|number| format!("?{number}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The chosen tags as the first parameters of a statement, to which
    /// the statement's own are added.
    pub(super) fn parameters(&self) -> Vec<Value> {
        self.tags.iter().cloned().map(Value::Text).collect()
    }

    /// SQL that is true of a note that passes the rule. `note` is the SQL
    /// for the note's ID. Each test is one look into the tags' own table
    /// by its key, so this suits a statement that already has the notes
    /// it wants to test: the matches of a search.
    pub(super) fn holds(&self, note: &str) -> String {
        if self.tags.is_empty() {
            return "1".to_owned();
        }
        match self.rule() {
            TagRule::All => (1..=self.tags.len())
                .map(|number| {
                    format!(
                        "EXISTS (SELECT 1 FROM note_tags WHERE note_id = {note} AND tag = ?{number})"
                    )
                })
                .collect::<Vec<_>>()
                .join(" AND "),
            TagRule::Any => format!(
                "EXISTS (SELECT 1 FROM note_tags WHERE note_id = {note} AND tag IN ({}))",
                self.placeholders()
            ),
        }
    }

    /// A `FROM` and a `WHERE` that give the notes passing the rule, as
    /// `n`, for a list of them. There must be a tag chosen.
    ///
    /// SQLite reads the notes in the order of the list and looks each one
    /// up among the tags by key, so a list of a hundred stops as soon as
    /// it has a hundred. At worst, for tags that few notes carry, that is
    /// one look for each chosen tag for every note there is. Under
    /// [`TagRule::Any`] the notes of the chosen tags are gathered once and
    /// each note is looked up among them.
    pub(super) fn notes_from_tags(&self) -> String {
        match self.rule() {
            TagRule::All => {
                let mut carried = vec!["t.tag = ?1".to_owned()];
                carried.extend((2..=self.tags.len()).map(|number| {
                    format!(
                        "EXISTS (SELECT 1 FROM note_tags \
                          WHERE note_id = n.id AND tag = ?{number})"
                    )
                }));
                format!(
                    "FROM notes n JOIN note_tags t ON t.note_id = n.id WHERE {}",
                    carried.join(" AND ")
                )
            }
            TagRule::Any => format!(
                "FROM notes n \
                 WHERE n.id IN (SELECT note_id FROM note_tags WHERE tag IN ({}))",
                self.placeholders()
            ),
        }
    }

    /// The same tags with the one fewest notes carry first, for a
    /// statement that reads the first tag's notes and tests them for the
    /// rest. `None` if no note carries one of them: nothing carries them
    /// all then.
    fn rarest_first(&self, connection: &Connection) -> Result<Option<Self>> {
        let mut statement = connection.prepare(&format!(
            "SELECT count(*), tag FROM note_tags WHERE tag IN ({}) GROUP BY tag",
            self.placeholders()
        ))?;
        let rows = statement.query_map(params_from_iter(self.parameters()), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut carried = Vec::with_capacity(self.tags.len());
        for row in rows {
            carried.push(row?);
        }
        if carried.len() < self.tags.len() {
            return Ok(None);
        }
        carried.sort();
        Ok(Some(Self {
            tags: carried.into_iter().map(|(_, tag)| tag).collect(),
            rule: self.rule,
        }))
    }
}

/// The start of a tag's name as typed after `#`, ready to compare. `None`
/// if no tag can start so: a tag is made of ASCII letters, digits, `_` and
/// `-`. Refusing anything else also keeps `*`, `?` and `[` out of the
/// pattern the start becomes.
fn tag_start(typed: &str) -> Option<String> {
    let start = normal(typed);
    start
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        .then_some(start)
}

/// The notes that match the words typed, in a statement: the full-text
/// index first, then each match's note.
const MATCHED: &str = "FROM notes_fts f CROSS JOIN notes n ON n.rowid = f.rowid";

/// The statement behind [`narrowing`], for these chosen tags. `words` and
/// `start` say whether there are words to match and a start of a name to
/// keep to. Its parameters are the chosen tags, then the words, then the
/// pattern for the name, then the limit.
///
/// Each shape first has the notes it counts in, as `m`, and then reads
/// those notes' tags. The notes are gathered first, and kept, so that a
/// chosen tag is looked for once for each note and not once for each of
/// the note's tags.
///
/// - With words, the full-text index is read first and answers once, as
///   in a search (see `search_sql` in `notes`).
/// - With no words and every chosen tag wanted, the first chosen tag's
///   notes are read and tested for the rest. [`narrowing`] puts the tag
///   fewest notes carry first.
/// - With no words and any chosen tag wanted, no note is read at all:
///   each tag's notes are counted in the tags' own index, and from that
///   are taken the notes that are there already, which are found by
///   reading the chosen tags' notes and no others.
fn narrowing_sql(chosen: &Chosen, words: bool, start: bool) -> String {
    let mut next = chosen.len();
    let mut parameter = || {
        next += 1;
        format!("?{next}")
    };
    let words = words.then(&mut parameter);
    let start = start.then(&mut parameter);
    let limit = parameter();

    // The tags that are counted: not the chosen ones, and with the name
    // typed so far.
    let mut counted = Vec::new();
    if !chosen.is_empty() {
        counted.push(format!("t.tag NOT IN ({})", chosen.placeholders()));
    }
    if let Some(start) = &start {
        counted.push(format!("t.tag GLOB {start}"));
    }
    let (only_counted, and_counted) = if counted.is_empty() {
        (String::new(), String::new())
    } else {
        let counted = counted.join(" AND ");
        (format!(" WHERE {counted}"), format!(" AND {counted}"))
    };
    let matching = words
        .as_ref()
        .map(|words| format!("WHERE notes_fts MATCH {words} AND n.deleted = 0"));
    // The tags of the notes gathered as `m`, tag by tag.
    let their_tags =
        format!("FROM m CROSS JOIN note_tags t ON t.note_id = m.id{only_counted} GROUP BY t.tag");

    let counts = match (chosen.rule_with_one_more(), &matching) {
        // Nothing is chosen: every tag, or every tag of the notes that
        // match.
        (TagRule::All, None) if chosen.is_empty() => {
            format!("SELECT t.tag AS tag, count(*) AS notes FROM note_tags t{only_counted} GROUP BY t.tag")
        }
        (TagRule::All, Some(matching)) if chosen.is_empty() => format!(
            "SELECT t.tag AS tag, count(*) AS notes {MATCHED} \
              CROSS JOIN note_tags t ON t.note_id = n.id {matching}{and_counted} GROUP BY t.tag"
        ),
        // Every chosen tag: count, tag by tag, the notes that carry them
        // all.
        (TagRule::All, Some(matching)) => format!(
            "WITH m(id) AS MATERIALIZED (SELECT n.id {MATCHED} {matching} AND {}) \
             SELECT t.tag AS tag, count(*) AS notes {their_tags}",
            chosen.holds("n.id")
        ),
        (TagRule::All, None) => {
            let others: Vec<String> = (2..=chosen.len())
                .map(|number| {
                    format!(
                        " AND EXISTS (SELECT 1 FROM note_tags \
                           WHERE note_id = c.note_id AND tag = ?{number})"
                    )
                })
                .collect();
            format!(
                "WITH m(id) AS MATERIALIZED \
                   (SELECT c.note_id FROM note_tags c WHERE c.tag = ?1{}) \
                 SELECT t.tag AS tag, count(*) AS notes {their_tags}",
                others.concat()
            )
        }
        // Any chosen tag: the notes there are already, and with them the
        // notes this tag would add. A tag is offered when a note that
        // matches the words carries it, whether or not the note is among
        // those already there.
        (TagRule::Any, Some(matching)) => format!(
            "WITH m(id, there) AS MATERIALIZED (SELECT n.id, {} {MATCHED} {matching}) \
             SELECT t.tag AS tag, \
                    (SELECT COALESCE(sum(there), 0) FROM m) + sum(NOT m.there) AS notes \
               {their_tags}",
            chosen.holds("n.id")
        ),
        (TagRule::Any, None) => format!(
            "WITH m(id) AS MATERIALIZED \
               (SELECT DISTINCT note_id FROM note_tags WHERE tag IN ({})) \
             SELECT every.tag AS tag, \
                    (SELECT count(*) FROM m) + every.notes - COALESCE(there.notes, 0) AS notes \
               FROM (SELECT t.tag AS tag, count(*) AS notes \
                       FROM note_tags t{only_counted} GROUP BY t.tag) every \
               LEFT JOIN (SELECT t.tag AS tag, count(*) AS notes {their_tags}) there \
                      ON there.tag = every.tag",
            chosen.placeholders()
        ),
    };
    format!("{counts} ORDER BY notes DESC, tag LIMIT {limit}")
}

/// The tags that could be chosen next, with how many notes each would
/// leave: see [`super::Store::tags_to_narrow`].
pub(super) fn narrowing(
    connection: &Connection,
    query: &str,
    chosen: &Chosen,
    start: Option<&str>,
    limit: usize,
) -> Result<Vec<TagCount>> {
    let words = full_text_query(query);
    if words.is_none() && !query.trim().is_empty() {
        // Text with nothing to search for finds no note, so no tag leaves
        // any.
        return Ok(Vec::new());
    }
    let start = match start.map(tag_start) {
        // No tag starts with what was typed.
        Some(None) => return Ok(Vec::new()),
        Some(Some(start)) if !start.is_empty() => Some(start),
        _ => None,
    };
    // Where the first chosen tag's notes are read, let it be the tag
    // with the fewest.
    let reordered;
    let chosen =
        if words.is_none() && chosen.len() > 1 && chosen.rule_with_one_more() == TagRule::All {
            let Some(rarest_first) = chosen.rarest_first(connection)? else {
                return Ok(Vec::new());
            };
            reordered = rarest_first;
            &reordered
        } else {
            chosen
        };

    let sql = narrowing_sql(chosen, words.is_some(), start.is_some());
    let mut parameters = chosen.parameters();
    parameters.extend(words.map(Value::Text));
    parameters.extend(start.map(|start| Value::Text(format!("{start}*"))));
    parameters.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(parameters), |row| {
        let tag: String = row.get(0)?;
        let notes: i64 = row.get(1)?;
        Ok(TagCount {
            tag,
            notes: usize::try_from(notes).unwrap_or_default(),
        })
    })?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chosen(tags: &[&str], rule: TagRule) -> Chosen {
        Chosen::new(tags, rule)
    }

    #[test]
    fn chosen_tags_are_kept_as_the_store_keeps_them() {
        let picked = chosen(&[" #Work ", "home", "work", "#", "  "], TagRule::All);
        assert_eq!(picked.tags, ["work", "home"]);
        assert!(chosen(&["#", ""], TagRule::Any).is_empty());
    }

    #[test]
    fn one_tag_is_the_same_under_either_rule() {
        let all = chosen(&["work"], TagRule::All);
        let any = chosen(&["work"], TagRule::Any);
        assert_eq!(all.holds("n.id"), any.holds("n.id"));
        assert_eq!(all.notes_from_tags(), any.notes_from_tags());
        // The tag chosen after it is counted by the rule all the same:
        // with it there are two, and the rules part.
        assert_ne!(
            narrowing_sql(&all, true, true),
            narrowing_sql(&any, true, true)
        );
        let none = (chosen(&[], TagRule::All), chosen(&[], TagRule::Any));
        assert_eq!(
            narrowing_sql(&none.0, true, true),
            narrowing_sql(&none.1, true, true)
        );
    }

    #[test]
    fn the_start_of_a_name_cannot_become_a_pattern() {
        assert_eq!(tag_start("#Co").as_deref(), Some("co"));
        assert_eq!(tag_start("my_tag-2").as_deref(), Some("my_tag-2"));
        assert_eq!(tag_start("").as_deref(), Some(""));
        for typed in ["co*", "c?", "[a-z]", "two words", "café", "a%"] {
            assert_eq!(tag_start(typed), None, "{typed}");
        }
    }

    /// The tables a statement reads, in the order SQLite reads them.
    fn read_order(connection: &Connection, sql: &str, parameters: usize) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let values = vec![Value::Text("\"word\"*".to_owned()); parameters];
        let steps = statement
            .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
            .unwrap();
        steps
            .map(|step| step.expect("a step of the plan"))
            .filter(|step| step.starts_with("SCAN") || step.starts_with("SEARCH"))
            .collect()
    }

    fn empty_database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut connection =
            super::super::schema::open_connection(&folder.path().join("notes.db")).unwrap();
        super::super::schema::set_up(&mut connection).unwrap();
        (folder, connection)
    }

    /// With words typed, the count begins at the full-text index, as a
    /// search does, and reads the notes that match and no others. A
    /// chosen tag is looked up once for each of those notes: the notes
    /// are gathered before their tags are read.
    #[test]
    fn with_words_the_count_reads_the_full_text_index_first() {
        let (_folder, connection) = empty_database();
        for picked in [
            chosen(&[], TagRule::All),
            chosen(&["a"], TagRule::All),
            chosen(&["a", "b"], TagRule::All),
            chosen(&["a"], TagRule::Any),
            chosen(&["a", "b"], TagRule::Any),
        ] {
            let sql = narrowing_sql(&picked, true, true);
            let plan = read_order(&connection, &sql, picked.len() + 3);
            let said = format!("{picked:?} reads in this order: {plan:?}");
            let index = plan
                .iter()
                .position(|step| step.contains("SCAN f VIRTUAL TABLE"))
                .unwrap_or_else(|| panic!("{said}"));
            let notes = plan
                .iter()
                .position(|step| step.starts_with("SEARCH n "))
                .unwrap_or_else(|| panic!("{said}"));
            let their_tags = plan
                .iter()
                .position(|step| step.starts_with("SEARCH t USING PRIMARY KEY"))
                .unwrap_or_else(|| panic!("{said}"));
            assert!(index < notes && notes < their_tags, "{said}");
            // Nothing is read from end to end but the index's answer and
            // the notes gathered from it.
            assert!(
                plan.iter()
                    .filter(|step| step.starts_with("SCAN"))
                    .all(|step| step.contains("SCAN f VIRTUAL TABLE") || step == "SCAN m"),
                "{said}"
            );
            // A chosen tag is looked up before the notes' tags are read.
            if !picked.is_empty() {
                let looked_up = plan
                    .iter()
                    .position(|step| {
                        // By either of the two keys that hold a note and a tag.
                        step.starts_with("SEARCH note_tags")
                            && step.contains("note_id=?")
                            && step.contains("tag=?")
                    })
                    .unwrap_or_else(|| panic!("{said}"));
                assert!(looked_up < their_tags, "{said}");
            }
        }
    }

    /// With tags chosen and no words, the count reads the chosen tags'
    /// notes and never the notes' table.
    #[test]
    fn without_words_the_count_reads_the_tags_alone() {
        let (_folder, connection) = empty_database();
        for (picked, all) in [
            (chosen(&["a"], TagRule::All), true),
            (chosen(&["a", "b"], TagRule::All), true),
            (chosen(&["a"], TagRule::Any), false),
            (chosen(&["a", "b"], TagRule::Any), false),
        ] {
            let sql = narrowing_sql(&picked, false, false);
            let plan = read_order(&connection, &sql, picked.len() + 1);
            let said = format!("{picked:?} reads in this order: {plan:?}");
            assert!(
                !plan
                    .iter()
                    .any(|step| step.contains(" n ") || step.contains("notes")),
                "{said}"
            );
            // The chosen tags' notes are found by the tag.
            assert!(
                plan.iter()
                    .any(|step| step.starts_with("SEARCH") && step.contains("(tag=?)")),
                "{said}"
            );
            let whole_table = plan
                .iter()
                .filter(|step| step.starts_with("SCAN t "))
                .count();
            if all {
                // Every chosen tag wanted: the first one's notes, and
                // nothing from end to end.
                assert_eq!(whole_table, 0, "{said}");
            } else {
                // Any wanted: the tags' own index once, to count each
                // tag's notes, and no table.
                assert_eq!(whole_table, 1, "{said}");
                assert!(
                    plan.iter()
                        .any(|step| step == "SCAN t USING COVERING INDEX note_tags_by_tag"),
                    "{said}"
                );
            }
        }
    }

    /// Where the first chosen tag's notes are read, it is the tag fewest
    /// notes carry.
    #[test]
    fn the_rarest_chosen_tag_is_read_first() {
        let (_folder, connection) = empty_database();
        connection
            .execute_batch(
                "INSERT INTO notes (id, doc, text, edited_at, deleted, revision) VALUES \
                   ('n1', x'', '', 0, 0, 1), ('n2', x'', '', 0, 0, 1), ('n3', x'', '', 0, 0, 1); \
                 INSERT INTO note_tags (note_id, tag) VALUES \
                   ('n1', 'common'), ('n2', 'common'), ('n3', 'common'), \
                   ('n1', 'middling'), ('n2', 'middling'), ('n1', 'rare');",
            )
            .unwrap();
        let picked = chosen(&["common", "rare", "middling"], TagRule::All);
        let ordered = picked.rarest_first(&connection).unwrap().unwrap();
        assert_eq!(ordered.tags, ["rare", "middling", "common"]);
        assert_eq!(ordered.rule, TagRule::All);
        // A tag that no note carries: nothing carries them all.
        let picked = chosen(&["common", "nowhere"], TagRule::All);
        assert_eq!(picked.rarest_first(&connection).unwrap(), None);
    }
}
