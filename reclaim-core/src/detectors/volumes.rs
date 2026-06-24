//! Container volumes (PRD §8). OrbStack / Docker volume data is *always*
//! Protected — it is real, persistent data (databases, app state) that does not
//! regenerate. It is listed for transparency so the user can see where the space
//! went, but it can never be selected or deleted.

use super::{item_at, slug, Context, Detector};
use crate::{Item, Risk};

pub struct ContainerVolumes;

impl Detector for ContainerVolumes {
    fn name(&self) -> &'static str {
        "container-volumes"
    }

    fn detect(&self, ctx: &Context) -> Vec<Item> {
        // Known persistent-data locations for OrbStack and Docker.
        let candidates = [
            (ctx.home.join(".orbstack/data"), "OrbStack data"),
            (
                ctx.library()
                    .join("Containers/com.docker.docker/Data/vms/0/data"),
                "Docker VM data",
            ),
            (ctx.home.join(".docker/volumes"), "Docker volumes"),
        ];

        candidates
            .into_iter()
            .filter_map(|(path, name)| {
                item_at(
                    format!("volume-{}", slug(&path)),
                    name,
                    path,
                    Risk::Protected,
                    self.name(),
                    "Persistent container volume data (databases, app state). Real data — \
                     never reclaimable; shown for transparency only.",
                    false,
                )
            })
            .collect()
    }
}
