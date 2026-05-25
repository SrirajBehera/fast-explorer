use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Initialize a new SQLite connection at the given path and set up
    /// the high-performance schema.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;

        // WAL mode is crucial for high concurrency and fast writes.
        // NORMAL synchronous relies on WAL for safety while being much faster.
        // MEMORY temp_store puts temporary tables/indices in RAM.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA mmap_size = 3000000000;
             
             CREATE TABLE IF NOT EXISTS entries (
                 inode INTEGER PRIMARY KEY,
                 parent_inode INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 is_dir BOOLEAN NOT NULL
             );
             
             -- Index to instantly find children of a directory
             CREATE INDEX IF NOT EXISTS idx_parent ON entries(parent_inode);"
        )?;

        Ok(Self { conn })
    }

    /// Insert a batch of entries into the database in a single transaction.
    /// Transaction batches are exponentially faster than individual inserts.
    pub fn insert_batch(&mut self, batch: &[crate::models::FileRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            // Cached prepared statement prevents recompiling the SQL for every row
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO entries (inode, parent_inode, name, is_dir) VALUES (?, ?, ?, ?)"
            )?;
            for record in batch {
                stmt.execute(params![record.inode, record.parent_inode, record.name.as_str(), record.is_dir])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
