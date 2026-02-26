//! Settings repository implementation

use crate::error::{Error, Result};
use crate::models::Settings;
use libsql::Connection;

/// Trait for settings storage operations (async)
#[allow(async_fn_in_trait)]
pub trait SettingsRepository {
    /// Load settings from the database
    async fn load(&self) -> Result<Settings>;

    /// Save settings to the database
    async fn save(&self, settings: &Settings) -> Result<()>;
}

/// libSQL implementation of `SettingsRepository`
pub struct LibSqlSettingsRepository<'a> {
    conn: &'a Connection,
}

impl<'a> LibSqlSettingsRepository<'a> {
    /// Create a new repository with the given connection
    pub const fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

impl SettingsRepository for LibSqlSettingsRepository<'_> {
    async fn load(&self) -> Result<Settings> {
        let mut settings = Settings::default();

        // Load each setting individually
        if let Some(value) = self.get_setting_optional("font_family").await? {
            settings.font_family = value;
        }

        if let Some(value) = self.get_setting_optional("font_size").await? {
            settings.font_size = value.parse::<u32>().map_err(|error| {
                Error::InvalidInput(format!("Invalid settings value for 'font_size': {error}"))
            })?;
        }

        if let Some(value) = self.get_setting_optional("theme").await? {
            settings.theme = serde_json::from_str(&format!("\"{value}\"")).map_err(|error| {
                Error::InvalidInput(format!("Invalid settings value for 'theme': {error}"))
            })?;
        }

        if let Some(value) = self.get_setting_optional("capture_hotkey").await? {
            settings.capture_hotkey = value;
        }

        if let Some(value) = self
            .get_setting_optional("voice_memo_transcription_enabled")
            .await?
        {
            settings.voice_memo_transcription_enabled =
                Self::parse_bool_setting("voice_memo_transcription_enabled", &value)?;
        }

        Ok(settings)
    }

    async fn save(&self, settings: &Settings) -> Result<()> {
        self.set_setting("font_family", &settings.font_family)
            .await?;
        self.set_setting("font_size", &settings.font_size.to_string())
            .await?;
        let theme_str = serde_json::to_string(&settings.theme)?
            .trim_matches('"')
            .to_string();
        self.set_setting("theme", &theme_str).await?;
        self.set_setting("capture_hotkey", &settings.capture_hotkey)
            .await?;
        self.set_setting(
            "voice_memo_transcription_enabled",
            if settings.voice_memo_transcription_enabled {
                "true"
            } else {
                "false"
            },
        )
        .await?;
        Ok(())
    }
}

impl LibSqlSettingsRepository<'_> {
    async fn get_setting_optional(&self, key: &str) -> Result<Option<String>> {
        match self.get_setting(key).await {
            Ok(value) => Ok(Some(value)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn parse_bool_setting(key: &str, raw: &str) -> Result<bool> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            other => Err(Error::InvalidInput(format!(
                "Invalid settings value for '{key}': '{other}'"
            ))),
        }
    }

    async fn get_setting(&self, key: &str) -> Result<String> {
        let mut rows = self
            .conn
            .query("SELECT value FROM settings WHERE key = ?", [key])
            .await?;

        if let Some(row) = rows.next().await? {
            let value: String = row.get(0)?;
            Ok(value)
        } else {
            Err(crate::error::Error::NotFound(key.to_string()))
        }
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
                [key, value],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::ThemeMode;

    async fn setup() -> Database {
        Database::open_in_memory().await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_default_settings() {
        let db = setup().await;
        let repo = LibSqlSettingsRepository::new(db.connection());

        let settings = repo.load().await.unwrap();
        assert_eq!(settings.font_size, 14);
        assert_eq!(settings.theme, ThemeMode::System);
        assert!(!settings.voice_memo_transcription_enabled);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_save_and_load_settings() {
        let db = setup().await;
        let repo = LibSqlSettingsRepository::new(db.connection());

        let settings = Settings {
            font_size: 18,
            theme: ThemeMode::Dark,
            font_family: "JetBrains Mono".to_string(),
            voice_memo_transcription_enabled: true,
            ..Settings::default()
        };

        repo.save(&settings).await.unwrap();

        let loaded = repo.load().await.unwrap();
        assert_eq!(loaded.font_size, 18);
        assert_eq!(loaded.theme, ThemeMode::Dark);
        assert_eq!(loaded.font_family, "JetBrains Mono");
        assert!(loaded.voice_memo_transcription_enabled);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_rejects_invalid_theme_value() {
        let db = setup().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.set_setting("theme", "invalid-theme")
            .await
            .expect("failed to seed invalid theme");

        let error = repo.load().await.unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_rejects_invalid_voice_memo_bool() {
        let db = setup().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.set_setting("voice_memo_transcription_enabled", "maybe")
            .await
            .expect("failed to seed invalid bool setting");

        let error = repo.load().await.unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)));
    }
}
