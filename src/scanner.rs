use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::{Notify, mpsc};

use crate::metrics::Metrics;
use crate::platform::{self, EntryType};
use crate::symlink::SymlinkGuard;

const FILE_CHANNEL_CAP: usize = 8_192;
const FILE_BATCH_SIZE: usize = 256;

pub async fn run(scan_path: String, worker_count: usize, metrics: Metrics) -> Result<usize> {
    let (dir_tx, dir_rx) = async_channel::unbounded::<PathBuf>();
    let (file_tx, mut file_rx) = mpsc::channel::<Vec<String>>(FILE_CHANNEL_CAP);

    let notify = Arc::new(Notify::new());
    let symlink_guard = SymlinkGuard::new();

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
            // scratch_buf: 256KB allocated once, passed into every read_dir call.
            // entries_buf: reused Vec to avoid per-directory allocation.
            // file_batch:  batches file path strings to cut channel send overhead.
            let mut scratch_buf = platform::new_scratch_buf();
            let mut file_batch = Vec::<String>::with_capacity(FILE_BATCH_SIZE);

            while let Ok(dir) = dir_rx.recv().await {
                // read_dir_entries calls std::fs::read_dir — a BLOCKING syscall.
                // Running it directly on the async executor thread stalls the
                // entire runtime. spawn_blocking offloads it to the blocking
                // thread pool so other async tasks keep progressing.
                let mut buf = std::mem::take(&mut scratch_buf);
                let dir_for_read = dir.clone(); // clone for the blocking closure; `dir` used below for joins
                let read_result = tokio::task::spawn_blocking(move || {
                    let mut entries = Vec::<platform::DirEntry>::with_capacity(128);
                    let result = platform::read_dir_entries(&dir_for_read, &mut buf, &mut entries);
                    (result, buf, entries)
                })
                .await;

                let (result, returned_buf, entries_buf) = match read_result {
                    Ok(triple) => triple,
                    Err(_) => {
                        // spawn_blocking panicked — treat as failed dir
                        metrics.dirs_failed.fetch_add(1, Ordering::SeqCst);
                        metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                        notify.notify_one();
                        continue;
                    }
                };
                // Reclaim the scratch buffer for the next iteration.
                scratch_buf = returned_buf;

                if result.is_err() {
                    metrics.dirs_failed.fetch_add(1, Ordering::SeqCst);
                    metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                    notify.notify_one();
                    continue;
                }

                for entry in &entries_buf {
                    if entry.name.starts_with('.') {
                        continue;
                    }

                    match entry.entry_type {
                        EntryType::Symlink => {
                            metrics.symlinks_skipped.fetch_add(1, Ordering::SeqCst);
                        }

                        EntryType::Dir => {
                            if !symlink_guard.check_and_mark(entry.inode) {
                                metrics.cycles_detected.fetch_add(1, Ordering::SeqCst);
                                continue;
                            }

                            metrics.in_flight.fetch_add(1, Ordering::SeqCst);
                            metrics.update_peak();

                            let path = dir.join(&entry.name);
                            if dir_tx.send(path).await.is_err() {
                                metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                                notify.notify_one();
                            }
                        }

                        EntryType::File => {
                            metrics.files_found.fetch_add(1, Ordering::SeqCst);
                            file_batch.push(dir.join(&entry.name).display().to_string());

                            if file_batch.len() >= FILE_BATCH_SIZE {
                                let batch = std::mem::replace(
                                    &mut file_batch,
                                    Vec::with_capacity(FILE_BATCH_SIZE),
                                );
                                let _ = file_tx.send(batch).await;
                            }
                        }

                        EntryType::Other => {}
                    }
                }

                metrics.dirs_scanned.fetch_add(1, Ordering::SeqCst);
                metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                notify.notify_one();
            }

            // Flush remaining files before worker exits
            if !file_batch.is_empty() {
                let _ = file_tx.send(file_batch).await;
            }
        }));
    }

    // Watcher: closes dir channel only when in_flight truly hits 0
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
    while let Some(batch) = file_rx.recv().await {
        for _path in &batch {
            total_files += 1;
            // println!("{}", _path);
        }
    }

    watcher.await?;
    for handle in handles {
        let _ = handle.await;
    }

    Ok(total_files)
}
