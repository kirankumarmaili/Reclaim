//! Scan orchestration: resolve the root, gather disk capacity, run the detector
//! catalogue, and assemble a [`ScanResult`] (the `Prd.md` §10 contract).

use crate::detectors::{self, Context};
use crate::{DiskInfo, ScanResult};
use std::path::{Path, PathBuf};

/// Scan `root`, classifying reclaimable items via the detector catalogue.
///
/// `root` defaults to `~/Library` when it is `~/Library` or `$HOME`; any path is
/// accepted but the safety gate still confines *deletion* to `$HOME`.
pub fn scan(root: &Path) -> std::io::Result<ScanResult> {
    let root = expand(root);
    let home = crate::safety::home_dir();

    let ctx = Context {
        home,
        root: root.clone(),
    };

    let items = detectors::run_all(&ctx);
    // Sum of classified items, not a full-tree walk. The capacity context the UI
    // needs (total disk, free, projected-free) comes from `disk` + the selection
    // sum; a recursive size of the whole root would cost ~20 s for no UX gain and
    // would blow the < 5 s budget (FR-4). Detectors are the only walk we pay for.
    let root_total_bytes = items.iter().map(|i| i.bytes).sum();
    let disk = disk_info(&root);

    Ok(ScanResult {
        root,
        scanned_at: chrono::Utc::now().to_rfc3339(),
        disk,
        root_total_bytes,
        items,
    })
}

/// Expand a leading `~` to the home directory.
fn expand(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~") {
        return crate::safety::home_dir().join(rest);
    }
    path.to_path_buf()
}

/// Capacity of the volume containing `path` via `statvfs`.
fn disk_info(path: &Path) -> DiskInfo {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c = match CString::new(path.as_os_str().as_bytes()) {
            Ok(c) => c,
            Err(_) => return DiskInfo { total_bytes: 0, free_bytes: 0 },
        };
        // SAFETY: `stat` is zeroed and `c` is a valid NUL-terminated path.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
        if rc == 0 {
            let frsize = stat.f_frsize as u64;
            return DiskInfo {
                total_bytes: stat.f_blocks as u64 * frsize,
                free_bytes: stat.f_bavail as u64 * frsize,
            };
        }
    }
    DiskInfo { total_bytes: 0, free_bytes: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_info_reports_capacity() {
        let info = disk_info(Path::new("/"));
        assert!(info.total_bytes > 0);
        assert!(info.free_bytes <= info.total_bytes);
    }

    #[test]
    fn tilde_expands() {
        let home = crate::safety::home_dir();
        assert_eq!(expand(Path::new("~/Library")), home.join("Library"));
    }
}
