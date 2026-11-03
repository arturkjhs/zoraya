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
    let out = Command::new("sh")
        .arg(watchdog)
        .arg(env!("CARGO_BIN_EXE_zoraya-memhog"))
        .arg("2048")
        .env("WATCHDOG_MB", "1024")
        .env("WATCHDOG_FOOTPRINT", env!("CARGO_BIN_EXE_zoraya-footprint"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("watchdog: killed"), "{stderr}");
    assert!(!out.status.success(), "the child was killed, not finished");
    // The child sleeps 300 s once it holds it all: a kill ends it long before.
    assert!(elapsed < Duration::from_secs(120), "{elapsed:?}");
}
