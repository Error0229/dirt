//! Voice memo encoding utilities.

use std::io::Cursor;

use crate::{Error, Result};

/// Voice memo WAV encoding options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceMemoOptions {
    /// PCM sample rate in Hz.
    pub sample_rate_hz: u32,
    /// Number of interleaved audio channels.
    pub channels: u16,
}

impl Default for VoiceMemoOptions {
    fn default() -> Self {
        Self {
            sample_rate_hz: 16_000,
            channels: 1,
        }
    }
}

impl VoiceMemoOptions {
    fn validate(self) -> Result<Self> {
        if self.sample_rate_hz == 0 {
            return Err(Error::InvalidInput(
                "Voice memo sample_rate_hz must be greater than zero".to_string(),
            ));
        }
        if self.channels == 0 {
            return Err(Error::InvalidInput(
                "Voice memo channels must be greater than zero".to_string(),
            ));
        }
        Ok(self)
    }
}

/// Encode interleaved PCM16 samples as a WAV byte buffer.
pub fn encode_voice_memo_wav(samples_pcm16: &[i16], options: VoiceMemoOptions) -> Result<Vec<u8>> {
    let options = options.validate()?;

    let spec = hound::WavSpec {
        channels: options.channels,
        sample_rate: options.sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(|error| {
            Error::InvalidInput(format!("Failed to initialize WAV writer: {error}"))
        })?;

        for &sample in samples_pcm16 {
            writer.write_sample(sample).map_err(|error| {
                Error::InvalidInput(format!("Failed to write WAV sample: {error}"))
            })?;
        }

        writer.finalize().map_err(|error| {
            Error::InvalidInput(format!("Failed to finalize WAV data: {error}"))
        })?;
    }

    Ok(cursor.into_inner())
}

/// Estimate voice memo duration in milliseconds for interleaved PCM samples.
pub fn estimate_voice_memo_duration_ms(
    sample_count: usize,
    options: VoiceMemoOptions,
) -> Result<u64> {
    let options = options.validate()?;
    let channels = usize::from(options.channels);

    let frame_count = sample_count / channels;
    let duration_ms = (frame_count as u128)
        .saturating_mul(1_000)
        .saturating_div(u128::from(options.sample_rate_hz));

    Ok(u64::try_from(duration_ms).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_encoding_generates_valid_header_and_sample_count() {
        let samples = vec![0_i16, 1200, -1200, 300, -300];

        let bytes = encode_voice_memo_wav(
            &samples,
            VoiceMemoOptions {
                sample_rate_hz: 16_000,
                channels: 1,
            },
        )
        .unwrap();

        assert!(!bytes.is_empty());

        let mut reader = hound::WavReader::new(Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);

        let decoded: Vec<i16> = reader
            .samples::<i16>()
            .map(std::result::Result::unwrap)
            .collect();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn duration_estimation_handles_mono_and_stereo() {
        let mono = estimate_voice_memo_duration_ms(
            16_000,
            VoiceMemoOptions {
                sample_rate_hz: 16_000,
                channels: 1,
            },
        )
        .unwrap();
        assert_eq!(mono, 1_000);

        // 2 channels interleaved: 32_000 samples = 16_000 frames = 1 second
        let stereo = estimate_voice_memo_duration_ms(
            32_000,
            VoiceMemoOptions {
                sample_rate_hz: 16_000,
                channels: 2,
            },
        )
        .unwrap();
        assert_eq!(stereo, 1_000);
    }

    #[test]
    fn invalid_options_are_rejected() {
        let err = encode_voice_memo_wav(
            &[1, 2, 3],
            VoiceMemoOptions {
                sample_rate_hz: 0,
                channels: 1,
            },
        )
        .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));

        let err = estimate_voice_memo_duration_ms(
            100,
            VoiceMemoOptions {
                sample_rate_hz: 16_000,
                channels: 0,
            },
        )
        .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }
}
