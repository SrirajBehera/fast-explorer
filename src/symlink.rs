use std::sync::Arc;

use dashmap::DashSet;

/// Tracks visited directory inodes to detect hard-link / bind-mount cycles.
///
/// ## Why DashSet instead of Mutex<HashSet>?
///
/// The old implementation used a single global `Mutex<HashSet<u64>>` that every
/// worker had to exclusively lock for every directory it visited.  With 10 workers
/// and 165,916 directories that is 165K contested lock acquisitions — all workers
/// serialise behind one another at each check.
///
/// `DashSet` internally shards the set into 2× CPU buckets, each guarded by its
/// own `RwLock`.  Two workers whose target inodes hash into different shards never
/// contend at all.  The typical contention drops from 10-way to ~1-way.
///
/// API is identical to the old `Mutex<HashSet>` version — it is a drop-in swap.
#[derive(Clone)]
pub struct SymlinkGuard {
    visited: Arc<DashSet<u64>>,
}

impl SymlinkGuard {
    pub fn new() -> Self {
        Self {
            visited: Arc::new(DashSet::new()),
        }
    }

    /// Returns `true` if this inode is safe to traverse (first time seen).
    /// Returns `false` if already visited — cycle detected, skip this dir.
    ///
    /// `DashSet::insert` returns `true` when the value was *not* already present,
    /// matching our "first-seen = safe" semantic exactly.
    pub fn check_and_mark(&self, inode: u64) -> bool {
        self.visited.insert(inode)
    }
}
