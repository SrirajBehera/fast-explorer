use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::{
    fs,
    sync::{Notify, mpsc},
};

use crate::metrics::Metrics;
use crate::symlink::{SymlinkGuard, SymlinkStatus};

const FILE_CHANNEL_CAP: usize = 8_192;

pub async fn run(scan_path: String, worker_count: usize, metrics: Metrics) -> Result<usize> {
    // Dir channel is unbounded — workers are both producers and consumers.
    // A bounded dir channel deadlocks when all workers block on send()
    // simultaneously with nobody left to call recv().
    let (dir_tx, dir_rx) = async_channel::unbounded::<PathBuf>();

    // File channel is bounded — main thread is an independent consumer,
    // so backpressure here is safe and keeps memory flat.
    let (file_tx, mut file_rx) = mpsc::channel::<String>(FILE_CHANNEL_CAP);

    // Watcher signals workers to stop once in_flight hits 0
    let notify = Arc::new(Notify::new());

    // Shared symlink guard — tracks visited inodes across all workers.
    // Prevents both symlink loops (A→B→A) and redundant traversal.
    let symlink_guard = SymlinkGuard::new();

    // Seed the root directory
    metrics.in_flight.fetch_add(1, Ordering::SeqCst);
    metrics.update_peak();
    dir_tx.send(PathBuf::from(&scan_path)).await?;

    let mut handles = vec![];

    for _ in 0..worker_count {
        let dir_tx = dir_tx.clone();
        let dir_rx = dir_rx.clone();
        let file_tx = file_tx.clone();
        let metrics = metrics.clone();
        let notify = Arc::clone(&notify);
        let symlink_guard = symlink_guard.clone();

        handles.push(tokio::spawn(async move {
            while let Ok(dir) = dir_rx.recv().await {
                let mut entries = match fs::read_dir(&dir).await {
                    Ok(e) => e,
                    Err(_) => {
                        metrics.dirs_failed.fetch_add(1, Ordering::SeqCst);
                        metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                        notify.notify_one();
                        continue;
                    }
                };

                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    let name = path.file_name().unwrap_or_default().to_string_lossy();

                    if name.starts_with('.') {
                        continue;
                    }

                    match entry.file_type().await {
                        Ok(ft) if ft.is_symlink() => {
                            // Symlink as reported by the OS — skip entirely.
                            // This catches the common case cheaply without stat.
                            metrics.symlinks_skipped.fetch_add(1, Ordering::SeqCst);
                        }
                        Ok(ft) if ft.is_dir() => {
                            // Deep check: verify inode hasn't been visited before.
                            // Catches cycles that don't show as symlinks at this
                            // level (e.g. bind mounts, hardlinked dirs on some FSes).
                            match symlink_guard.check_and_mark(&path).await {
                                SymlinkStatus::Safe => {
                                    // Increment BEFORE send — counter must be correct
                                    // before any worker can pick it up and finish it
                                    metrics.in_flight.fetch_add(1, Ordering::SeqCst);
                                    metrics.update_peak();

                                    if dir_tx.send(path).await.is_err() {
                                        // Channel closed during shutdown — undo increment
                                        metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                                        notify.notify_one();
                                    }
                                }
                                SymlinkStatus::IsSymlink => {
                                    metrics.symlinks_skipped.fetch_add(1, Ordering::SeqCst);
                                }
                                SymlinkStatus::Cycle => {
                                    metrics.cycles_detected.fetch_add(1, Ordering::SeqCst);
                                }
                                SymlinkStatus::Error => {}
                            }
                        }
                        Ok(ft) if ft.is_file() => {
                            metrics.files_found.fetch_add(1, Ordering::SeqCst);
                            let _ = file_tx.send(path.display().to_string()).await;
                        }
                        _ => {}
                    }
                }

                metrics.dirs_scanned.fetch_add(1, Ordering::SeqCst);
                metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                notify.notify_one();
            }
        }));
    }

    // Watcher: closes the dir channel only when in_flight truly hits 0.
    // This is the correct shutdown signal — no race conditions.
    let watcher = {
        let in_flight = Arc::clone(&metrics.in_flight) as Arc<AtomicI64>;
        let dir_tx = dir_tx.clone();

        tokio::spawn(async move {
            loop {
                notify.notified().await;
                if in_flight.load(Ordering::SeqCst) == 0 {
                    dir_tx.close();
                    break;
                }
            }
        })
    };

    drop(dir_tx);
    drop(file_tx);

    let mut total_files = 0;
    while let Some(_path) = file_rx.recv().await {
        total_files += 1;
        // println!("{}", path);
    }

    watcher.await?;

    for handle in handles {
        let _ = handle.await;
    }

    Ok(total_files)
}
