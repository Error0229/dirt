//! Settings repository implementation

use crate::error::Result;
use crate::models::Settings;
use rusqlite::{params, Connection};

/// Trait for settings storage operations
pub trait SettingsRepository {
    /// Load settings from the database
    fn load(&self) -> Result<Settings>;

    /// Save settings to the database
    fn save(&self, settings: &Settings) -> Result<()>;
}

/// `SQLite` implementation of `SettingsRepository`
pub struct SqliteSettingsRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SqliteSettingsRepository<'a> {
    /// Create a new repository with the given connection
    pub const fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

impl SettingsRepository for SqliteSettingsRepository<'_> {
    fn load(&self) -> Result<Settings> {
        let mut settings = Settings::default();

        // Load each setting individually
        if let Ok(value) = self.get_setting("font_family") {
            settings.font_family = value;
        }

        if let Ok(value) = self.get_setting("font_size") {
            if let Ok(size) = value.parse() {
                settings.font_size = size;
            }
        }

        if let Ok(value) = self.get_setting("theme") {
            settings.theme = serde_json::from_str(&format!("\"{value}\"")).unwrap_or_default();
        }

        if let Ok(value) = self.get_setting("capture_hotkey") {
            settings.capture_hotkey = value;
        }

        Ok(settings)
    }

    fn save(&self, settings: &Settings) -> Result<()> {
        self.set_setting("font_family", &settings.font_family)?;
        self.set_setting("font_size", &settings.font_size.to_string())?;
        let theme_str = serde_json::to_string(&settings.theme)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        self.set_setting("theme", &theme_str)?;
        self.set_setting("capture_hotkey", &settings.capture_hotkey)?;
        Ok(())
    }
}

impl SqliteSettingsRepository<'_> {
    fn get_setting(&self, key: &str) -> Result<String> {
        let value: String = self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?",
            params![key],
            |row| row.get(0),
        )?;
        Ok(value)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::ThemeMode;

    fn setup() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn test_load_default_settings() {
        let db = setup();
        let repo = SqliteSettingsRepository::new(db.connection());

        let settings = repo.load().unwrap();
        assert_eq!(settings.font_size, 14);
        assert_eq!(settings.theme, ThemeMode::System);
    }

    #[test]
    fn test_save_and_load_settings() {
        let db = setup();
        let repo = SqliteSettingsRepository::new(db.connection());

        let settings = Settings {
            font_size: 18,
            theme: ThemeMode::Dark,
            font_family: "JetBrains Mono".to_string(),
            ..Settings::default()
        };

        repo.save(&settings).unwrap();

        let loaded = repo.load().unwrap();
        assert_eq!(loaded.font_size, 18);
        assert_eq!(loaded.theme, ThemeMode::Dark);
        assert_eq!(loaded.font_family, "JetBrains Mono");
    }
}
