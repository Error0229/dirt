//! Voice memo recording helpers for the mobile app.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::Deserialize;

#[cfg(target_os = "android")]
use dioxus::document;

const START_RECORDING_SCRIPT: &str = r#"
(() => {
    const state = window.__dirtVoiceMemoRecorder;
    if (state && state.recorder && state.recorder.state !== "inactive") {
        return { ok: false, error: "Voice memo recorder is already running." };
    }
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        return { ok: false, error: "Microphone capture is unavailable in this runtime." };
    }
    return (async () => {
        try {
            const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
            const preferredTypes = [
                "audio/webm;codecs=opus",
                "audio/webm",
                "audio/mp4",
                "audio/ogg;codecs=opus",
                "audio/ogg",
                "audio/wav",
            ];

            let mimeType = "";
            for (const candidate of preferredTypes) {
                if (typeof MediaRecorder !== "undefined" && MediaRecorder.isTypeSupported(candidate)) {
                    mimeType = candidate;
                    break;
                }
            }

            const recorder = mimeType
                ? new MediaRecorder(stream, { mimeType })
                : new MediaRecorder(stream);
            const chunks = [];

            recorder.ondataavailable = (event) => {
                if (event.data && event.data.size > 0) {
                    chunks.push(event.data);
                }
            };
            recorder.start(250);

            window.__dirtVoiceMemoRecorder = {
                recorder,
                stream,
                chunks,
                mimeType: mimeType || recorder.mimeType || "audio/webm",
                startedAtMs: Date.now(),
            };

            return { ok: true };
        } catch (error) {
            return {
                ok: false,
                error: error && error.message ? error.message : String(error),
            };
        }
    })();
})()
"#;

const STOP_RECORDING_SCRIPT: &str = r#"
(() => {
    const state = window.__dirtVoiceMemoRecorder;
    if (!state || !state.recorder) {
        return { ok: false, error: "No active voice memo recording." };
    }

    const recorder = state.recorder;
    const stream = state.stream;
    const startedAtMs = state.startedAtMs || Date.now();
    const chunks = Array.isArray(state.chunks) ? state.chunks : [];
    const mimeType = state.mimeType || recorder.mimeType || "audio/webm";

    return (async () => {
        try {
            if (recorder.state !== "inactive") {
                await new Promise((resolve, reject) => {
                    recorder.addEventListener("stop", () => resolve(), { once: true });
                    recorder.addEventListener(
                        "error",
                        (event) => {
                            reject(event.error || new Error("Recorder stop failed"));
                        },
                        { once: true }
                    );
                    recorder.stop();
                });
            }

            if (stream && stream.getTracks) {
                for (const track of stream.getTracks()) {
                    track.stop();
                }
            }

            const blob = new Blob(chunks, { type: mimeType });
            const buffer = await blob.arrayBuffer();
            const bytes = new Uint8Array(buffer);

            let binary = "";
            const CHUNK = 0x8000;
            for (let i = 0; i < bytes.length; i += CHUNK) {
                const sub = bytes.subarray(i, i + CHUNK);
                binary += String.fromCharCode.apply(null, sub);
            }

            const encoded = btoa(binary);
            const durationMs = Math.max(0, Date.now() - startedAtMs);

            window.__dirtVoiceMemoRecorder = null;
            return {
                ok: true,
                base64: encoded,
                mimeType: blob.type || mimeType || "audio/webm",
                durationMs,
            };
        } catch (error) {
            if (stream && stream.getTracks) {
                for (const track of stream.getTracks()) {
                    track.stop();
                }
            }
            window.__dirtVoiceMemoRecorder = null;
            return {
                ok: false,
                error: error && error.message ? error.message : String(error),
            };
        }
    })();
})()
"#;

const DISCARD_RECORDING_SCRIPT: &str = r#"
(() => {
    const state = window.__dirtVoiceMemoRecorder;
    if (!state || !state.recorder) {
        window.__dirtVoiceMemoRecorder = null;
        return { ok: true };
    }

    try {
        if (state.recorder.state !== "inactive") {
            state.recorder.stop();
        }
        if (state.stream && state.stream.getTracks) {
            for (const track of state.stream.getTracks()) {
                track.stop();
            }
        }
        window.__dirtVoiceMemoRecorder = null;
        return { ok: true };
    } catch (error) {
        window.__dirtVoiceMemoRecorder = null;
        return {
            ok: false,
            error: error && error.message ? error.message : String(error),
        };
    }
})()
"#;

/// Recorder control state for voice memo UX.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VoiceMemoRecorderState {
    /// Recorder is idle and ready for a new capture.
    #[default]
    Idle,
    /// Recorder start has been requested and is awaiting microphone initialization.
    Starting,
    /// Recorder is actively capturing microphone input.
    Recording,
    /// Recorder stop has been requested and payload is being finalized.
    Stopping,
}

/// Discrete state-machine events for recorder transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMemoRecorderEvent {
    StartRequested,
    StartSucceeded,
    StartFailed,
    StopRequested,
    StopSucceeded,
    StopFailed,
    DiscardRequested,
}

/// Deterministic recorder state transition helper.
#[must_use]
pub const fn transition_voice_memo_state(
    state: VoiceMemoRecorderState,
    event: VoiceMemoRecorderEvent,
) -> VoiceMemoRecorderState {
    match (state, event) {
        (VoiceMemoRecorderState::Idle, VoiceMemoRecorderEvent::StartRequested) => {
            VoiceMemoRecorderState::Starting
        }
        (VoiceMemoRecorderState::Starting, VoiceMemoRecorderEvent::StartSucceeded) => {
            VoiceMemoRecorderState::Recording
        }
        (VoiceMemoRecorderState::Starting, VoiceMemoRecorderEvent::StartFailed)
        | (
            VoiceMemoRecorderState::Stopping,
            VoiceMemoRecorderEvent::StopSucceeded | VoiceMemoRecorderEvent::StopFailed,
        )
        | (_, VoiceMemoRecorderEvent::DiscardRequested) => VoiceMemoRecorderState::Idle,
        (VoiceMemoRecorderState::Recording, VoiceMemoRecorderEvent::StopRequested) => {
            VoiceMemoRecorderState::Stopping
        }
        _ => state,
    }
}

/// Completed voice memo capture payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedVoiceMemo {
    /// Suggested file name for attachment metadata.
    pub file_name: String,
    /// MIME type produced by the recorder.
    pub mime_type: String,
    /// Recorded audio bytes.
    pub bytes: Vec<u8>,
    /// Duration reported by the recorder.
    pub duration_ms: u64,
    /// Local temp file path where bytes were persisted.
    pub temp_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RecorderResult {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StopRecorderResult {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default, rename = "mimeType")]
    mime_type: Option<String>,
    #[serde(default)]
    base64: Option<String>,
    #[serde(default, rename = "durationMs")]
    duration_ms: Option<u64>,
}

/// Start a microphone recording session.
#[cfg(target_os = "android")]
pub async fn start_voice_memo_recording() -> Result<(), String> {
    let result: RecorderResult = document::eval(START_RECORDING_SCRIPT)
        .join()
        .await
        .map_err(|error| format!("Failed to start voice memo recorder: {error}"))?;
    parse_recorder_result(result)
}

/// Stop recording and return captured voice memo bytes.
#[cfg(target_os = "android")]
pub async fn stop_voice_memo_recording() -> Result<RecordedVoiceMemo, String> {
    let result: StopRecorderResult = document::eval(STOP_RECORDING_SCRIPT)
        .join()
        .await
        .map_err(|error| format!("Failed to stop voice memo recorder: {error}"))?;
    parse_stop_result(result)
}

/// Discard the active recording session.
#[cfg(target_os = "android")]
pub async fn discard_voice_memo_recording() -> Result<(), String> {
    let result: RecorderResult = document::eval(DISCARD_RECORDING_SCRIPT)
        .join()
        .await
        .map_err(|error| format!("Failed to discard voice memo recorder: {error}"))?;
    parse_recorder_result(result)
}

/// Start a microphone recording session.
#[cfg(not(target_os = "android"))]
pub async fn start_voice_memo_recording() -> Result<(), String> {
    std::future::ready(()).await;
    Err("Voice memo recording is only available on Android builds.".to_string())
}

/// Stop recording and return captured voice memo bytes.
#[cfg(not(target_os = "android"))]
pub async fn stop_voice_memo_recording() -> Result<RecordedVoiceMemo, String> {
    std::future::ready(()).await;
    Err("Voice memo recording is only available on Android builds.".to_string())
}

/// Discard the active recording session.
#[cfg(not(target_os = "android"))]
pub async fn discard_voice_memo_recording() -> Result<(), String> {
    std::future::ready(()).await;
    Err("Voice memo recording is only available on Android builds.".to_string())
}

/// Remove a temp voice memo file if it exists.
pub fn cleanup_temp_voice_memo(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        tracing::debug!("Failed to delete temp voice memo {:?}: {}", path, error);
    }
}

fn parse_recorder_result(result: RecorderResult) -> Result<(), String> {
    if result.ok {
        Ok(())
    } else {
        Err(result
            .error
            .unwrap_or_else(|| "Voice memo operation failed.".to_string()))
    }
}

fn parse_stop_result(result: StopRecorderResult) -> Result<RecordedVoiceMemo, String> {
    if !result.ok {
        return Err(result
            .error
            .unwrap_or_else(|| "Voice memo recorder did not return audio data.".to_string()));
    }

    let encoded = result.base64.ok_or_else(|| {
        "Voice memo recorder returned no audio payload. Check microphone permissions.".to_string()
    })?;
    let bytes = BASE64_STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| format!("Failed to decode recorded voice memo bytes: {error}"))?;
    if bytes.is_empty() {
        return Err("Recorded voice memo is empty.".to_string());
    }

    let mime_type = result
        .mime_type
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "audio/webm".to_string());
    let file_name = build_voice_memo_file_name(&mime_type);
    let temp_path = persist_temp_voice_memo(&file_name, &bytes)?;

    Ok(RecordedVoiceMemo {
        file_name,
        mime_type,
        bytes,
        duration_ms: result.duration_ms.unwrap_or(0),
        temp_path,
    })
}

fn build_voice_memo_file_name(mime_type: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_millis());
    let extension = voice_memo_extension(mime_type);
    format!("voice-memo-{timestamp}.{extension}")
}

fn persist_temp_voice_memo(file_name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir().join("dirt").join("voice-memos");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|error| format!("Failed to create temp voice memo directory: {error}"))?;

    let path = temp_dir.join(file_name);
    std::fs::write(&path, bytes)
        .map_err(|error| format!("Failed to persist temp voice memo file: {error}"))?;
    Ok(path)
}

fn voice_memo_extension(mime_type: &str) -> &'static str {
    let normalized = mime_type.trim().to_ascii_lowercase();

    if normalized.contains("wav") {
        "wav"
    } else if normalized.contains("mpeg") || normalized.contains("mp3") {
        "mp3"
    } else if normalized.contains("ogg") {
        "ogg"
    } else if normalized.contains("mp4") || normalized.contains("m4a") || normalized.contains("aac")
    {
        "m4a"
    } else {
        "webm"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_state_machine_covers_start_stop_and_discard() {
        let state = transition_voice_memo_state(
            VoiceMemoRecorderState::Idle,
            VoiceMemoRecorderEvent::StartRequested,
        );
        assert_eq!(state, VoiceMemoRecorderState::Starting);

        let state = transition_voice_memo_state(state, VoiceMemoRecorderEvent::StartSucceeded);
        assert_eq!(state, VoiceMemoRecorderState::Recording);

        let state = transition_voice_memo_state(state, VoiceMemoRecorderEvent::StopRequested);
        assert_eq!(state, VoiceMemoRecorderState::Stopping);

        let state = transition_voice_memo_state(state, VoiceMemoRecorderEvent::StopSucceeded);
        assert_eq!(state, VoiceMemoRecorderState::Idle);

        let state = transition_voice_memo_state(
            VoiceMemoRecorderState::Recording,
            VoiceMemoRecorderEvent::DiscardRequested,
        );
        assert_eq!(state, VoiceMemoRecorderState::Idle);
    }

    #[test]
    fn recorder_state_machine_handles_errors() {
        let state = transition_voice_memo_state(
            VoiceMemoRecorderState::Starting,
            VoiceMemoRecorderEvent::StartFailed,
        );
        assert_eq!(state, VoiceMemoRecorderState::Idle);

        let state = transition_voice_memo_state(
            VoiceMemoRecorderState::Stopping,
            VoiceMemoRecorderEvent::StopFailed,
        );
        assert_eq!(state, VoiceMemoRecorderState::Idle);
    }

    #[test]
    fn parse_stop_result_rejects_empty_payload() {
        let error = parse_stop_result(StopRecorderResult {
            ok: true,
            error: None,
            mime_type: Some("audio/webm".to_string()),
            base64: None,
            duration_ms: Some(100),
        })
        .unwrap_err();

        assert!(error.contains("no audio payload"));
    }

    #[test]
    fn file_name_extension_matches_mime_type() {
        assert_has_extension(build_voice_memo_file_name("audio/wav").as_str(), "wav");
        assert_has_extension(build_voice_memo_file_name("audio/mpeg").as_str(), "mp3");
        assert_has_extension(build_voice_memo_file_name("audio/ogg").as_str(), "ogg");
        assert_has_extension(build_voice_memo_file_name("audio/mp4").as_str(), "m4a");
        assert_has_extension(build_voice_memo_file_name("audio/webm").as_str(), "webm");
    }

    fn assert_has_extension(file_name: &str, extension: &str) {
        let path = std::path::Path::new(file_name);
        let actual = path.extension().and_then(|value| value.to_str());
        assert!(actual.is_some_and(|value| value.eq_ignore_ascii_case(extension)));
    }
}
