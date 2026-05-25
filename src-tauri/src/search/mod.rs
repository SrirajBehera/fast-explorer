use crate::models::FileRecord;
use anyhow::Result;
use compact_str::CompactString;
use nucleo::{pattern::{Pattern, CaseMatching}, Matcher};
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The global search engine state.
/// Holds the list of all files in memory for instantaneous fuzzy searching.
#[derive(Clone)]
pub struct SearchEngine {
    pub records: Arc<RwLock<Vec<FileRecord>>>,
    pub db_path: String,
}

impl SearchEngine {
    #[allow(dead_code)]
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        
        println!("Loading entries into memory for fast search...");
        let mut stmt = conn.prepare("SELECT inode, parent_inode, name, is_dir FROM entries")?;
        
        let records_iter = stmt.query_map([], |row| {
            let name_str: String = row.get(2)?;
            Ok(FileRecord {
                inode: row.get(0)?,
                parent_inode: row.get(1)?,
                name: CompactString::new(name_str),
                is_dir: row.get(3)?,
            })
        })?;

        let mut records = Vec::with_capacity(2_000_000);
        for record in records_iter {
            if let Ok(r) = record {
                records.push(r);
            }
        }
        
        println!("Loaded {} entries into memory.", records.len());

        Ok(Self {
            records: Arc::new(RwLock::new(records)),
            db_path: db_path.to_string(),
        })
    }

    /// Performs a fuzzy search and returns the top `limit` matches with resolved full paths.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let records = self.records.read().await;
        
        let mut matcher = Matcher::default();
        let pattern = Pattern::parse(query, CaseMatching::Ignore);

        // Score all records
        let mut scored: Vec<(u32, &FileRecord)> = records
            .iter()
            .filter_map(|record| {
                let u32_str = nucleo::Utf32String::from(record.name.as_str());
                let base_score = pattern.score(u32_str.slice(..), &mut matcher)?;
                
                let mut score = base_score;
                let name_lower = record.name.to_lowercase();
                let query_lower = query.to_lowercase();
                
                // Boost 1: Contiguous substring match (e.g. "resume" in "myresumefile.docx")
                if let Some(idx) = name_lower.find(&query_lower) {
                    score += 400;
                    
                    // Boost 2: Word-Boundary match (e.g. "resume" in "my-resume.txt")
                    let left_bound = idx == 0 || {
                        let c = name_lower.as_bytes().get(idx - 1).map(|&b| b as char).unwrap_or(' ');
                        !c.is_alphanumeric()
                    };
                    let right_bound = idx + query_lower.len() == name_lower.len() || {
                        let c = name_lower.as_bytes().get(idx + query_lower.len()).map(|&b| b as char).unwrap_or(' ');
                        !c.is_alphanumeric()
                    };
                    if left_bound && right_bound {
                        score += 600; // Total 1000 boost for exact word match!
                        
                        // Boost 3: Starts-with match (e.g. "resume.pdf")
                        if idx == 0 {
                            score += 500; // Total 1500 boost!
                        }
                    }
                }
                
                // Boost 4: Directory boost (Folders are high-level entry points)
                if record.is_dir {
                    score += 100;
                }
                
                Some((score, record))
            })
            .collect();

        // Sort by score descending
        scored.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        let top_matches = scored.into_iter().take(limit).map(|(_, r)| r).collect::<Vec<_>>();
        
        // Resolve full paths using SQLite
        let conn = Connection::open(&self.db_path)?;
        let mut results = Vec::with_capacity(top_matches.len());

        for record in top_matches {
            let full_path = resolve_path_recursive(&conn, record)?;
            results.push(SearchResult {
                name: record.name.to_string(),
                path: full_path,
                is_dir: record.is_dir,
            });
        }

        Ok(results)
    }
}

/// A structured result sent to the UI
#[derive(serde::Serialize)]
pub struct SearchResult {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// Recursively traverses `parent_inode` to build the full path.
fn resolve_path_recursive(conn: &Connection, target: &FileRecord) -> Result<String> {
    let mut components = vec![target.name.to_string()];
    let mut current_parent = target.parent_inode;

    // Loop until we hit the root (which has parent_inode 0)
    let mut stmt = conn.prepare_cached("SELECT parent_inode, name FROM entries WHERE inode = ?")?;

    while current_parent != 0 {
        let mut rows = stmt.query([current_parent])?;
        if let Some(row) = rows.next()? {
            let next_parent: u64 = row.get(0)?;
            let name: String = row.get(1)?;
            
            components.push(name);
            current_parent = next_parent;
        } else {
            // Parent not found, break to avoid infinite loop
            break;
        }
    }

    components.reverse();
    Ok(components.join("/"))
}
