//! Randomised convergence: three devices edit, delete, restore and sync at
//! random. Afterwards, merging their states in every order must give the same
//! note.

use dirt_core::notes::NoteDoc;

const IDS: [u64; 3] = [
    7_100_000_000_000_001,
    8_200_000_000_000_002,
    8_900_000_000_000_003,
];
const OBSERVER: u64 = 9_000_000_000_000_009;

const WORDS: [&str; 12] = [
    "milk", "bread", "today", "call", "Mina", "買", "牛奶", "明天", "🥛", "#todo", "the", "at six",
];

/// A small deterministic generator, so that a failing seed can be replayed.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).expect("fits")
    }
}

/// Changes the text the way a person would between two saves.
fn random_text_edit(doc: &NoteDoc, rng: &mut Rng, now: i64) {
    let mut chars: Vec<char> = doc.text().chars().collect();
    let word = WORDS[rng.below(WORDS.len())];
    match rng.below(3) {
        // Insert a word somewhere.
        0 => {
            let at = rng.below(chars.len() + 1);
            let insert: Vec<char> = format!("{word} ").chars().collect();
            chars.splice(at..at, insert);
        }
        // Remove a span.
        1 if !chars.is_empty() => {
            let from = rng.below(chars.len());
            let len = 1 + rng.below((chars.len() - from).min(6));
            chars.drain(from..from + len);
        }
        // Replace a span with a word.
        _ => {
            let from = rng.below(chars.len() + 1);
            let len = rng.below((chars.len() - from).min(5) + 1);
            chars.splice(from..from + len, word.chars());
        }
    }
    doc.set_text(&chars.into_iter().collect::<String>(), now);
}

const fn permutations() -> [[usize; 3]; 6] {
    [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ]
}

/// Runs one random history and returns whether the note ended up deleted.
fn run(seed: u64, steps: usize) -> bool {
    let mut rng = Rng(seed);
    let first = NoteDoc::new(IDS[0]);
    first.set_text("buy milk today", 0);
    let start = first.encode();
    let docs = [
        first,
        NoteDoc::from_bytes(IDS[1], &start).expect("load"),
        NoteDoc::from_bytes(IDS[2], &start).expect("load"),
    ];

    for step in 0..steps {
        let who = rng.below(3);
        let now = i64::try_from(step).expect("fits") + 1;
        match rng.below(10) {
            0 | 1 => docs[who].delete(),
            2 => docs[who].restore(now),
            // A one-way sync from another device.
            3 | 4 => {
                let from = (who + 1 + rng.below(2)) % 3;
                docs[who].merge(&docs[from].encode()).expect("merge");
            }
            _ => random_text_edit(&docs[who], &mut rng, now),
        }
    }

    // Half of the histories end with a delete, so that the final state is
    // deleted often enough to test. Half of those deletes come from a device
    // that has just seen everything; the other half from one that may not have.
    if rng.below(2) == 0 {
        let who = rng.below(3);
        if rng.below(2) == 0 {
            for from in 0..3 {
                if from != who {
                    docs[who].merge(&docs[from].encode()).expect("merge");
                }
            }
        }
        docs[who].delete();
    }

    let states: Vec<Vec<u8>> = docs.iter().map(NoteDoc::encode).collect();
    let mut results = Vec::new();
    for order in permutations() {
        let observer = NoteDoc::new(OBSERVER);
        for index in order {
            observer.merge(&states[index]).expect("merge");
        }
        results.push((
            observer.text(),
            observer.is_deleted(),
            observer.version_vector(),
            observer.edited_at_ms(),
        ));
    }
    for result in &results[1..] {
        assert_eq!(
            result, &results[0],
            "seed {seed}: merge order changed the note"
        );
    }

    // The devices themselves must agree too, once each has merged the others.
    for doc in &docs {
        for state in &states {
            doc.merge(state).expect("merge");
        }
    }
    for doc in &docs {
        assert_eq!(doc.text(), results[0].0, "seed {seed}: device text differs");
        assert_eq!(
            doc.is_deleted(),
            results[0].1,
            "seed {seed}: deleted differs"
        );
    }
    results[0].1
}

#[test]
fn three_devices_converge_whatever_the_merge_order() {
    let deleted = (1..=300).filter(|seed| run(*seed, 60)).count();
    // Guard against a test that passes without exercising the rule: the random
    // histories must end both ways.
    assert!(
        deleted >= 60,
        "only {deleted} of 300 histories ended deleted"
    );
    println!("{deleted} of 300 histories ended deleted");
    assert!(deleted <= 240, "{deleted} of 300 histories ended deleted");
}

#[test]
fn long_histories_converge() {
    for seed in 1000..1010 {
        run(seed, 600);
    }
}
