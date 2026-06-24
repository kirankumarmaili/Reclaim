//! JetBrains old versions (PRD §8).
//!
//! Under `~/Library/Application Support/JetBrains/<Product><Version>`, group by
//! product, keep the newest N (default 1) as Protected, and surface older
//! versions as Review (config you *might* still want, but the new version
//! supersedes it). Index caches under `~/Library/Caches/JetBrains` are Safe.

use super::{item_at, slug, Context, Detector};
use crate::{Item, Risk};
use std::collections::BTreeMap;

/// How many newest versions of each product to keep as Protected.
const KEEP_NEWEST: usize = 1;

pub struct JetBrains;

impl Detector for JetBrains {
    fn name(&self) -> &'static str {
        "jetbrains-old-versions"
    }

    fn detect(&self, ctx: &Context) -> Vec<Item> {
        let mut out = Vec::new();
        out.extend(self.old_config_versions(ctx));
        out.extend(self.index_caches(ctx));
        out
    }
}

impl JetBrains {
    fn old_config_versions(&self, ctx: &Context) -> Vec<Item> {
        let base = ctx.app_support().join("JetBrains");
        let Ok(entries) = std::fs::read_dir(&base) else {
            return Vec::new();
        };

        // product -> sorted (version, path); e.g. ("IntelliJIdea", "2024.3").
        let mut by_product: BTreeMap<String, Vec<(Version, std::path::PathBuf)>> = BTreeMap::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some((product, version)) = split_product_version(&name) {
                by_product.entry(product).or_default().push((version, p));
            }
        }

        let mut items = Vec::new();
        for (product, mut versions) in by_product {
            // Newest first.
            versions.sort_by(|a, b| b.0.cmp(&a.0));
            for (rank, (ver, path)) in versions.into_iter().enumerate() {
                let keep = rank < KEEP_NEWEST;
                let (risk, rationale) = if keep {
                    (
                        Risk::Protected,
                        format!("Current {product} ({ver}) configuration — kept."),
                    )
                } else {
                    (
                        Risk::Review,
                        format!(
                            "Superseded {product} ({ver}) configuration; a newer version is installed. \
                             Settings are not auto-migrated back, so review before removing."
                        ),
                    )
                };
                if let Some(it) = item_at(
                    format!("jetbrains-{}", slug(&path)),
                    format!("{product} {ver}"),
                    path,
                    risk,
                    self.name(),
                    rationale,
                    true,
                ) {
                    items.push(it);
                }
            }
        }
        items
    }

    fn index_caches(&self, ctx: &Context) -> Vec<Item> {
        let path = ctx.caches().join("JetBrains");
        item_at(
            "jetbrains-caches",
            "JetBrains index caches",
            path,
            Risk::Safe,
            self.name(),
            "IDE index and log caches — rebuilt automatically on next launch.",
            true,
        )
        .into_iter()
        .collect()
    }
}

/// A dotted version like `2024.3` compared numerically component-by-component.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Version(Vec<u32>);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<String> = self.0.iter().map(|n| n.to_string()).collect();
        write!(f, "{}", parts.join("."))
    }
}

/// Split `IntelliJIdea2024.3` into (`IntelliJIdea`, `2024.3`). The version is the
/// trailing run that starts at the first digit following a non-digit.
fn split_product_version(name: &str) -> Option<(String, Version)> {
    let bytes: Vec<char> = name.chars().collect();
    // Find the start of the trailing numeric/dotted version.
    let split = (0..bytes.len()).find(|&i| {
        bytes[i].is_ascii_digit()
            && i > 0
            && bytes[i - 1].is_ascii_alphabetic()
            // ensure the rest is only digits/dots
            && bytes[i..].iter().all(|c| c.is_ascii_digit() || *c == '.')
    })?;
    let product = name[..split].to_string();
    let version = &name[split..];
    let parts: Vec<u32> = version
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<Vec<u32>>>()?;
    if parts.is_empty() {
        return None;
    }
    Some((product, Version(parts)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_product_and_version() {
        let (p, v) = split_product_version("IntelliJIdea2024.3").unwrap();
        assert_eq!(p, "IntelliJIdea");
        assert_eq!(v.to_string(), "2024.3");
    }

    #[test]
    fn newer_version_sorts_higher() {
        let a = split_product_version("IntelliJIdea2024.3").unwrap().1;
        let b = split_product_version("IntelliJIdea2025.1").unwrap().1;
        assert!(b > a);
    }

    #[test]
    fn rejects_non_versioned_dirs() {
        assert!(split_product_version("consentOptions").is_none());
    }
}
