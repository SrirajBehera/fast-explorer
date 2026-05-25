use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};

/// All runtime metrics for the scanner.
/// Cheaply cloneable — backed by Arcs internally.
///
/// ## Atomic ordering rationale
///
/// `in_flight` coordinates the shutdown handshake between workers and the watcher:
///   - Workers use `Release` on writes so the watcher's subsequent `Acquire` load
///     is guaranteed to observe the decremented value after `notify_one()`.
///   - The `tokio::Notify` pair (notify_one / notified) provides additional
///     sequencing but we make the atomic ordering explicit for clarity.
///
/// All other counters (`files_found`, `dirs_scanned`, etc.) are independent tallies
/// read only at program exit — `Relaxed` is correct and avoids memory-barrier cost
/// on every increment.  At 1.54 M file events, `SeqCst` barriers there were
/// measurable overhead for no correctness benefit.
#[derive(Clone)]
pub struct Metrics {
    /// Dirs currently queued OR being processed.
    /// Incremented before send, decremented after full processing.
    pub in_flight: Arc<AtomicI64>,

    /// Highest value `in_flight` has ever reached — shows peak queue pressure.
    pub peak_in_flight: Arc<AtomicUsize>,

    /// Total directories successfully scanned.
    pub dirs_scanned: Arc<AtomicUsize>,

    /// Total directories that failed to open (permissions, broken symlinks, etc).
    pub dirs_failed: Arc<AtomicUsize>,

    /// Total files discovered.
    pub files_found: Arc<AtomicUsize>,

    /// Symlinks encountered and skipped.
    pub symlinks_skipped: Arc<AtomicUsize>,

    /// Symlink cycles detected and prevented.
    pub cycles_detected: Arc<AtomicUsize>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            in_flight: Arc::new(AtomicI64::new(0)),
            peak_in_flight: Arc::new(AtomicUsize::new(0)),
            dirs_scanned: Arc::new(AtomicUsize::new(0)),
            dirs_failed: Arc::new(AtomicUsize::new(0)),
            files_found: Arc::new(AtomicUsize::new(0)),
            symlinks_skipped: Arc::new(AtomicUsize::new(0)),
            cycles_detected: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Call after every `in_flight` increment to keep peak up to date.
    /// Relaxed is correct: peak is a best-effort high-water mark, not a
    /// synchronisation point.
    pub fn update_peak(&self) {
        let current = self.in_flight.load(Ordering::Relaxed) as usize;
        self.peak_in_flight.fetch_max(current, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn print(&self) {
        println!(
            "Peak dirs in-flight:      {}",
            self.peak_in_flight.load(Ordering::Relaxed)
        );
        println!(
            "Dirs scanned:             {}",
            self.dirs_scanned.load(Ordering::Relaxed)
        );
        println!(
            "Dirs failed:              {}",
            self.dirs_failed.load(Ordering::Relaxed)
        );
        println!(
            "Files found:              {}",
            self.files_found.load(Ordering::Relaxed)
        );
        println!(
            "Symlinks skipped:         {}",
            self.symlinks_skipped.load(Ordering::Relaxed)
        );
        println!(
            "Cycles detected:          {}",
            self.cycles_detected.load(Ordering::Relaxed)
        );
    }
}
