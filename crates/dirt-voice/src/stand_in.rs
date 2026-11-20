//! Sound for [`crate::Listening`] from somewhere.
//!
//! A stand-in for the microphone that plays a WAV file at the pace it was
//! recorded, then a
//! quiet room, and what both it and the microphone share: a [`Feed`] that
//! is stopped by dropping it, and the step that brings any sound to 16 kHz
//! and one channel.
//!
//! The stand-in is for trying voice on a computer whose microphone belongs
//! to someone else, and for tests: an app offers it only in a development
//! build.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::engine::LinearResampler;

use crate::folder::VoiceFolder;
use crate::{Result, VoiceError, RATE, RATE_USIZE};

/// Sound being handed over, until it is stopped or dropped.
pub struct Feed {
    stop: Arc<AtomicBool>,
    /// Wakes the thread that hands the sound over, for it to see the stop.
    wake: Option<Box<dyn Fn() + Send>>,
    thread: Option<JoinHandle<()>>,
}

impl Feed {
    pub(crate) fn new(stop: Arc<AtomicBool>, thread: JoinHandle<()>) -> Self {
        Self {
            stop,
            wake: None,
            thread: Some(thread),
        }
    }

    pub(crate) fn woken_by(mut self, wake: impl Fn() + Send + 'static) -> Self {
        self.wake = Some(Box::new(wake));
        self
    }

    /// Stops, and waits until nothing more is handed over.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(wake) = &self.wake {
            wake();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Brings sound of any rate and any number of channels to 16 kHz mono.
pub struct To16k {
    channels: usize,
    resampler: Option<LinearResampler>,
}

impl To16k {
    pub fn new(folder: &VoiceFolder, rate: u32, channels: u16) -> Result<Self> {
        folder.load_engine()?;
        let resampler = if rate == RATE {
            None
        } else {
            let from =
                i32::try_from(rate).map_err(|_| VoiceError::Recording(format!("{rate} Hz")))?;
            Some(
                LinearResampler::create(from, 16_000)
                    .ok_or_else(|| VoiceError::Engine(format!("no resampler from {rate} Hz")))?,
            )
        };
        Ok(Self {
            channels: usize::from(channels.max(1)),
            resampler,
        })
    }

    /// The sound given, interleaved, at 16 kHz mono.
    #[must_use]
    pub fn convert(&self, interleaved: &[f32]) -> Vec<f32> {
        let mono: Vec<f32> = if self.channels == 1 {
            interleaved.to_vec()
        } else {
            #[allow(clippy::cast_precision_loss)]
            let share = 1.0 / self.channels as f32;
            interleaved
                .chunks(self.channels)
                .map(|frame| frame.iter().sum::<f32>() * share)
                .collect()
        };
        match &self.resampler {
            Some(resampler) => resampler.resample(&mono, false),
            None => mono,
        }
    }

    /// What the resampler still holds, at the end of the sound.
    #[must_use]
    pub fn finish(&self) -> Vec<f32> {
        self.resampler
            .as_ref()
            .map(|resampler| resampler.resample(&[], true))
            .unwrap_or_default()
    }
}

/// A room with nobody speaking: noise near -70 dBFS, the same every time.
/// A microphone never gives exact zeros, and exact zeros flatter a
/// detector (trial 1).
fn room(samples: &mut [f32], state: &mut u32) {
    for sample in samples {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        #[allow(clippy::cast_precision_loss)]
        let unit = (*state >> 8) as f32 / 16_777_216.0;
        *sample = (unit - 0.5) * 0.0006;
    }
}

/// Reads a WAV file as 16 kHz mono.
pub fn read_wav(folder: &VoiceFolder, path: &Path) -> Result<Vec<f32>> {
    let bad = |error: hound::Error| VoiceError::Recording(format!("{}: {error}", path.display()));
    let mut reader = hound::WavReader::open(path).map_err(bad)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>(),
        hound::SampleFormat::Int => {
            #[allow(clippy::cast_precision_loss)]
            let scale = 1.0 / (1_i64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample.map(|value| {
                        #[allow(clippy::cast_precision_loss)]
                        let value = value as f32;
                        value * scale
                    })
                })
                .collect::<std::result::Result<_, _>>()
        }
    }
    .map_err(bad)?;
    let to16k = To16k::new(folder, spec.sample_rate, spec.channels)?;
    let mut sound = to16k.convert(&samples);
    sound.extend(to16k.finish());
    Ok(sound)
}

/// Plays the WAV file at `path` to `sink`, a tenth of a second at a time
/// at the pace it was recorded, and then a quiet room until stopped.
pub fn play_wav(
    folder: &VoiceFolder,
    path: &Path,
    mut sink: impl FnMut(&[f32]) + Send + 'static,
) -> Result<Feed> {
    let sound = read_wav(folder, path)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("dirt-voice-stand-in".to_owned())
        .spawn(move || {
            let tenth = RATE_USIZE / 10;
            let began = Instant::now();
            let mut quiet = vec![0.0_f32; tenth];
            let mut state = 0x2545_f491_u32;
            for index in 0.. {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let start = index * tenth;
                if start < sound.len() {
                    sink(&sound[start..(start + tenth).min(sound.len())]);
                } else {
                    room(&mut quiet, &mut state);
                    sink(&quiet);
                }
                // Each piece when its last sample would have been heard.
                let due = began + Duration::from_millis(100 * (index as u64 + 1));
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
        })
        .map_err(|error| VoiceError::Microphone(error.to_string()))?;
    Ok(Feed::new(stop, thread))
}
