//! Database connection management

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

use super::migrations;

/// Database wrapper for `SQLite` connections
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open a database at the given path, creating it if it doesn't exist
    ///
    /// Runs migrations automatically.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        let mut db = Self { conn };
        db.configure()?;
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (useful for testing)
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.configure()?;
        db.migrate()?;
        Ok(db)
    }

    /// Configure `SQLite` for optimal performance
    fn configure(&self) -> Result<()> {
        // Enable WAL mode for better concurrency
        self.conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA cache_size = 10000;
            ",
        )?;
        Ok(())
    }

    /// Run database migrations
    fn migrate(&mut self) -> Result<()> {
        migrations::run(&mut self.conn)
    }

    /// Get a reference to the underlying connection
    pub const fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Get a mutable reference to the underlying connection
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.connection().is_autocommit());
    }
}
