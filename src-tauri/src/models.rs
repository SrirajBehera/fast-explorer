use compact_str::CompactString;

/// Represents a file or directory discovered during the scan.
/// Designed to be lightweight for passing across channels and inserting into the database.
#[derive(Debug, Clone)]
pub struct FileRecord {
    pub inode: u64,
    pub parent_inode: u64,
    /// The name of the file or directory. CompactString stores strings ≤24 bytes inline.
    pub name: CompactString,
    pub is_dir: bool,
}
