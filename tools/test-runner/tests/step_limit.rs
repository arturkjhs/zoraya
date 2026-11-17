//! A GUI script's step has a limit of its own (`STEP_LIMIT`, or
//! `limit SECONDS STEP`), times four under TCG: a step that waits for ever
//! fails at that limit with its number and what it waited for, not at the
//! run's (run 43802095122 waited 1200 s for a screen that never came). The
//! runner plays against a fake machine: `sh` that prints nothing, and a QMP
//! socket served here that answers every command.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUNNER: &str = env!("CARGO_BIN_EXE_zoraya-test-runner");

fn scratch(name: &str) -> PathBuf {
    // Short: a socket's path is at most 104 bytes on macOS.
    let dir = std::env::temp_dir().join(format!("zsl-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Serves QMP at `path` once the runner has removed what was there: the
/// greeting, then `{"return": {}}` to every command, until the runner hangs up.
fn serve_qmp(path: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let mut out = stream.try_clone().unwrap();
        let _ = out.write_all(b"{\"QMP\": {}}\n");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
            line.clear();
            if out.write_all(b"{\"return\": {}}\n").is_err() {
                break;
            }
        }
    });
}

/// Plays `script` with the run's limit `timeout`; `tcg` puts `--accel tcg`
/// on the machine's command line. Gives the runner's output and how long
/// it took.
fn play(name: &str, script: &str, timeout: u64, tcg: bool) -> (String, Duration) {
    let dir = scratch(name);
    let path = dir.join("s.script");
    std::fs::write(&path, script).unwrap();
    let qmp = dir.join("q");
    serve_qmp(qmp.clone());
    // The machine: silent until killed; `--print-accel` (the runner asks
    // what the accelerator resolved to) answers nothing at once.
    let guest = "case \"$*\" in *--print-accel*) exit 1;; esac; exec sleep 120";
    let mut command = Command::new(RUNNER);
    command
        .args(["--label", name, "--timeout", &timeout.to_string()])
        .args(["--gui-script", path.to_str().unwrap()])
        .args(["--qmp", qmp.to_str().unwrap()])
        .args(["--", "sh", "-c", guest, "machine"]);
    if tcg {
        command.args(["--accel", "tcg"]);
    }
    let started = Instant::now();
    let out = command
        .env("ZORAYA_TEST_LOGS", dir.join("logs"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let _ = std::fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        took,
    )
}

#[test]
fn a_step_fails_at_its_own_limit_not_the_runs() {
    let (out, took) = play(
        "own",
        "pause 10\nlimit 2 serial a line that never comes\nexit\n",
        120,
        false,
    );
    assert!(out.contains("FAIL: step 2 of 3"), "{out}");
    // What it waited for: the step, in the failure's own words.
    assert!(out.contains("Serial(\"a line that never comes\")"), "{out}");
    assert!(out.contains("the step's limit of 2 s"), "{out}");
    assert!(took >= Duration::from_secs(2), "took {took:?}");
    assert!(took < Duration::from_secs(60), "took {took:?}");
}

#[test]
fn under_tcg_the_limit_is_four_times_longer() {
    let (out, took) = play("tcg", "limit 1 serial never\n", 120, true);
    assert!(out.contains("FAIL: step 1 of 1"), "{out}");
    assert!(out.contains("the step's limit of 4 s"), "{out}");
    assert!(took >= Duration::from_secs(4), "took {took:?}");
    assert!(took < Duration::from_secs(60), "took {took:?}");
}

#[test]
fn the_runs_limit_still_ends_a_step_that_comes_first() {
    // The default step limit (120 s) is longer than this run's 3 s: the
    // run's limit ends it, and the step's is not blamed.
    let (out, took) = play("run", "serial never\n", 3, false);
    assert!(out.contains("FAIL: step 1 of 1"), "{out}");
    assert!(out.contains("timeout"), "{out}");
    assert!(!out.contains("the step's limit"), "{out}");
    assert!(took < Duration::from_secs(60), "took {took:?}");
}

#[test]
fn a_pause_longer_than_the_limit_is_not_cut() {
    // 3 s of pause under a 1 s limit: the pause's own time comes on top.
    let (out, took) = play(
        "pause",
        "limit 1 pause 3000\nlimit 1 serial never\n",
        120,
        false,
    );
    assert!(out.contains("FAIL: step 2 of 2"), "{out}");
    assert!(out.contains("the step's limit of 1 s"), "{out}");
    assert!(took >= Duration::from_secs(4), "took {took:?}");
    assert!(took < Duration::from_secs(60), "took {took:?}");
}
