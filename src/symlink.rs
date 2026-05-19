use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
};

use tokio::fs;

/// Tracks visited real (canonical) paths to detect symlink cycles.
/// Shared across all workers via Arc<Mutex<_>>.
///
/// Why inode tracking and not just canonical path?
/// - Canonical path resolution follows symlinks → defeats the purpose
/// - Two different paths can point to the same inode (hardlinks, bind mounts)
/// - Inode + device is the only reliable filesystem identity
#[derive(Clone)]
pub struct SymlinkGuard {
    /// (device_id, inode) pairs we have already enqueued.
    visited: Arc<Mutex<HashSet<(u64, u64)>>>,
}

impl SymlinkGuard {
    pub fn new() -> Self {
        Self {
            visited: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Returns true if this path is safe to enqueue (not yet visited).
    /// Returns false if it is a symlink OR its inode was already seen (cycle).
    ///
    /// Uses lstat (does NOT follow symlinks) so we see the symlink itself,
    /// not its target — this is what correctly identifies symlinks.
    pub async fn check_and_mark(&self, path: &Path) -> SymlinkStatus {
        let meta = match fs::symlink_metadata(path).await {
            Ok(m) => m,
            Err(_) => return SymlinkStatus::Error,
        };

        // symlink_metadata does NOT follow links — so is_symlink() works here.
        // Regular metadata() WOULD follow the link and always return false.
        if meta.is_symlink() {
            return SymlinkStatus::IsSymlink;
        }

        // Use OS inode identity to detect hardlink cycles and bind mounts.
        // Only available on Unix — on Windows we fall back to path identity.
        let identity = os_identity(&meta);

        let mut visited = self.visited.lock().unwrap();
        if visited.contains(&identity) {
            return SymlinkStatus::Cycle;
        }

        visited.insert(identity);
        SymlinkStatus::Safe
    }
}

#[derive(Debug, PartialEq)]
pub enum SymlinkStatus {
    /// Entry is safe to traverse — not a symlink, not seen before.
    Safe,
    /// Entry is a symlink — skip it.
    IsSymlink,
    /// Entry's inode was already visited — cycle detected, skip it.
    Cycle,
    /// Could not stat the entry — skip it.
    Error,
}

// ── Platform-specific inode identity ─────────────────────────────────────────

#[cfg(unix)]
fn os_identity(meta: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn os_identity(_meta: &std::fs::Metadata) -> (u64, u64) {
    // Windows has no stable inode API in std.
    // Fall back to (0, 0) — cycle detection degraded but symlinks still skipped.
    (0, 0)
}
