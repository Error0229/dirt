//! whisper.cpp made to give words as they are heard.
//!
//! Whisper reads a finished recording. To show words while someone is still
//! speaking, the sound so far is read again every time enough new sound has
//! arrived, and a word counts as settled once two readings in a row agree
//! on it and on everything before it ("local agreement", Macháček et al.,
//! "Turning Whisper into Real-Time Transcription System", 2023). The words
//! after the agreed ones are shown as tentative.

use std::path::Path;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::audio::RATE;
use crate::live::{Live, Shown, Step};
use crate::words::common_prefix;

pub struct Options {
    pub threads: i32,
    /// Read again once this much new sound has arrived.
    pub step_ms: usize,
    /// Give the encoder only as much room as the sound needs, instead of
    /// its full thirty seconds. Much faster on short sound; whisper.cpp
    /// warns that it can cost accuracy.
    pub short_window: bool,
    /// Drop settled sound from the front once the buffer is this long.
    pub trim_seconds: f32,
}

pub struct SlidingWhisper {
    state: WhisperState,
    options: Options,
    /// Sound not yet dropped.
    buffer: Vec<f32>,
    /// Length of the buffer at the last reading.
    read_at: usize,
    /// Words whose sound has been dropped from the buffer.
    fixed: Vec<String>,
    /// Settled words whose sound is still in the buffer.
    settled: Vec<String>,
    /// The reading before this one, over the same buffer.
    previous: Vec<String>,
    pub readings: usize,
}

/// One reading: its words, and for each segment how many words it ends at
/// and where it ends in the sound, in samples.
struct Reading {
    words: Vec<String>,
    segments: Vec<(usize, usize)>,
}

impl SlidingWhisper {
    pub fn load(model: &Path, options: Options) -> Result<Self, String> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);
        let context = WhisperContext::new_with_params(model, params)
            .map_err(|e| format!("{}: {e}", model.display()))?;
        let state = context.create_state().map_err(|e| e.to_string())?;
        Ok(Self {
            state,
            options,
            buffer: Vec::new(),
            read_at: 0,
            fixed: Vec::new(),
            settled: Vec::new(),
            previous: Vec::new(),
            readings: 0,
        })
    }

    fn read(&mut self, samples: &[f32], prompt: &str, live: bool) -> Result<Reading, String> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.options.threads);
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_no_context(true);
        // One decoding only: the fallback to higher temperatures can take
        // several times as long, and a reading must be quick.
        params.set_temperature_inc(0.0);
        if !prompt.is_empty() {
            params.set_initial_prompt(prompt);
        }
        if live && self.options.short_window {
            // The encoder's positions are 20 ms each; 1500 is thirty seconds.
            let frames = samples.len() / 320 + 64;
            params.set_audio_ctx(frames.clamp(256, 1500) as i32);
        }
        // whisper.cpp refuses sound shorter than a second: pad it.
        let padded;
        let samples = if samples.len() < RATE + RATE / 10 {
            let mut copy = samples.to_vec();
            copy.resize(RATE + RATE / 10, 0.0);
            padded = copy;
            &padded[..]
        } else {
            samples
        };
        self.state
            .full(params, samples)
            .map_err(|e| e.to_string())?;
        self.readings += 1;

        let mut reading = Reading {
            words: Vec::new(),
            segments: Vec::new(),
        };
        for segment in self.state.as_iter() {
            let text = segment.to_str_lossy().map_err(|e| e.to_string())?;
            reading.words.extend(spoken_words(&text));
            let end = usize::try_from(segment.end_timestamp()).unwrap_or(0) * RATE / 100;
            reading.segments.push((reading.words.len(), end));
        }
        Ok(reading)
    }

    /// Drops from the front of the buffer the segments whose every word is
    /// settled, so that a long thought does not grow past whisper's window.
    fn trim(&mut self, reading: &mut Reading) {
        if (self.buffer.len() as f32) < self.options.trim_seconds * RATE as f32 {
            return;
        }
        let Some(&(words, end)) = reading
            .segments
            .iter()
            .rev()
            .find(|(words, end)| *words <= self.settled.len() && *end < self.buffer.len())
        else {
            return;
        };
        if words == 0 {
            return;
        }
        self.fixed.extend(self.settled.drain(..words));
        self.buffer.drain(..end);
        reading.words.drain(..words);
    }

    fn shown(&self, reading: &[String]) -> Shown {
        let settled: Vec<&str> = self
            .fixed
            .iter()
            .chain(&self.settled)
            .map(String::as_str)
            .collect();
        let tentative: Vec<&str> = reading
            .iter()
            .skip(self.settled.len())
            .map(String::as_str)
            .collect();
        Shown {
            settled: settled.join(" "),
            tentative: tentative.join(" "),
        }
    }
}

impl Live for SlidingWhisper {
    fn feed(&mut self, samples: &[f32]) -> Step {
        self.buffer.extend_from_slice(samples);
        let step = RATE * self.options.step_ms / 1000;
        if self.buffer.len() < self.read_at + step {
            return Step::default();
        }
        let buffer = std::mem::take(&mut self.buffer);
        // The last words whose sound is gone tell whisper where it is.
        let from = self.fixed.len().saturating_sub(40);
        let prompt = self.fixed[from..].join(" ");
        let reading = self.read(&buffer, &prompt, true);
        self.buffer = buffer;
        self.read_at = self.buffer.len();
        let mut reading = match reading {
            Ok(reading) => reading,
            Err(error) => {
                eprintln!("whisper: {error}");
                return Step::default();
            }
        };
        // Settle what this reading and the last agree on, and never take a
        // settled word back.
        let agreed = common_prefix(&self.previous, &reading.words);
        if agreed > self.settled.len() {
            let new = reading.words[self.settled.len()..agreed].to_vec();
            self.settled.extend(new);
        }
        let before = self.buffer.len();
        self.trim(&mut reading);
        self.read_at -= before - self.buffer.len();
        let shown = self.shown(&reading.words);
        self.previous = reading.words;
        Step {
            shown: Some(shown),
            ..Step::default()
        }
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.read_at = 0;
        self.fixed.clear();
        self.settled.clear();
        self.previous.clear();
    }

    /// The whole thought read once, as whisper was made to be used.
    fn second_pass(&mut self, samples: &[f32]) -> Option<String> {
        self.read(samples, "", false)
            .ok()
            .map(|r| r.words.join(" "))
    }
}

/// The words of a segment, without what whisper writes for sounds that are
/// not speech: "[BLANK_AUDIO]", "(wind blowing)", "*sigh*".
fn spoken_words(text: &str) -> Vec<String> {
    let mut plain = String::new();
    let mut depth = 0_i32;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    plain
        .split_whitespace()
        .filter(|w| !(w.starts_with('*') && w.ends_with('*')))
        .map(str::to_owned)
        .collect()
}

/// Silero's voice-activity model as whisper.cpp runs it: one probability of
/// speech for every 32 ms of the sound given. It reads a finished stretch of
/// sound, so a listening app would run it over the last few seconds.
pub fn vad_probabilities(model: &Path, threads: i32, samples: &[f32]) -> Result<Vec<f32>, String> {
    let mut params = whisper_rs::WhisperVadContextParams::new();
    params.set_n_threads(threads);
    params.set_use_gpu(false);
    let mut vad = whisper_rs::WhisperVadContext::new(&model.to_string_lossy(), params)
        .map_err(|e| e.to_string())?;
    vad.detect_speech(samples).map_err(|e| e.to_string())?;
    Ok(vad.probabilities().to_vec())
}
