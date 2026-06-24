//! The safety gate (PRD §11). The product's hard correctness bar — *zero
//! false-deletion of Protected data* — is enforced here, server-side, on every
//! delete. The UI and the agent both pass through this; neither can widen what
//! is deletable.
//!
//! Two independent checks, both of which must pass before any deletion:
//!   1. The item's *current* risk class (re-derived from the live filesystem,
//!      never trusted from the request) is not Protected.
//!   2. The item's *canonical* path (symlinks resolved) is inside the user's
//!      home directory.

use crate::{Item, Risk};
use std::path::{Path, PathBuf};

/// Why a target was rejected by the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denial {
    /// The item is Protected — real data or in-use config.
    Protected,
    /// The path resolves outside the user's home directory.
    OutsideHome,
    /// The path no longer exists (already gone, or never existed).
    Missing,
    /// The path could not be canonicalized (broken symlink, permissions).
    Unresolvable,
}

impl Denial {
    pub fn reason(&self) -> String {
        match self {
            Denial::Protected => "rejected: item is Protected and can never be deleted".into(),
            Denial::OutsideHome => {
                "rejected: path is outside the home directory; the core never operates outside $HOME".into()
            }
            Denial::Missing => "skipped: path no longer exists".into(),
            Denial::Unresolvable => "skipped: path could not be resolved".into(),
        }
    }
}

/// The user's home directory — the only tree the core will ever touch.
pub fn home_dir() -> PathBuf {
    dirs::home_dir().expect("a home directory is required")
}

/// Resolve a path to its real location with symlinks followed, so a symlink
/// pointing outside `$HOME` cannot be used to escape the boundary.
fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// True iff `path` (after symlink resolution) is the home dir or strictly inside it.
pub fn within_home(path: &Path) -> bool {
    within(path, &home_dir())
}

fn within(path: &Path, home: &Path) -> bool {
    let (Some(real), Some(home_real)) = (canonical(path), canonical(home)) else {
        return false;
    };
    real.starts_with(&home_real)
}

/// The gate. Validate a single target against an `authoritative` item that was
/// *just re-derived from the live filesystem* — not the caller's claim.
///
/// Returns `Ok(())` only when the item is non-Protected, exists, and lives
/// inside `$HOME`. Any failure is a [`Denial`], and the caller records a skip
/// and moves on (a rejected item never aborts the batch).
pub fn validate(authoritative: &Item) -> Result<(), Denial> {
    validate_against_home(authoritative, &home_dir())
}

/// Testable core of [`validate`] with an injectable home root.
pub fn validate_against_home(authoritative: &Item, home: &Path) -> Result<(), Denial> {
    // 1. Re-derived risk class. We never read a risk class from the request —
    //    `authoritative` came from re-running the detectors.
    if authoritative.risk == Risk::Protected {
        return Err(Denial::Protected);
    }

    // 2. The path must still exist.
    if std::fs::symlink_metadata(&authoritative.path).is_err() {
        return Err(Denial::Missing);
    }

    // 3. The canonical path must be inside home. Resolves `..` and symlinks.
    let real = canonical(&authoritative.path).ok_or(Denial::Unresolvable)?;
    let home_real = canonical(home).ok_or(Denial::Unresolvable)?;
    if !real.starts_with(&home_real) {
        return Err(Denial::OutsideHome);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: PathBuf, risk: Risk) -> Item {
        Item {
            id: "t".into(),
            name: "t".into(),
            path,
            bytes: 0,
            risk,
            detector: "test".into(),
            rationale: String::new(),
            reversible: true,
        }
    }

    #[test]
    fn protected_is_always_denied() {
        let home = tempfile::tempdir().unwrap();
        let p = home.path().join("vol");
        std::fs::create_dir(&p).unwrap();
        // Even though the path is a real, in-home directory, Protected is rejected.
        let res = validate_against_home(&item(p, Risk::Protected), home.path());
        assert_eq!(res, Err(Denial::Protected));
    }

    #[test]
    fn safe_inside_home_is_allowed() {
        let home = tempfile::tempdir().unwrap();
        let p = home.path().join("cache");
        std::fs::create_dir(&p).unwrap();
        assert_eq!(validate_against_home(&item(p, Risk::Safe), home.path()), Ok(()));
    }

    #[test]
    fn path_outside_home_is_denied() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let p = outside.path().join("data");
        std::fs::create_dir(&p).unwrap();
        assert_eq!(
            validate_against_home(&item(p, Risk::Safe), home.path()),
            Err(Denial::OutsideHome)
        );
    }

    #[test]
    fn symlink_escaping_home_is_denied() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret");
        std::fs::create_dir(&secret).unwrap();
        // A symlink that lives inside home but points outside it must not escape.
        let link = home.path().join("link");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert_eq!(
            validate_against_home(&item(link, Risk::Safe), home.path()),
            Err(Denial::OutsideHome)
        );
    }

    #[test]
    fn dotdot_traversal_is_denied() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("real");
        std::fs::create_dir(&target).unwrap();
        // home/escape/../../<outside>/real — canonicalization collapses it to outside.
        let traversal = home.path().join("..").join(
            outside.path().file_name().unwrap()
        ).join("real");
        let res = validate_against_home(&item(traversal, Risk::Safe), home.path());
        assert_eq!(res, Err(Denial::OutsideHome));
    }

    #[test]
    fn missing_path_is_skipped() {
        let home = tempfile::tempdir().unwrap();
        let p = home.path().join("gone");
        assert_eq!(
            validate_against_home(&item(p, Risk::Safe), home.path()),
            Err(Denial::Missing)
        );
    }
}
