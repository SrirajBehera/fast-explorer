/// macOS directory reader.
///
/// Uses std::fs::read_dir (getdirentries64 under the hood).
/// On macOS/APFS, d_type IS populated in the dirent, so file_type() is free.
/// Inode is read via DirEntryExt::ino() — also free from the dirent, no fstatat.
///
/// This gives us: name + type + inode all from the getdirentries64 buffer,
/// with zero extra syscalls per entry.
use std::{os::unix::fs::DirEntryExt, path::Path};

use super::{DirEntry, EntryType};

pub fn read_dir_entries(
    dir: &Path,
    _buf: &mut Vec<u8>,
    out: &mut Vec<DirEntry>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        // d_type from dirent — no extra syscall on APFS.
        let entry_type = match entry.file_type() {
            Ok(ft) if ft.is_file() => EntryType::File,
            Ok(ft) if ft.is_dir() => EntryType::Dir,
            Ok(ft) if ft.is_symlink() => EntryType::Symlink,
            _ => EntryType::Other,
        };

        // DirEntryExt::ino() reads d_ino straight from the dirent buffer —
        // zero extra syscalls, same as Linux getdents64.
        // entry.metadata() would issue a fstatat() per entry = 1.54M extra syscalls.
        let inode = entry.ino();

        out.push(DirEntry {
            name,
            entry_type,
            inode,
        });
    }
    Ok(())
}
