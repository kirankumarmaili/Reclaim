//! Xcode derived data, simulator caches, and simulator devices (PRD §8). These
//! rebuild, but the rebuild cost is non-trivial (recompiling, re-warming
//! simulators, losing installed app data), so they are Review rather than
//! Safe — reclaimable, but opt-in.
//!
//! `CoreSimulator/Devices` holds *every* simulator's state as one tree with no
//! per-device split, so if any simulator is currently booted, its live session
//! (and whatever app data is mid-test in it) could be sitting inside that same
//! tree. That is exactly the ambiguity PRD §6 says resolves *up* — we can't
//! prove nothing booted is in there, so the whole item downgrades to Protected.

use super::{item_at, Context, Detector};
use crate::{Item, Risk};
use std::process::Command;

pub struct XcodeDerived;

impl Detector for XcodeDerived {
    fn name(&self) -> &'static str {
        "xcode-derived-data"
    }

    fn detect(&self, ctx: &Context) -> Vec<Item> {
        let dev = ctx.home.join("Library/Developer");
        let mut items: Vec<Item> = [
            (
                dev.join("Xcode/DerivedData"),
                "Xcode derived data",
                "Build products and indexes; Xcode rebuilds them, but recompiling is slow.",
            ),
            (
                dev.join("CoreSimulator/Caches"),
                "Simulator caches",
                "Cached simulator runtimes and assets; re-downloaded/re-warmed when next used.",
            ),
        ]
        .into_iter()
        .filter_map(|(path, name, why)| {
            item_at(
                format!("xcode-{name}").replace(' ', "-"),
                name,
                path,
                Risk::Review,
                self.name(),
                why,
                true,
            )
        })
        .collect();

        items.extend(self.simulator_devices(&dev));
        items
    }
}

impl XcodeDerived {
    fn simulator_devices(&self, dev: &std::path::Path) -> Option<Item> {
        let (risk, rationale) = devices_risk(any_simulator_booted());
        item_at(
            "xcode-Simulator-devices",
            "Simulator devices",
            dev.join("CoreSimulator/Devices"),
            risk,
            self.name(),
            rationale,
            true,
        )
    }
}

/// Pure risk decision for the `CoreSimulator/Devices` item, split out from the
/// live `simctl` check so it is unit-testable without shelling out (PRD §6 /
/// CLAUDE.md "changing safety-critical code").
fn devices_risk(booted: bool) -> (Risk, String) {
    if booted {
        (
            Risk::Protected,
            "A simulator is currently booted; CoreSimulator/Devices has no \
             per-device split, so the booted session's live data could be inside \
             this same tree — protected until nothing is booted."
                .to_string(),
        )
    } else {
        (
            Risk::Review,
            "Installed simulator devices (apps, app data, per-device state); Xcode \
             recreates the default set on next launch, but any app data inside is lost."
                .to_string(),
        )
    }
}

/// True if `xcrun simctl` reports any simulator as booted. Fails toward `true`
/// (booted / protected) if `xcrun`/`simctl` can't be run — ambiguity resolves up.
fn any_simulator_booted() -> bool {
    Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("(Booted)"))
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booted_simulator_protects_the_whole_devices_tree() {
        let (risk, _) = devices_risk(true);
        assert_eq!(risk, Risk::Protected);
    }

    #[test]
    fn no_booted_simulator_is_review() {
        let (risk, _) = devices_risk(false);
        assert_eq!(risk, Risk::Review);
    }
}
