//! Feeding an engine recorded speech the way a microphone would, and
//! writing down everything it shows and when.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::audio::RATE;

/// What the engine would have on screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Shown {
    /// Words the engine will not change again: the text colour.
    pub settled: String,
    /// Words it may still change: grey.
    pub tentative: String,
}

/// What one delivery of sound produced.
#[derive(Default)]
pub struct Step {
    /// The words now shown, if the engine worked on this delivery.
    pub shown: Option<Shown>,
    /// The engine's own rule says the thought has ended.
    pub engine_endpoint: bool,
    /// The voice-activity detector hears speech, if there is one.
    pub vad_speech: Option<bool>,
}

/// An engine that takes sound as it arrives.
pub trait Live {
    /// Takes the sound that arrived since the last call.
    fn feed(&mut self, samples: &[f32]) -> Step;
    /// Forgets the thought, ready for the next.
    fn reset(&mut self);
    /// A second, slower reading of the whole thought, if the engine has one.
    fn second_pass(&mut self, _samples: &[f32]) -> Option<String> {
        None
    }
}

#[derive(Debug, Serialize)]
pub struct ShownAt {
    /// Seconds since the first sample was spoken, on the wall clock.
    pub t: f64,
    /// Seconds of sound the engine had been given.
    pub heard: f64,
    pub settled: String,
    pub tentative: String,
}

#[derive(Debug, Serialize)]
pub struct Mark {
    pub t: f64,
    pub heard: f64,
    pub what: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct SecondPass {
    pub text: String,
    pub seconds: f64,
}

/// Everything one thought produced.
#[derive(Debug, Serialize)]
pub struct Heard {
    pub id: String,
    /// Length of the speech itself, without the quiet added after it.
    pub speech_seconds: f64,
    /// Length of everything fed, the quiet included.
    pub fed_seconds: f64,
    /// Time the engine spent working, summed.
    pub compute_seconds: f64,
    /// The longest the engine fell behind the microphone.
    pub worst_lag_seconds: f64,
    pub shown: Vec<ShownAt>,
    /// Endpoints and voice-activity changes, in order.
    pub marks: Vec<Mark>,
    pub last: Shown,
    pub second_pass: Option<SecondPass>,
}

/// A clock that either is the wall clock, or only pretends to wait and
/// counts the engine's working time as time passed. The pretend clock gives
/// the same delays on an idle machine and runs as fast as the engine can.
pub struct Clock {
    real: bool,
    start: Instant,
    pretend: f64,
}

impl Clock {
    pub fn new(real: bool) -> Self {
        Self {
            real,
            start: Instant::now(),
            pretend: 0.0,
        }
    }

    pub fn now(&self) -> f64 {
        if self.real {
            self.start.elapsed().as_secs_f64()
        } else {
            self.pretend
        }
    }

    fn wait_until(&mut self, t: f64) {
        if self.real {
            let now = self.start.elapsed().as_secs_f64();
            if t > now {
                std::thread::sleep(Duration::from_secs_f64(t - now));
            }
        } else if t > self.pretend {
            self.pretend = t;
        }
    }

    fn spent(&mut self, seconds: f64) {
        if !self.real {
            self.pretend += seconds;
        }
    }
}

/// Gives the engine `lead` at once (quiet an app could put in front of the
/// microphone's sound itself, for a model that loses words spoken in the
/// stream's first moment), then feeds `speech` and `tail` (the quiet after
/// it) in pieces of `chunk_ms`. A piece is handed over at the moment its last sample would
/// have been recorded. If the engine was busy when pieces fell due, it gets
/// them together when it comes back, as a microphone's buffer would give them.
pub fn hear(
    engine: &mut dyn Live,
    id: &str,
    lead: &[f32],
    speech: &[f32],
    tail: &[f32],
    chunk_ms: usize,
    real_time: bool,
) -> Heard {
    let all: Vec<f32> = speech.iter().chain(tail).copied().collect();
    let chunk = RATE * chunk_ms / 1000;
    let mut clock = Clock::new(real_time);
    let mut heard = Heard {
        id: id.to_owned(),
        speech_seconds: speech.len() as f64 / RATE as f64,
        fed_seconds: all.len() as f64 / RATE as f64,
        compute_seconds: 0.0,
        worst_lag_seconds: 0.0,
        shown: Vec::new(),
        marks: Vec::new(),
        last: Shown::default(),
        second_pass: None,
    };
    let mut engine_endpoint = false;
    let mut vad_speech = false;
    if !lead.is_empty() {
        let began = Instant::now();
        engine.feed(lead);
        let took = began.elapsed().as_secs_f64();
        clock.spent(took);
        heard.compute_seconds += took;
    }
    let mut fed = 0;
    while fed < all.len() {
        // The next piece falls due when its last sample has been spoken.
        let next_end = (fed + chunk).min(all.len());
        clock.wait_until(next_end as f64 / RATE as f64);
        // Everything that has fallen due by now goes in together.
        let due = ((clock.now() * RATE as f64) as usize / chunk * chunk).clamp(next_end, all.len());
        let lag = clock.now() - next_end as f64 / RATE as f64;
        heard.worst_lag_seconds = heard.worst_lag_seconds.max(lag);

        let began = Instant::now();
        let step = engine.feed(&all[fed..due]);
        let took = began.elapsed().as_secs_f64();
        clock.spent(took);
        heard.compute_seconds += took;
        fed = due;

        let t = clock.now();
        let at = fed as f64 / RATE as f64;
        if let Some(shown) = step.shown {
            if shown != heard.last {
                heard.shown.push(ShownAt {
                    t,
                    heard: at,
                    settled: shown.settled.clone(),
                    tentative: shown.tentative.clone(),
                });
                heard.last = shown;
            }
        }
        if step.engine_endpoint != engine_endpoint {
            engine_endpoint = step.engine_endpoint;
            if engine_endpoint {
                heard.marks.push(Mark {
                    t,
                    heard: at,
                    what: "engine-endpoint",
                });
            }
        }
        if let Some(speech_now) = step.vad_speech {
            if speech_now != vad_speech {
                vad_speech = speech_now;
                let what = if speech_now {
                    "vad-speech"
                } else {
                    "vad-quiet"
                };
                heard.marks.push(Mark { t, heard: at, what });
            }
        }
    }
    heard
}
