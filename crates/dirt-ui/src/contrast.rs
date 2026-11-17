//! How well one colour reads on another, by WCAG 2's measure.
//!
//! The tests here are the check the design promises in sections 4.1 and
//! 4.1.1: every pair it names, in both themes and every accent, read from
//! the tables the style sheet is made from.

use crate::tokens::Color;

/// The least contrast for text of ordinary size (WCAG AA).
pub const TEXT: f64 = 4.5;

/// The least contrast for a control or an icon that is not text.
pub const CONTROL: f64 = 3.0;

/// The relative luminance of a colour, 0 for black to 1 for white.
#[must_use]
pub fn luminance(color: Color) -> f64 {
    fn channel(value: u8) -> f64 {
        let value = f64::from(value) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    let (red, green, blue) = color.rgb();
    0.2126f64.mul_add(
        channel(red),
        0.7152f64.mul_add(channel(green), 0.0722 * channel(blue)),
    )
}

/// The contrast between two colours, from 1 (the same) to 21 (black on
/// white). The order of the two does not matter.
#[must_use]
pub fn ratio(one: Color, other: Color) -> f64 {
    let (one, other) = (luminance(one), luminance(other));
    let (lighter, darker) = if one > other {
        (one, other)
    } else {
        (other, one)
    };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use dirt_core::models::Accent;

    use super::*;
    use crate::tokens::{accent_colors, neutral, Scheme};

    /// The backgrounds text is drawn on: the list, a card or panel, and a
    /// row under the pointer.
    const GROUNDS: [&str; 3] = ["bg", "surface", "surface-2"];

    fn check(what: &str, scheme: Scheme, text: Color, ground: Color, least: f64) -> f64 {
        let ratio = ratio(text, ground);
        assert!(
            ratio >= least,
            "{what} in the {scheme:?} theme is {ratio:.2}:1, under {least}:1 ({} on {})",
            text.hex(),
            ground.hex()
        );
        ratio
    }

    /// The lowest contrast of the neutral `text` on the three grounds.
    fn lowest_on_grounds(text: &str, scheme: Scheme, least: f64) -> f64 {
        GROUNDS
            .iter()
            .map(|ground| {
                check(
                    &format!("--{text} on --{ground}"),
                    scheme,
                    neutral(text, scheme),
                    neutral(ground, scheme),
                    least,
                )
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// A ratio as the document writes it: one decimal place.
    fn written(ratio: f64) -> String {
        format!("{ratio:.1}")
    }

    #[test]
    fn the_measure_agrees_with_known_pairs() {
        let black = Color(0);
        assert!((ratio(black, Color::WHITE) - 21.0).abs() < 1e-9);
        assert!((ratio(black, black) - 1.0).abs() < 1e-9);
        // #767676 is the lightest grey that passes on white.
        let grey = ratio(Color(0x0076_7676), Color::WHITE);
        assert!((4.5..4.6).contains(&grey), "{grey}");
    }

    #[test]
    fn body_text_on_the_background_is_over_fifteen_to_one() {
        for scheme in Scheme::BOTH {
            check(
                "--text on --bg",
                scheme,
                neutral("text", scheme),
                neutral("bg", scheme),
                15.0,
            );
        }
    }

    #[test]
    fn every_text_colour_passes_on_every_ground_it_is_drawn_on() {
        for scheme in Scheme::BOTH {
            for text in ["text", "text-2", "text-3", "warning", "danger"] {
                lowest_on_grounds(text, scheme, TEXT);
            }
            check(
                "--on-inverse on --inverse",
                scheme,
                neutral("on-inverse", scheme),
                neutral("inverse", scheme),
                TEXT,
            );
        }
    }

    /// The table under section 4.1: the lowest pair of each text colour, as
    /// the document writes it. If a colour changes, so must that table.
    #[test]
    fn the_lowest_pairs_are_the_ones_the_document_gives() {
        let lowest = |text: &str, scheme| written(lowest_on_grounds(text, scheme, TEXT));
        assert_eq!(lowest("text-3", Scheme::Light), "4.6");
        assert_eq!(lowest("text-3", Scheme::Dark), "4.6");
        assert_eq!(lowest("warning", Scheme::Light), "4.7");
        assert_eq!(lowest("warning", Scheme::Dark), "6.9");

        let clay = |scheme| accent_colors(Accent::Clay, scheme);
        let on_soft = |scheme| written(ratio(clay(scheme).accent, clay(scheme).soft));
        assert_eq!(on_soft(Scheme::Light), "4.6");
        assert_eq!(on_soft(Scheme::Dark), "4.8");
        let on_accent = |scheme| written(ratio(clay(scheme).on_accent, clay(scheme).accent));
        assert_eq!(on_accent(Scheme::Light), "5.6");
        assert_eq!(on_accent(Scheme::Dark), "6.3");
    }

    /// Section 4.1.1's twelve checks for each accent: the accent on the
    /// three grounds and on its own soft colour, the text on it, and Undo
    /// on a toast, in both themes.
    #[test]
    fn every_accent_passes_its_twelve_checks() {
        for accent in Accent::ALL {
            for scheme in Scheme::BOTH {
                let colors = accent_colors(accent, scheme);
                let name = accent.label();
                for ground in GROUNDS {
                    check(
                        &format!("{name} on --{ground}"),
                        scheme,
                        colors.accent,
                        neutral(ground, scheme),
                        TEXT,
                    );
                }
                check(
                    &format!("{name} on its --accent-soft"),
                    scheme,
                    colors.accent,
                    colors.soft,
                    TEXT,
                );
                check(
                    &format!("--on-accent on {name}"),
                    scheme,
                    colors.on_accent,
                    colors.accent,
                    TEXT,
                );
                check(
                    &format!("{name}'s --accent-inverse on a toast"),
                    scheme,
                    colors.inverse,
                    neutral("inverse", scheme),
                    TEXT,
                );
                // Not among the twelve, but drawn: the primary button
                // under the pointer, and body text beside a tag token.
                check(
                    &format!("--on-accent on {name}'s --accent-hover"),
                    scheme,
                    colors.on_accent,
                    colors.hover,
                    TEXT,
                );
                check(
                    &format!("--text on {name}'s --accent-soft"),
                    scheme,
                    neutral("text", scheme),
                    colors.soft,
                    TEXT,
                );
            }
        }
    }

    #[test]
    fn an_unchecked_control_is_outlined_at_three_to_one() {
        for scheme in Scheme::BOTH {
            lowest_on_grounds("border-control", scheme, CONTROL);
        }
        assert_eq!(
            written(ratio(
                neutral("border-control", Scheme::Light),
                neutral("surface-2", Scheme::Light)
            )),
            "3.1"
        );
    }

    /// Pairs the first version of section 4.1 did not name and the
    /// interface draws: a confirmation's row and its red button, and a
    /// search match.
    #[test]
    fn confirmations_and_search_matches_pass() {
        for scheme in Scheme::BOTH {
            check(
                "--on-danger on --danger",
                scheme,
                neutral("on-danger", scheme),
                neutral("danger", scheme),
                TEXT,
            );
            check(
                "--text on --danger-soft",
                scheme,
                neutral("text", scheme),
                neutral("danger-soft", scheme),
                TEXT,
            );
            // The bin under the pointer is an icon, not text.
            check(
                "--danger on --danger-soft",
                scheme,
                neutral("danger", scheme),
                neutral("danger-soft", scheme),
                CONTROL,
            );
            check(
                "--text on --mark",
                scheme,
                neutral("text", scheme),
                neutral("mark", scheme),
                TEXT,
            );
        }
    }
}
