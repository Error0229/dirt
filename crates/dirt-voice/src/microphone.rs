//! The desktop's microphone, through cpal: the system's default input, at
//! whatever rate and channels it gives, brought to 16 kHz mono and handed
//! on a tenth of a second at a time or so.
//!
//! The system's sound thread only copies what it is given into a channel;
//! the rest is done on a thread of this crate's, so that a slow moment
//! here never makes the system drop sound.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, SampleFormat};

use crate::folder::VoiceFolder;
use crate::stand_in::{Feed, To16k};
use crate::{Result, VoiceError};

/// What the system said, as one of this crate's errors. Windows' "access
/// denied" (`0x80070005`) is the microphone turned off for desktop apps in
/// its privacy settings.
fn refused_or_failed(error: &cpal::Error) -> VoiceError {
    let text = error.to_string();
    let denied = error.kind() == ErrorKind::PermissionDenied
        || text.contains("0x80070005")
        || text.to_lowercase().contains("access is denied");
    match error.kind() {
        _ if denied => VoiceError::Refused(text),
        ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => VoiceError::NoMicrophone,
        _ => VoiceError::Microphone(text),
    }
}

/// Opens the default microphone and hands its sound to `sink`.
///
/// The sound is at 16 kHz mono, until the feed is stopped or dropped. It
/// fails at once, before anything is heard, when there is no microphone or
/// it may not be used.
pub fn open(folder: &VoiceFolder, mut sink: impl FnMut(&[f32]) + Send + 'static) -> Result<Feed> {
    let folder = folder.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let (opened, started) = mpsc::channel::<Result<()>>();
    let thread = std::thread::Builder::new()
        .name("dirt-voice-microphone".to_owned())
        .spawn(move || {
            let (chunks, heard) = mpsc::channel::<Vec<f32>>();
            let made = (|| {
                let device = cpal::default_host()
                    .default_input_device()
                    .ok_or(VoiceError::NoMicrophone)?;
                let supported = device
                    .default_input_config()
                    .map_err(|error| refused_or_failed(&error))?;
                let format = supported.sample_format();
                let config = supported.config();
                let to16k = To16k::new(&folder, config.sample_rate, config.channels)?;
                let failed = |error: cpal::Error| tracing::warn!("The microphone failed: {error}");
                let stream = match format {
                    SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
                        config,
                        move |data, _| {
                            let _ = chunks.send(data.to_vec());
                        },
                        failed,
                        None,
                    ),
                    SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
                        config,
                        move |data, _| {
                            let _ =
                                chunks.send(data.iter().map(|&s| f32::from(s) / 32768.0).collect());
                        },
                        failed,
                        None,
                    ),
                    SampleFormat::U16 => device.build_input_stream::<u16, _, _>(
                        config,
                        move |data, _| {
                            let _ = chunks.send(
                                data.iter()
                                    .map(|&s| (f32::from(s) - 32768.0) / 32768.0)
                                    .collect(),
                            );
                        },
                        failed,
                        None,
                    ),
                    other => {
                        return Err(VoiceError::Microphone(format!(
                            "the microphone gives {other:?} samples"
                        )))
                    }
                }
                .map_err(|error| refused_or_failed(&error))?;
                stream.play().map_err(|error| refused_or_failed(&error))?;
                Ok((stream, to16k))
            })();
            let (stream, to16k) = match made {
                Ok(made) => {
                    let _ = opened.send(Ok(()));
                    made
                }
                Err(error) => {
                    let _ = opened.send(Err(error));
                    return;
                }
            };
            while !stopped.load(Ordering::SeqCst) {
                match heard.recv_timeout(Duration::from_millis(50)) {
                    Ok(chunk) => {
                        let sound = to16k.convert(&chunk);
                        if !sound.is_empty() {
                            sink(&sound);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            drop(stream);
        })
        .map_err(|error| VoiceError::Microphone(error.to_string()))?;
    match started.recv() {
        Ok(Ok(())) => Ok(Feed::new(stop, thread)),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            let _ = thread.join();
            Err(VoiceError::Microphone(
                "the microphone's thread ended".to_owned(),
            ))
        }
    }
}
