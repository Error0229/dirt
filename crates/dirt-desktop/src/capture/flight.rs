//! Where quick capture's window opens, where it goes as it is put away,
//! and how it gets there (interface design, sections 5.5 and 7.2).
//!
//! Put away, the box shrinks to its grain in the page while the window
//! carries the grain to the tray icon: across quickly, down slowly then
//! fast, so that the path curves (600 ms, the mockup's two curves). Where
//! the tray icon's place cannot be known, or it is on a screen of another
//! scale, which would redraw the page in mid-air, the grain falls and
//! fades where the window was (the plan's fallback).
//!
//! It is the window that moves: the page is only as large as the box, and
//! the screen outside it is not the app's to draw on. Rust moves it a
//! little at each frame, from when the page says the box has begun to
//! shrink, so with reduced motion, when the page only fades, nothing
//! moves.

use std::time::{Duration, Instant};

use dioxus::desktop::tao::dpi::{PhysicalPosition, PhysicalSize};
use dioxus::desktop::DesktopContext;
use dirt_ui::motion::Bezier;

/// How long the grain flies or falls: the page's shrinking takes the same.
const FLIGHT: Duration = Duration::from_millis(600);
/// Across, to the tray: quick off the mark.
const ACROSS: Bezier = Bezier::new(0.25, 0.65, 0.45, 1.0);
/// Down, to the tray: slowly, then fast.
const DOWN: Bezier = Bezier::new(0.6, 0.0, 0.9, 0.45);
/// A fall, where the tray's place is not known (gravity, section 5.4).
const GRAVITY: Bezier = Bezier::new(0.55, 0.0, 0.9, 0.45);
/// How far the grain falls, in CSS pixels: as far as an archived note's
/// grain sinks.
const FALLS: f64 = 140.0;
/// How often the window is moved while it flies.
const FRAME: Duration = Duration::from_millis(8);

/// How the box leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// Its grain flies to the tray icon.
    Tray,
    /// Its grain falls and fades where the window was.
    Fall,
    /// Nothing was written: it fades.
    Fade,
}

impl How {
    /// The name the script knows it by (`data-dui-capture`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tray => "tray",
            Self::Fall => "fall",
            Self::Fade => "fade",
        }
    }
}

/// Puts the window, `width` by `height` CSS pixels, in the middle of the
/// screen the pointer is on, or of the first screen. In a development
/// build with `DIRT_WINDOW_AT`, which keeps the main window off every
/// screen, it goes where the main window is instead.
pub fn place(desktop: &DesktopContext, width: f64, height: f64) {
    let (middle, scale) = if let Some((x, y)) = crate::app::asked_position() {
        let scale = desktop.scale_factor();
        (
            PhysicalPosition::new((x + 550.0) * scale, (y + 360.0) * scale),
            scale,
        )
    } else {
        let screen = desktop
            .cursor_position()
            .ok()
            .and_then(|at| desktop.monitor_from_point(at.x, at.y))
            .or_else(|| desktop.current_monitor())
            .or_else(|| desktop.primary_monitor());
        let Some(screen) = screen else {
            return;
        };
        let (at, size) = (screen.position(), screen.size());
        (
            PhysicalPosition::new(
                f64::from(at.x) + f64::from(size.width) / 2.0,
                f64::from(at.y) + f64::from(size.height) / 2.0,
            ),
            screen.scale_factor(),
        )
    };
    desktop.set_outer_position(PhysicalPosition::new(
        width.mul_add(-scale / 2.0, middle.x),
        height.mul_add(-scale / 2.0, middle.y),
    ));
    desktop.set_inner_size(PhysicalSize::new(width * scale, height * scale));
}

/// Where the window goes as it is put away, and from where.
#[derive(Debug, Clone, Copy)]
pub struct Flight {
    pub how: How,
    from: PhysicalPosition<f64>,
    to: PhysicalPosition<f64>,
}

impl Flight {
    /// The flight from where the window is now: to the tray icon, so that
    /// the middle of the window, where the grain is, lands on it; or a
    /// fall.
    pub fn from(desktop: &DesktopContext) -> Self {
        let from = desktop
            .outer_position()
            .map(|at| at.cast::<f64>())
            .unwrap_or_default();
        let size = desktop.outer_size().cast::<f64>();
        if let Some(tray) = tray_middle(desktop) {
            return Self {
                how: How::Tray,
                from,
                to: PhysicalPosition::new(tray.x - size.width / 2.0, tray.y - size.height / 2.0),
            };
        }
        Self {
            how: How::Fall,
            from,
            to: PhysicalPosition::new(from.x, FALLS.mul_add(desktop.scale_factor(), from.y)),
        }
    }

    /// Moves the window along the flight, for as long as `still` says the
    /// flight is wanted: opening again stops it where it is.
    pub async fn carry(self, desktop: &DesktopContext, still: impl Fn() -> bool) {
        let start = Instant::now();
        loop {
            if !still() {
                return;
            }
            let progress = (start.elapsed().as_secs_f64() / FLIGHT.as_secs_f64()).min(1.0);
            let (across, down) = match self.how {
                How::Tray => (ACROSS.at(progress), DOWN.at(progress)),
                How::Fall | How::Fade => (0.0, GRAVITY.at(progress)),
            };
            desktop.set_outer_position(PhysicalPosition::new(
                (self.to.x - self.from.x).mul_add(across, self.from.x),
                (self.to.y - self.from.y).mul_add(down, self.from.y),
            ));
            if progress >= 1.0 {
                return;
            }
            tokio::time::sleep(FRAME).await;
        }
    }
}

/// The middle of the tray icon, on the screen, if it can be known and is
/// on a screen of the window's own scale. In a development build,
/// `DIRT_TRAY_AT=x,y` puts it there instead, in screen points: off every
/// screen, the flight can be tried without crossing the person's screen.
fn tray_middle(desktop: &DesktopContext) -> Option<PhysicalPosition<f64>> {
    let scale = desktop.scale_factor();
    if let Some((x, y)) = crate::settings::test_hook("DIRT_TRAY_AT").and_then(|at| {
        let at = at.to_string_lossy().into_owned();
        let (x, y) = at.split_once(',')?;
        Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
    }) {
        return Some(PhysicalPosition::new(x * scale, y * scale));
    }
    let (at, size) = crate::tray::place()?;
    if size.width == 0 || size.height == 0 {
        return None;
    }
    let middle = PhysicalPosition::new(
        at.x + f64::from(size.width) / 2.0,
        at.y + f64::from(size.height) / 2.0,
    );
    let screen = desktop.monitor_from_point(middle.x, middle.y)?;
    #[allow(clippy::float_cmp)]
    let same_scale = screen.scale_factor() == scale;
    same_scale.then_some(middle)
}
