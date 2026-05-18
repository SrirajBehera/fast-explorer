use anyhow::Result;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Instant,
};
use tokio::{
    fs,
    sync::{Notify, mpsc},
};

const FILE_CHANNEL_CAP: usize = 8_192;

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    let scan_path = std::env::args().nth(1).unwrap_or(".".to_string());
    let worker_count = num_cpus::get();

    println!("Starting {} workers...\n", worker_count);

    // Dir channel is unbounded — workers are BOTH producers and consumers
    // of this channel. A bounded dir channel deadlocks when all workers
    // block on send() simultaneously with nobody left to call recv().
    let (dir_tx, dir_rx) = async_channel::unbounded::<PathBuf>();

    // File channel CAN be bounded — the file consumer (main thread) is
    // independent of workers, so backpressure here is safe.
    let (file_tx, mut file_rx) = mpsc::channel::<String>(FILE_CHANNEL_CAP);

    // Tracks dirs that are queued OR actively being processed.
    // Incremented before send, decremented after fully processed.
    // When it hits 0, the scan is truly complete.
    let in_flight = Arc::new(AtomicI64::new(0));
    let notify = Arc::new(Notify::new());

    in_flight.fetch_add(1, Ordering::SeqCst);
    dir_tx.send(PathBuf::from(&scan_path)).await?;

    let mut handles = vec![];

    for _ in 0..worker_count {
        let dir_tx = dir_tx.clone();
        let dir_rx = dir_rx.clone();
        let file_tx = file_tx.clone();
        let in_flight = Arc::clone(&in_flight);
        let notify = Arc::clone(&notify);

        handles.push(tokio::spawn(async move {
            while let Ok(dir) = dir_rx.recv().await {
                let mut entries = match fs::read_dir(&dir).await {
                    Ok(e) => e,
                    Err(_) => {
                        in_flight.fetch_sub(1, Ordering::SeqCst);
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
                        Ok(ft) if ft.is_dir() => {
                            // Increment BEFORE send — counter must be correct
                            // before any other worker can pick it up and finish it
                            in_flight.fetch_add(1, Ordering::SeqCst);

                            // Unbounded send — never blocks, never deadlocks
                            if dir_tx.send(path).await.is_err() {
                                // Channel closed during shutdown — undo increment
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                                notify.notify_one();
                            }
                        }
                        Ok(ft) if ft.is_file() => {
                            // Bounded send — safe because main thread (file consumer)
                            // is independent and always draining.
                            // If main thread is slow, workers slow down here — that
                            // is intentional backpressure on the output side only.
                            let _ = file_tx.send(path.display().to_string()).await;
                        }
                        _ => {}
                    }
                }

                // This dir is fully processed
                in_flight.fetch_sub(1, Ordering::SeqCst);
                notify.notify_one();
            }
        }));
    }

    // Watcher: the only place that closes the dir channel.
    // Wakes on every notify, checks if truly done, closes channel if so.
    let watcher = {
        let in_flight = Arc::clone(&in_flight);
        let notify = Arc::clone(&notify);
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

    println!("\nTotal files: {}", total_files);
    println!("Completed in: {:?}", start.elapsed());

    Ok(())
}
