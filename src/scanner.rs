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
    // Dir channel: unbounded — workers are both producers and consumers.
    // A bounded dir channel would deadlock when all workers block on send()
    // simultaneously with nobody left to call recv().
    let (dir_tx, dir_rx) = async_channel::unbounded::<PathBuf>();

    // File channel: bounded batches — main thread is an independent consumer,
    // so backpressure here is safe and keeps memory flat.
    let (file_tx, mut file_rx) = mpsc::channel::<Vec<String>>(FILE_CHANNEL_CAP);

    let notify = Arc::new(Notify::new());
    let symlink_guard = SymlinkGuard::new();

    // Seed the root directory.
    // Release: any worker that picks this up and loads in_flight > 0 will
    // observe our increment via its Acquire load in the watcher.
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
            // ── Per-worker allocations: done ONCE, reused every directory ──────
            //
            // scratch_buf   : passed into read_dir_entries as working memory
            //                 (currently unused by std::fs::read_dir, kept for
            //                  future getattrlistbulk/getdents64 fast path)
            //
            // path_scratch  : reusable String for building full file paths.
            //                 Old code: dir.join(name).display().to_string()
            //                   → allocates a PathBuf AND a String per file
            //                   → 3.08 M heap allocs for 1.54 M files
            //                 New code: push dir_str + '/' + name into scratch,
            //                 then .clone() only for the batch entry
            //                   → 1 alloc per file (the clone), 50% fewer allocs
            //
            // file_batch    : Vec flushed every FILE_BATCH_SIZE entries to cut
            //                 channel send overhead 256×.
            let mut scratch_buf = platform::new_scratch_buf();
            let mut path_scratch = String::with_capacity(512);
            let mut file_batch = Vec::<String>::with_capacity(FILE_BATCH_SIZE);

            loop {
                // Interleave cancellation check with directory receive.
                // `biased` ensures the cancellation branch is polled first —
                // once cancelled we stop taking new work immediately.
                let dir = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    res = dir_rx.recv() => match res {
                        Ok(dir) => dir,
                        Err(_)  => break,  // channel closed — normal shutdown
                    },
                };

                // ── Offload blocking read_dir to the blocking thread pool ──────
                // std::fs::read_dir is a synchronous syscall.  Calling it directly
                // on an async executor thread stalls the entire runtime; every other
                // task on that thread waits.  spawn_blocking dispatches to a
                // separate thread pool sized for blocking work.
                let mut buf = std::mem::take(&mut scratch_buf);
                let dir_for_read = dir.clone();
                let read_result = tokio::task::spawn_blocking(move || {
                    let mut entries = Vec::<platform::DirEntry>::with_capacity(128);
                    let result = platform::read_dir_entries(&dir_for_read, &mut buf, &mut entries);
                    (result, buf, entries)
                })
                .await;

                let (result, returned_buf, entries_buf) = match read_result {
                    Ok(triple) => triple,
                    Err(_) => {
                        // spawn_blocking task panicked — treat as failed dir.
                        metrics.dirs_failed.fetch_add(1, Ordering::Relaxed);
                        metrics.in_flight.fetch_sub(1, Ordering::Release);
                        notify.notify_one();
                        continue;
                    }
                };
                scratch_buf = returned_buf;

                if result.is_err() {
                    metrics.dirs_failed.fetch_add(1, Ordering::Relaxed);
                    metrics.in_flight.fetch_sub(1, Ordering::Release);
                    notify.notify_one();
                    continue;
                }

                // ── Pre-compute dir string once per directory ──────────────────
                // Converting PathBuf → &str is zero-copy on valid UTF-8 (Cow::Borrowed).
                // Doing it outside the per-entry loop avoids repeating the conversion
                // for every file in the directory.
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

                            // Increment BEFORE send — the counter must reflect
                            // queued-but-not-yet-started work before any worker
                            // can pick it up, finish it, and signal in_flight == 0.
                            metrics.in_flight.fetch_add(1, Ordering::Release);
                            metrics.update_peak();

                            let path = dir.join(&entry.name);
                            if dir_tx.send(path).await.is_err() {
                                // Channel closed during shutdown — undo the increment.
                                metrics.in_flight.fetch_sub(1, Ordering::Release);
                                notify.notify_one();
                            }
                        }

                        EntryType::File => {
                            metrics.files_found.fetch_add(1, Ordering::Relaxed);

                            // Build full path string without an intermediate PathBuf.
                            // push_str into a reused scratch buffer, then clone only
                            // the final string for the batch — one allocation per file.
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
                // Release: the watcher's Acquire load of in_flight after notified()
                // is guaranteed to see this decrement.
                metrics.in_flight.fetch_sub(1, Ordering::Release);
                notify.notify_one();
            }

            // Flush any remaining file paths before the worker exits.
            if !file_batch.is_empty() {
                let _ = file_tx.send(file_batch).await;
            }
        }));
    }

    // Watcher: closes the dir channel only when in_flight truly hits 0.
    // Acquire pairs with the workers' Release on fetch_sub — we are guaranteed
    // to observe the final decrement before deciding to shut down.
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

    // Drain file batches — count total and optionally print paths.
    let mut total_files = 0usize;
    while let Some(batch) = file_rx.recv().await {
        for _path in &batch {
            total_files += 1;
            // println!("{}", _path);   // uncomment to stream paths to stdout
        }
    }

    watcher.await?;
    for handle in handles {
        let _ = handle.await;
    }

    Ok(total_files)
}
