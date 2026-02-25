use std::sync::Arc;
use std::time::Instant;

use dioxus::prelude::*;

use dirt_core::NoteId;

use crate::queries::invalidate_notes_query;
use crate::services::{DatabaseService, TranscriptionService};

#[derive(Clone)]
pub(super) struct VoiceMemoTranscriptionContext {
    pub current_note_id: Option<NoteId>,
    pub editor_content: String,
    pub on_editor_content_change: EventHandler<String>,
    pub upload_error: Signal<Option<String>>,
}

pub(super) async fn apply_voice_memo_transcription_if_enabled(
    note_id: NoteId,
    file_name: &str,
    mime_type: &str,
    audio_bytes: Vec<u8>,
    transcription_service: Option<Arc<TranscriptionService>>,
    db: Option<Arc<DatabaseService>>,
    mut ui: VoiceMemoTranscriptionContext,
) {
    let Some(transcription_service) = transcription_service else {
        return;
    };
    let Some(db) = db else {
        return;
    };

    let transcript = match transcribe_voice_memo(
        transcription_service.as_ref(),
        file_name,
        mime_type,
        audio_bytes,
    )
    .await
    {
        Ok(transcript) => transcript,
        Err(error) => {
            ui.upload_error.set(Some(error));
            return;
        }
    };

    let latest_editor_content = if ui.current_note_id == Some(note_id) {
        Some(ui.editor_content.clone())
    } else {
        None
    };

    match append_voice_memo_transcription_to_note(
        db.as_ref(),
        &note_id,
        file_name,
        &transcript,
        latest_editor_content,
    )
    .await
    {
        Ok(Some(updated_content)) => {
            if ui.current_note_id == Some(note_id) {
                ui.on_editor_content_change.call(updated_content);
            }
            invalidate_notes_query().await;
        }
        Ok(None) => {
            tracing::debug!("Voice memo transcription returned no content; skipping note update");
        }
        Err(error) => {
            ui.upload_error.set(Some(error));
        }
    }
}

async fn transcribe_voice_memo(
    transcription_service: &TranscriptionService,
    file_name: &str,
    mime_type: &str,
    audio_bytes: Vec<u8>,
) -> Result<String, String> {
    transcription_service
        .transcribe_audio_bytes(file_name, mime_type, audio_bytes)
        .await
        .map_err(|error| {
            tracing::warn!("Voice memo transcription failed: {}", error);
            format!("Voice memo uploaded, but transcription failed: {error}")
        })
}

async fn append_voice_memo_transcription_to_note(
    db: &DatabaseService,
    note_id: &NoteId,
    file_name: &str,
    transcript: &str,
    latest_editor_content: Option<String>,
) -> Result<Option<String>, String> {
    let existing_note = db
        .get_note(note_id)
        .await
        .map_err(|error| {
            tracing::warn!("Failed to load note for transcription append: {}", error);
            format!("Voice memo uploaded, but transcription could not update the note: {error}")
        })?
        .ok_or_else(|| "Voice memo uploaded, but note was no longer available.".to_string())?;

    let base_content = latest_editor_content.unwrap_or(existing_note.content);
    let Some(updated_content) =
        append_voice_memo_transcription(&base_content, file_name, transcript)
    else {
        return Ok(None);
    };

    let updated_note = db
        .update_note(note_id, &updated_content)
        .await
        .map_err(|error| {
            tracing::warn!(
                "Failed to save voice memo transcription into note: {}",
                error
            );
            format!("Voice memo uploaded, but transcription could not be saved: {error}")
        })?;

    Ok(Some(updated_note.content))
}

fn append_voice_memo_transcription(
    existing_content: &str,
    file_name: &str,
    transcription: &str,
) -> Option<String> {
    let normalized_transcription = transcription.trim();
    if normalized_transcription.is_empty() {
        return None;
    }

    let normalized_file_name = if file_name.trim().is_empty() {
        "voice memo"
    } else {
        file_name.trim()
    };
    let transcription_block =
        format!("[Voice memo transcript: {normalized_file_name}]\n{normalized_transcription}");
    let normalized_existing = existing_content.trim_end();

    if normalized_existing.is_empty() {
        Some(transcription_block)
    } else {
        Some(format!("{normalized_existing}\n\n{transcription_block}"))
    }
}

pub(super) fn format_recording_duration(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1_000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

pub(super) fn elapsed_millis_u64(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_recording_duration_as_minutes_and_seconds() {
        assert_eq!(format_recording_duration(0), "00:00");
        assert_eq!(format_recording_duration(12_345), "00:12");
        assert_eq!(format_recording_duration(120_000), "02:00");
    }

    #[test]
    fn appends_voice_memo_transcription_with_separator() {
        let updated = append_voice_memo_transcription(
            "Existing note body",
            "memo.webm",
            "  Hello from transcript.  ",
        )
        .unwrap();

        assert_eq!(
            updated,
            "Existing note body\n\n[Voice memo transcript: memo.webm]\nHello from transcript."
        );
    }

    #[test]
    fn ignores_empty_voice_memo_transcription() {
        assert!(
            append_voice_memo_transcription("Existing note body", "memo.webm", "   ").is_none()
        );
    }
}
