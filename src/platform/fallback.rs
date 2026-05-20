/// Fallback directory reader for Linux and other Unix platforms.
///
/// Uses std::fs::read_dir which calls getdents64 under the hood.
/// On Linux with ext4/btrfs/xfs, d_type IS populated in the dirent,
/// so DirEntry::file_type() does NOT issue a separate fstatat —
/// it returns the cached value from the getdents64 result.
///
/// This means on Linux we already get near-optimal syscall counts
/// without needing a custom syscall layer. The main gains on Linux
/// come from batch sends and reduced allocations instead.
use std::{fs, path::Path};

use super::{DirEntry, EntryType};

pub fn read_dir_entries(dir: &Path) -> std::io::Result<Vec<DirEntry>> {
    let read_dir = fs::read_dir(dir)?;
    let mut entries = Vec::with_capacity(64);

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        // file_type() on Linux uses the d_type field already in the
        // getdents64 result — no extra fstatat syscall for most filesystems.
        let entry_type = match entry.file_type() {
            Ok(ft) if ft.is_file() => EntryType::File,
            Ok(ft) if ft.is_dir() => EntryType::Dir,
            Ok(ft) if ft.is_symlink() => EntryType::Symlink,
            _ => EntryType::Other,
        };

        // Get inode for cycle detection via metadata
        // On Linux this uses the cached stat from getdents64 — no extra syscall
        let inode = entry
            .metadata()
            .map(|m| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    m.ino()
                }
                #[cfg(not(unix))]
                {
                    0u64
                }
            })
            .unwrap_or(0);

        entries.push(DirEntry {
            name,
            entry_type,
            inode,
        });
    }

    Ok(entries)
}
