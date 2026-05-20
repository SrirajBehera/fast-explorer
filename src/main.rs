mod metrics;
mod platform;
mod reporter;
mod scanner;
mod symlink;

use anyhow::Result;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    let scan_path = std::env::args().nth(1).unwrap_or(".".to_string());
    let worker_count = num_cpus::get();

    println!("Starting {} workers...\n", worker_count);

    let metrics = metrics::Metrics::new();

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
    let reporter = reporter::spawn(metrics.files_found.clone());

    let total_files = scanner::run(scan_path, worker_count, metrics.clone(), cancel).await?;

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
