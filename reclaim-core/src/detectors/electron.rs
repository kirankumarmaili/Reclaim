//! Electron / Chromium app caches (PRD §8).
//!
//! Each Electron app keeps regenerable caches (`Cache`, `Code Cache`,
//! `GPUCache`, `DawnCache`, `CachedData`, `Crashpad`) under its Application
//! Support folder. These are Safe — the app rebuilds them. We never target the
//! app's data folder itself, only these well-known cache subfolders, and only
//! when they exceed a small threshold worth surfacing.

use super::{item_at, slug, Context, Detector};
use crate::{Item, Risk};

const CACHE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "CachedData",
    "Crashpad",
];

/// Only surface caches at least this large (PRD: ≥ 5 MB).
const MIN_BYTES: u64 = 5 * 1024 * 1024;

pub struct ElectronCaches;

impl Detector for ElectronCaches {
    fn name(&self) -> &'static str {
        "electron-app-caches"
    }

    fn detect(&self, ctx: &Context) -> Vec<Item> {
        let support = ctx.app_support();
        let Ok(apps) = std::fs::read_dir(&support) else {
            return Vec::new();
        };

        let mut items = Vec::new();
        for app in apps.flatten() {
            let app_dir = app.path();
            if !app_dir.is_dir() {
                continue;
            }
            let app_name = app.file_name().to_string_lossy().to_string();
            for cache in CACHE_DIRS {
                let cache_path = app_dir.join(cache);
                if let Some(mut it) = item_at(
                    format!("electron-{}", slug(&cache_path)),
                    format!("{app_name} — {cache}"),
                    cache_path,
                    Risk::Safe,
                    self.name(),
                    format!("Regenerable {cache} for {app_name}; the app rebuilds it on demand."),
                    true,
                ) {
                    if it.bytes >= MIN_BYTES {
                        it.detector = self.name().to_string();
                        items.push(it);
                    }
                }
            }
        }
        items
    }
}
