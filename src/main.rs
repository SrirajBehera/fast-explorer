mod db;
mod models;
mod platform;
mod scanner;
mod symlink;
mod telemetry;

use anyhow::Result;
use compact_str::CompactString;
use models::FileRecord;
use std::{os::unix::fs::MetadataExt, time::Instant};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    let scan_path = std::env::args().nth(1).unwrap_or(".".to_string());
    let worker_count = num_cpus::get();

    println!("Starting {} workers...\n", worker_count);

    let metrics = telemetry::Metrics::new();

    // Cancellation token — shared between the Ctrl-C handler and the scanner.
    // Dropping or calling .cancel() on any clone propagates to all clones.
    let cancel = CancellationToken::new();

    // Spawn a Ctrl-C listener that cancels the token so the scanner stops
    // cleanly without leaving orphaned tasks or zombie threads.
    let cancel_for_ctrlc = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\nInterrupted — stopping scan...");
            cancel_for_ctrlc.cancel();
        }
    });

    // Start live throughput reporter — prints files/sec to stderr every second.
    // Kept on stderr so stdout (file paths) stays clean for piping.
    // Dropping `reporter` stops it cleanly before final results print.
    let reporter = telemetry::reporter::spawn(metrics.files_found.clone());

    // Stat the root directory to get its inode
    let root_metadata = std::fs::metadata(&scan_path)?;
    let root_inode = root_metadata.ino();

    // Initialize DB
    let db_path = "fex.db";
    let mut database = db::Db::new(db_path)?;

    // Insert the root directory itself into the DB so recursive queries have a starting point
    // Using 0 as parent_inode for the root
    database.insert_batch(&[FileRecord {
        inode: root_inode,
        parent_inode: 0,
        name: CompactString::new(scan_path.clone()),
        is_dir: true,
    }])?;

    let total_files = scanner::run(
        scan_path,
        root_inode,
        worker_count,
        metrics.clone(),
        cancel,
        database,
    )
    .await?;

    // Stop reporter before printing final results — prevents interleaving.
    reporter.stop();

    let elapsed = start.elapsed();

    println!("\n=== Results ===");
    println!("Total files:              {}", total_files);
    println!("Completed in:             {:?}", elapsed);
    println!(
        "Throughput:               {:.0} files/sec",
        total_files as f64 / elapsed.as_secs_f64()
    );

    println!("\n=== Queue Pressure ===");
    metrics.print();

    Ok(())
}
