//! Staged-autonomy ladder for the agent path (PRD §9, mirrors the Brhaspati
//! rollout: shadow → assisted → auto-safe).
//!
//! This module only ever *proposes*. It computes which items a given policy
//! would reclaim and whether the policy is allowed to execute that itself. It
//! holds NO delete logic — execution always goes through `reclaim_core::reclaim`
//! and its safety gate. The two load-bearing guarantees here:
//!
//!   1. Only `Safe`-class items are ever candidates. `Review`/`Protected` are
//!      never proposed for automation — they require explicit human selection.
//!   2. `permanent` is never automated. Proposals are always move-to-Trash.

use reclaim_core::{Item, Risk, ScanResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where on the autonomy ladder the agent is operating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    /// Report what *would* be reclaimed; execute nothing. The safe default.
    #[default]
    Shadow,
    /// Propose Safe items; a human must approve before any `reclaim_space`.
    Assisted,
    /// Policy may auto-reclaim Safe-class items (to Trash) without a human in
    /// the loop. Review/Protected still always require explicit approval.
    AutoSafe,
}

impl Policy {
    /// Whether this policy may execute its own proposal without human approval.
    /// Only `auto_safe` can — and even then, only Safe items, only to Trash.
    fn may_auto_execute(self) -> bool {
        matches!(self, Policy::AutoSafe)
    }

    fn note(self) -> &'static str {
        match self {
            Policy::Shadow => {
                "shadow: reporting what would be reclaimed; nothing will execute"
            }
            Policy::Assisted => {
                "assisted: human approval required before calling reclaim_space"
            }
            Policy::AutoSafe => {
                "auto_safe: Safe-class items may be auto-reclaimed to Trash; \
                 Review and Protected items still require explicit approval"
            }
        }
    }
}

/// A reclaim proposal. Mode is always Trash — permanent deletion is never
/// produced by a policy. The agent echoes `item_ids` into `reclaim_space`.
#[derive(Debug, Serialize)]
pub struct Proposal {
    pub policy: Policy,
    pub root: PathBuf,
    /// Always `"trash"`. Permanent deletion is never automated (PRD §9).
    pub mode: &'static str,
    /// True only when the policy may execute this proposal itself (`auto_safe`).
    pub auto_executable: bool,
    pub projected_freed_bytes: u64,
    pub item_count: usize,
    /// The candidate items (Safe-class only), full schema for transparency.
    pub items: Vec<Item>,
    pub note: &'static str,
}

/// Compute the proposal for `scan` under `policy`.
///
/// Candidates are exactly the `Safe` items — independent of policy — because
/// nothing else is ever eligible for automation. The policy only decides
/// whether the proposal is `auto_executable`.
pub fn propose(scan: &ScanResult, policy: Policy) -> Proposal {
    let items: Vec<Item> = scan
        .items
        .iter()
        .filter(|i| i.risk == Risk::Safe)
        .cloned()
        .collect();

    let projected_freed_bytes = items.iter().map(|i| i.bytes).sum();

    Proposal {
        policy,
        root: scan.root.clone(),
        mode: "trash",
        auto_executable: policy.may_auto_execute(),
        projected_freed_bytes,
        item_count: items.len(),
        items,
        note: policy.note(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reclaim_core::{DiskInfo, Item, ScanResult};
    use std::path::PathBuf;

    fn item(id: &str, risk: Risk, bytes: u64) -> Item {
        Item {
            id: id.into(),
            name: id.into(),
            path: PathBuf::from(format!("/tmp/{id}")),
            bytes,
            risk,
            detector: "test".into(),
            rationale: String::new(),
            reversible: true,
        }
    }

    fn scan_of(items: Vec<Item>) -> ScanResult {
        ScanResult {
            root: PathBuf::from("/root"),
            scanned_at: "2026-06-24T00:00:00Z".into(),
            disk: DiskInfo {
                total_bytes: 0,
                free_bytes: 0,
            },
            root_total_bytes: items.iter().map(|i| i.bytes).sum(),
            items,
        }
    }

    #[test]
    fn proposal_only_ever_contains_safe_items() {
        let scan = scan_of(vec![
            item("a", Risk::Safe, 10),
            item("b", Risk::Review, 100),
            item("c", Risk::Protected, 1000),
            item("d", Risk::Safe, 5),
        ]);

        for policy in [Policy::Shadow, Policy::Assisted, Policy::AutoSafe] {
            let p = propose(&scan, policy);
            // Review and Protected are NEVER in a proposal, at any stage.
            assert!(p.items.iter().all(|i| i.risk == Risk::Safe));
            let ids: Vec<&str> = p.items.iter().map(|i| i.id.as_str()).collect();
            assert_eq!(ids, vec!["a", "d"]);
            assert_eq!(p.projected_freed_bytes, 15);
            // Permanent is never automated.
            assert_eq!(p.mode, "trash");
        }
    }

    #[test]
    fn only_auto_safe_may_self_execute() {
        let scan = scan_of(vec![item("a", Risk::Safe, 10)]);
        assert!(!propose(&scan, Policy::Shadow).auto_executable);
        assert!(!propose(&scan, Policy::Assisted).auto_executable);
        assert!(propose(&scan, Policy::AutoSafe).auto_executable);
    }

    #[test]
    fn policy_parses_from_snake_case() {
        let p: Policy = serde_json::from_value(serde_json::json!("auto_safe")).unwrap();
        assert_eq!(p, Policy::AutoSafe);
        assert_eq!(Policy::default(), Policy::Shadow);
    }
}
