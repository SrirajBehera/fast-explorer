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

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    let scan_path = std::env::args().nth(1).unwrap_or(".".to_string());
    let worker_count = num_cpus::get();

    println!("Starting {} workers...\n", worker_count);

    let (dir_tx, dir_rx) = async_channel::unbounded::<PathBuf>();
    let (file_tx, mut file_rx) = mpsc::unbounded_channel::<String>();

    // Counts dirs that are queued OR actively being processed.
    // Only when this hits 0 is the scan truly complete.
    let in_flight = Arc::new(AtomicI64::new(0));

    // Notified every time in_flight is decremented, so the shutdown
    // watcher wakes up and can check if we're done.
    let notify = Arc::new(Notify::new());

    // Seed the first directory
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
                        // This dir failed — still must decrement and notify
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
                            // Increment BEFORE sending — guarantees the counter
                            // is already correct before any worker can pick it up
                            in_flight.fetch_add(1, Ordering::SeqCst);
                            let _ = dir_tx.send(path).await;
                        }
                        Ok(ft) if ft.is_file() => {
                            let _ = file_tx.send(path.display().to_string());
                        }
                        _ => {}
                    }
                }

                // Done processing this dir
                in_flight.fetch_sub(1, Ordering::SeqCst);
                notify.notify_one();
            }
        }));
    }

    // Shutdown watcher — waits until in_flight truly hits 0, then
    // closes the dir channel so all workers exit their recv() loop.
    let watcher = {
        let in_flight = Arc::clone(&in_flight);
        let notify = Arc::clone(&notify);
        let dir_tx = dir_tx.clone(); // keep channel open until we decide to close it

        tokio::spawn(async move {
            loop {
                notify.notified().await;
                if in_flight.load(Ordering::SeqCst) == 0 {
                    // Closing the last sender closes the channel,
                    // making all dir_rx.recv() calls return Err → workers exit
                    dir_tx.close();
                    break;
                }
            }
        })
    };

    drop(dir_tx); // main thread's clone
    drop(file_tx); // when workers exit they drop theirs; this drops main's

    let mut total_files = 0;
    while let Some(path) = file_rx.recv().await {
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
