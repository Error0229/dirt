//! Grains, layers and the pile (interface design 6.17).
//!
//! Every note is a grain of sand. The list is laid in layers by when each
//! note was last edited, and under it lies the pile: one grain for every
//! note the device holds. All of it is worked out from the notes; nothing
//! here is stored, and nothing here reads a clock or a time zone. The
//! caller says what time it is and how far local time is from UTC, so the
//! same notes give the same answer in a test as on a device.

use chrono::{DateTime, Datelike, NaiveDate};

use super::NoteId;

/// One note as the pile needs it: which note, when it was last edited and
/// whether it is archived. [`crate::Store::pile`] gives every live note
/// this way, without its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PileNote {
    pub id: NoteId,
    /// When the note was last edited, in Unix milliseconds.
    pub edited_at: i64,
    /// An archived note is out of the list and still a grain in the pile.
    pub archived: bool,
}

impl PileNote {
    /// When the note was made, in Unix milliseconds, read from its ID. An
    /// ID that carries no time can only have been made outside this
    /// program, and such a note counts as made when it was last edited.
    #[must_use]
    pub fn created_at(&self) -> i64 {
        self.id.created_at_ms().unwrap_or(self.edited_at)
    }

    /// The note's shade of sand.
    #[must_use]
    pub fn grain(&self) -> Grain {
        Grain::of(&self.id)
    }
}

/// A note's shade of sand, as the three numbers of an HSL colour.
///
/// It is taken from the note's ID and from nothing else, so one note is
/// the same shade on every device and for as long as it exists, and
/// neighbours differ a little, as sand does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Grain {
    /// The hue in degrees, from 20 up to and not including 44.
    pub hue: u16,
    /// The saturation in percent, from 30 up to and not including 62.
    pub saturation: u8,
    /// The lightness in percent, from 48 up to and not including 72.
    pub lightness: u8,
    /// The number the shade was taken from. Anything else about a grain
    /// that has to be the same wherever it is drawn, such as which of a
    /// few slightly irregular shapes it has, can be taken from this.
    pub seed: u32,
}

impl Grain {
    /// The grain of the note with this ID.
    ///
    /// The ID is hashed as text, in its usual form of lower-case digits
    /// and hyphens, with the 32-bit FNV-1a hash. An ID begins with the
    /// time it was made, so two notes made together differ only towards
    /// its end; the hash spreads that difference over the whole number.
    /// This is the mockup's own function, so the two can be compared.
    #[must_use]
    pub fn of(id: &NoteId) -> Self {
        let mut text = [0_u8; uuid::fmt::Hyphenated::LENGTH];
        let text = id.as_uuid().hyphenated().encode_lower(&mut text);
        let seed = fnv1a(text.as_bytes());
        // Each remainder is far below the size of its type.
        let within = |value: u32, span: u32| u8::try_from(value % span).unwrap_or_default();
        Self {
            hue: 20 + u16::from(within(seed, 24)),
            saturation: 30 + within(seed >> 5, 32),
            lightness: 48 + within(seed >> 10, 24),
            seed,
        }
    }
}

/// The 32-bit FNV-1a hash.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2_166_136_261, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(16_777_619)
    })
}

/// The date at `at_ms` where local time is `utc_offset_secs` ahead of UTC.
/// A time too far off to be a date at all counts as the first day of 1970.
fn local_date(at_ms: i64, utc_offset_secs: i32) -> NaiveDate {
    let local = at_ms.saturating_add(i64::from(utc_offset_secs) * 1000);
    DateTime::from_timestamp_millis(local)
        .unwrap_or_default()
        .date_naive()
}

/// A month of a year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonth {
    pub year: i32,
    /// From 1, January, to 12.
    pub month: u8,
}

impl YearMonth {
    fn of(date: NaiveDate) -> Self {
        Self {
            year: date.year(),
            month: u8::try_from(date.month()).unwrap_or(1),
        }
    }

    /// The month's name in English: "November".
    #[must_use]
    pub fn name(&self) -> &'static str {
        month_name(self.month)
    }
}

/// The English name of a month, counted from 1. Nothing for a number that
/// is no month.
#[must_use]
pub fn month_name(month: u8) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    usize::from(month)
        .checked_sub(1)
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or_default()
}

/// How many days before today still count as this week.
const DAYS_OF_THIS_WEEK: i64 = 6;

/// The layer of the list a note lies in, by when it was last edited.
///
/// The layers, from the surface down:
///
/// - **Today**: edited on today's date. It has no name: it is the surface.
///   An edit time later than now, from a device whose clock is ahead, is
///   today too.
/// - **This week**: edited on one of the six days before today. Not the
///   week of the calendar: where a week starts differs from one country to
///   the next, and on its first day a calendar week would hold nothing, so
///   that yesterday's notes would lie under "Earlier this month". On a
///   Monday this layer reaches back to the Tuesday before. It may reach
///   into last month, and at the start of January into last year.
/// - **Earlier this month**: older than that, and in today's month.
/// - **A month**: everything older, month by month.
///
/// Dates are local: the caller gives how far local time is from UTC now,
/// and that one figure is used for every note. A note edited within an
/// hour of midnight, before the clocks last changed, can so fall on the
/// other side of that midnight. An hour of one night in a note's past
/// matters only if the night ended a month, and then only to which of two
/// months names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Today,
    ThisWeek,
    EarlierThisMonth,
    Month {
        month: YearMonth,
        /// Whether the month is of the year it is now. Its name has the
        /// year when it is not.
        this_year: bool,
    },
}

impl Layer {
    /// The layer of a note last edited at `edited_at_ms`, seen at `now_ms`
    /// where local time is `utc_offset_secs` ahead of UTC (behind, if
    /// negative). Both times are Unix milliseconds.
    #[must_use]
    pub fn of(edited_at_ms: i64, now_ms: i64, utc_offset_secs: i32) -> Self {
        let today = local_date(now_ms, utc_offset_secs);
        let day = local_date(edited_at_ms, utc_offset_secs);
        if edited_at_ms >= now_ms || day >= today {
            return Self::Today;
        }
        if (today - day).num_days() <= DAYS_OF_THIS_WEEK {
            return Self::ThisWeek;
        }
        let (month, this_month) = (YearMonth::of(day), YearMonth::of(today));
        if month == this_month {
            Self::EarlierThisMonth
        } else {
            Self::Month {
                month,
                this_year: month.year == this_month.year,
            }
        }
    }

    /// The name the layer starts with: "This week", "Earlier this month",
    /// "October", "December 2025". Nothing for today.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        match self {
            Self::Today => None,
            Self::ThisWeek => Some("This week".to_owned()),
            Self::EarlierThisMonth => Some("Earlier this month".to_owned()),
            Self::Month {
                month,
                this_year: true,
            } => Some(month.name().to_owned()),
            Self::Month {
                month,
                this_year: false,
            } => Some(format!("{} {}", month.name(), month.year)),
        }
    }
}

/// The numbers of the line under the pile: "1,254 thoughts since Nov
/// 2025", and "3 archived". The words are the interface's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PileSummary {
    /// How many grains the pile has: every note that is not deleted. An
    /// archived note is one of them, since it stays in the pile.
    pub thoughts: usize,
    /// How many of them are archived.
    pub archived: usize,
    /// The month in which the oldest of them was made, in local time.
    /// Nothing for an empty pile.
    pub since: Option<YearMonth>,
}

impl PileSummary {
    /// The summary of these notes, where local time is `utc_offset_secs`
    /// ahead of UTC. A note is as old as its making, not its last edit.
    #[must_use]
    pub fn of(notes: &[PileNote], utc_offset_secs: i32) -> Self {
        Self {
            thoughts: notes.len(),
            archived: notes.iter().filter(|note| note.archived).count(),
            since: notes
                .iter()
                .map(PileNote::created_at)
                .min()
                .map(|oldest| YearMonth::of(local_date(oldest, utc_offset_secs))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i32 = 3600;

    /// A local date and time as Unix milliseconds, where local time is
    /// `offset` seconds ahead of UTC.
    fn at(date: (i32, u32, u32), time: (u32, u32), offset: i32) -> i64 {
        let local = NaiveDate::from_ymd_opt(date.0, date.1, date.2)
            .unwrap()
            .and_hms_opt(time.0, time.1, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        local - i64::from(offset) * 1000
    }

    fn id(text: &str) -> NoteId {
        text.parse().unwrap()
    }

    /// An ID made at this time.
    fn made_at(ms: i64) -> NoteId {
        let seconds = u64::try_from(ms / 1000).unwrap();
        let nanos = u32::try_from(ms % 1000).unwrap() * 1_000_000;
        let time = uuid::Timestamp::from_unix(uuid::NoContext, seconds, nanos);
        NoteId::from_uuid(uuid::Uuid::new_v7(time))
    }

    /// The numbers are pinned: a build that gave another shade for the
    /// same ID would colour one note differently on two devices. They are
    /// the 32-bit FNV-1a hash of the ID's text, worked out apart from this
    /// code.
    #[test]
    fn a_grain_is_the_same_for_the_same_id_everywhere() {
        let grain = |text: &str| {
            let grain = Grain::of(&id(text));
            (grain.seed, grain.hue, grain.saturation, grain.lightness)
        };
        assert_eq!(
            grain("0192d3a8-0000-7000-8000-000000000001"),
            (4_134_277_249, 21, 34, 52)
        );
        assert_eq!(
            grain("0192d3a8-0000-7000-8000-000000000002"),
            (4_083_944_392, 36, 60, 50)
        );
        assert_eq!(
            grain("019a8f3e-5c2b-7d4a-9e1f-3b6c8d2a4f70"),
            (670_326_703, 27, 59, 63)
        );
        // However the ID was written, it is the same ID.
        assert_eq!(
            Grain::of(&id("019A8F3E-5C2B-7D4A-9E1F-3B6C8D2A4F70")),
            Grain::of(&id("019a8f3e5c2b7d4a9e1f3b6c8d2a4f70"))
        );
    }

    #[test]
    fn every_grain_is_a_shade_of_sand_and_neighbours_differ() {
        let mut shades = std::collections::HashSet::new();
        for _ in 0..2_000 {
            let grain = Grain::of(&NoteId::new());
            assert!((20..44).contains(&grain.hue), "{grain:?}");
            assert!((30..62).contains(&grain.saturation), "{grain:?}");
            assert!((48..72).contains(&grain.lightness), "{grain:?}");
            shades.insert((grain.hue, grain.saturation, grain.lightness));
        }
        // Notes made within the same moments are not one colour: of the
        // 18,432 shades there are, two thousand notes land on most of two
        // thousand.
        assert!(shades.len() > 1_500, "{} shades", shades.len());
    }

    #[test]
    fn a_layer_is_named_by_how_long_ago_the_edit_was() {
        // Wednesday 18 November 2026, a quarter past two, an hour ahead
        // of UTC.
        let now = at((2026, 11, 18), (14, 15), HOUR);
        let layer = |date, time| Layer::of(at(date, time, HOUR), now, HOUR);

        assert_eq!(layer((2026, 11, 18), (14, 14)), Layer::Today);
        assert_eq!(layer((2026, 11, 18), (0, 0)), Layer::Today);
        // A minute before midnight was yesterday.
        assert_eq!(layer((2026, 11, 17), (23, 59)), Layer::ThisWeek);
        // Six days before today, from its first minute.
        assert_eq!(layer((2026, 11, 12), (0, 0)), Layer::ThisWeek);
        assert_eq!(layer((2026, 11, 11), (23, 59)), Layer::EarlierThisMonth);
        assert_eq!(layer((2026, 11, 1), (0, 0)), Layer::EarlierThisMonth);
        let october = Layer::Month {
            month: YearMonth {
                year: 2026,
                month: 10,
            },
            this_year: true,
        };
        assert_eq!(layer((2026, 10, 31), (23, 59)), october);
        assert_eq!(layer((2026, 10, 1), (0, 0)), october);
        let last_december = layer((2025, 12, 31), (23, 59));
        assert_eq!(
            last_december,
            Layer::Month {
                month: YearMonth {
                    year: 2025,
                    month: 12
                },
                this_year: false
            }
        );
        // The same month of another year is another layer.
        assert_ne!(layer((2025, 11, 18), (14, 15)), Layer::EarlierThisMonth);

        assert_eq!(Layer::Today.name(), None);
        assert_eq!(Layer::ThisWeek.name().as_deref(), Some("This week"));
        assert_eq!(
            Layer::EarlierThisMonth.name().as_deref(),
            Some("Earlier this month")
        );
        assert_eq!(october.name().as_deref(), Some("October"));
        assert_eq!(last_december.name().as_deref(), Some("December 2025"));
        assert_eq!(
            layer((2026, 1, 3), (9, 0)).name().as_deref(),
            Some("January")
        );
    }

    /// "This week" is the six days before today, whatever day today is.
    #[test]
    fn this_week_does_not_wait_for_a_week_to_start() {
        // Monday 16 November 2026.
        let monday = at((2026, 11, 16), (8, 0), 0);
        let layer = |date| Layer::of(at(date, (12, 0), 0), monday, 0);
        assert_eq!(layer((2026, 11, 15)), Layer::ThisWeek, "Sunday");
        assert_eq!(layer((2026, 11, 10)), Layer::ThisWeek, "last Tuesday");
        assert_eq!(layer((2026, 11, 9)), Layer::EarlierThisMonth);

        // It reaches into last month before "earlier this month" can.
        let third = at((2026, 11, 3), (8, 0), 0);
        let layer = |date| Layer::of(at(date, (12, 0), 0), third, 0);
        assert_eq!(layer((2026, 11, 1)), Layer::ThisWeek);
        assert_eq!(layer((2026, 10, 28)), Layer::ThisWeek);
        assert_eq!(layer((2026, 10, 27)).name().as_deref(), Some("October"));
        // So early in a month nothing is "earlier this month".
        assert_ne!(layer((2026, 10, 27)), Layer::EarlierThisMonth);
    }

    #[test]
    fn layers_across_the_end_of_a_year() {
        // Sunday 3 January 2027.
        let now = at((2027, 1, 3), (10, 0), 0);
        let layer = |date| Layer::of(at(date, (12, 0), 0), now, 0);
        assert_eq!(layer((2027, 1, 1)), Layer::ThisWeek);
        assert_eq!(layer((2026, 12, 31)), Layer::ThisWeek);
        assert_eq!(layer((2026, 12, 28)), Layer::ThisWeek);
        // Last month is last year's, so it is named with its year.
        assert_eq!(
            layer((2026, 12, 27)).name().as_deref(),
            Some("December 2026")
        );
        assert_eq!(
            layer((2026, 11, 30)).name().as_deref(),
            Some("November 2026")
        );
        assert_eq!(layer((2026, 1, 3)).name().as_deref(), Some("January 2026"));

        // Later in January there is an "earlier this month", and it does
        // not take last January's notes.
        let later = at((2027, 1, 20), (10, 0), 0);
        let layer = |date| Layer::of(at(date, (12, 0), 0), later, 0);
        assert_eq!(layer((2027, 1, 2)), Layer::EarlierThisMonth);
        assert_eq!(layer((2026, 1, 2)).name().as_deref(), Some("January 2026"));
    }

    /// The date is the local one. The same two moments are the same day
    /// in one place and two days in another.
    #[test]
    fn a_layer_follows_the_local_date() {
        // Half past eleven at night in UTC, and a note from an hour
        // earlier.
        let now = at((2026, 11, 18), (23, 30), 0);
        let edited = at((2026, 11, 18), (22, 30), 0);
        assert_eq!(Layer::of(edited, now, 0), Layer::Today);
        // An hour ahead, it is half past midnight and the note is
        // yesterday's.
        assert_eq!(Layer::of(edited, now, HOUR), Layer::ThisWeek);
        // Eight hours behind, both are the afternoon of the same day.
        assert_eq!(Layer::of(edited, now, -8 * HOUR), Layer::Today);

        // The end of a month, seen from Taipei and from Los Angeles: the
        // note was edited at 20:00 UTC on 31 October.
        let edited = at((2026, 10, 31), (20, 0), 0);
        let now = at((2026, 11, 18), (12, 0), 0);
        assert_eq!(Layer::of(edited, now, 8 * HOUR), Layer::EarlierThisMonth);
        assert_eq!(
            Layer::of(edited, now, -8 * HOUR).name().as_deref(),
            Some("October")
        );
        // A quarter of an hour counts: Nepal is 5:45 ahead.
        let edited = at((2026, 10, 31), (18, 20), 0);
        assert_eq!(
            Layer::of(edited, now, 5 * HOUR + 45 * 60),
            Layer::EarlierThisMonth
        );
        assert_eq!(
            Layer::of(edited, now, 5 * HOUR + 30 * 60).name().as_deref(),
            Some("October")
        );
    }

    #[test]
    fn an_edit_time_from_a_clock_that_is_ahead_is_today() {
        let now = at((2026, 11, 18), (14, 15), 0);
        assert_eq!(
            Layer::of(at((2026, 11, 19), (9, 0), 0), now, 0),
            Layer::Today
        );
        assert_eq!(Layer::of(i64::MAX, now, 0), Layer::Today);
        // A time that is no date at all is not a reason to fail.
        let never = Layer::of(i64::MIN, now, 0);
        assert_eq!(never.name().as_deref(), Some("January 1970"));
    }

    #[test]
    fn the_pile_is_counted_from_its_oldest_thought() {
        assert_eq!(PileSummary::of(&[], 0), PileSummary::default());

        let note = |made: i64, archived| PileNote {
            id: made_at(made),
            edited_at: made + 5_000,
            archived,
        };
        let notes = [
            note(at((2026, 3, 9), (8, 0), 0), false),
            // The oldest, and not the first in the list.
            note(at((2025, 11, 30), (23, 30), 0), true),
            note(at((2026, 11, 17), (8, 0), 0), true),
        ];
        let summary = PileSummary::of(&notes, 0);
        assert_eq!(summary.thoughts, 3, "an archived note is a grain too");
        assert_eq!(summary.archived, 2);
        assert_eq!(
            summary.since,
            Some(YearMonth {
                year: 2025,
                month: 11
            })
        );
        assert_eq!(summary.since.unwrap().name(), "November");
        // An hour ahead of UTC, that note was made in December.
        assert_eq!(
            PileSummary::of(&notes, HOUR).since,
            Some(YearMonth {
                year: 2025,
                month: 12
            })
        );
        assert_eq!(notes[1].created_at(), at((2025, 11, 30), (23, 30), 0));
        assert_eq!(notes[1].grain(), Grain::of(&notes[1].id));

        // An ID from elsewhere carries no time: its note is as old as
        // its last edit.
        let foreign = PileNote {
            id: id("6ba7b810-9dad-41d1-80b4-00c04fd430c8"),
            edited_at: at((2024, 2, 29), (12, 0), 0),
            archived: false,
        };
        assert_eq!(foreign.created_at(), foreign.edited_at);
        assert_eq!(
            PileSummary::of(&[notes[0], foreign], 0).since,
            Some(YearMonth {
                year: 2024,
                month: 2
            })
        );
    }

    #[test]
    fn a_month_has_a_name() {
        assert_eq!(month_name(1), "January");
        assert_eq!(month_name(12), "December");
        assert_eq!(month_name(0), "");
        assert_eq!(month_name(13), "");
    }
}
