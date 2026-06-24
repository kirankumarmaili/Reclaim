//! The detector catalogue — where Reclaim's domain knowledge lives (PRD §8).
//!
//! Each detector is declarative: it knows its target paths, a default risk
//! class, and the live conditions that confirm or *downgrade* that class.
//! Adding a detector is additive — register it in [`catalogue`] and it shows up
//! in scans without any change to the UI or the safety core.
//!
//! Cardinal rule: a detector may return [`Risk::Safe`] only when it can
//! affirmatively confirm the item regenerates with no user-visible loss. Any
//! uncertainty resolves *up* toward Protected (PRD §6).

use crate::{Item, Risk};
use std::path::{Path, PathBuf};

mod aerials;
mod docker;
mod electron;
mod jetbrains;
mod volumes;
mod xcode;

/// What a detector is given to do its job.
pub struct Context {
    /// The user's home directory.
    pub home: PathBuf,
    /// The scan root the user requested (e.g. `~/Library`). Items outside it
    /// are filtered out by [`run_all`].
    pub root: PathBuf,
}

impl Context {
    pub fn library(&self) -> PathBuf {
        self.home.join("Library")
    }
    pub fn app_support(&self) -> PathBuf {
        self.library().join("Application Support")
    }
    pub fn caches(&self) -> PathBuf {
        self.library().join("Caches")
    }
}

pub trait Detector: Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, ctx: &Context) -> Vec<Item>;
}

/// The full, ordered catalogue. New detectors are appended here.
pub fn catalogue() -> Vec<Box<dyn Detector>> {
    vec![
        Box::new(docker::OrphanedDockerDesktop),
        Box::new(jetbrains::JetBrains),
        Box::new(electron::ElectronCaches),
        Box::new(aerials::AerialWallpapers),
        Box::new(xcode::XcodeDerived),
        Box::new(volumes::ContainerVolumes),
    ]
}

/// Run every detector and keep only items whose path is inside the scan root and
/// that actually occupy space (or are Protected — those are listed for
/// transparency even at zero bytes).
pub fn run_all(ctx: &Context) -> Vec<Item> {
    let mut items: Vec<Item> = catalogue()
        .iter()
        .flat_map(|d| d.detect(ctx))
        .filter(|it| it.path.starts_with(&ctx.root))
        .filter(|it| it.bytes > 0 || it.risk == Risk::Protected)
        .collect();
    // Largest first — the treemap and the side panel both lead with the big wins.
    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    items
}

// ---- shared helpers for detector authors -------------------------------------

/// Build an item, computing its true on-disk size. Returns `None` if the path
/// does not exist, so detectors can list candidate paths without guarding each.
pub(crate) fn item_at(
    id: impl Into<String>,
    name: impl Into<String>,
    path: PathBuf,
    risk: Risk,
    detector: &'static str,
    rationale: impl Into<String>,
    reversible: bool,
) -> Option<Item> {
    if std::fs::symlink_metadata(&path).is_err() {
        return None;
    }
    Some(Item {
        id: id.into(),
        name: name.into(),
        bytes: crate::fs_size::on_disk_size(&path),
        path,
        risk,
        detector: detector.into(),
        rationale: rationale.into(),
        reversible,
    })
}

/// A short, filesystem-safe id fragment derived from a path.
pub(crate) fn slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
