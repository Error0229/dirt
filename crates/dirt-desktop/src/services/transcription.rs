//! Optional audio transcription service foundation.
#![allow(dead_code)] // Foundation module; full wiring lands in follow-up issue.

use keyring::Entry;
use reqwest::{multipart, Client, Request, StatusCode};
use serde::Deserialize;
use thiserror::Error;

const ENV_OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const ENV_OPENAI_TRANSCRIPTION_MODEL: &str = "OPENAI_TRANSCRIPTION_MODEL";
const ENV_OPENAI_BASE_URL: &str = "OPENAI_BASE_URL";

const DEFAULT_MODEL: &str = "gpt-4o-mini-transcribe";
const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const KEYRING_SERVICE_NAME: &str = "dirt";
const KEYRING_OPENAI_API_KEY_USERNAME: &str = "openai_api_key";

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
    #[error("Transcription is not configured. Add an OpenAI API key in Settings.")]
    NotConfigured,
    #[error("Invalid transcription configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("Secure storage error: {0}")]
    SecureStorage(String),
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

#[derive(Debug, Clone)]
struct OpenAiApiKeyStore {
    service_name: String,
    username: String,
}

impl Default for OpenAiApiKeyStore {
    fn default() -> Self {
        Self {
            service_name: KEYRING_SERVICE_NAME.to_string(),
            username: KEYRING_OPENAI_API_KEY_USERNAME.to_string(),
        }
    }
}

impl OpenAiApiKeyStore {
    fn entry(&self) -> TranscriptionResult<Entry> {
        Entry::new(&self.service_name, &self.username)
            .map_err(|error| TranscriptionError::SecureStorage(error.to_string()))
    }

    fn load(&self) -> TranscriptionResult<Option<String>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(value) => {
                let normalized = value.trim();
                if normalized.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(normalized.to_string()))
                }
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(TranscriptionError::SecureStorage(error.to_string())),
        }
    }

    fn save(&self, api_key: &str) -> TranscriptionResult<()> {
        self.entry()?
            .set_password(api_key)
            .map_err(|error| TranscriptionError::SecureStorage(error.to_string()))
    }

    fn clear(&self) -> TranscriptionResult<()> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(TranscriptionError::SecureStorage(error.to_string())),
        }
    }
}

impl TranscriptionService {
    /// Build transcription service from secure storage.
    ///
    /// In debug builds, `OPENAI_API_KEY` is allowed as a local fallback.
    pub fn new() -> TranscriptionResult<Self> {
        let key_store = OpenAiApiKeyStore::default();
        let mut api_key = key_store.load()?;

        #[cfg(debug_assertions)]
        if api_key.is_none() {
            api_key = std::env::var(ENV_OPENAI_API_KEY)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
        }

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

    /// Persist `OpenAI` API key into secure storage.
    pub fn store_api_key(raw_api_key: &str) -> TranscriptionResult<()> {
        let api_key = raw_api_key.trim();
        if api_key.is_empty() {
            return Err(TranscriptionError::InvalidConfiguration(
                "OpenAI API key must not be empty",
            ));
        }
        OpenAiApiKeyStore::default().save(api_key)
    }

    /// Remove `OpenAI` API key from secure storage.
    pub fn clear_api_key() -> TranscriptionResult<()> {
        OpenAiApiKeyStore::default().clear()
    }

    /// Returns whether a secure `OpenAI` API key is currently stored.
    pub fn has_stored_api_key() -> TranscriptionResult<bool> {
        Ok(OpenAiApiKeyStore::default().load()?.is_some())
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
        self.transcribe_audio_bytes(file_name, "audio/wav", wav_bytes)
            .await
    }

    /// Transcribe arbitrary audio bytes into text (when configured).
    pub async fn transcribe_audio_bytes(
        &self,
        file_name: &str,
        mime_type: &str,
        audio_bytes: Vec<u8>,
    ) -> TranscriptionResult<String> {
        if file_name.trim().is_empty() {
            return Err(TranscriptionError::InvalidConfiguration(
                "file_name must not be empty",
            ));
        }
        if mime_type.trim().is_empty() {
            return Err(TranscriptionError::InvalidConfiguration(
                "mime_type must not be empty",
            ));
        }
        if !mime_type.trim().to_ascii_lowercase().starts_with("audio/") {
            return Err(TranscriptionError::InvalidConfiguration(
                "mime_type must start with audio/",
            ));
        }
        if audio_bytes.is_empty() {
            return Err(TranscriptionError::InvalidConfiguration(
                "audio payload must not be empty",
            ));
        }

        let request = self.build_transcription_request(file_name, mime_type, audio_bytes)?;
        let response = self.client.execute(request).await?;

        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(TranscriptionError::Api(
                "Unauthorized transcription request (check configured OpenAI API key)".to_string(),
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
        mime_type: &str,
        audio_bytes: Vec<u8>,
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
        let file_part = multipart::Part::bytes(audio_bytes)
            .file_name(file_name.to_string())
            .mime_str(mime_type)
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
            .build_transcription_request("memo.wav", "audio/wav", vec![0, 1, 2, 3])
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
            .build_transcription_request("memo.wav", "audio/wav", vec![1, 2, 3])
            .unwrap_err();
        assert!(matches!(err, TranscriptionError::NotConfigured));
    }

    #[test]
    fn request_supports_non_wav_audio_mime() {
        let service = configured_service();
        let request = service
            .build_transcription_request("memo.webm", "audio/webm", vec![0, 1, 2, 3])
            .unwrap();

        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(
            request.url().as_str(),
            "https://api.openai.com/v1/audio/transcriptions"
        );
    }

    #[test]
    fn parse_openai_response_text() {
        let payload: OpenAiTranscriptionResponse =
            serde_json::from_str(r#"{"text":"hello world"}"#).unwrap();
        assert_eq!(payload.text, "hello world");
    }
}
