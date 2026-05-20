use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

use crate::metrics::Metrics;
use crate::platform::{self, EntryType};
use crate::symlink::SymlinkGuard;

const FILE_CHANNEL_CAP: usize = 8_192;
const FILE_BATCH_SIZE: usize = 256;

pub async fn run(
    scan_path: String,
    worker_count: usize,
    metrics: Metrics,
    cancel: CancellationToken,
) -> Result<usize> {
    let (dir_tx, dir_rx) = async_channel::unbounded::<PathBuf>();
    let (file_tx, mut file_rx) = mpsc::channel::<Vec<String>>(FILE_CHANNEL_CAP);

    let notify = Arc::new(Notify::new());
    let symlink_guard = SymlinkGuard::new();

    metrics.in_flight.fetch_add(1, Ordering::Release);
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
        let cancel = cancel.clone();

        handles.push(tokio::spawn(async move {
            // Per-worker allocations — done once, reused across every directory.
            //
            // scratch_buf  : 256KB buffer passed into getdirentries64 via read_dir_entries.
            //                Allocated once per worker; the blocking closure receives it
            //                via std::mem::take and returns it so it can be reused.
            //
            // entries_buf  : Reusable Vec to store DirEntry structs.
            //                Reusing this prevents 165K Vec allocations per scan.
            //
            // path_scratch : reusable String for building full file paths.
            //                Avoids the PathBuf+String double allocation from
            //                dir.join(&name).display().to_string().
            //
            // file_batch   : Vec flushed every FILE_BATCH_SIZE to cut channel sends.
            let mut scratch_buf = platform::new_scratch_buf();
            let mut entries_buf = Vec::<platform::DirEntry>::with_capacity(128);
            let mut path_scratch = String::with_capacity(512);
            let mut file_batch = Vec::<String>::with_capacity(FILE_BATCH_SIZE);

            loop {
                let dir = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    res = dir_rx.recv() => match res {
                        Ok(dir) => dir,
                        Err(_)  => break,
                    },
                };

                // Offload blocking read_dir_entries to tokio's shared blocking pool.
                let mut buf = std::mem::take(&mut scratch_buf);
                let mut entries = std::mem::take(&mut entries_buf);
                let dir_for_read = dir.clone();
                let read_result = tokio::task::spawn_blocking(move || {
                    let result = platform::read_dir_entries(&dir_for_read, &mut buf, &mut entries);
                    (result, buf, entries)
                })
                .await;

                let (result, returned_buf, returned_entries) = match read_result {
                    Ok(triple) => triple,
                    Err(_) => {
                        metrics.dirs_failed.fetch_add(1, Ordering::Relaxed);
                        metrics.in_flight.fetch_sub(1, Ordering::Release);
                        notify.notify_one();
                        continue;
                    }
                };
                scratch_buf = returned_buf;
                entries_buf = returned_entries;

                if result.is_err() {
                    metrics.dirs_failed.fetch_add(1, Ordering::Relaxed);
                    metrics.in_flight.fetch_sub(1, Ordering::Release);
                    notify.notify_one();
                    continue;
                }

                // Compute dir string once per directory — zero-copy Cow<str>
                // on valid UTF-8 (APFS always produces valid UTF-8 names).
                let dir_str = dir.to_string_lossy();
                let dir_str = dir_str.as_ref();
                let needs_sep = !dir_str.ends_with('/');

                for entry in &entries_buf {
                    if entry.name.starts_with('.') {
                        continue;
                    }

                    match entry.entry_type {
                        EntryType::Symlink => {
                            metrics.symlinks_skipped.fetch_add(1, Ordering::Relaxed);
                        }

                        EntryType::Dir => {
                            if !symlink_guard.check_and_mark(entry.inode) {
                                metrics.cycles_detected.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                            metrics.in_flight.fetch_add(1, Ordering::Release);
                            metrics.update_peak();

                            let path = dir.join(&entry.name);
                            if dir_tx.send(path).await.is_err() {
                                metrics.in_flight.fetch_sub(1, Ordering::Release);
                                notify.notify_one();
                            }
                        }

                        EntryType::File => {
                            metrics.files_found.fetch_add(1, Ordering::Relaxed);

                            path_scratch.clear();
                            path_scratch.push_str(dir_str);
                            if needs_sep {
                                path_scratch.push('/');
                            }
                            path_scratch.push_str(&entry.name);
                            file_batch.push(path_scratch.clone());

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

                metrics.dirs_scanned.fetch_add(1, Ordering::Relaxed);
                metrics.in_flight.fetch_sub(1, Ordering::Release);
                notify.notify_one();
            }

            if !file_batch.is_empty() {
                let _ = file_tx.send(file_batch).await;
            }
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
