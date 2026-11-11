//! Measures how large a note's CRDT document gets.
//!
//! The redesign uploads the whole document on every change, so its size after
//! many edits decides whether that approach holds. Run with
//! `cargo run --release` and read the table.

use yrs_size_trial::NoteDoc;

const DEVICE: u64 = 7_100_000_000_000_001;

const BASE: &str = "Remember to email the landlord about the heating, then pick up the \
parcel before six. Ask Mina whether Thursday still works for dinner, and book the table if \
it does. #home #todo";

const SHORT: &str = "buy milk and eggs on the way home from work today ok";

const CJK: &str = "記得寫信給房東談暖氣的事，然後六點前去拿包裹。問問米娜星期四晚餐還行不行，\
可以的話就訂位。 #家 #待辦";

const WORDS: [&str; 10] = [
    "landlord", "parcel", "Thursday", "dinner", "tomorrow", "nine", "Sam", "tickets", "quiet", "soon",
];

const CJK_WORDS: [&str; 8] = ["房東", "包裹", "星期五", "午餐", "明天", "九點", "小山", "車票"];

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % n as u64).expect("fits")
    }
}

struct Row {
    scenario: &'static str,
    edits: usize,
    text_bytes: usize,
    v1: usize,
    v2: usize,
    /// Size of the same document before any of the measured edits.
    start_v2: usize,
}

impl Row {
    fn measure(scenario: &'static str, edits: usize, doc: &NoteDoc, start_v2: usize) -> Self {
        Self {
            scenario,
            edits,
            text_bytes: doc.text().len(),
            v1: doc.encode_v1().len(),
            v2: doc.encode().len(),
            start_v2,
        }
    }

    fn per_edit(&self) -> f64 {
        if self.edits == 0 {
            return 0.0;
        }
        (self.v2 as f64 - self.start_v2 as f64) / self.edits as f64
    }
}

/// Replaces one word, chosen at random, with another word.
fn change_one_word(text: &str, pool: &[&str], separator: char, rng: &mut Rng) -> String {
    let mut parts: Vec<&str> = text.split(separator).collect();
    let at = rng.below(parts.len());
    parts[at] = pool[rng.below(pool.len())];
    parts.join(&separator.to_string())
}

/// Replaces a short run of characters, for text without spaces between words.
fn change_a_few_chars(text: &str, pool: &[&str], rng: &mut Rng) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let from = rng.below(chars.len().saturating_sub(2).max(1));
    let to = (from + 2).min(chars.len());
    chars.splice(from..to, pool[rng.below(pool.len())].chars());
    chars.into_iter().collect()
}

fn started(text: &str) -> (NoteDoc, usize) {
    let doc = NoteDoc::new(DEVICE);
    doc.set_text(text, 0);
    let start = doc.encode().len();
    (doc, start)
}

fn word_changes(scenario: &'static str, base: &str, edits: usize, whole: bool) -> Row {
    let (doc, start) = started(base);
    let mut rng = Rng(42);
    for i in 0..edits {
        let next = change_one_word(&doc.text(), &WORDS, ' ', &mut rng);
        let now = i64::try_from(i).expect("fits") + 1;
        if whole {
            doc.replace_whole_text(&next, now);
        } else {
            doc.set_text(&next, now);
        }
    }
    Row::measure(scenario, edits, &doc, start)
}

fn cjk_changes(edits: usize) -> Row {
    let (doc, start) = started(CJK);
    let mut rng = Rng(42);
    for i in 0..edits {
        let next = change_a_few_chars(&doc.text(), &CJK_WORDS, &mut rng);
        doc.set_text(&next, i64::try_from(i).expect("fits") + 1);
    }
    Row::measure("Non-ASCII note, change a few characters", edits, &doc, start)
}

/// Types the note one character per save.
fn typing() -> Row {
    let doc = NoteDoc::new(DEVICE);
    let start = doc.encode().len();
    let mut typed = String::new();
    for (i, ch) in BASE.chars().enumerate() {
        typed.push(ch);
        doc.set_text(&typed, i64::try_from(i).expect("fits"));
    }
    Row::measure("Type the note, one character per save", BASE.chars().count(), &doc, start)
}

/// Appends a line per run, reloading the document from bytes each time, as the
/// CLI would. `fresh_id` gives every run a new client ID.
fn cli_appends(scenario: &'static str, edits: usize, fresh_id: bool) -> Row {
    let first = NoteDoc::new(DEVICE);
    first.set_text("log", 0);
    let start = first.encode().len();
    let mut stored = first.encode();
    for i in 0..edits {
        let id = if fresh_id {
            DEVICE + 1 + u64::try_from(i).expect("fits") * 7_919
        } else {
            DEVICE
        };
        let doc = NoteDoc::from_bytes(id, &stored).expect("load");
        let next = format!("{}\nitem {i}", doc.text());
        doc.set_text(&next, i64::try_from(i).expect("fits") + 1);
        stored = doc.encode();
    }
    let doc = NoteDoc::from_bytes(DEVICE, &stored).expect("load");
    Row::measure(scenario, edits, &doc, start)
}

fn main() {
    let rows = vec![
        word_changes("Note as first saved (no edits)", BASE, 0, false),
        typing(),
        word_changes("Change one word, 100 times", BASE, 100, false),
        word_changes("Change one word, 1,000 times", BASE, 1_000, false),
        word_changes("Change one word, 10,000 times", BASE, 10_000, false),
        word_changes("Short note, change one word, 1,000 times", SHORT, 1_000, false),
        cjk_changes(1_000),
        word_changes("Whole-text replacement, 1,000 times", BASE, 1_000, true),
        cli_appends("CLI appends a line, 1,000 runs, same ID", 1_000, false),
        cli_appends("CLI appends a line, 1,000 runs, new ID each run", 1_000, true),
    ];

    println!("| Scenario | Edits | Text bytes | v1 bytes | v2 bytes | v2 bytes per edit |");
    println!("|---|---|---|---|---|---|");
    for row in &rows {
        println!(
            "| {} | {} | {} | {} | {} | {:.1} |",
            row.scenario,
            row.edits,
            row.text_bytes,
            row.v1,
            row.v2,
            row.per_edit()
        );
    }

    // The cost of deleting: one marker per deleting device.
    let (doc, _) = started(BASE);
    let before = doc.encode().len();
    doc.delete();
    println!();
    println!(
        "Deletion marker: {} bytes added to a {}-byte document.",
        doc.encode().len() - before,
        before
    );
}
