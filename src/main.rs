use anyhow::Result;
use std::time::Instant;
use tokio::sync::mpsc;
use walkdir::WalkDir;

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    let (tx, mut rx) = mpsc::channel(1000);

    let scan_path = std::env::args()
    .nth(1)
    .unwrap_or(".".to_string());

    tokio::spawn(async move {
        for entry in WalkDir::new(scan_path)
            .into_iter()
            .filter_entry(|e| {
                let file_name = e.file_name().to_string_lossy();

                // Skip hidden files/folders
                if file_name.starts_with('.') {
                    return false;
                }

                // Skip .git explicitly
                if file_name == ".git" {
                    return false;
                }

                true
            })
        {
            match entry {
                Ok(entry) => {
                    // Count only files
                    if entry.file_type().is_file() {
                        let path = entry.path().display().to_string();

                        if tx.send(path).await.is_err() {
                            break;
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                }
            }
        }
    });

    let mut total_files = 0;

    while let Some(path) = rx.recv().await {
        total_files += 1;

        println!("{}", path);
    }

    println!("\nTotal files: {}", total_files);
    println!("Scan completed in: {:?}", start.elapsed());

    Ok(())
}