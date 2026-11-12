//! Runs a test kernel under QEMU and checks its ZORAYA-TEST output.
//!
//! Usage:
//!
//! ```text
//! zoraya-test-runner [--label L] [--timeout SECS] [--plain] [--expect-panic NAME]...
//!                    [--known-failure NAME=NOTE]...
//!                    [--expect-line TEXT]... -- tools/run-qemu.sh --test ... KERNEL
//! ```
//!
//! The command after `--` is started with stdin closed and stdout read line
//! by line: the whole of it to `target/test-logs/LABEL.log`, the first and
//! last 200 lines to the terminal, prefixed with the label (`output.rs`
//! bounds it all: past 64 MiB of output QEMU is killed and the run fails). Exits 0 only if the protocol
//! was complete and consistent, every failure was an expected panic, the
//! kernel powered off after its end line, and QEMU exited with status 0.
//! The guest's exit status is never the only signal: the verdict comes from
//! the output lines. With `--plain` the kernel is not expected to speak the
//! protocol: it must print the expected lines, not panic, and power off.
//! With `--script FILE` the runner plays a script of keys against the output
//! (see `script.rs`); its stdin is then the serial line. With `--gui-script
//! FILE --qmp PATH` it plays a GUI script through QEMU's QMP socket at PATH
//! (see `gui.rs`).

#[macro_use]
mod say;
mod gui;
mod output;
mod protocol;
mod script;

use std::io::BufReader;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use protocol::{Checker, Exit, Flow};

/// How long to wait for power-off after the end line.
const POWER_OFF_GRACE: Duration = Duration::from_secs(5);

struct Args {
    label: String,
    timeout: u64,
    expect_panic: Vec<String>,
    known_failure: Vec<(String, String)>,
    expect_line: Vec<String>,
    plain: bool,
    script: Option<String>,
    gui_script: Option<String>,
    qmp: Option<String>,
    command: Vec<String>,
}

fn usage(msg: &str) -> ExitCode {
    esay!("zoraya-test-runner: {msg}");
    esay!(
        "usage: zoraya-test-runner [--label L] [--timeout SECS] [--plain] [--expect-panic NAME]... [--known-failure NAME=NOTE]... \
         [--expect-line TEXT]... [--script FILE] [--gui-script FILE --qmp PATH] -- COMMAND..."
    );
    ExitCode::from(2)
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        label: "qemu".into(),
        timeout: 60,
        expect_panic: Vec::new(),
        known_failure: Vec::new(),
        expect_line: Vec::new(),
        plain: false,
        script: None,
        gui_script: None,
        qmp: None,
        command: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--label" => a.label = value()?,
            "--timeout" => {
                a.timeout = value()?
                    .parse()
                    .map_err(|_| "--timeout needs whole seconds".to_string())?;
            }
            "--expect-panic" => a.expect_panic.push(value()?),
            "--known-failure" => {
                let v = value()?;
                let (name, note) = v
                    .split_once('=')
                    .ok_or("--known-failure needs NAME=NOTE".to_string())?;
                a.known_failure.push((name.into(), note.into()));
            }
            "--expect-line" => a.expect_line.push(value()?),
            "--plain" => a.plain = true,
            "--script" => a.script = Some(value()?),
            "--gui-script" => a.gui_script = Some(value()?),
            "--qmp" => a.qmp = Some(value()?),
            "--" => {
                a.command = it.collect();
                break;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if a.command.is_empty() {
        return Err("no command after --".into());
    }
    Ok(a)
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => return usage(&e),
    };
    if let Some(file) = &args.script {
        return script::run(file, &args.label, args.timeout, &args.command);
    }
    if let Some(file) = &args.gui_script {
        let Some(qmp) = &args.qmp else {
            return usage("--gui-script needs --qmp");
        };
        return gui::run(file, &args.label, args.timeout, qmp, &args.command);
    }
    let label = &args.label;
    let mut child = match Command::new(&args.command[0])
        .args(&args.command[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            esay!("[{label}] cannot start {}: {e}", args.command[0]);
            return ExitCode::FAILURE;
        }
    };

    let stdout = child.stdout.take().expect("stdout is piped");
    // Bounded: a guest that prints faster than lines are checked waits.
    let (tx, rx) = mpsc::sync_channel::<(String, output::Read)>(1024);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        while let Some(read) = output::read_line(&mut reader, &mut buf) {
            let line = String::from_utf8_lossy(&buf);
            let line = line.trim_end_matches(['\r', '\n']).to_string();
            if tx.send((line, read)).is_err() {
                break;
            }
        }
    });
    let mut out = output::Output::new(label);

    let mut checker = if args.plain {
        Checker::plain(args.expect_line.clone())
    } else {
        let mut c = Checker::new(args.expect_panic.clone(), args.expect_line.clone());
        for (name, note) in &args.known_failure {
            c = c.known_failure(name.clone(), note.clone());
        }
        c
    };
    let started = Instant::now();
    let deadline = started + Duration::from_secs(args.timeout);
    let mut ended_at: Option<Instant> = None;
    let exit = loop {
        let limit = match ended_at {
            Some(t) => (t + POWER_OFF_GRACE).min(deadline),
            None => deadline,
        };
        let wait = limit.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok((line, read)) => {
                if let Err(flood) = out.counted_line(&line, read.cut, read.bytes) {
                    kill(&mut child);
                    out.finish();
                    say!("[{label}] FAIL:");
                    say!("[{label}]   {flood}");
                    return ExitCode::FAILURE;
                }
                match checker.feed(&line) {
                    Flow::Continue => {}
                    Flow::Ended => ended_at = Some(Instant::now()),
                    Flow::Abort => {
                        kill(&mut child);
                        break Exit::Aborted;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                kill(&mut child);
                say!(
                    "[{label}] timed out after {} s (limit {} s): {}",
                    started.elapsed().as_secs(),
                    args.timeout,
                    checker.progress()
                );
                break if ended_at.is_some() {
                    Exit::NoPowerOff
                } else {
                    Exit::Timeout
                };
            }
            // Output closed: QEMU is exiting.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let left = deadline.saturating_duration_since(Instant::now());
                break wait_exit(&mut child, left.max(Duration::from_secs(1)));
            }
        }
    };

    out.finish();
    let report = checker.finish(exit, args.timeout);
    for k in &report.known {
        say!("[{label}] {k}");
    }
    if report.passed {
        say!(
            "[{label}] PASS: {} passed, {} expected failures",
            report.ok,
            report.expected_failures
        );
        ExitCode::SUCCESS
    } else {
        say!("[{label}] FAIL:");
        for p in &report.problems {
            say!("[{label}]   {p}");
        }
        ExitCode::FAILURE
    }
}

/// Waits for an exiting child, killing it after `limit`.
fn wait_exit(child: &mut Child, limit: Duration) -> Exit {
    let until = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Exit::Status(status.code()),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                kill(child);
                return Exit::Timeout;
            }
        }
    }
}
