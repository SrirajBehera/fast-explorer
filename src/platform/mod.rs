#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod fallback;

/// Unified directory entry — same shape on all platforms.
#[derive(Debug)]
pub struct DirEntry {
    /// Entry name only, not full path — avoids per-entry PathBuf allocation.
    pub name: String,
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
/// `out` is cleared on each call. `buf` is never reallocated — allocate
/// it once per worker with `new_scratch_buf()` and pass it in every call.
///
/// Platform behaviour:
///   macOS : std::fs::read_dir — benchmarked faster than getattrlistbulk
///           on shallow dirs (avg 9 entries). getattrlistbulk only wins
///           at 500+ entries/dir which this workload never reaches.
///   Linux : std::fs::read_dir — getdents64 with cached d_type, no fstatat.
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

/// Allocate a scratch buffer once per worker — passed into read_dir_entries
/// every call so no allocation happens inside the hot loop.
/// Size is kept at 256KB to match a typical getattrlistbulk buffer,
/// in case we re-enable it for large-dir workloads later.
pub fn new_scratch_buf() -> Vec<u8> {
    vec![0u8; 256 * 1024]
}
