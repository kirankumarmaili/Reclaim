//! Xcode derived data & simulator caches (PRD §8). These rebuild, but the
//! rebuild cost is non-trivial (recompiling, re-warming simulators), so they are
//! Review rather than Safe — reclaimable, but opt-in.

use super::{item_at, Context, Detector};
use crate::{Item, Risk};

pub struct XcodeDerived;

impl Detector for XcodeDerived {
    fn name(&self) -> &'static str {
        "xcode-derived-data"
    }

    fn detect(&self, ctx: &Context) -> Vec<Item> {
        let dev = ctx.home.join("Library/Developer");
        let targets = [
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
        ];

        targets
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
            .collect()
    }
}
