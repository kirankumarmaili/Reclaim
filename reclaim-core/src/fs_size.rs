//! True on-disk size accounting.
//!
//! We deliberately use allocated blocks (`st_blocks * 512`), not logical file
//! length, so sparse files like OrbStack's `data.img` report the space they
//! actually occupy on disk rather than their nominal size (PRD FR-2). Walking
//! is parallelized to hit the < 5 s `~/Library` target (FR-4).

use jwalk::WalkDir;
use std::path::Path;

/// macOS/Unix `st_blocks` are always reported in 512-byte units, independent of
/// the filesystem's block size.
const BLOCK_SIZE: u64 = 512;

#[cfg(unix)]
fn entry_on_disk_bytes(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * BLOCK_SIZE
}

#[cfg(not(unix))]
fn entry_on_disk_bytes(meta: &std::fs::Metadata) -> u64 {
    meta.len()
}

/// Total on-disk bytes occupied by `path` (a file or a directory tree).
///
/// Symlinks are not followed and hardlinks are counted once per visited entry.
/// Unreadable entries are skipped rather than aborting the walk — a scan must
/// never fail wholesale because one subdirectory is permission-denied.
pub fn on_disk_size(path: &Path) -> u64 {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return 0,
    };

    if meta.is_file() || meta.file_type().is_symlink() {
        return entry_on_disk_bytes(&meta);
    }

    WalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .map(|m| entry_on_disk_bytes(&m))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sums_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("a.bin")).unwrap();
        f.write_all(&[0u8; 8192]).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut g = std::fs::File::create(dir.path().join("sub/b.bin")).unwrap();
        g.write_all(&[0u8; 8192]).unwrap();

        // Two 8 KiB files allocate at least 16 KiB of blocks.
        assert!(on_disk_size(dir.path()) >= 16 * 1024);
    }

    #[test]
    fn missing_path_is_zero() {
        assert_eq!(on_disk_size(Path::new("/no/such/path/xyzzy")), 0);
    }
}
