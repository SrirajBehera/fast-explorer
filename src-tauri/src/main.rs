#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod db;
mod models;
mod platform;
mod scanner;
mod search;
mod symlink;
mod telemetry;
mod watcher;

use anyhow::Result;
use compact_str::CompactString;
use models::FileRecord;
use std::os::unix::fs::MetadataExt;
use tauri::State;
use tokio_util::sync::CancellationToken;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::hash::{Hash, Hasher};

#[tauri::command]
async fn perform_search(
    query: String,
    limit: usize,
    engine: State<'_, Arc<search::SearchEngine>>,
) -> Result<Vec<search::SearchResult>, String> {
    engine.search(&query, limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_total_count(
    engine: State<'_, Arc<search::SearchEngine>>,
) -> Result<usize, String> {
    let records = engine.records.read().await;
    Ok(records.len())
}

#[tauri::command]
fn open_file(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct DirEntryInfo {
    name: String,
    path: String,
    is_dir: bool,
    size: Option<u64>,
}

#[tauri::command]
fn get_directory_contents(path: String) -> Result<Vec<DirEntryInfo>, String> {
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(&path).map_err(|e| e.to_string())?;
    for entry in read_dir {
        if let Ok(entry) = entry {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path_buf = entry.path();
            let path_str = path_buf.to_string_lossy().to_string();
            let is_dir = path_buf.is_dir();
            let size = path_buf.metadata().ok().map(|m| m.len());
            entries.push(DirEntryInfo {
                name,
                path: path_str,
                is_dir,
                size,
            });
        }
    }
    entries.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            b.is_dir.cmp(&a.is_dir)
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });
    Ok(entries)
}
#[tauri::command]
fn get_home_dir() -> Result<String, String> {
    let home_dir = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    Ok(home_dir.to_string_lossy().to_string())
}

#[tauri::command]
fn close_window(window: tauri::Window) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

#[tauri::command]
fn minimize_window(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn toggle_fullscreen(window: tauri::Window) -> Result<(), String> {
    let is_fullscreen = window.is_fullscreen().map_err(|e| e.to_string())?;
    window.set_fullscreen(!is_fullscreen).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct FileMetadataInfo {
    path: String,
    size: u64,
    is_dir: bool,
    created: Option<u64>,     // Unix timestamp
    modified: Option<u64>,    // Unix timestamp
    item_count: Option<usize>, // Item count if directory
}

#[tauri::command]
async fn get_file_metadata(path: String) -> Result<FileMetadataInfo, String> {
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let is_dir = metadata.is_dir();
    let size = metadata.len();

    let created = metadata.created().ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let modified = metadata.modified().ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let item_count = if is_dir {
        std::fs::read_dir(&path).ok().map(|rd| rd.filter_map(|e| e.ok()).count())
    } else {
        None
    };

    Ok(FileMetadataInfo {
        path,
        size,
        is_dir,
        created,
        modified,
        item_count,
    })
}
#[tauri::command]
fn open_terminal(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-a")
            .arg("Terminal")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .arg("/c")
            .arg("start")
            .arg("cmd")
            .current_dir(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("x-terminal-emulator")
            .current_dir(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_in_vscode(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-a")
            .arg("Visual Studio Code")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        std::process::Command::new("code")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        let parent = std::path::Path::new(&path).parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        std::process::Command::new("xdg-open")
            .arg(&parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_system_setting(pane: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = format!("x-apple.systempreferences:com.apple.preference.{}", pane);
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn get_icon_name_from_plist(app_path: &str) -> Option<String> {
    let plist_path = format!("{}/Contents/Info.plist", app_path);
    if !std::path::Path::new(&plist_path).exists() {
        return None;
    }
    let output = std::process::Command::new("plutil")
        .arg("-extract")
        .arg("CFBundleIconFile")
        .arg("raw")
        .arg(&plist_path)
        .output()
        .ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn locate_icns_file(app_path: &str) -> Option<String> {
    let resources_dir = format!("{}/Contents/Resources", app_path);
    let resources_path = std::path::Path::new(&resources_dir);
    if !resources_path.exists() || !resources_path.is_dir() {
        return None;
    }

    if let Some(icon_name) = get_icon_name_from_plist(app_path) {
        let mut candidates = vec![icon_name.clone()];
        if !icon_name.ends_with(".icns") {
            candidates.push(format!("{}.icns", icon_name));
        }
        for candidate in candidates {
            let candidate_path = resources_path.join(&candidate);
            if candidate_path.exists() && candidate_path.is_file() {
                return Some(candidate_path.to_string_lossy().to_string());
            }
        }
    }

    // Fallback: search the Resources folder for any .icns file
    if let Ok(entries) = std::fs::read_dir(resources_path) {
        let mut first_icns = None;
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_ascii_lowercase() == "icns" {
                        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
                        if stem.contains("appicon") || stem.contains("icon") {
                            return Some(path.to_string_lossy().to_string());
                        }
                        if first_icns.is_none() {
                            first_icns = Some(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
        if let Some(p) = first_icns {
            return Some(p);
        }
    }

    None
}

fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() { data[i + 1] as usize } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as usize } else { 0 };

        let val = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARSET[(val >> 18) & 63] as char);
        result.push(CHARSET[(val >> 12) & 63] as char);

        if i + 1 < data.len() {
            result.push(CHARSET[(val >> 6) & 63] as char);
        } else {
            result.push('=');
        }

        if i + 2 < data.len() {
            result.push(CHARSET[val & 63] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }
    result
}

#[tauri::command]
fn get_app_icon(path: String) -> Result<String, String> {
    if !path.ends_with(".app") {
        return Err("Not an application bundle".to_string());
    }

    let app_path = std::path::Path::new(&path);
    let app_name = app_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| "Could not find data local dir".to_string())?
        .join("fast-explorer")
        .join("cache")
        .join("icons");

    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let hash_val = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        hasher.finish()
    };

    let cache_png_path = data_dir.join(format!("{}_{}.png", app_name, hash_val));

    // If cache exists, read and return base64
    if cache_png_path.exists() {
        let bytes = std::fs::read(&cache_png_path).map_err(|e| e.to_string())?;
        return Ok(format!("data:image/png;base64,{}", base64_encode(&bytes)));
    }

    // Locate .icns file
    let icns_path = locate_icns_file(&path)
        .ok_or_else(|| format!("Could not find .icns file for {}", app_name))?;

    // Convert using sips to 128x128 PNG
    let status = std::process::Command::new("sips")
        .arg("-s")
        .arg("format")
        .arg("png")
        .arg("-z")
        .arg("128")
        .arg("128")
        .arg(&icns_path)
        .arg("--out")
        .arg(&cache_png_path)
        .output()
        .map_err(|e| format!("sips execute failed: {}", e))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(format!("sips failed: {}", stderr));
    }

    // Read and return base64
    let bytes = std::fs::read(&cache_png_path).map_err(|e| e.to_string())?;
    Ok(format!("data:image/png;base64,{}", base64_encode(&bytes)))
}

fn main() {
    // 1. Initialize scanning directory (default to user's home dir)
    let home_dir = dirs::home_dir().expect("Could not find home directory");
    let scan_path = home_dir.to_string_lossy().to_string();

    // Store fex.db in the OS app-data dir so Tauri's hot-reload watcher
    // (which watches src-tauri/) never sees DB writes and doesn't infinite-loop.
    let data_dir = dirs::data_local_dir()
        .expect("Could not find data dir")
        .join("fast-explorer");
    std::fs::create_dir_all(&data_dir).expect("Failed to create app data dir");
    let db_path = data_dir.join("fex.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    println!("Database: {}", db_path_str);

    // 2. Pre-create the search engine state with empty records
    let engine = Arc::new(search::SearchEngine {
        records: Arc::new(RwLock::new(Vec::new())),
        db_path: db_path_str.clone(),
    });

    let engine_state = engine.clone();
    let db_path_clone = db_path_str.clone();
    let scan_path_clone = scan_path.clone();

    // 3. Start Tauri Window
    let app = tauri::Builder::default()
        .manage(engine)
        .setup(move |_app| {
            let engine_state = engine_state.clone();
            let db_path_str = db_path_clone.clone();
            let scan_path = scan_path_clone.clone();

            // Spawn the background scanning and loading task
            tauri::async_runtime::spawn(async move {
                // Let's first load any existing records into memory so search works immediately!
                println!("Pre-loading existing entries from database...");
                let db_path_for_load = db_path_str.clone();
                let records = tauri::async_runtime::spawn_blocking(move || {
                    let mut records = Vec::new();
                    if let Ok(conn) = rusqlite::Connection::open(&db_path_for_load) {
                        if let Ok(mut stmt) = conn.prepare("SELECT inode, parent_inode, name, is_dir FROM entries") {
                            let records_iter = stmt.query_map([], |row| {
                                let name_str: String = row.get(2)?;
                                Ok(FileRecord {
                                    inode: row.get(0)?,
                                    parent_inode: row.get(1)?,
                                    name: CompactString::new(name_str),
                                    is_dir: row.get(3)?,
                                })
                            });

                            if let Ok(records_iter) = records_iter {
                                records.reserve(2_000_000);
                                for record in records_iter {
                                    if let Ok(r) = record {
                                        records.push(r);
                                    }
                                }
                            }
                        }
                    }
                    records
                }).await.unwrap_or_default();

                let count = records.len();
                *engine_state.records.write().await = records;
                println!("Pre-loaded {} entries into memory from existing DB.", count);

                // Start the live watcher
                if let Err(e) = watcher::start_watcher(&scan_path, db_path_str.clone()).await {
                    eprintln!("Warning: Failed to start live watcher: {}", e);
                }

                // Sequential scanning pipeline: Applications first (instant), then Home dir
                let scan_paths = vec![
                    "/Applications".to_string(),
                    scan_path.clone(),
                ];

                for path in scan_paths {
                    if !std::path::Path::new(&path).exists() {
                        continue;
                    }

                    // Initialize the DB connection & schema inside the loop
                    let db_path_for_db = db_path_str.clone();
                    let mut database = match db::Db::new(&db_path_for_db) {
                        Ok(d) => d,
                        Err(e) => {
                            eprintln!("Failed to initialize DB for {}: {}", path, e);
                            continue;
                        }
                    };

                    let root_inode = match std::fs::metadata(&path) {
                        Ok(meta) => meta.ino(),
                        Err(e) => {
                            eprintln!("Failed to stat root path {}: {}", path, e);
                            continue;
                        }
                    };

                    // Ensure root inode is in the database
                    let _ = database.insert_batch(&[FileRecord {
                        inode: root_inode,
                        parent_inode: 0,
                        name: CompactString::new(path.clone()),
                        is_dir: true,
                    }]);

                    let worker_count = num_cpus::get();
                    println!("Starting {} background scanning workers on {}...", worker_count, path);

                    let metrics = telemetry::Metrics::new();
                    let cancel = CancellationToken::new();
                    let reporter = telemetry::reporter::spawn(metrics.files_found.clone());

                    match scanner::run(
                        path.clone(),
                        root_inode,
                        worker_count,
                        metrics.clone(),
                        cancel,
                        database,
                    ).await {
                        Ok(total_files) => {
                            reporter.stop();
                            println!("Background scan complete for {}. Total files scanned: {}", path, total_files);
                        }
                        Err(e) => {
                            reporter.stop();
                            eprintln!("Background scanner failed for {}: {}", path, e);
                        }
                    }
                }

                // Reload all entries from the updated DB to match the latest filesystem state
                println!("Reloading entries from updated DB into memory...");
                let db_path_for_reload = db_path_str.clone();
                let records = tauri::async_runtime::spawn_blocking(move || {
                    let mut records = Vec::new();
                    if let Ok(conn) = rusqlite::Connection::open(&db_path_for_reload) {
                        if let Ok(mut stmt) = conn.prepare("SELECT inode, parent_inode, name, is_dir FROM entries") {
                            let records_iter = stmt.query_map([], |row| {
                                let name_str: String = row.get(2)?;
                                Ok(FileRecord {
                                    inode: row.get(0)?,
                                    parent_inode: row.get(1)?,
                                    name: CompactString::new(name_str),
                                    is_dir: row.get(3)?,
                                })
                            });

                            if let Ok(records_iter) = records_iter {
                                records.reserve(2_000_000);
                                for record in records_iter {
                                    if let Ok(r) = record {
                                        records.push(r);
                                    }
                                }
                            }
                        }
                    }
                    records
                }).await.unwrap_or_default();

                let count = records.len();
                *engine_state.records.write().await = records;
                println!("Reloaded {} entries into memory from updated DB.", count);
            });

            Ok(())
        })
        .on_window_event(|event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let _ = event.window().hide();
                api.prevent_close();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            perform_search,
            get_total_count,
            open_file,
            get_directory_contents,
            get_home_dir,
            close_window,
            minimize_window,
            toggle_fullscreen,
            get_file_metadata,
            open_terminal,
            open_in_vscode,
            reveal_in_finder,
            get_app_icon,
            open_system_setting
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| match event {
        tauri::RunEvent::Ready => {
            println!("Tauri application is ready!");
        }
        _ => {}
    });
}
