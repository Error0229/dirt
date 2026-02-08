//! Optional audio transcription service foundation.
#![allow(dead_code)] // Foundation module; full wiring lands in follow-up issue.

use reqwest::{multipart, Client, Request, StatusCode};
use serde::Deserialize;
use thiserror::Error;

const ENV_OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const ENV_OPENAI_TRANSCRIPTION_MODEL: &str = "OPENAI_TRANSCRIPTION_MODEL";
const ENV_OPENAI_BASE_URL: &str = "OPENAI_BASE_URL";

const DEFAULT_MODEL: &str = "gpt-4o-mini-transcribe";
const DEFAULT_BASE_URL: &str = "https://api.openai.com";

#[derive(Clone, Debug, PartialEq, Eq)]
enum TranscriptionMode {
    Disabled,
    OpenAi {
        base_url: String,
        api_key: String,
        model: String,
    },
}

/// Basic configuration status for transcription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptionConfigStatus {
    pub enabled: bool,
    pub provider: &'static str,
    pub model: Option<String>,
}

/// Errors from transcription service setup and requests.
#[derive(Debug, Error)]
pub enum TranscriptionError {
    #[error("Transcription is not configured. Set OPENAI_API_KEY to enable it.")]
    NotConfigured,
    #[error("Invalid transcription configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Transcription API error: {0}")]
    Api(String),
}

type TranscriptionResult<T> = Result<T, TranscriptionError>;

#[derive(Clone)]
pub struct TranscriptionService {
    client: Client,
    mode: TranscriptionMode,
}

impl TranscriptionService {
    /// Build transcription service from environment.
    pub fn new_from_env() -> TranscriptionResult<Self> {
        let api_key = std::env::var(ENV_OPENAI_API_KEY)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let mode = if let Some(api_key) = api_key {
            let base_url = std::env::var(ENV_OPENAI_BASE_URL)
                .ok()
                .map(|value| value.trim().trim_end_matches('/').to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

            if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
                return Err(TranscriptionError::InvalidConfiguration(
                    "OPENAI_BASE_URL must start with http:// or https://",
                ));
            }

            let model = std::env::var(ENV_OPENAI_TRANSCRIPTION_MODEL)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| DEFAULT_MODEL.to_string());

            TranscriptionMode::OpenAi {
                base_url,
                api_key,
                model,
            }
        } else {
            TranscriptionMode::Disabled
        };

        Ok(Self {
            client: Client::builder().build()?,
            mode,
        })
    }

    #[must_use]
    pub fn config_status(&self) -> TranscriptionConfigStatus {
        match &self.mode {
            TranscriptionMode::Disabled => TranscriptionConfigStatus {
                enabled: false,
                provider: "none",
                model: None,
            },
            TranscriptionMode::OpenAi { model, .. } => TranscriptionConfigStatus {
                enabled: true,
                provider: "openai",
                model: Some(model.clone()),
            },
        }
    }

    /// Transcribe WAV bytes into text (when configured).
    pub async fn transcribe_wav_bytes(
        &self,
        file_name: &str,
        wav_bytes: Vec<u8>,
    ) -> TranscriptionResult<String> {
        if file_name.trim().is_empty() {
            return Err(TranscriptionError::InvalidConfiguration(
                "file_name must not be empty",
            ));
        }
        if wav_bytes.is_empty() {
            return Err(TranscriptionError::InvalidConfiguration(
                "audio payload must not be empty",
            ));
        }

        let request = self.build_transcription_request(file_name, wav_bytes)?;
        let response = self.client.execute(request).await?;

        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(TranscriptionError::Api(
                "Unauthorized transcription request (check OPENAI_API_KEY)".to_string(),
            ));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(TranscriptionError::Api(format!(
                "Transcription request failed with {status}: {body}"
            )));
        }

        let payload: OpenAiTranscriptionResponse = response.json().await?;
        Ok(payload.text.trim().to_string())
    }

    fn build_transcription_request(
        &self,
        file_name: &str,
        wav_bytes: Vec<u8>,
    ) -> TranscriptionResult<Request> {
        let (base_url, api_key, model) = match &self.mode {
            TranscriptionMode::Disabled => return Err(TranscriptionError::NotConfigured),
            TranscriptionMode::OpenAi {
                base_url,
                api_key,
                model,
            } => (base_url, api_key, model),
        };

        let endpoint = format!("{base_url}/v1/audio/transcriptions");
        let file_part = multipart::Part::bytes(wav_bytes)
            .file_name(file_name.to_string())
            .mime_str("audio/wav")
            .map_err(TranscriptionError::Http)?;

        let form = multipart::Form::new()
            .text("model", model.clone())
            .part("file", file_part);

        self.client
            .post(endpoint)
            .bearer_auth(api_key)
            .multipart(form)
            .build()
            .map_err(TranscriptionError::Http)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_service() -> TranscriptionService {
        TranscriptionService {
            client: Client::builder().build().unwrap(),
            mode: TranscriptionMode::OpenAi {
                base_url: "https://api.openai.com".to_string(),
                api_key: "test-key".to_string(),
                model: "gpt-4o-mini-transcribe".to_string(),
            },
        }
    }

    #[test]
    fn disabled_status_when_not_configured() {
        let service = TranscriptionService {
            client: Client::builder().build().unwrap(),
            mode: TranscriptionMode::Disabled,
        };

        let status = service.config_status();
        assert!(!status.enabled);
        assert_eq!(status.provider, "none");
        assert_eq!(status.model, None);
    }

    #[test]
    fn openai_request_shape_is_correct() {
        let service = configured_service();
        let request = service
            .build_transcription_request("memo.wav", vec![0, 1, 2, 3])
            .unwrap();

        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(
            request.url().as_str(),
            "https://api.openai.com/v1/audio/transcriptions"
        );

        let auth = request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth.starts_with("Bearer "));
    }

    #[test]
    fn request_fails_when_disabled() {
        let service = TranscriptionService {
            client: Client::builder().build().unwrap(),
            mode: TranscriptionMode::Disabled,
        };
        let err = service
            .build_transcription_request("memo.wav", vec![1, 2, 3])
            .unwrap_err();
        assert!(matches!(err, TranscriptionError::NotConfigured));
    }

    #[test]
    fn parse_openai_response_text() {
        let payload: OpenAiTranscriptionResponse =
            serde_json::from_str(r#"{"text":"hello world"}"#).unwrap();
        assert_eq!(payload.text, "hello world");
    }
}
