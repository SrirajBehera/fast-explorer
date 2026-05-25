/// macOS fast-path directory reader using raw `getdirentries64`.
///
/// ## Why getdirentries64 instead of std::fs::read_dir?
///
/// std::fs::read_dir calls getdirentries64 internally, but adds overhead:
///   - allocates an OsString for each entry name
///   - wraps results in std::fs::DirEntry (heap + reference counting)
///   - returns an iterator that calls the syscall lazily per batch
///
/// We call getdirentries64 directly with our pre-allocated 256KB scratch
/// buffer and parse the packed dirent structs in-place:
///   - name copied directly from the kernel buffer into our DirEntry.name
///   - inode and d_type read from fixed offsets — no extra fstatat
///   - zero heap allocation for the intermediary OsString/DirEntry layer
///
/// ## macOS dirent64 layout (sys/dirent.h, __DARWIN_64_BIT_INO_T):
///
///   offset  0 .. 8  : u64  d_fileno  (inode)
///   offset  8 ..16  : u64  d_seekoff
///   offset 16 ..18  : u16  d_reclen  (total record length, 8-byte aligned)
///   offset 18 ..20  : u16  d_namlen  (name length, NOT including null byte)
///   offset 20       : u8   d_type    (DT_REG / DT_DIR / DT_LNK / …)
///   offset 21 ..    : char d_name[]  (null-terminated, d_namlen bytes + '\0')
use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

use compact_str::CompactString;
use libc::{O_DIRECTORY, O_RDONLY, c_char, c_int, close, open};

use super::{DirEntry, EntryType};

// d_type constants (from <sys/dirent.h>)
const DT_REG: u8 = 8; // regular file
const DT_DIR: u8 = 4; // directory
const DT_LNK: u8 = 10; // symbolic link

const DIRENT_HDR: usize = 21; // bytes before d_name begins
const BUF_SIZE: usize = 256 * 1024; // matches scratch_buf size in scanner

// SYS_getdirentries64 = 344 (stable across macOS versions, defined in <sys/syscall.h>).
// Apple removed the symbol from public headers in macOS 10.15+ but the syscall is
// still present and used internally by libc. We invoke it via the raw syscall gate.
const SYS_GETDIRENTRIES64: libc::c_int = 344;

#[inline]
fn getdirentries64(fd: c_int, buf: &mut [u8], basep: &mut i64) -> libc::ssize_t {
    unsafe {
        libc::syscall(
            SYS_GETDIRENTRIES64,
            fd,
            buf.as_mut_ptr() as *mut c_char,
            buf.len() as i64,
            basep as *mut i64,
        ) as libc::ssize_t
    }
}

pub fn read_dir_entries(
    dir: &Path,
    buf: &mut Vec<u8>,
    out: &mut Vec<DirEntry>,
) -> std::io::Result<()> {
    // Open the directory for reading only.
    let path_cstr = CString::new(dir.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let fd = unsafe { open(path_cstr.as_ptr(), O_RDONLY | O_DIRECTORY) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let _guard = FdGuard(fd);

    // Grow scratch buf if needed (first call only — subsequent calls reuse it).
    if buf.len() < BUF_SIZE {
        buf.resize(BUF_SIZE, 0);
    }

    let mut basep: i64 = 0;

    loop {
        // One syscall fills the buffer with as many dirent records as fit.
        // For a directory with N entries this is typically 1-2 calls.
        let n = getdirentries64(fd, buf, &mut basep);

        match n {
            0 => break, // no more entries
            n if n < 0 => return Err(std::io::Error::last_os_error()),
            n => parse_dirents(&buf[..n as usize], out),
        }
    }

    Ok(())
}

/// Parse packed dirent records from a raw getdirentries64 buffer.
///
/// Each record starts on an 8-byte boundary. `d_reclen` gives the total
/// length including padding. We read the 5 fields we care about at their
/// fixed offsets and copy the name slice directly into a String.
#[inline]
fn parse_dirents(buf: &[u8], out: &mut Vec<DirEntry>) {
    let mut pos = 0;

    while pos + DIRENT_HDR < buf.len() {
        // d_reclen at offset 16
        let reclen = u16::from_ne_bytes(buf[pos + 16..pos + 18].try_into().unwrap()) as usize;
        if reclen == 0 || pos + reclen > buf.len() {
            break;
        }

        let d_type = buf[pos + 20];
        let namlen = u16::from_ne_bytes(buf[pos + 18..pos + 20].try_into().unwrap()) as usize;
        let inode = u64::from_ne_bytes(buf[pos..pos + 8].try_into().unwrap());

        let name_end = pos + DIRENT_HDR + namlen;
        if name_end <= buf.len() {
            let name_bytes = &buf[pos + DIRENT_HDR..name_end];

            // Skip the mandatory "." and ".." entries.
            if name_bytes != b"." && name_bytes != b".." {
                // CompactString::from_utf8_lossy inlines strings ≤ 24 bytes —
                // no heap allocation for filenames that fit (covers ~95% of names).
                let name = CompactString::from_utf8_lossy(name_bytes);

                let entry_type = match d_type {
                    DT_REG => EntryType::File,
                    DT_DIR => EntryType::Dir,
                    DT_LNK => EntryType::Symlink,
                    _ => EntryType::Other,
                };

                out.push(DirEntry {
                    name,
                    entry_type,
                    inode,
                });
            }
        }

        pos += reclen;
    }
}

/// RAII guard that closes the directory fd on any exit path.
struct FdGuard(c_int);

impl Drop for FdGuard {
    fn drop(&mut self) {
        unsafe {
            close(self.0);
        }
    }
}
