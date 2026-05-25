#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod fallback;

use compact_str::CompactString;

/// Unified directory entry — same shape on all platforms.
///
/// ## Why CompactString instead of String?
///
/// On APFS the average filename is ~15 characters.  `CompactString` stores
/// strings up to 24 bytes inline (on the stack / in the struct) without any
/// heap allocation.  For a scan of 1.54M files + 165K dirs that eliminates
/// roughly 1.5M of the 1.7M name heap allocations — all the short names.
/// Longer names (> 24 bytes) fall back to a heap allocation exactly like String.
/// The API is identical to String via Deref<Target = str>.
#[derive(Debug)]
pub struct DirEntry {
    /// Entry name only — no full path, avoids per-entry PathBuf allocation.
    pub name: CompactString,
    pub entry_type: EntryType,
    /// Inode — used by SymlinkGuard for cycle detection at zero extra cost.
    pub inode: u64,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum EntryType {
    File,
    Dir,
    Symlink,
    Other,
}

/// Read all entries in `dir` into `out`, reusing `buf` as a scratch buffer.
///
/// `out` is cleared on each call.  Allocate `buf` once per worker with
/// `new_scratch_buf()` and `out` with reasonable capacity; pass both in on
/// every call so no allocations happen inside the hot loop.
pub fn read_dir_entries(
    dir: &std::path::Path,
    buf: &mut Vec<u8>,
    out: &mut Vec<DirEntry>,
) -> std::io::Result<()> {
    out.clear();

    #[cfg(target_os = "macos")]
    return macos::read_dir_entries(dir, buf, out);

    #[cfg(not(target_os = "macos"))]
    return fallback::read_dir_entries(dir, out);
}

/// Allocate a scratch buffer once per worker.
/// 256 KB matches the getdirentries64 read size — one syscall fills it.
pub fn new_scratch_buf() -> Vec<u8> {
    vec![0u8; 256 * 1024]
}
