//! Cutting a long recording into pieces the model can read one at a time.
//!
//! The model's memory grows with what it is handed: Parakeet 110M held
//! 1,055 MiB for 60 seconds whole and 604 MiB for the same in pieces
//! (trial 1, second measurement). A recording longer than [`WINDOW`]
//! seconds is cut, and every sample is kept: the detector only chooses
//! where. Each cut is in the middle of the latest pause that keeps the
//! piece within the window; where the detector heard no pause in the second
//! half of the window, the cut is at the quietest tenth of a second there.
//! The trial's first cutter threw away the sound between pieces and lost
//! words at the joins; this one is the cutter it ended with.

use crate::engine::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::folder::{VoiceFolder, DETECTOR};
use crate::{Result, VoiceError, RATE_USIZE};

/// The longest piece, in seconds.
pub const WINDOW: usize = 25;

/// Silero's detector, as the pauses are looked for.
pub(crate) fn detector(folder: &VoiceFolder, longest_speech: f32) -> Result<VoiceActivityDetector> {
    folder.load_engine()?;
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(folder.file(DETECTOR)),
            threshold: 0.5,
            min_silence_duration: 0.3,
            min_speech_duration: 0.1,
            window_size: 512,
            max_speech_duration: longest_speech,
        },
        sample_rate: 16_000,
        num_threads: 1,
        ..VadModelConfig::default()
    };
    VoiceActivityDetector::create(&config, 120.0)
        .ok_or_else(|| VoiceError::Engine(format!("{DETECTOR} could not be loaded")))
}

/// Finds the pauses in a recording and cuts it at them.
pub struct Cutter {
    detector: VoiceActivityDetector,
}

impl Cutter {
    pub fn load(folder: &VoiceFolder) -> Result<Self> {
        // Past this the detector looks harder for a place to cut.
        #[allow(clippy::cast_precision_loss)]
        let longest = (WINDOW as f32 * 0.6).max(5.0);
        Ok(Self {
            detector: detector(folder, longest)?,
        })
    }

    /// The pieces of a recording, as sample ranges that follow one another
    /// with nothing left out. A recording within the window is one piece.
    pub fn cut(&mut self, samples: &[f32]) -> Vec<(usize, usize)> {
        let window = WINDOW * RATE_USIZE;
        if samples.len() <= window {
            return vec![(0, samples.len())];
        }
        self.detector.reset();
        let mut speech: Vec<(usize, usize)> = Vec::new();
        for chunk in samples.chunks(512 * 20) {
            self.detector.accept_waveform(chunk);
            take(&self.detector, &mut speech);
        }
        self.detector.flush();
        take(&self.detector, &mut speech);
        cut_at(samples, &speech, window)
    }
}

/// The cuts, given where the detector heard speech.
fn cut_at(samples: &[f32], speech: &[(usize, usize)], window: usize) -> Vec<(usize, usize)> {
    // The middle of every pause between two stretches of speech.
    let pauses: Vec<usize> = speech
        .windows(2)
        .filter(|pair| pair[1].0 > pair[0].1)
        .map(|pair| usize::midpoint(pair[0].1, pair[1].0))
        .collect();
    let mut out = Vec::new();
    let mut start = 0;
    while samples.len() - start > window {
        let latest = start + window;
        let cut = pauses
            .iter()
            .copied()
            .rfind(|&pause| pause > start + window / 2 && pause <= latest)
            .unwrap_or_else(|| quietest(samples, start + window / 2, latest));
        out.push((start, cut));
        start = cut;
    }
    out.push((start, samples.len()));
    out
}

/// The middle of the quietest tenth of a second between `from` and `to`.
fn quietest(samples: &[f32], from: usize, to: usize) -> usize {
    let step = RATE_USIZE / 10;
    (from..to.saturating_sub(step))
        .step_by(RATE_USIZE / 100)
        .min_by(|&a, &b| {
            let energy = |at: usize| samples[at..at + step].iter().map(|x| x * x).sum::<f32>();
            energy(a).total_cmp(&energy(b))
        })
        .map_or(to, |at| at + step / 2)
}

fn take(detector: &VoiceActivityDetector, speech: &mut Vec<(usize, usize)>) {
    // Each segment is let go of before the detector drops it from its queue.
    while let Some((start, length)) = detector.front().map(|segment| {
        (
            usize::try_from(segment.start()).unwrap_or(0),
            usize::try_from(segment.n()).unwrap_or(0),
        )
    }) {
        speech.push((start, start + length));
        detector.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: usize = RATE_USIZE;

    fn joined(pieces: &[(usize, usize)], length: usize) {
        assert_eq!(pieces.first().map(|p| p.0), Some(0));
        assert_eq!(pieces.last().map(|p| p.1), Some(length));
        for pair in pieces.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "a sample was lost or read twice");
        }
    }

    #[test]
    fn a_recording_is_cut_in_the_middle_of_its_latest_pause() {
        let length = 60 * SECOND;
        let samples = vec![0.1_f32; length];
        // Speech with pauses at 14-16 s, 20-22 s and 44-46 s.
        let speech = [
            (0, 14 * SECOND),
            (16 * SECOND, 20 * SECOND),
            (22 * SECOND, 44 * SECOND),
            (46 * SECOND, 60 * SECOND),
        ];
        let pieces = cut_at(&samples, &speech, 25 * SECOND);
        assert_eq!(
            pieces,
            [
                (0, 21 * SECOND),
                (21 * SECOND, 45 * SECOND),
                (45 * SECOND, length)
            ]
        );
        joined(&pieces, length);
    }

    #[test]
    fn with_no_pause_it_is_cut_where_it_is_quietest() {
        let length = 40 * SECOND;
        let mut samples = vec![0.5_f32; length];
        // A hush at 20 s.
        for sample in &mut samples[20 * SECOND..20 * SECOND + SECOND / 5] {
            *sample = 0.0;
        }
        let pieces = cut_at(&samples, &[(0, length)], 25 * SECOND);
        joined(&pieces, length);
        assert_eq!(pieces.len(), 2);
        let cut = pieces[0].1;
        assert!(
            (20 * SECOND..20 * SECOND + SECOND / 5).contains(&cut),
            "cut at {cut}"
        );
    }

    #[test]
    fn every_piece_is_within_the_window() {
        let length = 600 * SECOND;
        let samples = vec![0.2_f32; length];
        let pieces = cut_at(&samples, &[], 25 * SECOND);
        joined(&pieces, length);
        assert!(pieces.iter().all(|(a, b)| b - a <= 25 * SECOND));
    }
}
