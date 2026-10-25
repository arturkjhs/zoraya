//! Runs a test kernel under QEMU and checks its ZORAYA-TEST output.
//!
//! Usage:
//!
//! ```text
//! zoraya-test-runner [--label L] [--timeout SECS] [--expect-panic NAME]...
//!                    [--expect-line TEXT]... -- tools/run-qemu.sh --test ... KERNEL
//! ```
//!
//! The command after `--` is started with stdin closed and stdout read line
//! by line (echoed, prefixed with the label). Exits 0 only if the protocol
//! was complete and consistent, every failure was an expected panic, the
//! kernel powered off after its end line, and QEMU exited with status 0.
//! The guest's exit status is never the only signal: the verdict comes from
//! the output lines.

mod protocol;

use std::io::{BufRead, BufReader};
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
    expect_line: Vec<String>,
    command: Vec<String>,
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("zoraya-test-runner: {msg}");
    eprintln!(
        "usage: zoraya-test-runner [--label L] [--timeout SECS] [--expect-panic NAME]... \
         [--expect-line TEXT]... -- COMMAND..."
    );
    ExitCode::from(2)
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        label: "qemu".into(),
        timeout: 60,
        expect_panic: Vec::new(),
        expect_line: Vec::new(),
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
            "--expect-line" => a.expect_line.push(value()?),
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
            eprintln!("[{label}] cannot start {}: {e}", args.command[0]);
            return ExitCode::FAILURE;
        }
    };

    let stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim_end_matches(['\r', '\n']).to_string();
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut checker = Checker::new(args.expect_panic.clone(), args.expect_line.clone());
    let deadline = Instant::now() + Duration::from_secs(args.timeout);
    let mut ended_at: Option<Instant> = None;
    let exit = loop {
        let limit = match ended_at {
            Some(t) => (t + POWER_OFF_GRACE).min(deadline),
            None => deadline,
        };
        let wait = limit.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(line) => {
                println!("[{label}] {line}");
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

    let report = checker.finish(exit, args.timeout);
    if report.passed {
        println!(
            "[{label}] PASS: {} passed, {} expected failures",
            report.ok, report.expected_failures
        );
        ExitCode::SUCCESS
    } else {
        println!("[{label}] FAIL:");
        for p in &report.problems {
            println!("[{label}]   {p}");
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
