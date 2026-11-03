//! A guest that never stops printing must fail the run, not take the host
//! (docs/backlog.md, the 6c memory entry): the runner, in its line mode and
//! its script mode, against a fake guest that prints lines for ever and one
//! that prints one endless line. Each ends in FAIL with "output flood" well
//! within its timeout, and the runner's own memory stays under 256 MiB.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUNNER: &str = env!("CARGO_BIN_EXE_zoraya-test-runner");
const TIMEOUT: u64 = 60;
const MEMORY_LIMIT_KIB: u64 = 256 * 1024;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zoraya-flood-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn rss_kib(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Runs the runner with `mode` against `guest` (a shell command), and gives
/// its stdout and the most memory it held.
fn run(name: &str, mode: &[&str], guest: &str) -> (String, u64, Duration) {
    let dir = scratch(name);
    let stdout = std::fs::File::create(dir.join("stdout")).unwrap();
    let started = Instant::now();
    let mut child = Command::new(RUNNER)
        .args(["--label", name, "--timeout", &TIMEOUT.to_string()])
        .args(mode)
        .args(["--", "sh", "-c", guest])
        .env("ZORAYA_TEST_LOGS", dir.join("logs"))
        .stdin(Stdio::null())
        .stdout(stdout)
        .spawn()
        .unwrap();
    let mut peak = 0;
    loop {
        if let Some(kib) = rss_kib(child.id()) {
            peak = peak.max(kib);
            if kib > MEMORY_LIMIT_KIB {
                let _ = child.kill();
            }
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(TIMEOUT + 30) {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.wait();
    let elapsed = started.elapsed();
    let text = std::fs::read_to_string(dir.join("stdout")).unwrap_or_default();
    let log =
        std::fs::metadata(dir.join("logs").join(format!("{name}.log"))).map_or(0, |m| m.len());
    assert!(
        log <= 64 * 1024 * 1024 + 70 * 1024,
        "{name}: the log is capped, not {log} bytes"
    );
    let _ = std::fs::remove_dir_all(&dir);
    (text, peak, elapsed)
}

fn floods(name: &str, mode: &[&str], guest: &str) {
    let (text, peak, elapsed) = run(name, mode, guest);
    assert!(
        text.contains("FAIL") && text.contains("output flood: more than 64 MiB"),
        "{name}: {}",
        &text[text.len().saturating_sub(2000)..]
    );
    assert!(
        elapsed < Duration::from_secs(TIMEOUT),
        "{name}: took {elapsed:?}"
    );
    assert!(
        peak < MEMORY_LIMIT_KIB,
        "{name}: the runner held {peak} KiB"
    );
    assert!(
        text.lines().count() <= 200 + 200 + 10,
        "{name}: the terminal got {} lines",
        text.lines().count()
    );
}

#[test]
fn endless_lines_fail_the_line_mode() {
    floods("lines", &[], "yes ZORAYA-TEST flood");
}

#[test]
fn one_endless_line_fails_the_line_mode() {
    floods("one-line", &[], "yes x | tr -d '\\n'");
}

fn script(name: &str) -> String {
    let dir = scratch(&format!("{name}-script"));
    let path = dir.join("wait.script");
    std::fs::write(&path, "< a line that never comes\n").unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn endless_lines_fail_the_script_mode() {
    let s = script("script-lines");
    floods("script-lines", &["--script", &s], "yes ZORAYA-TEST flood");
}

#[test]
fn one_endless_line_fails_the_script_mode() {
    let s = script("script-one-line");
    floods("script-one-line", &["--script", &s], "yes x | tr -d '\\n'");
}
