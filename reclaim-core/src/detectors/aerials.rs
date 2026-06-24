//! Aerial wallpapers (PRD §8). macOS re-fetches these on demand, so the cached
//! aerial movies are Safe to remove.

use super::{item_at, Context, Detector};
use crate::{Item, Risk};

pub struct AerialWallpapers;

impl Detector for AerialWallpapers {
    fn name(&self) -> &'static str {
        "aerial-wallpapers"
    }

    fn detect(&self, ctx: &Context) -> Vec<Item> {
        let path = ctx.app_support().join("com.apple.wallpaper/aerials");
        item_at(
            "aerials",
            "Aerial wallpapers",
            path,
            Risk::Safe,
            self.name(),
            "Downloaded aerial screensaver movies — macOS re-fetches them on demand.",
            true,
        )
        .into_iter()
        .collect()
    }
}
