//! Words as they are heard from a model that reads finished sound: the
//! sound so far is read again every time enough new sound has arrived, and
//! a word is settled once two readings in a row agree on it and on
//! everything before it. The same idea as `whisper_engine`, without its
//! trimming, for a model quick enough to read a whole thought many times
//! a second (Moonshine).

use crate::audio::RATE;
use crate::live::{Live, Shown, Step};
use crate::words::common_prefix;

pub struct Sliding<F: FnMut(&[f32]) -> Option<String>> {
    read: F,
    step: usize,
    buffer: Vec<f32>,
    read_at: usize,
    settled: Vec<String>,
    previous: Vec<String>,
}

impl<F: FnMut(&[f32]) -> Option<String>> Sliding<F> {
    pub fn new(read: F, step_ms: usize) -> Self {
        Self {
            read,
            step: RATE * step_ms / 1000,
            buffer: Vec::new(),
            read_at: 0,
            settled: Vec::new(),
            previous: Vec::new(),
        }
    }
}

impl<F: FnMut(&[f32]) -> Option<String>> Live for Sliding<F> {
    fn feed(&mut self, samples: &[f32]) -> Step {
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() < self.read_at + self.step {
            return Step::default();
        }
        self.read_at = self.buffer.len();
        let Some(text) = (self.read)(&self.buffer) else {
            return Step::default();
        };
        let words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
        let agreed = common_prefix(&self.previous, &words);
        if agreed > self.settled.len() {
            let new = words[self.settled.len()..agreed].to_vec();
            self.settled.extend(new);
        }
        let tentative: Vec<&str> = words
            .iter()
            .skip(self.settled.len())
            .map(String::as_str)
            .collect();
        let shown = Shown {
            settled: self.settled.join(" "),
            tentative: tentative.join(" "),
        };
        self.previous = words;
        Step {
            shown: Some(shown),
            ..Step::default()
        }
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.read_at = 0;
        self.settled.clear();
        self.previous.clear();
    }

    fn second_pass(&mut self, samples: &[f32]) -> Option<String> {
        (self.read)(samples)
    }
}
