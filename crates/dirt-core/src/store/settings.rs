//! Settings, kept as key and value rows.

use rusqlite::{Connection, OptionalExtension};

use super::schema::write_transaction;
use crate::models::{Accent, FocusStyle, Settings};
use crate::{Error, Result};

fn get(connection: &Connection, key: &str) -> Result<Option<String>> {
    Ok(connection
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

fn invalid(key: &str, error: impl std::fmt::Display) -> Error {
    Error::InvalidInput(format!("Invalid settings value for '{key}': {error}"))
}

/// The stored settings. A setting that was never saved keeps its default.
pub(super) fn load(connection: &Connection) -> Result<Settings> {
    let mut settings = Settings::default();
    if let Some(value) = get(connection, "font_family")? {
        settings.font_family = value;
    }
    if let Some(value) = get(connection, "font_size")? {
        settings.font_size = value.parse().map_err(|error| invalid("font_size", error))?;
    }
    if let Some(value) = get(connection, "theme")? {
        settings.theme = serde_json::from_value(serde_json::Value::String(value))
            .map_err(|error| invalid("theme", error))?;
    }
    // An accent this build has no colours for, written by a later one, reads
    // as the default: the app must still start.
    if let Some(value) = get(connection, "accent")? {
        settings.accent = Accent::from_name(&value).unwrap_or_default();
    }
    if let Some(value) = get(connection, "capture_hotkey")? {
        settings.capture_hotkey = value;
    }
    // As for the accent: a style a later build added reads as the default.
    if let Some(value) = get(connection, "focus_style")? {
        settings.focus_style = FocusStyle::from_name(&value).unwrap_or_default();
    }
    Ok(settings)
}

pub(super) fn save(connection: &mut Connection, settings: &Settings) -> Result<()> {
    let theme = match serde_json::to_value(settings.theme)? {
        serde_json::Value::String(name) => name,
        other => other.to_string(),
    };
    let tx = write_transaction(connection)?;
    for (key, value) in [
        ("font_family", settings.font_family.clone()),
        ("font_size", settings.font_size.to_string()),
        ("theme", theme),
        ("accent", settings.accent.name().to_owned()),
        ("capture_hotkey", settings.capture_hotkey.clone()),
        ("focus_style", settings.focus_style.name().to_owned()),
    ] {
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            [key, value.as_str()],
        )?;
    }
    tx.commit()?;
    Ok(())
}
