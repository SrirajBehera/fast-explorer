use anyhow::Result;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, params};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use tokio::sync::mpsc;

pub async fn start_watcher<P: AsRef<Path>>(path: P, db_path: String) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(1024);

    let mut watcher = RecommendedWatcher::new(
        move |res: std::result::Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        },
        Config::default(),
    )?;

    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    tokio::spawn(async move {
        // Keep watcher alive
        let _watcher = watcher;

        // Open a dedicated DB connection for the watcher
        let conn = match Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Watcher failed to open DB: {}", e);
                return;
            }
        };

        // Ensure WAL mode is active on this connection too
        let _ = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;");

        while let Some(event) = rx.recv().await {
            match event.kind {
                notify::EventKind::Create(_) => {
                    for path in event.paths {
                        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
                            if let (Ok(meta), Ok(parent_meta)) = (
                                std::fs::symlink_metadata(&path),
                                std::fs::symlink_metadata(parent),
                            ) {
                                let inode = meta.ino();
                                let parent_inode = parent_meta.ino();
                                let is_dir = meta.is_dir();
                                let name_str = name.to_string_lossy();

                                let _ = conn.execute(
                                    "INSERT OR REPLACE INTO entries (inode, parent_inode, name, is_dir) VALUES (?, ?, ?, ?)",
                                    params![inode, parent_inode, name_str.as_ref(), is_dir],
                                );
                                // println!("[Live] Added: {}", path.display());
                            }
                        }
                    }
                }
                notify::EventKind::Remove(_) => {
                    for path in event.paths {
                        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
                            // The file is gone, so we can only stat the parent
                            if let Ok(parent_meta) = std::fs::symlink_metadata(parent) {
                                let parent_inode = parent_meta.ino();
                                let name_str = name.to_string_lossy();

                                // We delete the exact file in O(1) time thanks to the idx_parent index
                                let _ = conn.execute(
                                    "DELETE FROM entries WHERE parent_inode = ? AND name = ?",
                                    params![parent_inode, name_str.as_ref()],
                                );
                                // println!("[Live] Removed: {}", path.display());
                            }
                        }
                    }
                }
                notify::EventKind::Modify(_) => {
                    // Ignored for now — we don't index file sizes/timestamps yet
                }
                _ => {}
            }
        }
    });

    Ok(())
}
