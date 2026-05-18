use anyhow::Result;
use std::{
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use tokio::{
    fs,
    sync::{mpsc, Mutex},
};

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    let scan_path = std::env::args()
        .nth(1)
        .unwrap_or(".".to_string());

    // Directory queue
    let dir_queue = Arc::new(
        Mutex::new(vec![PathBuf::from(scan_path)])
    );

    // File results channel
    let (tx, mut rx) = mpsc::channel::<String>(10000);

    let worker_count = num_cpus::get();

    println!("Starting {} workers...\n", worker_count);

    let mut handles = vec![];

    for _ in 0..worker_count {
        let dir_queue = Arc::clone(&dir_queue);
        let tx = tx.clone();

        let handle = tokio::spawn(async move {
            loop {
                // Get next directory
                let dir = {
                    let mut queue = dir_queue.lock().await;

                    queue.pop()
                };

                let dir = match dir {
                    Some(d) => d,
                    None => break,
                };

                // Read directory
                let mut entries = match fs::read_dir(&dir).await {
                    Ok(entries) => entries,
                    Err(_) => continue,
                };

                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();

                    let file_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();

                    // Skip hidden files/folders
                    if file_name.starts_with('.') {
                        continue;
                    }

                    match entry.file_type().await {
                        Ok(file_type) => {
                            if file_type.is_dir() {
                                // Push subdirectory into queue
                                let mut queue = dir_queue.lock().await;
                                queue.push(path);
                            } else if file_type.is_file() {
                                let _ = tx
                                    .send(path.display().to_string())
                                    .await;
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        });

        handles.push(handle);
    }

    drop(tx);

    let mut total_files = 0;

    while let Some(path) = rx.recv().await {
        total_files += 1;

        println!("{}", path);
    }

    for handle in handles {
        let _ = handle.await;
    }

    println!("\nTotal files: {}", total_files);
    println!("Completed in: {:?}", start.elapsed());

    Ok(())
}