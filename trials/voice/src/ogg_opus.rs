//! The recording as a file: Opus in an Ogg container (RFC 7845), written
//! from the same 16 kHz mono samples the recogniser hears.

use std::path::Path;

use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Encoder};

use crate::audio::{read_wav, RATE};

/// Samples in one 20 ms packet.
const FRAME: usize = RATE / 50;
/// Ogg counts Opus time in 48 kHz samples whatever the input's rate.
const TO_48K: u64 = 48_000 / RATE as u64;

pub fn encode_file(wav: &Path, out: &Path, bitrate: i32) -> Result<(), String> {
    let samples = read_wav(wav)?;
    let bytes = encode(&samples, bitrate)?;
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{}: {:.1} s, {} bytes, {:.1} kbit/s; ten minutes would be {:.2} MiB",
        out.display(),
        samples.len() as f64 / RATE as f64,
        bytes.len(),
        bytes.len() as f64 * 8.0 / (samples.len() as f64 / RATE as f64) / 1000.0,
        bytes.len() as f64 / (samples.len() as f64 / RATE as f64) * 600.0 / 1_048_576.0
    );
    Ok(())
}

pub fn encode(samples: &[f32], bitrate: i32) -> Result<Vec<u8>, String> {
    let e = |error: opus::Error| error.to_string();
    let mut encoder = Encoder::new(RATE as u32, Channels::Mono, Application::Voip).map_err(e)?;
    encoder.set_bitrate(Bitrate::Bits(bitrate)).map_err(e)?;
    // What the decoder must throw away at the start, in 48 kHz samples.
    let pre_skip = encoder.get_lookahead().map_err(e)? as u64 * TO_48K;

    let mut file = Vec::new();
    let mut writer = PacketWriter::new(&mut file);
    let serial = 0x6469_7274;
    let io = |error: std::io::Error| error.to_string();

    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1); // version
    head.push(1); // channels
    head.extend_from_slice(&(pre_skip as u16).to_le_bytes());
    head.extend_from_slice(&(RATE as u32).to_le_bytes()); // the rate it was recorded at
    head.extend_from_slice(&0_i16.to_le_bytes()); // gain
    head.push(0); // one stream, mono or stereo
    writer
        .write_packet(head, serial, PacketWriteEndInfo::EndPage, 0)
        .map_err(io)?;

    let vendor = b"dirt voice trial";
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0_u32.to_le_bytes()); // no comments
    writer
        .write_packet(tags, serial, PacketWriteEndInfo::EndPage, 0)
        .map_err(io)?;

    let frames = samples.len().div_ceil(FRAME).max(1);
    let mut packet = vec![0_u8; 4000];
    for index in 0..frames {
        let start = index * FRAME;
        let mut frame = [0.0_f32; FRAME];
        let have = samples.len().saturating_sub(start).min(FRAME);
        frame[..have].copy_from_slice(&samples[start..start + have]);
        let length = encoder.encode_float(&frame, &mut packet).map_err(e)?;
        let last = index + 1 == frames;
        // A page a second; the last page says where the sound really ends.
        let (end, position) = if last {
            (
                PacketWriteEndInfo::EndStream,
                pre_skip + samples.len() as u64 * TO_48K,
            )
        } else {
            let end = if index % 50 == 49 {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            (end, pre_skip + (start + FRAME) as u64 * TO_48K)
        };
        writer
            .write_packet(packet[..length].to_vec(), serial, end, position)
            .map_err(io)?;
    }
    drop(writer);
    Ok(file)
}
