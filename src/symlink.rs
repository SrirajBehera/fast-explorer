use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

/// Tracks visited inodes to detect symlink cycles.
/// Shared across all workers via Arc<Mutex>.
///
/// On macOS, inode comes directly from getattrlistbulk at zero extra cost.
/// On Linux, inode comes from the getdents64 d_ino field — also free.
/// No extra fstatat/stat syscall needed for cycle detection.
#[derive(Clone)]
pub struct SymlinkGuard {
    visited: Arc<Mutex<HashSet<u64>>>,
}

impl SymlinkGuard {
    pub fn new() -> Self {
        Self {
            visited: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Returns true if safe to traverse (inode seen for first time).
    /// Returns false if already visited — cycle detected, skip this dir.
    pub fn check_and_mark(&self, inode: u64) -> bool {
        let mut visited = self.visited.lock().unwrap();
        visited.insert(inode) // false = already present = cycle
    }
}
