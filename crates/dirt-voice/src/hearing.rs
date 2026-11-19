//! Listening: the sound as a person speaks.
//!
//! Nothing is turned into words here; that comes after (`reading`). What
//! is made is the recording,
//! written as the sound comes, and what the screen and the end of a
//! thought need to know: how loud it is now, whether speech has been heard
//! at all, and how long the quiet after it has lasted.
//!
//! Silero's detector tells speech from the rest. "Nothing heard" is a
//! recording in which it found no speech: the app keeps no note of it
//! (6.18). Started with no hand on the screen (Android's Talk), 1.6 s of
//! quiet after speech ends the thought: the detector is asked, not the
//! level, because a breath and a room are not quiet to a level meter.

use sherpa_onnx::VoiceActivityDetector;

use crate::cut;
use crate::folder::VoiceFolder;
use crate::recording::OpusWriter;
use crate::{Result, MOST_SECONDS, RATE_USIZE};

/// The level, in decibels below full scale, that shows as nothing, and
/// the one that shows as the loudest.
const QUIETEST_DB: f32 = -55.0;
const LOUDEST_DB: f32 = -15.0;

/// What has been heard so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heard {
    /// How loud the latest sound was, from 0 (quiet) to 1.
    pub level: f32,
    /// How long it has been listening, in milliseconds of sound.
    pub ms: u64,
    /// Speech has been heard.
    pub speech: bool,
    /// How long it has been quiet since the last speech, in milliseconds.
    /// `None` before any speech, and while someone is speaking.
    pub quiet_ms: Option<u64>,
    /// The longest recording has been reached: listening stops, keeping
    /// what was said.
    pub full: bool,
}

/// A finished recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    /// The file: Ogg Opus (`recording`).
    pub file: Vec<u8>,
    /// How long it lasts.
    pub duration_ms: u64,
    /// Speech was heard in it. A recording without is not kept.
    pub speech: bool,
}

/// Listening, from the first sample to the last.
pub struct Listening {
    writer: OpusWriter,
    detector: VoiceActivityDetector,
    taken: usize,
    speech: bool,
    /// The sample at which speech was last heard.
    last_speech: usize,
    level: f32,
}

impl Listening {
    /// Starts listening. Only the detector is loaded, which is small and
    /// quick: the model waits for the recording to end.
    pub fn start(folder: &VoiceFolder) -> Result<Self> {
        // One long stretch of speech is fine here: nothing is cut while
        // listening.
        let detector = cut::detector(folder, 60.0)?;
        Ok(Self {
            writer: OpusWriter::new()?,
            detector,
            taken: 0,
            speech: false,
            last_speech: 0,
            level: 0.0,
        })
    }

    /// Takes more sound, at 16 kHz.
    pub fn take(&mut self, samples: &[f32]) -> Result<Heard> {
        let most = MOST_SECONDS as usize * RATE_USIZE;
        let room = most.saturating_sub(self.taken);
        let samples = &samples[..samples.len().min(room)];
        self.writer.push(samples)?;
        self.taken += samples.len();
        self.detector.accept_waveform(samples);
        let mut ended = false;
        while !self.detector.is_empty() {
            self.detector.pop();
            ended = true;
        }
        if ended || self.detector.detected() {
            self.speech = true;
            if self.detector.detected() {
                self.last_speech = self.taken;
            }
        }
        if !samples.is_empty() {
            self.level = level(samples);
        }
        Ok(self.heard())
    }

    fn heard(&self) -> Heard {
        let quiet_ms = (self.speech && !self.detector.detected())
            .then(|| ((self.taken - self.last_speech.min(self.taken)) * 1000 / RATE_USIZE) as u64);
        Heard {
            level: self.level,
            ms: self.writer.duration_ms(),
            speech: self.speech,
            quiet_ms,
            full: self.taken >= MOST_SECONDS as usize * RATE_USIZE,
        }
    }

    /// The recording, ending where the sound ends.
    pub fn finish(self) -> Result<Recording> {
        self.detector.flush();
        let speech = self.speech || !self.detector.is_empty();
        let duration_ms = self.writer.duration_ms();
        Ok(Recording {
            file: self.writer.finish()?,
            duration_ms,
            speech,
        })
    }
}

/// How loud `samples` are, from 0 to 1.
fn level(samples: &[f32]) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let mean = samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32;
    let db = 10.0 * mean.max(1e-10).log10();
    ((db - QUIETEST_DB) / (LOUDEST_DB - QUIETEST_DB)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_is_nothing_and_loud_is_everything() {
        assert!(level(&[0.0; 160]) < f32::EPSILON);
        assert!(level(&[0.0003; 160]) < 0.1);
        assert!((level(&[0.9; 160]) - 1.0).abs() < f32::EPSILON);
        let middle = level(&[0.03; 160]);
        assert!(middle > 0.3 && middle < 0.7, "{middle}");
    }
}
