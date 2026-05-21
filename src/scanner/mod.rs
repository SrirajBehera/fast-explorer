mod worker;

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

use crate::models::FileRecord;
use crate::symlink::SymlinkGuard;
use crate::telemetry::Metrics;

const FILE_CHANNEL_CAP: usize = 8_192;
const FILE_BATCH_SIZE: usize = 256;

pub async fn run(
    scan_path: String,
    root_inode: u64,
    worker_count: usize,
    metrics: Metrics,
    cancel: CancellationToken,
    mut db: crate::db::Db,
) -> Result<usize> {
    let (dir_tx, dir_rx) = async_channel::unbounded::<(PathBuf, u64)>();
    let (file_tx, mut file_rx) = mpsc::channel::<Vec<FileRecord>>(FILE_CHANNEL_CAP);

    let notify = Arc::new(Notify::new());
    let symlink_guard = SymlinkGuard::new();

    metrics.in_flight.fetch_add(1, Ordering::Release);
    metrics.update_peak();
    dir_tx.send((PathBuf::from(&scan_path), root_inode)).await?;

    let mut handles = vec![];

    for _ in 0..worker_count {
        let dir_tx = dir_tx.clone();
        let dir_rx = dir_rx.clone();
        let file_tx = file_tx.clone();
        let metrics = metrics.clone();
        let notify = Arc::clone(&notify);
        let symlink_guard = symlink_guard.clone();
        let cancel = cancel.clone();

        handles.push(tokio::spawn(async move {
            worker::worker_loop(
                dir_tx,
                dir_rx,
                file_tx,
                metrics,
                notify,
                symlink_guard,
                cancel,
                FILE_BATCH_SIZE,
            ).await;
        }));
    }

    // Watcher: closes dir channel when in_flight hits 0.
    // Acquire pairs with workers' Release fetch_sub.
    let watcher = {
        let in_flight = Arc::clone(&metrics.in_flight) as Arc<AtomicI64>;
        let dir_tx = dir_tx.clone();
        tokio::spawn(async move {
            loop {
                notify.notified().await;
                if in_flight.load(Ordering::Acquire) == 0 {
                    dir_tx.close();
                    break;
                }
            }
        })
    };

    drop(dir_tx);
    drop(file_tx);

    let mut total_files = 0usize;
    let mut super_batch = Vec::with_capacity(8192);

    while let Some(mut batch) = file_rx.recv().await {
        total_files += batch.len();
        super_batch.append(&mut batch);

        // Commit transaction every ~8192 entries to amortize transaction overhead
        if super_batch.len() >= 8192 {
            if let Err(e) = db.insert_batch(&super_batch) {
                eprintln!("Failed to insert batch into SQLite: {}", e);
            }
            super_batch.clear();
        }
    }

    // Flush any remaining entries
    if !super_batch.is_empty() {
        if let Err(e) = db.insert_batch(&super_batch) {
            eprintln!("Failed to insert batch into SQLite: {}", e);
        }
    }

    watcher.await?;
    for handle in handles {
        let _ = handle.await;
    }

    Ok(total_files)
}
