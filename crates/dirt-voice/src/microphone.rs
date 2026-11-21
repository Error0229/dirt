//! The desktop's microphone, through cpal, brought to 16 kHz mono.
//!
//! It is the input the person chose in Settings, or the system's default,
//! at whatever rate and channels it gives, handed on a tenth of a second at
//! a time or so. A chosen input that is not connected, or that cannot be
//! set up or started, gives way to the default ([`unusable`] says when it
//! could not record).
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
use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceDirection, ErrorKind, SampleFormat};

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

/// An input the system offers: the ID it is chosen and kept by, which the
/// system usually keeps across restarts, and the name it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    /// The system's ID for it, as text.
    pub id: String,
    /// Its name, as the system shows it.
    pub name: String,
}

/// Whether an input the system lists is offered. Elsewhere every input is.
/// ALSA (Linux) lists every PCM it knows, most of them with no direction
/// (`null`, which records silence, the sound server's plugins, several
/// names for each card), and many of those cannot record. So there an input
/// is offered when ALSA marks it as one, or when it is a card's own
/// device that cpal found able to capture (`plughw:CARD=0,DEV=0`, which
/// converts to what the app asks for); `null` never is. The system's
/// default is offered apart, first.
fn offered(host: &str, pcm: &str, direction: DeviceDirection) -> bool {
    if host != "alsa" {
        return true;
    }
    let numbers = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let probed_card = pcm
        .strip_prefix("plughw:CARD=")
        .and_then(|rest| rest.split_once(",DEV="))
        .is_some_and(|(card, device)| numbers(card) && numbers(device));
    pcm != "null"
        && match direction {
            DeviceDirection::Input => true,
            DeviceDirection::Duplex => probed_card,
            _ => false,
        }
}

/// The inputs connected now, in the system's order. Nothing when the
/// system lists none, or cannot be asked. It takes a moment: call it off
/// the window's thread.
#[must_use]
pub fn inputs() -> Vec<Input> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?;
            let about = device.description().ok();
            let direction = about
                .as_ref()
                .map_or(DeviceDirection::Duplex, cpal::DeviceDescription::direction);
            if !offered(&id.host().to_string(), id.id(), direction) {
                return None;
            }
            let name = about.map_or_else(|| device.to_string(), |about| about.name().to_owned());
            Some(Input {
                id: id.to_string(),
                name,
            })
        })
        .collect()
}

/// The chosen input that could not record, the last time it was asked
/// for, while the default could: the ID Settings shows it by.
static UNUSABLE: Mutex<Option<String>> = Mutex::new(None);

/// The ID of the chosen input if it could not be set up or started the
/// last time it was asked for, and the default recorded in its place.
/// Nothing once it has recorded again, or if the default failed too.
#[must_use]
pub fn unusable() -> Option<String> {
    UNUSABLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn note_unusable(id: Option<String>) {
    *UNUSABLE.lock().unwrap_or_else(PoisonError::into_inner) = id;
}

/// The input `chosen` names, while it is connected.
fn chosen_input(chosen: &str) -> Option<cpal::Device> {
    let found = chosen
        .parse::<cpal::DeviceId>()
        .ok()
        .and_then(|id| cpal::default_host().device_by_id(&id))
        .filter(DeviceTrait::supports_input);
    if found.is_none() {
        tracing::info!("The chosen microphone is not connected: the default is used");
    }
    found
}

/// Sets `device` up at its own rate and channels and starts it, its sound
/// going to `chunks`.
fn start(
    device: &cpal::Device,
    folder: &VoiceFolder,
    chunks: &mpsc::Sender<Sound>,
) -> Result<(cpal::Stream, To16k)> {
    let supported = device
        .default_input_config()
        .map_err(|error| refused_or_failed(&error))?;
    let format = supported.sample_format();
    let config = supported.config();
    let to16k = To16k::new(folder, config.sample_rate, config.channels)?;
    let failed = |error: cpal::Error| tracing::warn!("The microphone failed: {error}");
    let chunks = chunks.clone();
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
}

/// Starts the input `chosen` names, and if it is not connected, or cannot
/// be set up or started, the system's default once in its place.
fn start_chosen(
    chosen: Option<&str>,
    folder: &VoiceFolder,
    chunks: &mpsc::Sender<Sound>,
) -> Result<(cpal::Stream, To16k)> {
    let default = || {
        cpal::default_host()
            .default_input_device()
            .ok_or(VoiceError::NoMicrophone)
            .and_then(|device| start(&device, folder, chunks))
    };
    let Some(id) = chosen else {
        return default();
    };
    let Some(device) = chosen_input(id) else {
        return default();
    };
    match start(&device, folder, chunks) {
        Ok(started) => {
            note_unusable(None);
            Ok(started)
        }
        Err(error) => {
            tracing::warn!("The chosen microphone could not record ({error}): the default is used");
            let started = default()?;
            note_unusable(Some(id.to_owned()));
            Ok(started)
        }
    }
}

/// Opens the microphone `chosen` names, or the default one, and hands its
/// sound to `sink`.
///
/// The sound is at 16 kHz mono, until the feed is stopped or dropped. It
/// fails at once, before anything is heard, when there is no microphone or
/// it may not be used.
pub fn open(
    folder: &VoiceFolder,
    chosen: Option<String>,
    mut sink: impl FnMut(&[f32]) + Send + 'static,
) -> Result<Feed> {
    let folder = folder.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let (opened, started) = mpsc::channel::<Result<()>>();
    let (chunks, heard) = mpsc::channel::<Sound>();
    let stopper = chunks.clone();
    let thread = std::thread::Builder::new()
        .name("dirt-voice-microphone".to_owned())
        .spawn(move || {
            let made = start_chosen(chosen.as_deref(), &folder, &chunks);
            // Only the stream holds a sender now: closed, it ends the wait
            // below once the stop has come.
            drop(chunks);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alsa_offers_only_what_can_record() {
        // Marked as an input by ALSA, and a card found able to capture.
        assert!(offered(
            "alsa",
            "dsnoop:CARD=PCH,DEV=0",
            DeviceDirection::Input
        ));
        assert!(offered(
            "alsa",
            "plughw:CARD=1,DEV=0",
            DeviceDirection::Duplex
        ));
        assert!(offered(
            "alsa",
            "plughw:CARD=0,DEV=3",
            DeviceDirection::Input
        ));
        // Silence, and names with no direction of their own.
        assert!(!offered("alsa", "null", DeviceDirection::Duplex));
        assert!(!offered("alsa", "null", DeviceDirection::Input));
        assert!(!offered("alsa", "pipewire", DeviceDirection::Duplex));
        assert!(!offered(
            "alsa",
            "sysdefault:CARD=PCH",
            DeviceDirection::Duplex
        ));
        assert!(!offered(
            "alsa",
            "plughw:CARD=PCH,DEV=0",
            DeviceDirection::Duplex
        ));
        assert!(!offered("alsa", "hw:CARD=1,DEV=0", DeviceDirection::Duplex));
        // A card that only plays.
        assert!(!offered(
            "alsa",
            "plughw:CARD=0,DEV=3",
            DeviceDirection::Output
        ));
        // Elsewhere the system lists inputs only.
        assert!(offered(
            "wasapi",
            "{0.0.1.00000000}.{a}",
            DeviceDirection::Duplex
        ));
        assert!(offered(
            "coreaudio",
            "BuiltInMicrophoneDevice",
            DeviceDirection::Unknown
        ));
    }
}
