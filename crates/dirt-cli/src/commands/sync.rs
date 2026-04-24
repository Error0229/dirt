use std::path::Path;

use crate::commands::common::open_sync_database;
use crate::error::CliError;

pub async fn run_sync(db_path: &Path) -> Result<(), CliError> {
    let db = open_sync_database(db_path).await?;
    if !db.is_sync_enabled().await {
        return Err(CliError::SyncNotConfigured);
    }

    db.sync().await?;
    println!("Sync completed");
    Ok(())
}
