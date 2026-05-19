mod metrics;
mod scanner;
mod symlink;

use anyhow::Result;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    let scan_path = std::env::args().nth(1).unwrap_or(".".to_string());
    let worker_count = num_cpus::get();

    println!("Starting {} workers...\n", worker_count);

    let metrics = metrics::Metrics::new();
    let total_files = scanner::run(scan_path, worker_count, metrics.clone()).await?;

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
