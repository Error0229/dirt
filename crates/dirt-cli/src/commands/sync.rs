use std::path::Path;

use crate::commands::common::{
    format_sync_conflict_lines, list_sync_conflicts, open_sync_database, sync_conflict_to_item,
    SyncConflictItem,
};
use crate::error::CliError;

pub async fn run_sync(db_path: &Path) -> Result<(), CliError> {
    let db = open_sync_database(db_path).await?;
    if !db.is_sync_enabled() {
        return Err(CliError::SyncNotConfigured);
    }

    db.sync().await?;
    println!("Sync completed");
    Ok(())
}

pub async fn run_sync_conflicts(
    limit: usize,
    as_json: bool,
    db_path: &Path,
) -> Result<(), CliError> {
    let conflicts = list_sync_conflicts(limit, db_path).await?;

    if as_json {
        let json_items = conflicts
            .iter()
            .map(sync_conflict_to_item)
            .collect::<Vec<SyncConflictItem>>();
        println!("{}", serde_json::to_string_pretty(&json_items)?);
        return Ok(());
    }

    if conflicts.is_empty() {
        println!("No sync conflicts recorded.");
        return Ok(());
    }

    for line in format_sync_conflict_lines(&conflicts) {
        println!("{line}");
    }
    Ok(())
}
