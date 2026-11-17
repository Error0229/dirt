//! sherpa-onnx: a recogniser made for speech as it arrives (a streaming
//! transducer), its own rule for the end of a thought, Silero's
//! voice-activity model beside it, and optionally a second, slower model
//! that reads the finished thought again (Moonshine).

use std::path::{Path, PathBuf};

use sherpa_onnx::{
    OfflineMoonshineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OnlineRecognizer,
    OnlineRecognizerConfig, OnlineStream, SileroVadModelConfig, VadModelConfig,
    VoiceActivityDetector,
};

use crate::audio::RATE;
use crate::live::{Live, Shown, Step};

pub struct Options {
    pub threads: i32,
    /// Take the 8-bit files of a model where it has them.
    pub int8: bool,
    /// "greedy_search" or "modified_beam_search".
    pub decoding: String,
    /// Quiet after speech that ends a thought, by the recogniser's own rule.
    pub end_after_seconds: f32,
    /// Silero's model file, to run the detector beside the recogniser.
    pub vad: Option<PathBuf>,
    /// A Moonshine model's folder, for the second pass.
    pub second_pass: Option<PathBuf>,
}

pub struct Streaming {
    recognizer: OnlineRecognizer,
    stream: OnlineStream,
    vad: Option<VoiceActivityDetector>,
    second: Option<OfflineRecognizer>,
}

/// The file in `folder` whose name has `stem` in it, the 8-bit one if
/// asked for and there.
pub fn model_file(folder: &Path, stem: &str, int8: bool) -> Result<String, String> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            name.contains(stem) && (name.ends_with(".onnx") || name.ends_with(".ort"))
        })
        .collect();
    found.sort();
    let is_int8 = |p: &PathBuf| p.to_string_lossy().contains(".int8.");
    found
        .iter()
        .find(|p| is_int8(p) == int8)
        .or_else(|| found.first())
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| format!("no {stem}*.onnx in {}", folder.display()))
}

impl Streaming {
    pub fn load(folder: &Path, options: &Options) -> Result<Self, String> {
        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = Some(model_file(folder, "encoder", options.int8)?);
        config.model_config.transducer.decoder = Some(model_file(folder, "decoder", options.int8)?);
        config.model_config.transducer.joiner = Some(model_file(folder, "joiner", options.int8)?);
        config.model_config.tokens = Some(folder.join("tokens.txt").to_string_lossy().into_owned());
        config.model_config.num_threads = options.threads;
        config.decoding_method = Some(options.decoding.clone());
        config.max_active_paths = 4;
        config.enable_endpoint = true;
        // Rule 1: quiet with nothing said yet. Rule 2: quiet after words.
        // Rule 3: a thought this long ends anyway; ten minutes is the design's limit.
        config.rule1_min_trailing_silence = 30.0;
        config.rule2_min_trailing_silence = options.end_after_seconds;
        config.rule3_min_utterance_length = 600.0;
        let recognizer = OnlineRecognizer::create(&config)
            .ok_or_else(|| format!("could not load {}", folder.display()))?;
        let stream = recognizer.create_stream();

        let vad = match &options.vad {
            None => None,
            Some(model) => {
                let config = VadModelConfig {
                    silero_vad: SileroVadModelConfig {
                        model: Some(model.to_string_lossy().into_owned()),
                        threshold: 0.5,
                        // The detector says "quiet" once it has heard this much of it.
                        min_silence_duration: 0.25,
                        min_speech_duration: 0.1,
                        window_size: 512,
                        max_speech_duration: 600.0,
                    },
                    sample_rate: RATE as i32,
                    num_threads: 1,
                    ..VadModelConfig::default()
                };
                Some(
                    VoiceActivityDetector::create(&config, 60.0)
                        .ok_or_else(|| format!("could not load {}", model.display()))?,
                )
            }
        };

        let second = match &options.second_pass {
            None => None,
            Some(folder) => Some(load_moonshine(folder, options.threads)?),
        };
        Ok(Self {
            recognizer,
            stream,
            vad,
            second,
        })
    }
}

fn load_moonshine(folder: &Path, threads: i32) -> Result<OfflineRecognizer, String> {
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.moonshine = OfflineMoonshineModelConfig {
        encoder: Some(model_file(folder, "encoder", false)?),
        merged_decoder: Some(model_file(folder, "decoder_model_merged", false)?),
        ..OfflineMoonshineModelConfig::default()
    };
    config.model_config.tokens = Some(folder.join("tokens.txt").to_string_lossy().into_owned());
    config.model_config.num_threads = threads;
    OfflineRecognizer::create(&config).ok_or_else(|| format!("could not load {}", folder.display()))
}

impl Live for Streaming {
    fn feed(&mut self, samples: &[f32]) -> Step {
        self.stream.accept_waveform(RATE as i32, samples);
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
        let text = self
            .recognizer
            .get_result(&self.stream)
            .map(|result| result.text)
            .unwrap_or_default();
        // A transducer read greedily never takes a word back, but the last
        // word may still grow ("ask" becoming "asking"), so it is shown as
        // tentative and the rest as settled.
        let text = text.trim();
        let (settled, tentative) = match text.rfind(' ') {
            Some(space) => (&text[..space], &text[space + 1..]),
            None => ("", text),
        };
        let engine_endpoint = self.recognizer.is_endpoint(&self.stream);
        let vad_speech = self.vad.as_ref().map(|vad| {
            vad.accept_waveform(samples);
            while !vad.is_empty() {
                vad.pop();
            }
            vad.detected()
        });
        Step {
            shown: Some(Shown {
                settled: settled.to_owned(),
                tentative: tentative.to_owned(),
            }),
            engine_endpoint,
            vad_speech,
        }
    }

    fn reset(&mut self) {
        self.stream = self.recognizer.create_stream();
        if let Some(vad) = &self.vad {
            vad.reset();
        }
    }

    fn second_pass(&mut self, samples: &[f32]) -> Option<String> {
        let second = self.second.as_ref()?;
        let stream = second.create_stream();
        stream.accept_waveform(RATE as i32, samples);
        second.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text.trim().to_owned())
    }
}

/// Moonshine reading the sound so far again and again (`sliding`), instead
/// of a streaming model: one small model for the words as they are heard
/// and for the text that is kept.
pub fn sliding_moonshine(
    folder: &Path,
    threads: i32,
    step_ms: usize,
) -> Result<Box<dyn Live>, String> {
    let recognizer = load_moonshine(folder, threads)?;
    let read = move |samples: &[f32]| {
        let stream = recognizer.create_stream();
        stream.accept_waveform(RATE as i32, samples);
        recognizer.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text.trim().to_owned())
    };
    Ok(Box::new(crate::sliding::Sliding::new(read, step_ms)))
}

/// sherpa-onnx reading a finished stretch of sound with one of its
/// offline models. `kind` says which family the folder holds.
pub struct Finished {
    recognizer: OfflineRecognizer,
}

impl Finished {
    pub fn load(folder: &Path, kind: &str, threads: i32, int8: bool) -> Result<Self, String> {
        let file = |stem: &str| model_file(folder, stem, int8).map(Some);
        let tokens = std::fs::read_dir(folder)
            .map_err(|e| format!("{}: {e}", folder.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.to_string_lossy().ends_with("tokens.txt"))
            .ok_or_else(|| format!("no tokens.txt in {}", folder.display()))?;
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.tokens = Some(tokens.to_string_lossy().into_owned());
        config.model_config.num_threads = threads;
        match kind {
            "whisper" => {
                config.model_config.whisper = sherpa_onnx::OfflineWhisperModelConfig {
                    encoder: file("encoder")?,
                    decoder: file("decoder")?,
                    language: Some("en".to_owned()),
                    task: Some("transcribe".to_owned()),
                    tail_paddings: -1,
                    ..Default::default()
                };
            }
            "moonshine" => {
                return load_moonshine(folder, threads).map(|recognizer| Self { recognizer })
            }
            "nemo-transducer" | "transducer" => {
                config.model_config.transducer = sherpa_onnx::OfflineTransducerModelConfig {
                    encoder: file("encoder")?,
                    decoder: file("decoder")?,
                    joiner: file("joiner")?,
                };
                if kind == "nemo-transducer" {
                    config.model_config.model_type = Some("nemo_transducer".to_owned());
                }
            }
            "canary" => {
                config.model_config.canary = sherpa_onnx::OfflineCanaryModelConfig {
                    encoder: file("encoder")?,
                    decoder: file("decoder")?,
                    src_lang: Some("en".to_owned()),
                    tgt_lang: Some("en".to_owned()),
                    use_pnc: true,
                };
            }
            "nemo-ctc" => {
                config.model_config.nemo_ctc = sherpa_onnx::OfflineNemoEncDecCtcModelConfig {
                    model: file("model")?,
                };
            }
            other => return Err(format!("no kind of sherpa-onnx model called {other:?}")),
        }
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| format!("could not load {}", folder.display()))?;
        Ok(Self { recognizer })
    }
}

impl crate::finished::Reader for Finished {
    fn read(&mut self, samples: &[f32]) -> Result<String, String> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(RATE as i32, samples);
        self.recognizer.decode(&stream);
        Ok(stream
            .get_result()
            .map(|result| result.text.trim().to_owned())
            .unwrap_or_default())
    }
}
