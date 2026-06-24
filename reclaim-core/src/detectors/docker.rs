//! Orphaned Docker Desktop (PRD §8).
//!
//! `~/Library/Containers/com.docker.docker` is ~Safe only when Docker Desktop is
//! genuinely gone: the app is absent, no Docker Desktop process is running, and
//! the active docker context is not `desktop-linux`. If *any* of those signals
//! says Docker Desktop might be in use, the bundle is real, in-use data →
//! Protected. Ambiguity resolves up.

use super::{item_at, Context, Detector};
use crate::{Item, Risk};
use std::process::Command;

pub struct OrphanedDockerDesktop;

impl Detector for OrphanedDockerDesktop {
    fn name(&self) -> &'static str {
        "orphaned-docker-desktop"
    }

    fn detect(&self, ctx: &Context) -> Vec<Item> {
        let path = ctx.library().join("Containers/com.docker.docker");

        let app_present = std::path::Path::new("/Applications/Docker.app").exists();
        let process_running = docker_desktop_running();
        let active_desktop = docker_context_is_desktop();

        let orphaned = !app_present && !process_running && !active_desktop;

        let (risk, rationale) = if orphaned {
            (
                Risk::Safe,
                "No Docker.app, no Docker Desktop process, and the active docker \
                 context is not desktop-linux — this container bundle is orphaned."
                    .to_string(),
            )
        } else {
            let mut why = Vec::new();
            if app_present {
                why.push("Docker.app is installed");
            }
            if process_running {
                why.push("Docker Desktop is running");
            }
            if active_desktop {
                why.push("the active docker context is desktop-linux");
            }
            (
                Risk::Protected,
                format!(
                    "Docker Desktop appears in use ({}); its container data is protected.",
                    why.join(", ")
                ),
            )
        };

        item_at(
            "docker-desktop",
            "Docker Desktop",
            path,
            risk,
            self.name(),
            rationale,
            // A multi-GB VM bundle is a poor Trash candidate.
            false,
        )
        .into_iter()
        .collect()
    }
}

/// True if a process named like Docker Desktop is currently running.
fn docker_desktop_running() -> bool {
    Command::new("/bin/ps")
        .args(["-Axo", "comm"])
        .output()
        .ok()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.lines().any(|l| l.contains("Docker Desktop") || l.contains("com.docker.backend"))
        })
        .unwrap_or(false)
}

/// True if `docker context show` reports `desktop-linux` (Docker Desktop's
/// context). If the docker CLI is absent, this is `false` — not having docker is
/// itself evidence Docker Desktop is gone.
fn docker_context_is_desktop() -> bool {
    Command::new("docker")
        .args(["context", "show"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "desktop-linux")
        .unwrap_or(false)
}
