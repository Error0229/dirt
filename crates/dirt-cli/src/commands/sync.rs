//! `dirt sync`: unavailable until Dirt's sync is rebuilt.

use crate::error::CliError;

/// Always fails, so that nobody takes a silent success to mean that their
/// notes reached another device.
pub const fn run_sync() -> Result<(), CliError> {
    Err(CliError::SyncUnavailable)
}
