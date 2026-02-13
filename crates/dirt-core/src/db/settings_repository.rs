//! Settings repository implementation

use crate::error::Result;
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
        if let Ok(value) = self.get_setting("font_family").await {
            settings.font_family = value;
        }

        if let Ok(value) = self.get_setting("font_size").await {
            if let Ok(size) = value.parse() {
                settings.font_size = size;
            }
        }

        if let Ok(value) = self.get_setting("theme").await {
            settings.theme = serde_json::from_str(&format!("\"{value}\"")).unwrap_or_default();
        }

        if let Ok(value) = self.get_setting("capture_hotkey").await {
            settings.capture_hotkey = value;
        }

        if let Ok(value) = self.get_setting("voice_memo_transcription_enabled").await {
            settings.voice_memo_transcription_enabled = matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }

        Ok(settings)
    }

    async fn save(&self, settings: &Settings) -> Result<()> {
        self.set_setting("font_family", &settings.font_family)
            .await?;
        self.set_setting("font_size", &settings.font_size.to_string())
            .await?;
        let theme_str = serde_json::to_string(&settings.theme)
            .unwrap_or_default()
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
}
