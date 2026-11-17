//! The second measurement: reading a finished recording once, after the
//! person has let go. No words are shown while they speak, so what counts
//! is how right the text is, how long it takes, and what it costs.
//!
//! A reader takes a stretch of sound and gives its text. A recording
//! longer than a reader can take at once is cut at pauses, found by
//! Silero's voice-activity model, and the pieces are read one after the
//! other and joined.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::audio::RATE;

/// Something that reads a stretch of finished sound.
pub trait Reader {
    fn read(&mut self, samples: &[f32]) -> Result<String, String>;
}

/// One recording read.
#[derive(Debug, Serialize)]
pub struct Read {
    pub id: String,
    pub audio_seconds: f64,
    /// The time of every reading asked for, in order.
    pub read_seconds: Vec<f64>,
    /// Time spent finding the pauses, in the last reading.
    pub cut_seconds: f64,
    /// Where each piece began and ended in the recording, in seconds.
    pub pieces: Vec<(f64, f64)>,
    /// The text of each piece, in order.
    pub piece_texts: Vec<String>,
    pub text: String,
}

/// Finds the pauses in a recording and joins the stretches of speech
/// between them into pieces no longer than `window` seconds.
pub struct Cutter {
    #[cfg(feature = "sherpa")]
    vad: sherpa_onnx::VoiceActivityDetector,
    window: f32,
}

impl Cutter {
    #[cfg(feature = "sherpa")]
    pub fn load(model: &Path, window: f32) -> Result<Self, String> {
        use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
        let config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(model.to_string_lossy().into_owned()),
                threshold: 0.5,
                min_silence_duration: 0.3,
                min_speech_duration: 0.1,
                window_size: 512,
                // Past this the detector looks harder for a place to cut.
                max_speech_duration: (window * 0.6).max(5.0),
            },
            sample_rate: RATE as i32,
            num_threads: 1,
            ..VadModelConfig::default()
        };
        let vad = VoiceActivityDetector::create(&config, 120.0)
            .ok_or_else(|| format!("could not load {}", model.display()))?;
        Ok(Self { vad, window })
    }

    #[cfg(not(feature = "sherpa"))]
    pub fn load(_model: &Path, _window: f32) -> Result<Self, String> {
        Err("cutting at pauses needs the sherpa feature, for Silero's model".to_owned())
    }

    /// The pieces of a recording, as sample ranges. Speech is never cut
    /// inside a stretch the detector heard as one, unless that stretch
    /// alone is longer than the window, when it is cut in equal parts.
    #[cfg(feature = "sherpa")]
    pub fn cut(&mut self, samples: &[f32]) -> Vec<(usize, usize)> {
        self.vad.reset();
        let mut speech: Vec<(usize, usize)> = Vec::new();
        for chunk in samples.chunks(512 * 20) {
            self.vad.accept_waveform(chunk);
            take(&self.vad, &mut speech);
        }
        self.vad.flush();
        take(&self.vad, &mut speech);

        let window = (self.window * RATE as f32) as usize;
        let margin = RATE / 5;
        let mut pieces: Vec<(usize, usize)> = Vec::new();
        for (start, end) in speech {
            match pieces.last_mut() {
                Some(piece) if end - piece.0 <= window => piece.1 = end,
                _ => pieces.push((start, end)),
            }
        }
        let mut out = Vec::new();
        for (start, end) in pieces {
            // A little quiet either side, so no word is clipped.
            let start = start.saturating_sub(margin);
            let end = (end + margin).min(samples.len());
            let parts = (end - start).div_ceil(window + 2 * margin).max(1);
            let each = (end - start).div_ceil(parts);
            for part in 0..parts {
                out.push((start + part * each, (start + (part + 1) * each).min(end)));
            }
        }
        out
    }

    #[cfg(not(feature = "sherpa"))]
    pub fn cut(&mut self, samples: &[f32]) -> Vec<(usize, usize)> {
        vec![(0, samples.len())]
    }
}

#[cfg(feature = "sherpa")]
fn take(vad: &sherpa_onnx::VoiceActivityDetector, speech: &mut Vec<(usize, usize)>) {
    while let Some(segment) = vad.front() {
        let start = usize::try_from(segment.start()).unwrap_or(0);
        speech.push((start, start + segment.n() as usize));
        drop(segment);
        vad.pop();
    }
}

/// Reads one recording `repeat` times. With a cutter, a recording longer
/// than its window is cut at pauses first; without, the reader gets it whole.
pub fn read_recording(
    reader: &mut dyn Reader,
    cutter: Option<&mut Cutter>,
    id: &str,
    samples: &[f32],
    repeat: usize,
) -> Result<Read, String> {
    let mut read = Read {
        id: id.to_owned(),
        audio_seconds: samples.len() as f64 / RATE as f64,
        read_seconds: Vec::new(),
        cut_seconds: 0.0,
        pieces: Vec::new(),
        piece_texts: Vec::new(),
        text: String::new(),
    };
    let mut cutter = cutter;
    for _ in 0..repeat.max(1) {
        let began = Instant::now();
        let pieces = match cutter.as_deref_mut() {
            Some(cutter) if samples.len() as f32 > cutter.window * RATE as f32 => {
                cutter.cut(samples)
            }
            _ => vec![(0, samples.len())],
        };
        read.cut_seconds = began.elapsed().as_secs_f64();
        let mut texts = Vec::new();
        for &(start, end) in &pieces {
            texts.push(reader.read(&samples[start..end])?.trim().to_owned());
        }
        read.read_seconds.push(began.elapsed().as_secs_f64());
        read.pieces = pieces
            .iter()
            .map(|&(a, b)| (a as f64 / RATE as f64, b as f64 / RATE as f64))
            .collect();
        read.text = texts
            .iter()
            .filter(|t| !t.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        read.piece_texts = texts;
    }
    Ok(read)
}

/// So that a timing is not disturbed by another copy of this program
/// reading a test set beside it: the copy that times makes a file, and a
/// copy that only reads waits while the file is there.
pub struct Turn {
    file: std::path::PathBuf,
    mine: std::path::PathBuf,
}

impl Turn {
    pub fn new(file: &Path) -> Self {
        let mine = file.with_extension(format!("busy.{}", std::process::id()));
        Self {
            file: file.to_owned(),
            mine,
        }
    }

    fn others_busy(&self) -> bool {
        let Some(folder) = self.file.parent() else {
            return false;
        };
        let stem = self
            .file
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        std::fs::read_dir(folder).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with(&format!("{stem}.busy.")) && entry.path() != self.mine
            })
        })
    }

    /// For the copy that times: asks the others to wait, and waits for them.
    pub fn take(&self) {
        let _ = std::fs::write(&self.file, b"timing");
        std::thread::sleep(std::time::Duration::from_millis(400));
        while self.others_busy() {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    pub fn give_back(&self) {
        let _ = std::fs::remove_file(&self.file);
    }

    /// For a copy that only reads: waits while another is timing, then
    /// says it is busy. A request older than twenty minutes is taken for
    /// one left behind by a copy that died.
    pub fn wait_then_busy(&self) {
        loop {
            let fresh = std::fs::metadata(&self.file)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() < 1200);
            if !fresh {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let _ = std::fs::write(&self.mine, b"reading");
    }

    pub fn idle(&self) {
        let _ = std::fs::remove_file(&self.mine);
    }
}
