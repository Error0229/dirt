//! The engine's types, as the rest of the crate uses them.
//!
//! With the engine linked (`voice_engine`, set by `build.rs`) they are
//! sherpa-onnx's own. Without it (`DIRT_VOICE=off`, a build that could not
//! fetch the engine, a target with nothing pinned) they are types with no
//! values, whose constructors give `None`: listening and reading then say
//! that voice is not in this build, and nothing refers to a native symbol
//! that is not there to link. The configurations are sherpa-onnx's in
//! either case; they are plain Rust.

#[cfg(voice_engine)]
pub use sherpa_onnx::{
    LinearResampler, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
    SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};

// Nothing of sherpa-onnx is named without the engine, not even its
// configurations: they are compiled with the rest of its code, which the
// linker would then want.
#[cfg(not(voice_engine))]
pub use absent::{
    LinearResampler, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
    SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};

/// Stand-ins that are never made: each constructor gives `None`, so no
/// method of theirs is ever called.
#[cfg(not(voice_engine))]
#[allow(clippy::missing_const_for_fn, clippy::unused_self, dead_code)]
mod absent {
    #[derive(Default)]
    pub struct SileroVadModelConfig {
        pub model: Option<String>,
        pub threshold: f32,
        pub min_silence_duration: f32,
        pub min_speech_duration: f32,
        pub window_size: i32,
        pub max_speech_duration: f32,
    }

    #[derive(Default)]
    pub struct VadModelConfig {
        pub silero_vad: SileroVadModelConfig,
        pub sample_rate: i32,
        pub num_threads: i32,
        pub provider: Option<String>,
        pub debug: bool,
    }

    #[derive(Default)]
    pub struct OfflineTransducerModelConfig {
        pub encoder: Option<String>,
        pub decoder: Option<String>,
        pub joiner: Option<String>,
    }

    #[derive(Default)]
    pub struct OfflineModelConfig {
        pub transducer: OfflineTransducerModelConfig,
        pub model_type: Option<String>,
        pub tokens: Option<String>,
        pub num_threads: i32,
    }

    #[derive(Default)]
    pub struct OfflineRecognizerConfig {
        pub model_config: OfflineModelConfig,
    }

    pub struct OfflineRecognizerResult {
        pub text: String,
    }

    pub struct VoiceActivityDetector;

    impl VoiceActivityDetector {
        pub fn create(_config: &VadModelConfig, _seconds: f32) -> Option<Self> {
            None
        }
        pub fn accept_waveform(&self, _samples: &[f32]) {}
        pub fn is_empty(&self) -> bool {
            true
        }
        pub fn detected(&self) -> bool {
            false
        }
        pub fn pop(&self) {}
        pub fn flush(&self) {}
        pub fn reset(&self) {}
        pub fn front(&self) -> Option<SpeechSegment> {
            None
        }
    }

    pub struct SpeechSegment;

    impl SpeechSegment {
        pub fn start(&self) -> i64 {
            0
        }
        pub fn n(&self) -> i32 {
            0
        }
    }

    pub struct LinearResampler;

    impl LinearResampler {
        pub fn create(_from: i32, _to: i32) -> Option<Self> {
            None
        }
        pub fn resample(&self, _samples: &[f32], _flush: bool) -> Vec<f32> {
            Vec::new()
        }
    }

    pub struct OfflineRecognizer;

    impl OfflineRecognizer {
        pub fn create(_config: &OfflineRecognizerConfig) -> Option<Self> {
            None
        }
        pub fn create_stream(&self) -> OfflineStream {
            OfflineStream
        }
        pub fn decode(&self, _stream: &OfflineStream) {}
    }

    pub struct OfflineStream;

    impl OfflineStream {
        pub fn accept_waveform(&self, _rate: i32, _samples: &[f32]) {}
        pub fn get_result(&self) -> Option<OfflineRecognizerResult> {
            None
        }
    }
}

/// Whether this build has the engine.
pub const LINKED: bool = cfg!(voice_engine);
