//! `reclaim` — a thin CLI over the core. Useful for demos, CI cleanup, and as a
//! second client (besides the webview and MCP) proving the engine is UI-agnostic.
//!
//!   reclaim scan [ROOT] [--pretty]
//!   reclaim reclaim ROOT ID[:permanent] [ID...]   # default mode is Trash
//!   reclaim safe   ROOT                            # reclaim all Safe items to Trash

use reclaim_core::{reclaim, scan, Mode, ReclaimTarget, Risk};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("scan") => cmd_scan(&args[1..]),
        Some("reclaim") => cmd_reclaim(&args[1..]),
        Some("safe") => cmd_safe(&args[1..]),
        _ => {
            eprintln!(
                "Reclaim — risk-aware disk recovery\n\n\
                 Usage:\n  \
                 reclaim scan [ROOT] [--pretty]\n  \
                 reclaim reclaim ROOT ID[:permanent] [ID...]\n  \
                 reclaim safe ROOT\n\n\
                 ROOT defaults to ~/Library. Deletion is confined to $HOME and\n\
                 defaults to move-to-Trash."
            );
            ExitCode::from(2)
        }
    }
}

fn default_root() -> PathBuf {
    reclaim_core::safety::home_dir().join("Library")
}

fn cmd_scan(args: &[String]) -> ExitCode {
    let pretty = args.iter().any(|a| a == "--pretty");
    let root = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(default_root);

    match scan::scan(&root) {
        Ok(result) => {
            let json = if pretty {
                serde_json::to_string_pretty(&result)
            } else {
                serde_json::to_string(&result)
            }
            .expect("scan result serializes");
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("scan failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_reclaim(args: &[String]) -> ExitCode {
    let Some(root) = args.first() else {
        eprintln!("reclaim: ROOT required");
        return ExitCode::from(2);
    };
    let root = PathBuf::from(root);
    let targets: Vec<ReclaimTarget> = args[1..]
        .iter()
        .map(|spec| match spec.split_once(':') {
            Some((id, "permanent")) => ReclaimTarget {
                id: id.to_string(),
                mode: Mode::Permanent,
            },
            _ => ReclaimTarget {
                id: spec.clone(),
                mode: Mode::Trash,
            },
        })
        .collect();

    run_reclaim(&root, &targets)
}

fn cmd_safe(args: &[String]) -> ExitCode {
    let Some(root) = args.first() else {
        eprintln!("safe: ROOT required");
        return ExitCode::from(2);
    };
    let root = PathBuf::from(root);
    let scanned = match scan::scan(&root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("scan failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let targets: Vec<ReclaimTarget> = scanned
        .items
        .iter()
        .filter(|i| i.risk == Risk::Safe)
        .map(|i| ReclaimTarget {
            id: i.id.clone(),
            mode: Mode::Trash,
        })
        .collect();

    if targets.is_empty() {
        println!("Nothing Safe to reclaim.");
        return ExitCode::SUCCESS;
    }
    run_reclaim(&root, &targets)
}

fn run_reclaim(root: &std::path::Path, targets: &[ReclaimTarget]) -> ExitCode {
    match reclaim::reclaim(root, targets) {
        Ok(result) => {
            println!("Freed {}", human(result.freed_bytes));
            for d in &result.moved_to_trash {
                println!("  ↩ trashed   {} ({})", d.path.display(), human(d.bytes));
            }
            for d in &result.permanently_deleted {
                println!("  ✗ deleted   {} ({})", d.path.display(), human(d.bytes));
            }
            for s in &result.skipped {
                println!("  – {} [{}]", s.id, s.reason);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("reclaim failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn human(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}
