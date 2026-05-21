use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{sync::watch, time};

/// Spawns a background task that prints files/sec every second.
///
/// Returns a `ShutdownTx` — drop it (or call `.stop()`) to cleanly
/// stop the reporter after the scan finishes. This prevents a stray
/// "Files/sec: 0" line printing after results are shown.
pub fn spawn(files_found: Arc<AtomicUsize>) -> ShutdownTx {
    // One-shot channel: sender drop = receiver sees channel closed = task exits.
    let (tx, mut rx) = watch::channel(());

    tokio::spawn(async move {
        let mut last: usize = 0;
        let mut interval = time::interval(Duration::from_secs(1));

        // Tick immediately to align the interval to "now", not first sleep.
        interval.tick().await;

        loop {
            tokio::select! {
                // Every second: compute delta and print
                _ = interval.tick() => {
                    let current = files_found.load(Ordering::Relaxed);
                    let delta   = current.saturating_sub(last);
                    last        = current;

                    // Format with thousands separator for readability
                    eprintln!("[throughput] {:>10} files/sec  |  total: {}",
                        fmt_thousands(delta),
                        fmt_thousands(current),
                    );
                }

                // Shutdown signal received — exit cleanly
                _ = rx.changed() => {
                    break;
                }
            }
        }
    });

    ShutdownTx(tx)
}

/// Handle returned to the caller. Drop or call `.stop()` to shut down reporter.
pub struct ShutdownTx(#[allow(dead_code)] watch::Sender<()>);

impl ShutdownTx {
    /// Explicitly stop the reporter. Equivalent to dropping the handle.
    pub fn stop(self) {
        drop(self);
    }
}

// ── Formatting helper ─────────────────────────────────────────────────────────

/// Formats a usize with _ thousands separators: 1_540_158
fn fmt_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);

    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push('_');
        }
        result.push(ch);
    }

    result.chars().rev().collect()
}
