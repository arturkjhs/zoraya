//! The TASK-20 freeze, reproduced (docs/backlog.md): a child that holds
//! 2 GiB of touched zeros, memory macOS compresses so that its resident size
//! can stay small, under tools/mutants/watchdog.sh with WATCHDOG_MB=1024,
//! must be killed. The watchdog reads it through zoraya-footprint.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn a_child_holding_compressible_memory_is_killed() {
    let watchdog = concat!(env!("CARGO_MANIFEST_DIR"), "/../mutants/watchdog.sh");
    let started = Instant::now();
    // A grandchild, as a test under cargo is: the watchdog merges the
    // helper's figures for several processes.
    let hog = format!("{} 2048; true", env!("CARGO_BIN_EXE_zoraya-memhog"));
    let out = Command::new("sh")
        .arg(watchdog)
        .args(["sh", "-c", &hog])
        .env("WATCHDOG_MB", "1024")
        .env("WATCHDOG_FOOTPRINT", env!("CARGO_BIN_EXE_zoraya-footprint"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("watchdog: killed"), "{stderr}");
    assert!(
        stderr.contains("zoraya-memhog"),
        "the hog, not its shell: {stderr}"
    );
    assert!(!stderr.contains("awk:"), "the merge runs: {stderr}");
    // The child sleeps 300 s once it holds it all: a kill ends it long before.
    assert!(elapsed < Duration::from_secs(120), "{elapsed:?}");
}

/// The merge itself, on a list of several processes: the helper's figure
/// replaces ps's for each pid it answers.
#[test]
fn the_watchdog_merges_the_helper_for_every_process() {
    let watchdog = concat!(env!("CARGO_MANIFEST_DIR"), "/../mutants/watchdog.sh");
    // Three processes, none large: nothing is killed, and no awk error.
    let out = Command::new("sh")
        .arg(watchdog)
        .args(["sh", "-c", "sleep 2 & sleep 2 & wait"])
        .env("WATCHDOG_FOOTPRINT", env!("CARGO_BIN_EXE_zoraya-footprint"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        !stderr.contains("awk:") && !stderr.contains("killed"),
        "{stderr}"
    );
}
