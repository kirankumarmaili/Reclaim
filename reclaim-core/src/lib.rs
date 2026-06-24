//! Reclaim core — the single source of truth.
//!
//! The UI and the MCP layer are thin clients over this crate. All risk
//! classification and the safety gate live here; a buggy or compromised front
//! end cannot delete protected data because the core re-validates every target.
//!
//! See `Prd.md` §9–§11 for the contract this crate implements.

pub mod detectors;
pub mod fs_size;
pub mod reclaim;
pub mod safety;
pub mod scan;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How safe an item is to delete. Exactly one class per item.
///
/// Ordering matters: `Protected > Review > Safe`. When a detector is uncertain
/// it must classify *up* (toward Protected), never down (PRD §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Regenerates with no user-visible loss. Selectable; "Select safe" eligible.
    Safe,
    /// Reclaimable but with a cost (re-download, rebuild, possibly-wanted state).
    /// Selectable, but opt-in per item — never bulk-selected.
    Review,
    /// Real data or in-use config. Never selectable. Listed for transparency only.
    Protected,
}

impl Risk {
    /// Whether the UI may offer this item for selection at all.
    pub fn selectable(self) -> bool {
        !matches!(self, Risk::Protected)
    }

    /// Whether "Select safe" may auto-include this item.
    pub fn auto_selectable(self) -> bool {
        matches!(self, Risk::Safe)
    }
}

/// A single reclaimable (or transparency-only) item produced by a detector.
/// Serializes to the schema in `Prd.md` §10.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    /// Stable identifier the UI/agent echoes back on reclaim.
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    /// True on-disk size in bytes (block-based, honors sparse files).
    pub bytes: u64,
    pub risk: Risk,
    /// Which detector produced this item.
    pub detector: String,
    /// Human-readable reason for the classification, shown to the user.
    pub rationale: String,
    /// Whether move-to-Trash is sensible for this item (false for huge items
    /// where Trash adds no value, e.g. a 29 GB VM).
    pub reversible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Output of a scan. Consumed identically by the UI and the MCP layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub root: PathBuf,
    /// RFC 3339 timestamp.
    pub scanned_at: String,
    pub disk: DiskInfo,
    pub root_total_bytes: u64,
    pub items: Vec<Item>,
}

/// Default deletion mode for a reclaim target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Reversible — move to Trash. The default for every target.
    #[default]
    Trash,
    /// Irreversible — only when the caller explicitly opts in per item.
    Permanent,
}

/// One item the caller asks to reclaim. `mode` defaults to Trash if omitted.
#[derive(Debug, Clone, Deserialize)]
pub struct ReclaimTarget {
    pub id: String,
    #[serde(default)]
    pub mode: Mode,
}

/// What actually happened to one item.
#[derive(Debug, Clone, Serialize)]
pub struct Disposition {
    pub id: String,
    pub path: PathBuf,
    pub bytes: u64,
}

/// Why an item was not reclaimed (rejected by the gate, in use, missing, …).
#[derive(Debug, Clone, Serialize)]
pub struct Skip {
    pub id: String,
    pub reason: String,
}

/// Result of a reclaim. Mirrors `Prd.md` §10.
#[derive(Debug, Clone, Serialize)]
pub struct ReclaimResult {
    pub requested_ids: Vec<String>,
    pub freed_bytes: u64,
    pub moved_to_trash: Vec<Disposition>,
    pub permanently_deleted: Vec<Disposition>,
    pub skipped: Vec<Skip>,
}
