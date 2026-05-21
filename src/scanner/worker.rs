use std::sync::atomic::Ordering;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

use crate::models::FileRecord;
use crate::platform::{self, EntryType};
use crate::symlink::SymlinkGuard;
use crate::telemetry::Metrics;

pub async fn worker_loop(
    dir_tx: async_channel::Sender<(PathBuf, u64)>,
    dir_rx: async_channel::Receiver<(PathBuf, u64)>,
    file_tx: mpsc::Sender<Vec<FileRecord>>,
    metrics: Metrics,
    notify: Arc<Notify>,
    symlink_guard: SymlinkGuard,
    cancel: CancellationToken,
    file_batch_size: usize,
) {
    let mut scratch_buf = platform::new_scratch_buf();
    let mut entries_buf = Vec::<platform::DirEntry>::with_capacity(128);
    let mut file_batch = Vec::<FileRecord>::with_capacity(file_batch_size);

    loop {
        let (dir, parent_inode) = tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            res = dir_rx.recv() => match res {
                Ok(item) => item,
                Err(_)  => break,
            },
        };

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

                    let path = dir.join(&*entry.name);
                    if dir_tx.send((path, entry.inode)).await.is_err() {
                        metrics.in_flight.fetch_sub(1, Ordering::Release);
                        notify.notify_one();
                    }

                    file_batch.push(FileRecord {
                        inode: entry.inode,
                        parent_inode,
                        name: entry.name.clone(),
                        is_dir: true,
                    });
                    if file_batch.len() >= file_batch_size {
                        let batch =
                            std::mem::replace(&mut file_batch, Vec::with_capacity(file_batch_size));
                        let _ = file_tx.send(batch).await;
                    }
                }

                EntryType::File => {
                    metrics.files_found.fetch_add(1, Ordering::Relaxed);

                    file_batch.push(FileRecord {
                        inode: entry.inode,
                        parent_inode,
                        name: entry.name.clone(),
                        is_dir: false,
                    });
                    if file_batch.len() >= file_batch_size {
                        let batch =
                            std::mem::replace(&mut file_batch, Vec::with_capacity(file_batch_size));
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
}
