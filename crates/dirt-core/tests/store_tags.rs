//! Narrowing by tags as the interface redesign asks (design 6.3): several
//! tags under either rule, and the tags that could be chosen next with how
//! many notes each would leave.

use std::collections::BTreeSet;
use std::time::Duration;

use dirt_core::store::{TagCount, TagRule};
use dirt_core::{Note, Store};

fn temp_store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(folder.path().join("notes.db")).unwrap();
    (folder, store)
}

/// Long enough for two notes to have different edit times.
async fn a_moment() {
    tokio::time::sleep(Duration::from_millis(3)).await;
}

fn tags(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn texts(notes: &[Note]) -> Vec<&str> {
    notes.iter().map(|note| note.content.as_str()).collect()
}

fn counts(found: &[TagCount]) -> Vec<(&str, usize)> {
    found
        .iter()
        .map(|count| (count.tag.as_str(), count.notes))
        .collect()
}

/// A kitchen's worth of notes. One is archived and one deleted.
async fn kitchen(store: &Store) {
    for text in [
        "buy milk #shop #home",
        "buy bread #shop",
        "milk the goat #farm #home",
        "call the plumber #home #urgent",
        "write the report #work #urgent",
        "milk prices are up #work #shop",
        "nothing to tag",
    ] {
        store.create_note(text).await.unwrap();
        a_moment().await;
    }
    let put_away = store.create_note("old milk run #shop #done").await.unwrap();
    store.archive_note(&put_away.id).await.unwrap();
    let gone = store.create_note("deleted milk #shop #gone").await.unwrap();
    store.delete_note(&gone.id).await.unwrap();
}

#[tokio::test]
async fn with_nothing_chosen_every_tag_is_offered_with_its_notes() {
    let (_folder, store) = temp_store();
    kitchen(&store).await;

    let offered = store
        .tags_to_narrow("", &[], TagRule::All, None, 50)
        .await
        .unwrap();
    // The most notes first, and equals by name. The archived note counts,
    // and its own tag is offered; the deleted note and its tag are not.
    assert_eq!(
        counts(&offered),
        [
            ("shop", 4),
            ("home", 3),
            ("urgent", 2),
            ("work", 2),
            ("done", 1),
            ("farm", 1),
        ]
    );
    // The same numbers the list of tags has.
    let mut listed = store.list_tags().await.unwrap();
    listed.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let listed: Vec<(&str, usize)> = listed.iter().map(|(tag, n)| (tag.as_str(), *n)).collect();
    assert_eq!(counts(&offered), listed);
    // "Any of them" is no different while nothing is chosen.
    assert_eq!(
        store
            .tags_to_narrow("", &[], TagRule::Any, None, 50)
            .await
            .unwrap(),
        offered
    );

    // The caller shows the first few.
    let first = store
        .tags_to_narrow("", &[], TagRule::All, None, 3)
        .await
        .unwrap();
    assert_eq!(counts(&first), [("shop", 4), ("home", 3), ("urgent", 2)]);
    assert!(store
        .tags_to_narrow("", &[], TagRule::All, None, 0)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_chosen_tag_leaves_the_tags_its_notes_also_carry() {
    let (_folder, store) = temp_store();
    kitchen(&store).await;

    let offered = store
        .tags_to_narrow("", &tags(&["shop"]), TagRule::All, None, 50)
        .await
        .unwrap();
    // #farm and #urgent are on no #shop note: choosing either would empty
    // the list, so neither is offered. #shop itself is chosen already.
    assert_eq!(counts(&offered), [("done", 1), ("home", 1), ("work", 1)]);

    // A second tag narrows further. With or without the sign, in any case.
    let offered = store
        .tags_to_narrow("", &tags(&["#Home", "URGENT"]), TagRule::All, None, 50)
        .await
        .unwrap();
    assert!(offered.is_empty(), "{offered:?}");
    let offered = store
        .tags_to_narrow("", &tags(&["home"]), TagRule::All, None, 50)
        .await
        .unwrap();
    assert_eq!(counts(&offered), [("farm", 1), ("shop", 1), ("urgent", 1)]);

    // A tag no note carries leaves nothing to choose.
    assert!(store
        .tags_to_narrow("", &tags(&["nowhere"]), TagRule::All, None, 50)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn the_words_typed_narrow_the_tags_too() {
    let (_folder, store) = temp_store();
    kitchen(&store).await;

    // Four live notes hold "milk", the archived one among them.
    let offered = store
        .tags_to_narrow("milk", &[], TagRule::All, None, 50)
        .await
        .unwrap();
    assert_eq!(
        counts(&offered),
        [
            ("shop", 3),
            ("home", 2),
            ("done", 1),
            ("farm", 1),
            ("work", 1)
        ]
    );
    // The start of a word is enough, as in a search.
    assert_eq!(
        store
            .tags_to_narrow("mil", &[], TagRule::All, None, 50)
            .await
            .unwrap(),
        offered
    );

    let offered = store
        .tags_to_narrow("milk", &tags(&["home"]), TagRule::All, None, 50)
        .await
        .unwrap();
    assert_eq!(counts(&offered), [("farm", 1), ("shop", 1)]);

    // Words that no note holds, and text that is no word at all.
    for query in ["cheese", "!!!", " # "] {
        for rule in [TagRule::All, TagRule::Any] {
            let offered = store
                .tags_to_narrow(query, &tags(&["home"]), rule, None, 50)
                .await
                .unwrap();
            assert!(offered.is_empty(), "{query:?} {rule:?}: {offered:?}");
        }
    }
}

#[tokio::test]
async fn typing_after_the_sign_keeps_the_tags_that_start_so() {
    let (_folder, store) = temp_store();
    for text in [
        "#cooking #coffee",
        "#cooking #code",
        "#Coffee again",
        "#c_d and #cxd",
        "#work",
    ] {
        store.create_note(text).await.unwrap();
    }
    let starting = |typed: &'static str| {
        let store = store.clone();
        async move {
            let offered = store
                .tags_to_narrow("", &[], TagRule::All, Some(typed), 50)
                .await
                .unwrap();
            offered
                .into_iter()
                .map(|count| (count.tag, count.notes))
                .collect::<Vec<_>>()
        }
    };
    let named = |pairs: &[(&str, usize)]| -> Vec<(String, usize)> {
        pairs
            .iter()
            .map(|(tag, notes)| ((*tag).to_owned(), *notes))
            .collect()
    };

    assert_eq!(
        starting("co").await,
        named(&[("coffee", 2), ("cooking", 2), ("code", 1)])
    );
    assert_eq!(starting("#COF").await, named(&[("coffee", 2)]));
    assert_eq!(starting("work").await, named(&[("work", 1)]));
    assert!(starting("z").await.is_empty());
    // Nothing typed yet after the sign: every tag.
    assert_eq!(starting("#").await.len(), 6);
    assert_eq!(starting("").await.len(), 6);
    // An underscore is a letter of a tag's name and stands for no other.
    assert_eq!(starting("c_").await, named(&[("c_d", 1)]));
    // What cannot be in a tag's name starts no tag, and is no pattern.
    for typed in ["c*", "c?", "[a-z]", "%", "co ok", "é"] {
        assert!(starting(typed).await.is_empty(), "{typed}");
    }

    // With a tag chosen and words typed as well.
    let offered = store
        .tags_to_narrow("again", &tags(&["coffee"]), TagRule::All, Some("c"), 50)
        .await
        .unwrap();
    assert!(offered.is_empty(), "{offered:?}");
    let offered = store
        .tags_to_narrow("", &tags(&["cooking"]), TagRule::All, Some("co"), 50)
        .await
        .unwrap();
    assert_eq!(counts(&offered), [("code", 1), ("coffee", 1)]);
}

#[tokio::test]
async fn any_of_them_counts_what_a_tag_would_add() {
    let (_folder, store) = temp_store();
    kitchen(&store).await;
    let any = |query: &'static str, chosen: &'static [&'static str]| {
        let store = store.clone();
        async move {
            store
                .tags_to_narrow(query, &tags(chosen), TagRule::Any, None, 50)
                .await
                .unwrap()
        }
    };

    // Two notes are urgent and one is of the farm: three are there. Each
    // tag is counted with those three, and a note that is there already is
    // not counted twice: #home is on three notes, two of them there, so
    // with it there would be four.
    let offered = any("", &["urgent", "farm"]).await;
    assert_eq!(
        counts(&offered),
        [("shop", 7), ("done", 4), ("home", 4), ("work", 4)]
    );

    // With words: of the four notes that hold "milk", one is of the farm
    // and none is urgent. Every other tag on a "milk" note is offered,
    // with that one note and what the tag would bring.
    let offered = any("milk", &["farm", "urgent"]).await;
    assert_eq!(
        counts(&offered),
        [("shop", 4), ("done", 2), ("home", 2), ("work", 2)]
    );

    // A tag whose notes are all there already is offered with the number
    // there is: choosing it changes nothing, and empties nothing.
    let offered = any("", &["shop", "home"]).await;
    let farm = offered.iter().find(|count| count.tag == "farm").unwrap();
    let there = store
        .find_notes_by_tags("", &tags(&["shop", "home"]), TagRule::Any, 100)
        .await
        .unwrap()
        .len();
    assert_eq!(farm.notes, there);
    assert_eq!(there, 6);

    // One chosen tag finds the same notes under either rule, but the tag
    // chosen after it is counted by the rule: with #shop's four notes,
    // #home would make six, where "all of them" would leave one. #done is
    // on one of the four, and adds none.
    let offered = any("", &["shop"]).await;
    assert_eq!(
        counts(&offered),
        [
            ("home", 6),
            ("urgent", 6),
            ("farm", 5),
            ("work", 5),
            ("done", 4)
        ]
    );
}

/// The rule the counts are held to: the number beside a tag is the number
/// of notes that choosing it then shows. Tried for every pair of chosen
/// tags, both rules, and words that match many notes, few and none.
#[tokio::test]
async fn a_count_is_what_choosing_the_tag_then_shows() {
    let (_folder, store) = temp_store();
    kitchen(&store).await;
    // More notes, with the five tags in every combination.
    let names = ["shop", "home", "work", "urgent", "farm"];
    for number in 0..40_usize {
        let about = if number % 3 == 0 {
            "cheese"
        } else {
            "milk and bread"
        };
        let carried: String = names
            .iter()
            .enumerate()
            .filter(|(position, _)| (number >> position) & 1 == 1)
            .flat_map(|(_, name)| [" #", name])
            .collect();
        let text = format!("note {number} about {about}{carried}");
        let note = store.create_note(&text).await.unwrap();
        if number % 7 == 0 {
            store.archive_note(&note.id).await.unwrap();
        }
        if number % 11 == 0 {
            store.delete_note(&note.id).await.unwrap();
        }
    }
    let every_tag: Vec<String> = store
        .list_tags()
        .await
        .unwrap()
        .into_iter()
        .map(|(tag, _)| tag)
        .collect();

    let mut choices: Vec<Vec<String>> = vec![Vec::new()];
    for (first, name) in names.iter().enumerate() {
        choices.push(tags(&[name]));
        for other in &names[first + 1..] {
            choices.push(tags(&[name, other]));
        }
    }
    choices.push(tags(&["shop", "home", "work"]));

    let mut checked = 0;
    for query in ["", "milk", "chee", "note", "plumber", "nothing-holds-this"] {
        for chosen in &choices {
            for rule in [TagRule::All, TagRule::Any] {
                let offered = store
                    .tags_to_narrow(query, chosen, rule, None, 100)
                    .await
                    .unwrap();
                let offered_names: BTreeSet<&str> =
                    offered.iter().map(|count| count.tag.as_str()).collect();
                assert_eq!(offered_names.len(), offered.len(), "a tag twice");
                assert!(
                    offered
                        .windows(2)
                        .all(|pair| (pair[1].notes, &pair[0].tag) <= (pair[0].notes, &pair[1].tag)),
                    "the most notes first, then by name: {offered:?}"
                );

                for tag in &every_tag {
                    let mut with = chosen.clone();
                    with.push(tag.clone());
                    let shown = store
                        .find_notes_by_tags(query, &with, rule, 1_000)
                        .await
                        .unwrap()
                        .len();
                    let count = offered.iter().find(|count| &count.tag == tag);
                    let said = format!("{query:?} {chosen:?} {rule:?} + {tag}");
                    if chosen.contains(tag) {
                        assert!(count.is_none(), "a chosen tag is offered: {said}");
                        continue;
                    }
                    match (count, rule) {
                        (Some(count), _) => {
                            assert_eq!(count.notes, shown, "{said}");
                            assert!(shown > 0, "{said}");
                        }
                        // Not offered under "all": it would leave no note.
                        (None, TagRule::All) => assert_eq!(shown, 0, "{said}"),
                        // Not offered under "any": no note that matches
                        // the words carries it, so it would add nothing.
                        (None, TagRule::Any) => {
                            let carrying = store
                                .find_notes_by_tags(
                                    query,
                                    std::slice::from_ref(tag),
                                    TagRule::All,
                                    1_000,
                                )
                                .await
                                .unwrap();
                            assert!(carrying.is_empty(), "{said}");
                        }
                    }
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 800, "{checked} counts were checked");
}

#[tokio::test]
async fn several_tags_find_notes_with_all_of_them_or_any() {
    let (_folder, store) = temp_store();
    kitchen(&store).await;
    let find = |query: &'static str, chosen: &'static [&'static str], rule| {
        let store = store.clone();
        async move {
            store
                .find_notes_by_tags(query, &tags(chosen), rule, 100)
                .await
                .unwrap()
        }
    };

    // All of them, the default: most recently edited first.
    let both = find("", &["shop", "home"], TagRule::All).await;
    assert_eq!(texts(&both), ["buy milk #shop #home"]);
    assert!(find("", &["shop", "farm"], TagRule::All).await.is_empty());
    assert_eq!(TagRule::default(), TagRule::All);

    // Any of them. The archived note is found too, after the rest.
    let either = find("", &["farm", "done"], TagRule::Any).await;
    assert_eq!(
        texts(&either),
        ["milk the goat #farm #home", "old milk run #shop #done"]
    );
    assert!(either[1].archived && !either[0].archived);
    let either = find("", &["urgent", "farm"], TagRule::Any).await;
    assert_eq!(
        texts(&either),
        [
            "write the report #work #urgent",
            "call the plumber #home #urgent",
            "milk the goat #farm #home",
        ]
    );

    // With words as well.
    let found = find("milk", &["shop", "work"], TagRule::All).await;
    assert_eq!(texts(&found), ["milk prices are up #work #shop"]);
    let found = find("milk", &["farm", "work"], TagRule::Any).await;
    let found: BTreeSet<&str> = texts(&found).into_iter().collect();
    assert_eq!(
        found,
        BTreeSet::from([
            "milk the goat #farm #home",
            "milk prices are up #work #shop"
        ])
    );
    assert!(find("plumber", &["shop", "work"], TagRule::Any)
        .await
        .is_empty());
    // Text that is no word finds nothing, whatever is chosen.
    assert!(find("!!!", &["shop"], TagRule::Any).await.is_empty());

    // A tag named twice, or in another case, is one tag.
    let once = find("", &["#Shop", "shop", " SHOP "], TagRule::All).await;
    assert_eq!(once.len(), 4);
    // A deleted note is not found by its tag.
    assert!(find("", &["gone"], TagRule::Any).await.is_empty());

    // No tag and no words: the list, which leaves the archived note out.
    let list = find("", &[], TagRule::Any).await;
    assert_eq!(list.len(), 7);
    assert!(list.iter().all(|note| !note.archived));
    // An empty tag is no tag.
    assert_eq!(find("", &["#", " "], TagRule::All).await, list);

    // What takes one tag gives what it gave.
    assert_eq!(
        store.find_notes("", Some("shop"), 100).await.unwrap(),
        find("", &["shop"], TagRule::All).await
    );
    assert_eq!(
        store.find_notes("milk", Some("#home"), 100).await.unwrap(),
        find("milk", &["home"], TagRule::Any).await
    );
    assert_eq!(store.find_notes("", None, 100).await.unwrap(), list);
}

#[tokio::test]
async fn the_list_and_the_archive_take_several_tags() {
    let (_folder, store) = temp_store();
    kitchen(&store).await;

    // The list leaves archived notes out, tags or no tags.
    let listed = store
        .list_notes_by_tags(&tags(&["shop", "done"]), TagRule::Any, 100, 0)
        .await
        .unwrap();
    assert_eq!(
        texts(&listed),
        [
            "milk prices are up #work #shop",
            "buy bread #shop",
            "buy milk #shop #home",
        ]
    );
    // In pages, as the list is.
    let second = store
        .list_notes_by_tags(&tags(&["shop", "done"]), TagRule::Any, 2, 2)
        .await
        .unwrap();
    assert_eq!(texts(&second), ["buy milk #shop #home"]);

    let all = store
        .list_notes_by_tags(&tags(&["shop", "work"]), TagRule::All, 100, 0)
        .await
        .unwrap();
    assert_eq!(texts(&all), ["milk prices are up #work #shop"]);

    let archived = store
        .list_archived_notes_by_tags(&tags(&["shop", "done"]), TagRule::All, 100, 0)
        .await
        .unwrap();
    assert_eq!(texts(&archived), ["old milk run #shop #done"]);
    assert!(store
        .list_archived_notes_by_tags(&tags(&["home", "farm"]), TagRule::Any, 100, 0)
        .await
        .unwrap()
        .is_empty());

    // No tags: the list, and the archive.
    assert_eq!(
        store
            .list_notes_by_tags(&[], TagRule::All, 100, 0)
            .await
            .unwrap(),
        store.list_notes(100, 0).await.unwrap()
    );
    assert_eq!(
        store
            .list_archived_notes_by_tags(&[], TagRule::Any, 100, 0)
            .await
            .unwrap(),
        store.list_archived_notes(100, 0).await.unwrap()
    );

    // One tag, as before: an empty one is carried by no note.
    assert_eq!(
        store.list_notes_by_tag("#SHOP", 100, 0).await.unwrap(),
        store
            .list_notes_by_tags(&tags(&["shop"]), TagRule::Any, 100, 0)
            .await
            .unwrap()
    );
    assert!(store
        .list_notes_by_tag("", 100, 0)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .list_archived_notes_by_tag(" # ", 100, 0)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_archived_notes_by_tag("done", 100, 0)
            .await
            .unwrap()
            .len(),
        1
    );
}

/// The counts follow the notes: archiving changes none, deleting takes
/// the note out, and bringing it back puts it in again.
#[tokio::test]
async fn the_counts_follow_what_happens_to_a_note() {
    let (_folder, store) = temp_store();
    let note = store
        .create_note("paint the fence #home #weekend")
        .await
        .unwrap();
    store.create_note("mow the lawn #home").await.unwrap();
    let offered = |store: Store| async move {
        let offered = store
            .tags_to_narrow("", &tags(&["home"]), TagRule::All, None, 10)
            .await
            .unwrap();
        offered
            .into_iter()
            .map(|count| (count.tag, count.notes))
            .collect::<Vec<_>>()
    };
    let weekend = vec![("weekend".to_owned(), 1)];
    assert_eq!(offered(store.clone()).await, weekend);

    store.archive_note(&note.id).await.unwrap();
    assert_eq!(offered(store.clone()).await, weekend);

    store.delete_note(&note.id).await.unwrap();
    assert!(offered(store.clone()).await.is_empty());

    store.restore_note(&note.id).await.unwrap();
    assert_eq!(offered(store.clone()).await, weekend);

    store
        .update_note(&note.id, "paint the fence #home #garden")
        .await
        .unwrap();
    assert_eq!(offered(store.clone()).await, [("garden".to_owned(), 1)]);
}
