//! The recording as a file.
//!
//! Opus in an Ogg container (RFC 7845), 16 kHz mono at 24 kbit/s (trial
//! 1): ten minutes is about 1.6 MiB. It is
//! written as the sound comes, 20 ms at a time, so that it is ready the
//! moment the person lets go.
//!
//! It is read back the same way it is written, for the words to be made
//! from it, and decoded into a WAV in memory for a web view that cannot
//! play Ogg Opus (by its documentation, macOS's before 15.4).

use ogg::reading::PacketReader;
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};

use crate::{Result, VoiceError, RATE, RATE_USIZE};

/// What a recording is called on a note, after its length.
pub const MEDIA_TYPE: &str = "audio/ogg";

/// The bits a second the recording is written at.
const BITRATE: i32 = 24_000;
/// Samples in one 20 ms packet.
const FRAME: usize = RATE_USIZE / 50;
/// Ogg counts Opus time in 48 kHz samples whatever the input's rate.
const TO_48K: u64 = 48_000 / RATE as u64;
/// The stream's serial number in its Ogg pages: "dirt".
const SERIAL: u32 = 0x6469_7274;

#[allow(clippy::needless_pass_by_value)]
fn opus_error(error: opus::Error) -> VoiceError {
    VoiceError::Recording(error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
fn io_error(error: std::io::Error) -> VoiceError {
    VoiceError::Recording(error.to_string())
}

/// Writes a recording as the sound comes.
pub struct OpusWriter {
    encoder: Encoder,
    /// One writer for the whole file: each page it writes knows the one
    /// before (a writer made afresh would mark every page a stream's first).
    pages: PacketWriter<'static, Vec<u8>>,
    /// Samples not yet a whole packet.
    waiting: Vec<f32>,
    /// Samples taken in all.
    samples: u64,
    /// What a player throws away at the start, in 48 kHz samples.
    pre_skip: u64,
    packets: u64,
    /// The last packet, held back until it is known whether it ends the
    /// stream.
    held: Option<(Vec<u8>, u64)>,
}

impl OpusWriter {
    pub fn new() -> Result<Self> {
        let mut encoder =
            Encoder::new(RATE, Channels::Mono, Application::Voip).map_err(opus_error)?;
        encoder
            .set_bitrate(Bitrate::Bits(BITRATE))
            .map_err(opus_error)?;
        let pre_skip =
            u64::try_from(encoder.get_lookahead().map_err(opus_error)?).unwrap_or(0) * TO_48K;
        let mut writer = Self {
            encoder,
            pages: PacketWriter::new(Vec::new()),
            waiting: Vec::with_capacity(FRAME),
            samples: 0,
            pre_skip,
            packets: 0,
            held: None,
        };
        writer.write_headers()?;
        Ok(writer)
    }

    fn write_headers(&mut self) -> Result<()> {
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1); // version
        head.push(1); // one channel
        head.extend_from_slice(
            &u16::try_from(self.pre_skip)
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        head.extend_from_slice(&RATE.to_le_bytes()); // the rate it was recorded at
        head.extend_from_slice(&0_i16.to_le_bytes()); // no gain
        head.push(0); // one stream
        let vendor = b"Dirt";
        let mut tags = Vec::new();
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&u32::try_from(vendor.len()).unwrap_or(0).to_le_bytes());
        tags.extend_from_slice(vendor);
        tags.extend_from_slice(&0_u32.to_le_bytes()); // no comments
        self.pages
            .write_packet(head, SERIAL, PacketWriteEndInfo::EndPage, 0)
            .map_err(io_error)?;
        self.pages
            .write_packet(tags, SERIAL, PacketWriteEndInfo::EndPage, 0)
            .map_err(io_error)?;
        Ok(())
    }

    /// Takes more sound.
    pub fn push(&mut self, samples: &[f32]) -> Result<()> {
        self.samples += samples.len() as u64;
        for &sample in samples {
            self.waiting.push(sample);
            if self.waiting.len() == FRAME {
                let frame = std::mem::take(&mut self.waiting);
                self.encode(&frame)?;
                self.waiting = frame;
                self.waiting.clear();
            }
        }
        Ok(())
    }

    fn encode(&mut self, frame: &[f32]) -> Result<()> {
        let mut packet = vec![0_u8; 4000];
        let length = self
            .encoder
            .encode_float(frame, &mut packet)
            .map_err(opus_error)?;
        packet.truncate(length);
        self.packets += 1;
        let position = self.pre_skip + self.packets * FRAME as u64 * TO_48K;
        if let Some((held, at)) = self.held.replace((packet, position)) {
            // A page a second.
            let end = if self.packets.is_multiple_of(50) {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            self.pages
                .write_packet(held, SERIAL, end, at)
                .map_err(io_error)?;
        }
        Ok(())
    }

    /// How long the sound taken so far lasts, in milliseconds.
    #[must_use]
    pub const fn duration_ms(&self) -> u64 {
        self.samples * 1000 / RATE as u64
    }

    /// The file, ending where the sound ends.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        // The encoder gives its sound back late by its look-ahead: quiet
        // after the end brings the last of it out. The last page's
        // position says where the sound ends, so none of it is played.
        let ahead = usize::try_from(self.pre_skip / TO_48K).unwrap_or(0);
        let mut tail = std::mem::take(&mut self.waiting);
        tail.resize(tail.len() + ahead, 0.0);
        for frame in tail.chunks(FRAME) {
            let mut frame = frame.to_vec();
            frame.resize(FRAME, 0.0);
            self.encode(&frame)?;
        }
        if let Some((held, _)) = self.held.take() {
            // The last page says where the sound really ends, so the
            // padding of the last packet is not played.
            let end = self.pre_skip + self.samples * TO_48K;
            self.pages
                .write_packet(held, SERIAL, PacketWriteEndInfo::EndStream, end)
                .map_err(io_error)?;
        }
        Ok(self.pages.into_inner())
    }
}

/// Writes `samples` as a recording, all at once.
pub fn encode(samples: &[f32]) -> Result<Vec<u8>> {
    let mut writer = OpusWriter::new()?;
    writer.push(samples)?;
    writer.finish()
}

/// The sound of a recording, at 16 kHz, as it was written.
pub fn decode(file: &[u8]) -> Result<Vec<f32>> {
    let bad = |why: &str| VoiceError::Recording(why.to_owned());
    let mut reader = PacketReader::new(std::io::Cursor::new(file));
    let head = reader
        .read_packet()
        .map_err(|error| VoiceError::Recording(error.to_string()))?
        .ok_or_else(|| bad("the file is empty"))?;
    let data = &head.data;
    if data.len() < 19 || &data[..8] != b"OpusHead" {
        return Err(bad("the file is not Ogg Opus"));
    }
    let channels = data[9];
    let pre_skip = u64::from(u16::from_le_bytes([data[10], data[11]]));
    let channels = match channels {
        1 => Channels::Mono,
        2 => Channels::Stereo,
        _ => return Err(bad("the file has more channels than one stream holds")),
    };
    let stereo = matches!(channels, Channels::Stereo);
    let mut decoder = Decoder::new(RATE, channels).map_err(opus_error)?;
    let mut sound = Vec::new();
    let mut end = None;
    // 120 ms, the longest packet, in both channels.
    let mut buffer = vec![0.0_f32; RATE_USIZE / 1000 * 120 * 2];
    let mut first = true;
    while let Some(packet) = reader
        .read_packet()
        .map_err(|error| VoiceError::Recording(error.to_string()))?
    {
        if first {
            // OpusTags.
            first = false;
            continue;
        }
        let decoded = decoder
            .decode_float(&packet.data, &mut buffer, false)
            .map_err(opus_error)?;
        if stereo {
            sound.extend(
                buffer[..decoded * 2]
                    .chunks(2)
                    .map(|pair| f32::midpoint(pair[0], pair[1])),
            );
        } else {
            sound.extend_from_slice(&buffer[..decoded]);
        }
        if packet.last_in_stream() {
            end = Some(packet.absgp_page());
        }
    }
    let skip = usize::try_from(pre_skip / TO_48K)
        .unwrap_or(0)
        .min(sound.len());
    sound.drain(..skip);
    if let Some(end) = end {
        let length = usize::try_from(end.saturating_sub(pre_skip) / TO_48K).unwrap_or(usize::MAX);
        sound.truncate(length);
    }
    Ok(sound)
}

/// The sound as a 16-bit WAV file, in memory: for a web view that cannot
/// play Ogg Opus. Nothing is written to disk.
#[must_use]
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let mut file = Vec::with_capacity(44 + samples.len() * 2);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16_u32.to_le_bytes());
    file.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    file.extend_from_slice(&1_u16.to_le_bytes()); // one channel
    file.extend_from_slice(&RATE.to_le_bytes());
    file.extend_from_slice(&(RATE * 2).to_le_bytes());
    file.extend_from_slice(&2_u16.to_le_bytes());
    file.extend_from_slice(&16_u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        #[allow(clippy::cast_possible_truncation)]
        let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        file.extend_from_slice(&value.to_le_bytes());
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone that rises and falls, the same every run.
    fn tone(seconds: f32) -> Vec<f32> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (seconds * 16_000.0) as usize;
        (0..count)
            .map(|index| {
                #[allow(clippy::cast_precision_loss)]
                let t = index as f32 / 16_000.0;
                0.3 * (2.0 * std::f32::consts::PI * 440.0 * t).sin() * (t * 3.0).sin().abs()
            })
            .collect()
    }

    #[test]
    fn a_recording_comes_back_as_long_as_it_went_in() {
        for seconds in [0.013, 1.0, 2.37] {
            let sound = tone(seconds);
            let file = encode(&sound).unwrap();
            let back = decode(&file).unwrap();
            assert_eq!(back.len(), sound.len(), "{seconds} s");
        }
    }

    #[test]
    fn written_in_pieces_it_is_the_same_file() {
        let sound = tone(1.3);
        let whole = encode(&sound).unwrap();
        let mut writer = OpusWriter::new().unwrap();
        for piece in sound.chunks(777) {
            writer.push(piece).unwrap();
        }
        assert_eq!(writer.duration_ms(), 1300);
        assert_eq!(writer.finish().unwrap(), whole);
    }

    #[test]
    fn ten_minutes_fit_well_inside_a_file() {
        // Speech-like sound costs about what speech does; this checks
        // the order of size, not the exact figure trial 1 measured.
        let minute = tone(60.0);
        let file = encode(&minute).unwrap();
        assert!(
            file.len() * 10 < 3 * 1024 * 1024,
            "{} bytes a minute",
            file.len()
        );
        assert_eq!(&file[..4], b"OggS");
    }

    #[test]
    fn a_wav_has_its_header_and_every_sample() {
        let file = wav(&[0.0, 1.0, -1.0]);
        assert_eq!(&file[..4], b"RIFF");
        assert_eq!(file.len(), 44 + 6);
        assert_eq!(&file[44..], &[0, 0, 0xff, 0x7f, 0x01, 0x80]);
    }

    #[test]
    fn what_is_not_a_recording_is_refused() {
        assert!(decode(b"not a recording").is_err());
        assert!(decode(&[]).is_err());
    }
}
