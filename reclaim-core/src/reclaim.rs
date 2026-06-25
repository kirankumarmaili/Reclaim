//! Reclaim execution — the only path that deletes anything.
//!
//! The flow re-derives authoritative items by re-scanning the root (so the risk
//! class and path come from the *live* filesystem, never from the caller's
//! request), runs every target through the [`safety`](crate::safety) gate, and
//! only then deletes. A rejected or failed item becomes a skip and the batch
//! continues — one failure never aborts the rest (PRD §12).

use crate::safety;
use crate::{Disposition, Item, Mode, ReclaimResult, ReclaimTarget, Skip};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Abstracts the actual deletion so the gate logic is testable without touching
/// the real filesystem or Trash.
pub trait Executor {
    /// Move to Trash (reversible).
    fn to_trash(&self, path: &Path) -> std::io::Result<()>;
    /// Permanently delete.
    fn permanent(&self, path: &Path) -> std::io::Result<()>;
}

/// The production executor: real Trash, real removal.
pub struct SystemExecutor;

impl Executor for SystemExecutor {
    fn to_trash(&self, path: &Path) -> std::io::Result<()> {
        trash::delete(path).map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn permanent(&self, path: &Path) -> std::io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    }
}

/// Reclaim the given targets under `root` using the real system executor.
pub fn reclaim(root: &Path, targets: &[ReclaimTarget]) -> std::io::Result<ReclaimResult> {
    let authoritative = crate::scan::scan(root)?.items;
    Ok(reclaim_with(
        &authoritative,
        targets,
        &safety::home_dir(),
        &SystemExecutor,
    ))
}

/// Testable core: validate each target against freshly-derived `authoritative`
/// items and `home`, then execute via `exec`. Pure of any global state.
pub fn reclaim_with(
    authoritative: &[Item],
    targets: &[ReclaimTarget],
    home: &Path,
    exec: &dyn Executor,
) -> ReclaimResult {
    let by_id: HashMap<&str, &Item> = authoritative.iter().map(|i| (i.id.as_str(), i)).collect();

    let mut result = ReclaimResult {
        requested_ids: targets.iter().map(|t| t.id.clone()).collect(),
        freed_bytes: 0,
        moved_to_trash: Vec::new(),
        permanently_deleted: Vec::new(),
        skipped: Vec::new(),
    };

    for target in targets {
        let Some(item) = by_id.get(target.id.as_str()) else {
            result.skipped.push(Skip {
                id: target.id.clone(),
                reason: "skipped: no longer present in scan (already gone or never existed)".into(),
            });
            continue;
        };

        // THE GATE. Re-validated risk class + HOME boundary. Non-negotiable.
        if let Err(denial) = safety::validate_against_home(item, home) {
            result.skipped.push(Skip {
                id: item.id.clone(),
                reason: denial.reason(),
            });
            continue;
        }

        let path: PathBuf = item.path.clone();
        let outcome = match target.mode {
            Mode::Trash => exec.to_trash(&path),
            Mode::Permanent => exec.permanent(&path),
        };

        match outcome {
            Ok(()) => {
                let disp = Disposition {
                    id: item.id.clone(),
                    path,
                    bytes: item.bytes,
                };
                result.freed_bytes += item.bytes;
                match target.mode {
                    Mode::Trash => result.moved_to_trash.push(disp),
                    Mode::Permanent => result.permanently_deleted.push(disp),
                }
            }
            // A failed delete (in use, permissions) is reported, never fatal.
            Err(e) => result.skipped.push(Skip {
                id: item.id.clone(),
                reason: format!("skipped: deletion failed ({e})"),
            }),
        }
    }

    result
}

impl ReclaimResult {
    /// True if nothing was actually removed.
    pub fn deleted_nothing(&self) -> bool {
        self.moved_to_trash.is_empty() && self.permanently_deleted.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Risk;
    use std::cell::RefCell;

    /// Records what it was asked to delete; never touches the filesystem.
    #[derive(Default)]
    struct SpyExecutor {
        trashed: RefCell<Vec<PathBuf>>,
        deleted: RefCell<Vec<PathBuf>>,
    }
    impl Executor for SpyExecutor {
        fn to_trash(&self, path: &Path) -> std::io::Result<()> {
            self.trashed.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
        fn permanent(&self, path: &Path) -> std::io::Result<()> {
            self.deleted.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
    }

    fn item(home: &Path, id: &str, risk: Risk) -> Item {
        let path = home.join(id);
        std::fs::create_dir_all(&path).unwrap();
        Item {
            id: id.into(),
            name: id.into(),
            path,
            bytes: 1000,
            risk,
            detector: "test".into(),
            rationale: String::new(),
            reversible: true,
        }
    }

    fn target(id: &str, mode: Mode) -> ReclaimTarget {
        ReclaimTarget { id: id.into(), mode }
    }

    #[test]
    fn trash_is_the_default_and_protected_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let items = vec![
            item(home.path(), "safe", Risk::Safe),
            item(home.path(), "prot", Risk::Protected),
        ];
        let spy = SpyExecutor::default();

        // A request that asks to delete BOTH — including the Protected item.
        let targets = vec![target("safe", Mode::Trash), target("prot", Mode::Trash)];
        let res = reclaim_with(&items, &targets, home.path(), &spy);

        // Safe item went to Trash; Protected was refused by the gate.
        assert_eq!(res.moved_to_trash.len(), 1);
        assert_eq!(res.moved_to_trash[0].id, "safe");
        assert_eq!(res.freed_bytes, 1000);
        assert_eq!(res.skipped.len(), 1);
        assert_eq!(res.skipped[0].id, "prot");
        assert!(res.skipped[0].reason.contains("Protected"));
        // The executor was never even asked to touch the protected path.
        assert_eq!(spy.trashed.borrow().len(), 1);
        assert!(spy.deleted.borrow().is_empty());
    }

    #[test]
    fn forged_risk_class_cannot_widen_deletion() {
        // The caller claims a Protected item is Safe by sending its id. The gate
        // uses the AUTHORITATIVE item (Protected), so the forge is ignored.
        let home = tempfile::tempdir().unwrap();
        let authoritative = vec![item(home.path(), "vol", Risk::Protected)];
        let spy = SpyExecutor::default();
        let res = reclaim_with(&authoritative, &[target("vol", Mode::Permanent)], home.path(), &spy);
        assert!(res.permanently_deleted.is_empty());
        assert!(res.deleted_nothing());
        assert!(res.skipped[0].reason.contains("Protected"));
    }

    #[test]
    fn one_failure_does_not_abort_the_batch() {
        let home = tempfile::tempdir().unwrap();
        let items = vec![
            item(home.path(), "a", Risk::Safe),
            item(home.path(), "b", Risk::Safe),
        ];
        // Executor that fails on "a" but succeeds on "b".
        struct Flaky;
        impl Executor for Flaky {
            fn to_trash(&self, path: &Path) -> std::io::Result<()> {
                if path.ends_with("a") {
                    Err(std::io::Error::other("in use"))
                } else {
                    Ok(())
                }
            }
            fn permanent(&self, _: &Path) -> std::io::Result<()> {
                Ok(())
            }
        }
        let res = reclaim_with(
            &items,
            &[target("a", Mode::Trash), target("b", Mode::Trash)],
            home.path(),
            &Flaky,
        );
        assert_eq!(res.moved_to_trash.len(), 1);
        assert_eq!(res.moved_to_trash[0].id, "b");
        assert_eq!(res.skipped.len(), 1);
        assert!(res.skipped[0].reason.contains("in use"));
    }

    #[test]
    fn permanent_only_when_explicitly_requested() {
        let home = tempfile::tempdir().unwrap();
        let items = vec![item(home.path(), "big", Risk::Safe)];
        let spy = SpyExecutor::default();
        let res = reclaim_with(&items, &[target("big", Mode::Permanent)], home.path(), &spy);
        assert_eq!(res.permanently_deleted.len(), 1);
        assert_eq!(spy.deleted.borrow().len(), 1);
        assert!(spy.trashed.borrow().is_empty());
    }
}
