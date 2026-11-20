//! The desktop's microphone, through cpal: the system's default input, at
//! whatever rate and channels it gives, brought to 16 kHz mono and handed
//! on a tenth of a second at a time or so.
//!
//! The system's sound thread only copies what it is given into a channel;
//! the rest is done on a thread of this crate's, so that a slow moment
//! here never makes the system drop sound. That thread waits on the
//! channel and nothing else: stopping sends it a message. It closes the
//! microphone first and then hands over what was still in the channel, so
//! the last moment before ✓ is kept.

/// What the microphone's thread is told.
enum Sound {
    Heard(Vec<f32>),
    Stop,
}

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

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
    let (opened, started) = mpsc::channel::<Result<()>>();
    let (chunks, heard) = mpsc::channel::<Sound>();
    let stopper = chunks.clone();
    let thread = std::thread::Builder::new()
        .name("dirt-voice-microphone".to_owned())
        .spawn(move || {
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
                            let _ = chunks.send(Sound::Heard(data.to_vec()));
                        },
                        failed,
                        None,
                    ),
                    SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
                        config,
                        move |data, _| {
                            let _ = chunks.send(Sound::Heard(
                                data.iter().map(|&s| f32::from(s) / 32768.0).collect(),
                            ));
                        },
                        failed,
                        None,
                    ),
                    SampleFormat::U16 => device.build_input_stream::<u16, _, _>(
                        config,
                        move |data, _| {
                            let _ = chunks.send(Sound::Heard(
                                data.iter()
                                    .map(|&s| (f32::from(s) - 32768.0) / 32768.0)
                                    .collect(),
                            ));
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
            let mut hand_on = |chunk: &[f32]| {
                let sound = to16k.convert(chunk);
                if !sound.is_empty() {
                    sink(&sound);
                }
            };
            while let Ok(Sound::Heard(chunk)) = heard.recv() {
                hand_on(&chunk);
            }
            // Closed, the microphone sends nothing more; what it sent
            // before is still kept.
            drop(stream);
            while let Ok(sound) = heard.try_recv() {
                if let Sound::Heard(chunk) = sound {
                    hand_on(&chunk);
                }
            }
            let last = to16k.finish();
            if !last.is_empty() {
                sink(&last);
            }
        })
        .map_err(|error| VoiceError::Microphone(error.to_string()))?;
    match started.recv() {
        Ok(Ok(())) => Ok(Feed::new(stop, thread).woken_by(move || {
            let _ = stopper.send(Sound::Stop);
        })),
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
