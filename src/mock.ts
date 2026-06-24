// Realistic sample data for browser dev, modeled on an actual ~/Library scan.
// Lets the UI be developed and reviewed without building the Tauri app.

import type { ScanResult } from "./types";

const GB = 1024 ** 3;
const MB = 1024 ** 2;

export function mockScan(): ScanResult {
  const items = sampleItems();
  return {
    root: "/Users/dev/Library",
    scanned_at: new Date().toISOString(),
    disk: { total_bytes: 494 * GB, free_bytes: 42 * GB },
    root_total_bytes: items.reduce((s, i) => s + i.bytes, 0),
    items,
  };
}

function sampleItems(): ScanResult["items"] {
  return [
      mk("docker-desktop", "Docker Desktop", "Containers/com.docker.docker", 29.1 * GB, "safe",
        "orphaned-docker-desktop",
        "No Docker.app, no Docker Desktop process, and the active docker context is not desktop-linux — this container bundle is orphaned.",
        false),
      mk("orbstack-data", "OrbStack data", ".orbstack/data", 18.4 * GB, "protected",
        "container-volumes",
        "Persistent container volume data (databases, app state). Real data — never reclaimable; shown for transparency only.",
        false),
      mk("aerials", "Aerial wallpapers", "Application Support/com.apple.wallpaper/aerials", 4.65 * GB, "safe",
        "aerial-wallpapers",
        "Downloaded aerial screensaver movies — macOS re-fetches them on demand.", true),
      mk("idea-2026", "IntelliJIdea 2026.1", "Application Support/JetBrains/IntelliJIdea2026.1", 4.71 * GB, "protected",
        "jetbrains-old-versions", "Current IntelliJIdea (2026.1) configuration — kept.", true),
      mk("idea-2025", "IntelliJIdea 2025.3", "Application Support/JetBrains/IntelliJIdea2025.3", 4.22 * GB, "review",
        "jetbrains-old-versions",
        "Superseded IntelliJIdea (2025.3) configuration; a newer version is installed. Settings are not auto-migrated back, so review before removing.",
        true),
      mk("xcode-derived", "Xcode derived data", "Developer/Xcode/DerivedData", 3.9 * GB, "review",
        "xcode-derived-data",
        "Build products and indexes; Xcode rebuilds them, but recompiling is slow.", true),
      mk("discord-cache", "discord — Cache", "Application Support/discord/Cache", 791 * MB, "safe",
        "electron-app-caches", "Regenerable Cache for discord; the app rebuilds it on demand.", true),
      mk("slack-cache", "Slack — Cache", "Application Support/Slack/Cache", 612 * MB, "safe",
        "electron-app-caches", "Regenerable Cache for Slack; the app rebuilds it on demand.", true),
      mk("code-cache", "Code — CachedData", "Application Support/Code/CachedData", 540 * MB, "safe",
        "electron-app-caches", "Regenerable CachedData for Code; the app rebuilds it on demand.", true),
      mk("jb-caches", "JetBrains index caches", "Caches/JetBrains", 2.1 * GB, "safe",
        "jetbrains-old-versions", "IDE index and log caches — rebuilt automatically on next launch.", true),
      mk("slack-gpu", "Slack — GPUCache", "Application Support/Slack/GPUCache", 96 * MB, "safe",
        "electron-app-caches", "Regenerable GPUCache for Slack; the app rebuilds it on demand.", true),
      mk("sim-caches", "Simulator caches", "Developer/CoreSimulator/Caches", 1.6 * GB, "review",
        "xcode-derived-data", "Cached simulator runtimes and assets; re-downloaded/re-warmed when next used.", true),
  ];
}

function mk(
  id: string, name: string, rel: string, bytes: number,
  risk: ScanResult["items"][number]["risk"], detector: string, rationale: string, reversible: boolean,
): ScanResult["items"][number] {
  return { id, name, path: `/Users/dev/Library/${rel}`, bytes: Math.round(bytes), risk, detector, rationale, reversible };
}
