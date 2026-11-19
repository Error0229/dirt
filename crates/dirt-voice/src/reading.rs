//! Turning a finished recording into words, once, after the person has let
//! go (interface design 6.18; trial 1, second measurement).
//!
//! The model is Parakeet TDT-CTC 110M, read greedily through sherpa-onnx's
//! offline recogniser. It writes capitals and punctuation. It is loaded only
//! to read, and let go afterwards: it holds about 0.6 GiB while it works,
//! which is a lot to keep for an app that is mostly waiting. A recording
//! longer than [`crate::cut::WINDOW`] seconds is read in pieces, one at a
//! time, so that what is held is that of one piece.

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use crate::cut::Cutter;
use crate::folder::VoiceFolder;
use crate::{recording, Result, VoiceError};

/// The model, loaded, with the cutter.
pub struct Reader {
    recognizer: OfflineRecognizer,
    cutter: Cutter,
}

/// How many threads the model reads with: four, as it was measured, or
/// half the processor where that is less.
fn threads() -> i32 {
    let have = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    i32::try_from((have / 2).clamp(1, 4)).unwrap_or(1)
}

impl Reader {
    /// Loads the model. It takes a second or two.
    pub fn load(folder: &VoiceFolder) -> Result<Self> {
        if !folder.has_model() {
            return Err(VoiceError::NotInstalled(format!(
                "{} has no speech model",
                folder.path().display()
            )));
        }
        folder.load_engine()?;
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(folder.file("encoder.int8.onnx")),
            decoder: Some(folder.file("decoder.int8.onnx")),
            joiner: Some(folder.file("joiner.int8.onnx")),
        };
        config.model_config.model_type = Some("nemo_transducer".to_owned());
        config.model_config.tokens = Some(folder.file("tokens.txt"));
        config.model_config.num_threads = threads();
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| VoiceError::Engine("the speech model could not be loaded".to_owned()))?;
        Ok(Self {
            recognizer,
            cutter: Cutter::load(folder)?,
        })
    }

    /// The words of a stretch of sound at 16 kHz. Empty when the model
    /// found none, which it now and then does for unclear speech.
    pub fn read(&mut self, samples: &[f32]) -> String {
        let pieces = self.cutter.cut(samples);
        let mut words: Vec<String> = Vec::new();
        for (start, end) in pieces {
            let piece = self.read_piece(&samples[start..end]);
            if !piece.is_empty() {
                words.push(piece);
            }
        }
        words.join(" ")
    }

    fn read_piece(&self, samples: &[f32]) -> String {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(16_000, samples);
        self.recognizer.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text.trim().to_owned())
            .unwrap_or_default()
    }
}

/// The words of a recording, as it was kept: an Ogg Opus file. The model
/// is loaded for it and let go before this returns. Empty when no words
/// were found.
pub fn words_of(folder: &VoiceFolder, file: &[u8]) -> Result<String> {
    let sound = recording::decode(file)?;
    let mut reader = Reader::load(folder)?;
    let words = reader.read(&sound);
    drop(reader);
    Ok(words)
}
