use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};

/// All runtime metrics for the scanner.
/// Cheaply cloneable — backed by Arcs internally.
#[derive(Clone)]
pub struct Metrics {
    /// Dirs currently queued OR being processed.
    /// Incremented before send, decremented after full processing.
    pub in_flight: Arc<AtomicI64>,

    /// Highest value in_flight has ever reached — shows peak queue pressure.
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

    /// Call after every in_flight increment to keep peak up to date.
    pub fn update_peak(&self) {
        let current = self.in_flight.load(Ordering::SeqCst) as usize;
        self.peak_in_flight.fetch_max(current, Ordering::SeqCst);
    }

    pub fn print(&self) {
        println!(
            "Peak dirs in-flight:      {}",
            self.peak_in_flight.load(Ordering::SeqCst)
        );
        println!(
            "Dirs scanned:             {}",
            self.dirs_scanned.load(Ordering::SeqCst)
        );
        println!(
            "Dirs failed:              {}",
            self.dirs_failed.load(Ordering::SeqCst)
        );
        println!(
            "Files found:              {}",
            self.files_found.load(Ordering::SeqCst)
        );
        println!(
            "Symlinks skipped:         {}",
            self.symlinks_skipped.load(Ordering::SeqCst)
        );
        println!(
            "Cycles detected:          {}",
            self.cycles_detected.load(Ordering::SeqCst)
        );
    }
}
