//! Tauri command layer. This is the *thin* bridge from the webview to
//! reclaim-core — it contains NO business logic. The two commands map directly
//! onto the core's `scan` and `reclaim`, and the safety gate lives entirely in
//! the core, so this layer cannot widen what is deletable.

use reclaim_core::{reclaim as core_reclaim, scan as core_scan, ReclaimResult, ReclaimTarget, ScanResult};
use std::path::PathBuf;

/// Scan a root (defaults to `~/Library`). Returns the §10 schema.
#[tauri::command]
fn scan(root: Option<String>) -> Result<ScanResult, String> {
    let root = root
        .map(PathBuf::from)
        .unwrap_or_else(|| reclaim_core::safety::home_dir().join("Library"));
    core_scan::scan(&root).map_err(|e| e.to_string())
}

/// Reclaim the given targets. The core re-derives risk + path and refuses
/// anything Protected or outside `$HOME`, regardless of what the UI claims.
#[tauri::command]
fn reclaim(targets: Vec<ReclaimTarget>) -> Result<ReclaimResult, String> {
    // The root is re-scanned inside the core to produce authoritative items.
    let root = reclaim_core::safety::home_dir();
    core_reclaim::reclaim(&root, &targets).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![scan, reclaim])
        .run(tauri::generate_context!())
        .expect("error while running Reclaim");
}
