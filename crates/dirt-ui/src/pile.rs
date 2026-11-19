//! Grains and the pile, drawn (interface design, section 6.17).
//!
//! Every note is a grain of sand, a shade taken from its ID
//! ([`dirt_core::models::Grain`]). Under the list lies the pile: one tiny
//! grain for every thought, piled the way sand piles. [`Heap::of`] works
//! the heap out from the notes the core gives ([`dirt_core::Store::pile`])
//! and draws it as one picture. It runs once each time the notes change,
//! never per frame, and the script only plays the new grain's drop.
//!
//! How the sand piles: grains are added oldest first. Each is dropped near
//! the middle, a little to either side, as from a hand, and rolls down
//! while the slope under it is steeper than sand's angle of repose, now
//! and then rolling a step further. The heap that results has sand's own
//! shape at any size: a rounded top, straight flanks, a foot that spreads.
//!
//! Each grain's chances come from its own note, not from one stream for
//! the whole heap: so the same notes always make the same heap, and a new
//! note adds a grain on top without moving any grain already there.

use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dirt_core::models::{Grain, PileNote, PileSummary};
use dirt_core::store::NoteCounts;

/// The widest the heap may be under the list, in CSS pixels. Wider than
/// that, its grains get smaller instead.
pub const LIST_WIDTH: f64 = 250.0;

/// A grain at its largest, in CSS pixels: about a third of a millimetre on
/// most screens, too small to see one alone, so that many read as sand.
const GRAIN: f64 = 1.7;

/// How far apart two rows of grains are, as a share of a grain: grains
/// overlap a little, so no background shows between them.
const ROW: f64 = 0.64;

/// Coordinates are written in tenths of a pixel, as whole numbers, which
/// keeps the picture short.
const SCALE: f64 = 10.0;

/// The CSS colour of a grain's shade of sand.
#[must_use]
pub fn shade(grain: &Grain) -> String {
    format!(
        "hsl({} {}% {}%)",
        grain.hue, grain.saturation, grain.lightness
    )
}

/// The outline of a grain drawn large enough to see (a dot on the rail, a
/// grain that settles): one of four slightly irregular rounds, the same
/// everywhere for the same note. A CSS `border-radius`.
#[must_use]
pub const fn shape(grain: &Grain) -> &'static str {
    const SHAPES: [&str; 4] = [
        "50%",
        "46% 54% 52% 48%",
        "54% 46% 48% 52%",
        "48% 52% 55% 45%",
    ];
    SHAPES[(grain.seed >> 16) as usize % SHAPES.len()]
}

/// A number as it is written in English, with a comma between thousands:
/// "1,254".
#[must_use]
pub fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// "1 thought", "1,254 thoughts": what pointing at the heap says.
#[must_use]
pub fn thoughts(count: usize) -> String {
    let word = if count == 1 { "thought" } else { "thoughts" };
    format!("{} {word}", grouped(count))
}

/// The line under the heap: "1,254 thoughts since Nov 2025". Nothing for
/// an empty pile.
#[must_use]
pub fn line(summary: &PileSummary) -> Option<String> {
    let since = summary.since?;
    let month: String = since.name().chars().take(3).collect();
    Some(format!(
        "{} since {month} {}",
        thoughts(summary.thoughts),
        since.year
    ))
}

/// The small random numbers each grain's fall is made of: Mulberry32,
/// started from the grain's own seed, as the mockup's piling is.
struct Chances(u32);

impl Chances {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 0x3D));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// Where one grain came to rest: its column, counted from the middle, and
/// its row, counted from the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rest {
    column: i32,
    row: u32,
}

/// The heights of the columns, growing sideways as grains reach further
/// out. A column no grain has reached is empty ground.
struct Ground {
    heights: Vec<u32>,
    /// The index in `heights` of column 0, the middle.
    middle: usize,
}

impl Ground {
    fn new() -> Self {
        Self {
            heights: vec![0; 33],
            middle: 16,
        }
    }

    fn index(&self, column: i32) -> Option<usize> {
        let index = isize::try_from(self.middle).ok()? + isize::try_from(column).ok()?;
        usize::try_from(index)
            .ok()
            .filter(|index| *index < self.heights.len())
    }

    fn height(&self, column: i32) -> u32 {
        self.index(column).map_or(0, |index| self.heights[index])
    }

    /// Makes room for `column` and one more on either side of it.
    fn reach(&mut self, column: i32) {
        while self.index(column - 1).is_none() {
            let more = self.heights.len();
            self.heights.splice(0..0, std::iter::repeat_n(0, more));
            self.middle += more;
        }
        while self.index(column + 1).is_none() {
            let more = self.heights.len();
            self.heights.extend(std::iter::repeat_n(0, more));
        }
    }

    fn add(&mut self, column: i32) -> u32 {
        self.reach(column);
        let index = self.index(column).unwrap_or_default();
        let row = self.heights[index];
        self.heights[index] += 1;
        row
    }
}

/// Drops one grain onto the ground and lets it roll: down while the slope
/// under it is steeper than sand's angle of repose (two rows or more
/// above the lower neighbour, and nearly always one), and now and then a
/// step further once it is moving.
fn drop_one(ground: &mut Ground, chances: &mut Chances) -> Rest {
    let toss = chances.next() + chances.next() + chances.next() - 1.5;
    // Within a few columns of the middle: never far from zero.
    #[allow(clippy::cast_possible_truncation)]
    let mut column = (toss * 3.4).round() as i32;
    let mut moving = false;
    loop {
        ground.reach(column);
        let here = ground.height(column);
        let (left, right) = (ground.height(column - 1), ground.height(column + 1));
        let lower = left.min(right);
        let drop = i64::from(here) - i64::from(lower);
        let side = match left.cmp(&right) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal if chances.next() < 0.5 => -1,
            std::cmp::Ordering::Greater | std::cmp::Ordering::Equal => 1,
        };
        if drop >= 2 || (drop == 1 && chances.next() > 0.12) {
            column += side;
            moving = true;
        } else if moving && drop >= 0 && chances.next() < 0.3 {
            column += side;
        } else {
            break;
        }
    }
    Rest {
        column,
        row: ground.add(column),
    }
}

/// Every grain's resting place, in the order given (oldest first), and
/// the jitter that keeps the rows from looking laid.
fn pile_up(seeds: &[u32]) -> Vec<(Rest, f64, f64)> {
    let mut ground = Ground::new();
    seeds
        .iter()
        .map(|seed| {
            let mut chances = Chances(*seed);
            let rest = drop_one(&mut ground, &mut chances);
            let jitter = (chances.next() - 0.5, chances.next() - 0.5);
            (rest, jitter.0, jitter.1)
        })
        .collect()
}

/// The pile drawn: one picture of every grain, ready for the page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Heap {
    /// How many grains it has.
    pub grains: usize,
    /// An SVG element: the soft shadow under the heap, the grains grouped
    /// by shade, and the newest grain on its own (`dui-pile-top`), which
    /// the script drops onto the heap as a thought settles. Empty for an
    /// empty pile.
    pub svg: String,
}

impl Heap {
    /// The heap of `notes`, oldest first as [`dirt_core::Store::pile`]
    /// gives them, no wider than `widest` CSS pixels. Its area grows with
    /// the number of thoughts: ten are a pinch 15 px wide, a year's 1,250
    /// about 110 px wide and 50 high. When it would be wider than
    /// `widest`, the grains get smaller instead.
    #[must_use]
    pub fn of(notes: &[PileNote], widest: f64) -> Self {
        if notes.is_empty() {
            return Self::default();
        }
        let grains: Vec<Grain> = notes.iter().map(PileNote::grain).collect();
        let seeds: Vec<u32> = grains.iter().map(|grain| grain.seed).collect();
        let rests = pile_up(&seeds);
        Self {
            grains: notes.len(),
            svg: draw(&grains, &rests, widest),
        }
    }
}

/// The pile as a list shows it under its last note: the heap, and the
/// numbers of the line under it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PileView {
    pub heap: Arc<Heap>,
    pub summary: PileSummary,
    /// Which notes the heap is made of, as one number.
    made_of: u64,
    /// What the store's counts were when the notes were read: how many
    /// there were, and their mark ([`NoteCounts::live_mark`]).
    read_for: Option<(usize, i64)>,
}

impl PileView {
    /// The pile now, given the store's counts from the same reading of the
    /// list, where local time is `utc_offset_secs` ahead of UTC.
    ///
    /// The list is read again at every pause in typing, every sync and
    /// every page loaded, and [`dirt_core::Store::pile`] reads every live
    /// note. So the notes are read (`read`) only when the counts say they
    /// may have changed since `before` read them: a note made, deleted or
    /// brought back. An edit or an archive reads nothing; the numbers of
    /// the line are the counts' own. The heap is worked out again only
    /// when the notes read are not those it was made of, away from the
    /// screen's own thread: on a debug build 5,000 grains take about
    /// 16 ms (`docs/ui/design.md`, 6.17). If the notes cannot be read, the
    /// pile is as it was.
    pub async fn read<F, Fut>(
        before: Option<Self>,
        counts: NoteCounts,
        utc_offset_secs: i32,
        read: F,
    ) -> Self
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = dirt_core::Result<Vec<PileNote>>>,
    {
        let key = (counts.listed + counts.archived, counts.live_mark);
        let counted = |mut view: Self| {
            view.summary.thoughts = key.0;
            view.summary.archived = counts.archived;
            view
        };
        if let Some(before) = before
            .as_ref()
            .filter(|before| before.read_for == Some(key))
        {
            return counted(before.clone());
        }
        match read().await {
            Ok(notes) => {
                let mut view = Self::of(notes, before.as_ref(), utc_offset_secs).await;
                view.read_for = Some(key);
                counted(view)
            }
            Err(error) => {
                tracing::error!("Could not read the pile: {error}");
                before.unwrap_or_default()
            }
        }
    }

    /// The pile of `notes`, as [`dirt_core::Store::pile`] gives them. The
    /// heap is `before`'s if it was made of the same notes.
    async fn of(notes: Vec<PileNote>, before: Option<&Self>, utc_offset_secs: i32) -> Self {
        let summary = PileSummary::of(&notes, utc_offset_secs);
        let made_of = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for note in &notes {
                note.id.hash(&mut hasher);
            }
            hasher.finish()
        };
        if let Some(before) = before.filter(|before| before.made_of == made_of) {
            return Self {
                heap: Arc::clone(&before.heap),
                summary,
                made_of,
                read_for: None,
            };
        }
        let started = std::time::Instant::now();
        let count = notes.len();
        let heap = tokio::task::spawn_blocking(move || Heap::of(&notes, LIST_WIDTH))
            .await
            .unwrap_or_default();
        tracing::debug!(
            "The pile of {count} grains took {:?} to work out",
            started.elapsed()
        );
        Self {
            heap: Arc::new(heap),
            summary,
            made_of,
            read_for: None,
        }
    }
}

/// Rounds a length in pixels to the tenths the picture is written in.
#[allow(clippy::cast_possible_truncation)]
fn tenths(pixels: f64) -> i64 {
    (pixels * SCALE).round() as i64
}

/// The picture of grains that have come to rest.
#[allow(clippy::cast_precision_loss)]
fn draw(grains: &[Grain], rests: &[(Rest, f64, f64)], widest: f64) -> String {
    let count = rests.len();
    let size = GRAIN.min(widest / 2.3f64.mul_add((count as f64).sqrt(), 6.0));
    let row = size * ROW;
    let low = rests.iter().map(|rest| rest.0.column).min().unwrap_or(0);
    let high = rests.iter().map(|rest| rest.0.column).max().unwrap_or(0);
    // The tallest column: the first of the tallest, from the left.
    let mut tallest = (low, 0);
    for rest in rests {
        if rest.0.row + 1 > tallest.1 || (rest.0.row + 1 == tallest.1 && rest.0.column < tallest.0)
        {
            tallest = (rest.0.column, rest.0.row + 1);
        }
    }
    let (peak, top_row) = tallest;
    let width = f64::from(high - low + 2) * size;
    let height = f64::from(top_row).mul_add(row, size);

    // Light from the upper left: the shade deepens smoothly towards the
    // far side, by up to a quarter, with no line where the flanks meet.
    let half = (f64::from(high - low) / 2.0).max(1.0);
    let light = |column: i32| {
        let t = (half.mul_add(0.5, f64::from(column - peak)) / (half * 1.3)).clamp(0.0, 1.0);
        0.24f64.mul_add(-(t * t * 2.0f64.mul_add(-t, 3.0)), 1.06)
    };
    let place = |rest: &(Rest, f64, f64)| {
        let (Rest { column, row: level }, jitter_x, jitter_y) = *rest;
        let offset = if level % 2 == 1 { size / 2.0 } else { 0.0 };
        let x =
            (jitter_x * size).mul_add(0.5, (f64::from(column - low) + 0.5).mul_add(size, offset));
        let y = (jitter_y * size).mul_add(0.3, -f64::from(level).mul_add(row, row * 0.7));
        (tenths(x), tenths(y))
    };
    // A shade as it is drawn: the note's, deepened by the light, a little
    // coarser than the eye can tell apart, so that grains of one shade can
    // be drawn together.
    let colour = |grain: &Grain, column: i32| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let lightness = (f64::from(grain.lightness) * light(column) / 2.0).round() as u8 * 2;
        ((grain.hue / 3) * 3, (grain.saturation / 4) * 4, lightness)
    };
    let css = |(hue, saturation, lightness): (u16, u8, u8)| {
        format!("hsl({hue} {saturation}% {lightness}%)")
    };

    let mut groups: Vec<((u16, u8, u8), String)> = Vec::new();
    let mut index_of = std::collections::HashMap::<(u16, u8, u8), usize>::new();
    let newest = count - 1;
    for (grain, rest) in grains.iter().zip(rests).take(newest) {
        let shade = colour(grain, rest.0.column);
        let (x, y) = place(rest);
        let at = *index_of.entry(shade).or_insert_with(|| {
            groups.push((shade, String::new()));
            groups.len() - 1
        });
        // A line of no length with round ends is a dot.
        let _ = write!(groups[at].1, "M{x} {y}h0");
    }

    let left = tenths(-size);
    let top = tenths(-height - 2.0);
    let view_width = tenths(2.0f64.mul_add(size, width));
    let view_height = tenths(size.mul_add(1.6, height + 2.0));
    let mut svg = format!(
        "<svg viewBox=\"{left} {top} {view_width} {view_height}\" width=\"{:.0}\" height=\"{:.0}\" aria-hidden=\"true\">\
         <defs><radialGradient id=\"dui-pile-shadow\"><stop offset=\"0\" stop-color=\"#000\" stop-opacity=\".28\"/>\
         <stop offset=\"1\" stop-color=\"#000\" stop-opacity=\"0\"/></radialGradient></defs>\
         <ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"url(#dui-pile-shadow)\"/>\
         <g fill=\"none\" stroke-linecap=\"round\" stroke-width=\"{}\">",
        2.0f64.mul_add(size, width),
        size.mul_add(1.6, height + 2.0),
        tenths(width / 2.0),
        tenths(size * 0.3),
        tenths(width * 0.6),
        tenths(size * 1.3),
        tenths(size * 1.6),
    );
    for (shade, dots) in &groups {
        let _ = write!(svg, "<path stroke=\"{}\" d=\"{dots}\"/>", css(*shade));
    }
    svg.push_str("</g>");
    let (x, y) = place(&rests[newest]);
    let _ = write!(
        svg,
        "<circle class=\"dui-pile-top\" cx=\"{x}\" cy=\"{y}\" r=\"{}\" fill=\"{}\"/></svg>",
        tenths(size * 0.8),
        css(colour(&grains[newest], rests[newest].0.column)),
    );
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use dirt_core::models::{NoteId, YearMonth};

    fn notes(count: usize) -> Vec<PileNote> {
        let mut notes: Vec<PileNote> = (0..count)
            .map(|_| PileNote {
                id: NoteId::new(),
                edited_at: 0,
                archived: false,
            })
            .collect();
        notes.sort_unstable_by_key(|note| note.id);
        notes
    }

    fn seeds(count: u32) -> Vec<u32> {
        // Spread as hashes are, not counted up.
        (0..count)
            .map(|n| n.wrapping_mul(2_654_435_761) ^ 0x9E37_79B9)
            .collect()
    }

    /// The width and height of the heap, in grains and rows.
    fn extent(rests: &[(Rest, f64, f64)]) -> (i32, u32) {
        let low = rests.iter().map(|rest| rest.0.column).min().unwrap();
        let high = rests.iter().map(|rest| rest.0.column).max().unwrap();
        let tall = rests.iter().map(|rest| rest.0.row + 1).max().unwrap();
        (high - low + 1, tall)
    }

    /// The width and height the picture says, in CSS pixels.
    fn size_of(svg: &str) -> (f64, f64) {
        let number = |name: &str| {
            let start = svg.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
            svg[start..]
                .split('"')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        };
        (number("width"), number("height"))
    }

    #[test]
    fn the_same_notes_make_the_same_heap() {
        let notes = notes(300);
        assert_eq!(Heap::of(&notes, LIST_WIDTH), Heap::of(&notes, LIST_WIDTH));
        assert_eq!(Heap::of(&notes, LIST_WIDTH).grains, 300);
        assert_eq!(Heap::of(&[], LIST_WIDTH), Heap::default());
    }

    /// A new thought adds a grain on top: no grain already there moves.
    #[test]
    fn a_new_grain_moves_no_other() {
        let seeds = seeds(800);
        let before = pile_up(&seeds[..799]);
        let after = pile_up(&seeds);
        assert_eq!(&after[..799], &before[..]);
    }

    /// Every grain rests on the ground or on the grain below it: nothing
    /// floats, and the newest grains are on the surface.
    #[test]
    fn grains_rest_on_grains() {
        let rests = pile_up(&seeds(2_000));
        let mut heights = std::collections::HashMap::<i32, u32>::new();
        for (rest, _, _) in &rests {
            let height = heights.entry(rest.column).or_default();
            assert_eq!(rest.row, *height, "a grain fell into its column's top");
            *height += 1;
        }
    }

    /// Sand's shape: a heap a little more than twice as wide as it is
    /// high, measured from its top to its foot (the design's 110 by 50 px),
    /// and the same shape whatever its size.
    #[test]
    fn the_heap_keeps_sands_shape_at_any_size() {
        let angles: Vec<f64> = [200_u32, 1_250, 5_000]
            .iter()
            .map(|count| {
                let (wide, tall) = extent(&pile_up(&seeds(*count)));
                // In pixels: a row is less than a grain high.
                let (wide, tall) = (f64::from(wide), f64::from(tall) * ROW);
                (tall / (wide / 2.0)).atan().to_degrees()
            })
            .collect();
        for angle in &angles {
            assert!((32.0..50.0).contains(angle), "{angles:?}");
        }
        let (least, most) = angles
            .iter()
            .fold((90.0_f64, 0.0_f64), |(least, most), angle| {
                (least.min(*angle), most.max(*angle))
            });
        assert!(most - least < 8.0, "{angles:?}");
    }

    /// Ten are a pinch 15 px wide; a year's 1,250 are about 110 px wide and
    /// 50 high. Far more, and the grains get smaller to fit.
    #[test]
    fn the_heap_is_as_big_as_the_pile() {
        let (wide, _) = size_of(&Heap::of(&notes(10), LIST_WIDTH).svg);
        assert!((12.0..26.0).contains(&wide), "ten are {wide} px wide");
        let (wide, high) = size_of(&Heap::of(&notes(1_250), LIST_WIDTH).svg);
        assert!((90.0..135.0).contains(&wide), "a year is {wide} px wide");
        assert!((38.0..65.0).contains(&high), "a year is {high} px high");
        let (wide, _) = size_of(&Heap::of(&notes(20_000), LIST_WIDTH).svg);
        assert!(
            wide <= LIST_WIDTH + 8.0,
            "twenty thousand are {wide} px wide"
        );
        let (narrow, _) = size_of(&Heap::of(&notes(1_250), 60.0).svg);
        assert!(narrow <= 68.0, "in a narrow place, {narrow} px");
    }

    /// The far side is in shade: the right flank's grains are darker than
    /// the left's.
    #[test]
    fn light_comes_from_the_upper_left() {
        let rests = pile_up(&seeds(1_000));
        let grain = Grain {
            hue: 30,
            saturation: 40,
            lightness: 60,
            seed: 0,
        };
        let grains = vec![grain; rests.len()];
        let svg = draw(&grains, &rests, LIST_WIDTH);
        let lightness: Vec<u32> = svg
            .match_indices("hsl(30 40% ")
            .map(|(at, found)| {
                svg[at + found.len()..]
                    .split('%')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert!(lightness.iter().any(|&light| light >= 62), "{lightness:?}");
        assert!(lightness.iter().any(|&light| light <= 50), "{lightness:?}");
    }

    #[test]
    fn the_newest_grain_is_drawn_on_its_own_on_top() {
        let notes = notes(40);
        let svg = Heap::of(&notes, LIST_WIDTH).svg;
        assert_eq!(svg.matches("dui-pile-top").count(), 1);
        assert!(svg.rfind("dui-pile-top").unwrap() > svg.rfind("<path").unwrap());
        // 39 dots in the groups, and the newest alone.
        assert_eq!(svg.matches("h0").count(), 39);
        let one = Heap::of(&notes[..1], LIST_WIDTH).svg;
        assert_eq!(one.matches("h0").count(), 0);
        assert_eq!(one.matches("dui-pile-top").count(), 1);
    }

    #[test]
    fn the_words_of_the_pile() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_254), "1,254");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(thoughts(1), "1 thought");
        assert_eq!(thoughts(1_254), "1,254 thoughts");
        let summary = PileSummary {
            thoughts: 1_254,
            archived: 3,
            since: Some(YearMonth {
                year: 2025,
                month: 11,
            }),
        };
        assert_eq!(
            line(&summary).as_deref(),
            Some("1,254 thoughts since Nov 2025")
        );
        assert_eq!(line(&PileSummary::default()), None);
    }

    #[test]
    fn a_grains_shade_and_shape_are_its_notes() {
        let grain = Grain {
            hue: 21,
            saturation: 34,
            lightness: 52,
            seed: 4_134_277_249,
        };
        assert_eq!(shade(&grain), "hsl(21 34% 52%)");
        assert_eq!(shape(&grain), shape(&grain));
        let shapes: std::collections::HashSet<&str> = (0..64_u32)
            .map(|n| {
                shape(&Grain {
                    seed: n << 16,
                    ..grain
                })
            })
            .collect();
        assert_eq!(shapes.len(), 4);
    }

    /// The store is asked for the notes only when the counts say they
    /// may have changed: not for an edit or an archive, whose numbers come
    /// from the counts.
    #[tokio::test]
    async fn the_notes_are_read_only_when_the_counts_say_they_changed() {
        let mut notes = notes(5);
        let counts = |listed, archived, live_mark| NoteCounts {
            listed,
            archived,
            live_mark,
        };
        let reads = std::cell::Cell::new(0);
        let read = |notes: Vec<PileNote>| {
            reads.set(reads.get() + 1);
            async move { Ok(notes) }
        };

        let first = PileView::read(None, counts(5, 0, 15), 0, || read(notes.clone())).await;
        assert_eq!((reads.get(), first.summary.thoughts), (1, 5));
        assert_eq!(first.heap.grains, 5);

        // An edit: the same counts. Nothing is read.
        let edited = PileView::read(Some(first.clone()), counts(5, 0, 15), 0, || {
            read(notes.clone())
        })
        .await;
        assert_eq!(reads.get(), 1);
        assert_eq!(edited, first);

        // An archive: one note more archived, the same notes. Nothing is
        // read, and the line says so.
        let archived =
            PileView::read(Some(edited), counts(4, 1, 15), 0, || read(notes.clone())).await;
        assert_eq!(reads.get(), 1);
        assert_eq!(
            (archived.summary.thoughts, archived.summary.archived),
            (5, 1)
        );
        assert!(Arc::ptr_eq(&archived.heap, &first.heap));

        // A note made: read, and the heap worked out again.
        notes.extend(super::tests::notes(1));
        let made =
            PileView::read(Some(archived), counts(5, 1, 21), 0, || read(notes.clone())).await;
        assert_eq!(reads.get(), 2);
        assert_eq!((made.summary.thoughts, made.heap.grains), (6, 6));

        // A read that fails leaves the pile as it was.
        let failed = PileView::read(Some(made.clone()), counts(6, 1, 28), 0, || async {
            Err(dirt_core::Error::Database("closed".to_owned()))
        })
        .await;
        assert_eq!(failed, made);
    }

    /// How long the heap takes to work out, on piles of several sizes. It
    /// runs once each time the notes change. `cargo test -p dirt-ui
    /// --release heap_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement, not a check"]
    fn heap_cost() {
        for count in [1_000, 5_000, 20_000] {
            let notes = notes(count);
            let start = std::time::Instant::now();
            let runs = 5;
            let mut bytes = 0;
            for _ in 0..runs {
                bytes = Heap::of(&notes, LIST_WIDTH).svg.len();
            }
            println!(
                "{count} grains: {:?} each, {} KB of picture",
                start.elapsed() / runs,
                bytes / 1024
            );
        }
    }
}
