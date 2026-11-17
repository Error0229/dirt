//! Reading the test speech: 16 kHz mono WAV files.

use std::path::Path;

pub const RATE: usize = 16_000;

/// Reads a 16 kHz mono 16-bit WAV file as samples between -1 and 1.
pub fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_rate as usize != RATE || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!(
            "{}: want 16 kHz mono 16-bit, got {} Hz, {} channels, {} bits",
            path.display(),
            spec.sample_rate,
            spec.channels,
            spec.bits_per_sample
        ));
    }
    reader
        .samples::<i16>()
        .map(|s| s.map(|v| f32::from(v) / 32768.0).map_err(|e| e.to_string()))
        .collect()
}

/// A room with nobody speaking: noise near -70 dBFS, the same every run.
/// A microphone never gives exact zeros, and exact zeros flatter a detector.
pub fn quiet(seconds: f32) -> Vec<f32> {
    let mut state: u32 = 0x2545_f491;
    (0..(seconds * RATE as f32) as usize)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 8) as f32 / 16_777_216.0 - 0.5) * 0.0006
        })
        .collect()
}
