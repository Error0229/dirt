//! The motion tokens (interface design, section 5): three springs and
//! three curves, as custom properties, and the one script that moves what
//! a style sheet cannot.
//!
//! The values are the mockup's, which is what was approved by eye. A spring
//! is written as a CSS `linear()` easing of 49 points. A web view that does
//! not know `linear()` gets a `cubic-bezier` near it instead; the style
//! sheet asks the web view which it knows (`@supports`), and so does the
//! script, so no engine is named anywhere.

use std::fmt::Write as _;

/// A spring: how a position, a size or a scale moves (section 5.3).
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    /// The token's name: `--sp-<name>` is the easing, `--d-<name>` the time.
    pub name: &'static str,
    /// The period of the spring without damping, in seconds.
    pub response: f64,
    /// 1 does not bounce; lower bounces more.
    pub damping: f64,
    /// How long it takes to settle, in milliseconds.
    pub settles_ms: u32,
    /// Where it is at each forty-eighth of that time, from 0 to 1.
    pub points: [f64; 49],
    /// The curve for a web view without `linear()`.
    pub fallback: &'static str,
}

/// Small controls moving: segment thumbs, switches, selection, popovers.
pub const SNAP: Spring = Spring {
    name: "snap",
    response: 0.3,
    damping: 0.78,
    settles_ms: 380,
    points: [
        0.0, 0.0126, 0.0462, 0.0952, 0.1549, 0.2214, 0.2916, 0.3629, 0.4333, 0.5014, 0.566, 0.6264,
        0.6821, 0.7327, 0.7783, 0.8188, 0.8545, 0.8856, 0.9123, 0.9351, 0.9542, 0.9701, 0.9831,
        0.9936, 1.0018, 1.0081, 1.0127, 1.016, 1.0182, 1.0194, 1.0199, 1.0198, 1.0192, 1.0183,
        1.0172, 1.0159, 1.0145, 1.0131, 1.0117, 1.0103, 1.009, 1.0078, 1.0066, 1.0056, 1.0047,
        1.0038, 1.0031, 1.0024, 1.0,
    ],
    fallback: "cubic-bezier(.2,.9,.3,1.02)",
};

/// Confirmations and things let go of: a tick, a card released, undo.
pub const POP: Spring = Spring {
    name: "pop",
    response: 0.38,
    damping: 0.58,
    settles_ms: 600,
    points: [
        0.0, 0.0197, 0.0723, 0.1488, 0.2412, 0.3426, 0.4473, 0.5506, 0.6488, 0.7395, 0.8206,
        0.8913, 0.951, 0.9998, 1.0382, 1.067, 1.0871, 1.0997, 1.1058, 1.1065, 1.1031, 1.0965,
        1.0877, 1.0774, 1.0664, 1.0552, 1.0443, 1.0341, 1.0248, 1.0165, 1.0093, 1.0033, 0.9985,
        0.9948, 0.992, 0.9902, 0.9891, 0.9886, 0.9887, 0.9892, 0.99, 0.991, 0.9921, 0.9933, 0.9945,
        0.9956, 0.9967, 0.9977, 1.0,
    ],
    fallback: "cubic-bezier(.34,1.4,.64,1)",
};

/// Large surfaces: sheets, panels, the list folding, the note's column.
pub const GLIDE: Spring = Spring {
    name: "glide",
    response: 0.55,
    damping: 1.0,
    settles_ms: 750,
    points: [
        0.0, 0.0142, 0.0504, 0.1012, 0.1607, 0.2248, 0.2903, 0.3552, 0.4178, 0.4772, 0.5327, 0.584,
        0.6311, 0.6738, 0.7125, 0.7472, 0.7783, 0.8059, 0.8305, 0.8522, 0.8713, 0.8882, 0.9029,
        0.9159, 0.9271, 0.937, 0.9456, 0.953, 0.9595, 0.9651, 0.97, 0.9742, 0.9778, 0.9809, 0.9836,
        0.986, 0.988, 0.9897, 0.9912, 0.9925, 0.9935, 0.9945, 0.9953, 0.996, 0.9966, 0.9971,
        0.9975, 0.9979, 1.0,
    ],
    fallback: "cubic-bezier(.2,0,0,1)",
};

/// The three springs.
pub const SPRINGS: [Spring; 3] = [SNAP, POP, GLIDE];

impl Spring {
    /// The spring as a CSS `linear()` easing.
    #[must_use]
    pub fn linear(&self) -> String {
        let mut css = String::from("linear(");
        for (index, point) in self.points.iter().enumerate() {
            if index > 0 {
                css.push_str(", ");
            }
            let _ = write!(css, "{point}");
        }
        css.push(')');
        css
    }

    /// Where a spring of this response and damping is `seconds` after it is
    /// let go, by the equation of a damped spring. The points above were
    /// made by this; the test below holds them to it.
    #[must_use]
    pub fn at(&self, seconds: f64) -> f64 {
        let natural = std::f64::consts::TAU / self.response;
        let decay = (-self.damping * natural * seconds).exp();
        if self.damping >= 1.0 {
            // Critically damped: it arrives without swinging past.
            return decay.mul_add(-natural.mul_add(seconds, 1.0), 1.0);
        }
        let damped = natural * self.damping.mul_add(-self.damping, 1.0).sqrt();
        let turn = damped * seconds;
        let swing = (self.damping * natural / damped).mul_add(turn.sin(), turn.cos());
        decay.mul_add(-swing, 1.0)
    }

    /// How far past its end the spring swings, as a share of the way.
    #[must_use]
    pub fn overshoot(&self) -> f64 {
        self.points.iter().copied().fold(1.0, f64::max) - 1.0
    }
}

/// A curve for what does not move: opacity, colour, blur (section 5.4).
#[derive(Debug, Clone, Copy)]
pub struct Curve {
    /// The custom property's name, without the dashes.
    pub name: &'static str,
    /// The curve.
    pub value: &'static str,
}

/// Standard: fades and colour changes in place.
pub const STANDARD: Curve = Curve {
    name: "e-std",
    value: "cubic-bezier(.2,0,0,1)",
};

/// Enter: an ease-out, for fades that travel a little.
pub const ENTER: Curve = Curve {
    name: "e-out",
    value: "cubic-bezier(.05,.7,.1,1)",
};

/// Exit: an ease-in, for everything that leaves.
pub const EXIT: Curve = Curve {
    name: "e-in",
    value: "cubic-bezier(.3,0,.8,.15)",
};

/// The three curves.
pub const CURVES: [Curve; 3] = [STANDARD, ENTER, EXIT];

/// A cubic Bézier easing, as CSS's `cubic-bezier()` draws it.
///
/// It is for the one movement Rust plays itself: the quick-capture window
/// carried to the tray (section 5.5), which is a window moving over the
/// screen and not something in the page. Everything in the page uses the
/// CSS value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl Bezier {
    /// The curve `cubic-bezier(x1, y1, x2, y2)`. `x1` and `x2` are between
    /// 0 and 1, as CSS requires.
    #[must_use]
    pub const fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// How far along the movement is when `progress` of its time has
    /// passed: both 0 at the start and 1 at the end.
    #[must_use]
    pub fn at(&self, progress: f64) -> f64 {
        let x = progress.clamp(0.0, 1.0);
        // The curve's time is found from x by halving: x rises with it,
        // since x1 and x2 are between 0 and 1.
        let (mut low, mut high) = (0.0_f64, 1.0_f64);
        let mut t = x;
        for _ in 0..40 {
            let now = Self::along(t, self.x1, self.x2);
            if (now - x).abs() < 1e-7 {
                break;
            }
            if now < x {
                low = t;
            } else {
                high = t;
            }
            t = f64::midpoint(low, high);
        }
        Self::along(t, self.y1, self.y2)
    }

    /// One coordinate of the curve at time `t`, its ends at 0 and 1 and its
    /// two handles at `a` and `b`.
    fn along(t: f64, a: f64, b: f64) -> f64 {
        let u = 1.0 - t;
        t * (3.0 * u * u).mul_add(a, t * (3.0 * u).mul_add(b, t))
    }
}

/// The four lengths of time a curve runs for, in milliseconds: `--d-1` for
/// hover and press, `--d-4` for the longest fade.
pub const DURATIONS_MS: [u32; 4] = [120, 200, 280, 400];

/// With reduced motion every spring and every duration but the first
/// becomes this many milliseconds of `ease` (section 5.6).
pub const REDUCED_MS: u32 = 90;

/// The motion tokens as a style sheet.
#[must_use]
pub fn css() -> String {
    let mut css = String::with_capacity(4 * 1024);

    // Without `linear()`: the curves near each spring.
    css.push_str(":root{");
    for (index, duration) in DURATIONS_MS.iter().enumerate() {
        let _ = write!(css, "--d-{}:{duration}ms;", index + 1);
    }
    for curve in CURVES {
        let _ = write!(css, "--{}:{};", curve.name, curve.value);
    }
    for spring in SPRINGS {
        let _ = write!(
            css,
            "--sp-{0}:{1};--d-{0}:{2}ms;",
            spring.name, spring.fallback, spring.settles_ms
        );
    }
    css.push_str("--e-spring:var(--sp-pop)}");

    // With it: the springs themselves.
    css.push_str("@supports (transition-timing-function:linear(0,1)){:root{");
    for spring in SPRINGS {
        let _ = write!(css, "--sp-{}:{};", spring.name, spring.linear());
    }
    css.push_str("}}");

    // Reduced motion, asked for by the system or by `data-motion`. The
    // rule comes last, so it holds whether or not `linear()` is known.
    let mut reduced =
        format!("--d-1:1ms;--d-2:{REDUCED_MS}ms;--d-3:{REDUCED_MS}ms;--d-4:{REDUCED_MS}ms;");
    for spring in SPRINGS {
        let _ = write!(
            reduced,
            "--sp-{0}:ease;--d-{0}:{REDUCED_MS}ms;",
            spring.name
        );
    }
    let _ = write!(css, ":root[data-motion=\"reduced\"]{{{reduced}}}");
    let _ = write!(
        css,
        "@media (prefers-reduced-motion:reduce){{:root{{{reduced}}}}}"
    );
    css
}

/// The one script both apps run: the list's animation, reduced motion, and
/// what position-dependent motion and gestures need. The root component
/// runs it once; it does nothing the second time.
pub const SCRIPT: &str = include_str!("js/dirt-ui.js");

/// The springs and curves as the script wants them, so that it does not
/// keep a copy of its own.
#[must_use]
pub fn script_tokens() -> serde_json::Value {
    let springs: serde_json::Map<String, serde_json::Value> = SPRINGS
        .iter()
        .map(|spring| {
            (
                spring.name.to_owned(),
                serde_json::json!({
                    "linear": spring.linear(),
                    "fallback": spring.fallback,
                    "ms": spring.settles_ms,
                }),
            )
        })
        .collect();
    serde_json::json!({
        "springs": springs,
        "standard": STANDARD.value,
        "enter": ENTER.value,
        "exit": EXIT.value,
        "reducedMs": REDUCED_MS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The points are the mockup's. They were made by simulating a spring
    /// of the response and damping the design gives, so they must lie on
    /// that spring's curve; only the last is pulled to exactly 1.
    #[test]
    fn the_points_lie_on_the_spring_they_are_named_for() {
        for spring in SPRINGS {
            let step = f64::from(spring.settles_ms) / 1000.0 / 48.0;
            for (index, point) in spring.points.iter().enumerate().take(48) {
                let index = u32::try_from(index).unwrap();
                let expected = spring.at(step * f64::from(index));
                assert!(
                    (point - expected).abs() < 0.001,
                    "{} at step {index}: {point} is not {expected:.4}",
                    spring.name
                );
            }
            assert!((spring.points[48] - 1.0).abs() < f64::EPSILON);
        }
    }

    /// The overshoots section 5.3 gives: 2.0%, 10.7% and none.
    #[test]
    fn each_spring_overshoots_as_the_design_says() {
        assert!((SNAP.overshoot() - 0.020).abs() < 0.0006);
        assert!((POP.overshoot() - 0.107).abs() < 0.0006);
        assert!(GLIDE.overshoot().abs() < f64::EPSILON);
    }

    /// "Reaches 90%" in section 5.3 is the first of the 48 steps at or
    /// past nine tenths of the way.
    #[test]
    fn each_spring_reaches_ninety_percent_when_the_design_says() {
        let reaches = |spring: &Spring| {
            let step = spring
                .points
                .iter()
                .position(|point| *point >= 0.9)
                .unwrap();
            (f64::from(spring.settles_ms) * f64::from(u32::try_from(step).unwrap()) / 48.0).round()
        };
        assert!((reaches(&SNAP) - 143.0).abs() < 1.5);
        assert!((reaches(&POP) - 150.0).abs() < 1.5);
        assert!((reaches(&GLIDE) - 344.0).abs() < 1.5);
    }

    #[test]
    fn the_sheet_falls_back_by_asking_the_web_view() {
        let css = css();
        assert!(css.contains("--sp-snap:cubic-bezier(.2,.9,.3,1.02);--d-snap:380ms;"));
        assert!(css.contains(
            "@supports (transition-timing-function:linear(0,1)){:root{--sp-snap:linear(0, 0.0126,"
        ));
        assert!(css.contains("--d-pop:600ms;"));
        assert!(css.contains("--d-glide:750ms;"));
        assert!(css.contains(":root[data-motion=\"reduced\"]{--d-1:1ms;--d-2:90ms;"));
        assert!(css.contains("@media (prefers-reduced-motion:reduce){:root{--d-1:1ms;"));
        assert_eq!(css.matches('{').count(), css.matches('}').count());
        // Nothing in the sheet or the script may ask which engine it is.
        for name in ["WebKit", "Chrome", "userAgent", "Gecko"] {
            assert!(!css.contains(name), "the style sheet names {name}");
            assert!(!SCRIPT.contains(name), "the script names {name}");
        }
    }

    #[test]
    fn a_spring_is_written_as_the_mockup_writes_it() {
        assert!(SNAP.linear().starts_with("linear(0, 0.0126, 0.0462,"));
        assert!(SNAP.linear().ends_with("1.0031, 1.0024, 1)"));
    }

    #[test]
    fn a_bezier_runs_from_start_to_end_as_css_draws_it() {
        let linear = Bezier::new(0.0, 0.0, 1.0, 1.0);
        for step in 0..=10 {
            let progress = f64::from(step) / 10.0;
            assert!((linear.at(progress) - progress).abs() < 1e-6);
        }
        // CSS's `ease-in-out`, at half its time, is half way.
        let ease = Bezier::new(0.42, 0.0, 0.58, 1.0);
        assert!((ease.at(0.5) - 0.5).abs() < 1e-6);
        // Gravity is slow at the top and fastest as it lands.
        let gravity = Bezier::new(0.55, 0.0, 0.9, 0.45);
        assert!(gravity.at(0.0).abs() < 1e-9 && (gravity.at(1.0) - 1.0).abs() < 1e-9);
        assert!(gravity.at(0.5) < 0.25);
        let mut before = 0.0;
        for step in 1..=20 {
            let now = gravity.at(f64::from(step) / 20.0);
            assert!(now >= before);
            before = now;
        }
    }
}
