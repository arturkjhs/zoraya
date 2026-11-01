//! Plays keys to a kernel on its serial line, and checks what comes back.
//!
//! A script is lines, each an instruction, run in order:
//!
//! ```text
//! # a comment
//! < TEXT      wait until TEXT appears after the last match
//! > TEXT      send TEXT; \r \e \\ and \xHH are escapes
//! = N         send N bytes of 'x' at once
//! ! TEXT      fail if TEXT appears anywhere, from here to the end
//! ```
//!
//! A run passes if every step passed, no forbidden text appeared, and QEMU
//! then exited with status 0, which only the kernel's power-off gives.

use std::io::{Read, Write};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Expect(String),
    Send(Vec<u8>),
    Forbid(String),
}

/// Text every run forbids: the kernel gave up.
const ALWAYS_FORBIDDEN: [&str; 2] = ["ZORAYA-PANIC", "ZORAYA-FAULT"];

/// What `> TEXT` sends.
pub fn unescape(text: &str) -> Result<Vec<u8>, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'\\' {
            out.push(bytes[at]);
            at += 1;
            continue;
        }
        match bytes.get(at + 1) {
            Some(b'r') => out.push(b'\r'),
            Some(b'e') => out.push(0x1b),
            Some(b'\\') => out.push(b'\\'),
            Some(b'x') => {
                let hex = text.get(at + 2..at + 4).ok_or("short \\x escape")?;
                out.push(u8::from_str_radix(hex, 16).map_err(|e| e.to_string())?);
                at += 2;
            }
            other => return Err(format!("unknown escape {other:?}")),
        }
        at += 2;
    }
    Ok(out)
}

pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let number = i + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line
            .split_once(' ')
            .ok_or(format!("line {number}: no argument"))?;
        steps.push(match kind {
            "<" => Step::Expect(rest.to_string()),
            ">" => Step::Send(unescape(rest).map_err(|e| format!("line {number}: {e}"))?),
            "=" => {
                let n = rest
                    .parse()
                    .map_err(|_| format!("line {number}: not a count"))?;
                Step::Send(vec![b'x'; n])
            }
            "!" => Step::Forbid(rest.to_string()),
            _ => return Err(format!("line {number}: unknown step")),
        });
    }
    Ok(steps)
}

/// What has been seen so far, and where the next expectation starts.
#[derive(Default)]
pub struct Seen {
    text: String,
    from: usize,
    forbidden: Vec<String>,
}

impl Seen {
    pub fn add(&mut self, bytes: &[u8]) {
        self.text.push_str(&String::from_utf8_lossy(bytes));
    }

    pub fn forbid(&mut self, text: &str) {
        self.forbidden.push(text.to_string());
    }

    /// True once `text` has appeared after the last match, and moves past it.
    pub fn take(&mut self, text: &str) -> bool {
        match self.text[self.from..].find(text) {
            Some(at) => {
                self.from += at + text.len();
                true
            }
            None => false,
        }
    }

    /// The first forbidden text in the whole output.
    pub fn violation(&self) -> Option<&str> {
        ALWAYS_FORBIDDEN
            .iter()
            .copied()
            .chain(self.forbidden.iter().map(String::as_str))
            .find(|t| self.text.contains(t))
    }
}

fn pump(child: &mut Child) -> mpsc::Receiver<Vec<u8>> {
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

fn drive(
    steps: &[Step],
    child: &mut Child,
    seen: &mut Seen,
    deadline: Instant,
    label: &str,
) -> Result<(), String> {
    let mut stdin = child.stdin.take().ok_or("stdin is not piped")?;
    let rx = pump(child);
    let receive = |seen: &mut Seen, limit: Instant| -> Result<bool, String> {
        let wait = limit.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(bytes) => {
                print!("{}", String::from_utf8_lossy(&bytes).replace('\r', ""));
                seen.add(&bytes);
                Ok(true)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err("timed out".to_string()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(false),
        }
    };
    for step in steps {
        match step {
            Step::Forbid(text) => seen.forbid(text),
            Step::Send(bytes) => {
                stdin
                    .write_all(bytes)
                    .and_then(|()| stdin.flush())
                    .map_err(|e| format!("send: {e}"))?;
            }
            Step::Expect(text) => {
                while !seen.take(text) {
                    if let Some(bad) = seen.violation() {
                        return Err(format!("forbidden output: {bad}"));
                    }
                    match receive(seen, deadline) {
                        Ok(true) => {}
                        Ok(false) => return Err(format!("output ended before {text:?}")),
                        Err(e) => return Err(format!("{e} waiting for {text:?}")),
                    }
                }
            }
        }
    }
    // The script's last step should have ended the machine: drain the rest.
    drop(stdin);
    while let Ok(true) = receive(seen, deadline) {}
    println!();
    if let Some(bad) = seen.violation() {
        return Err(format!("forbidden output: {bad}"));
    }
    let _ = label;
    wait_exit(child, deadline)
}

fn wait_exit(child: &mut Child, deadline: Instant) -> Result<(), String> {
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("QEMU exited with {status}")),
            None if Instant::now() > deadline => return Err("no power-off".to_string()),
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

pub fn run(file: &str, label: &str, timeout: u64, command: &[String]) -> ExitCode {
    let steps = match std::fs::read_to_string(file)
        .map_err(|e| e.to_string())
        .and_then(|text| parse(&text))
    {
        Ok(steps) => steps,
        Err(e) => {
            eprintln!("[{label}] {file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut child = match Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("[{label}] cannot start {}: {e}", command[0]);
            return ExitCode::FAILURE;
        }
    };
    let mut seen = Seen::default();
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let result = drive(&steps, &mut child, &mut seen, deadline, label);
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(()) => {
            println!("[{label}] PASS: {} steps", steps.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("[{label}] FAIL: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_become_bytes() {
        assert_eq!(unescape("a\\r\\e[A\\x03\\\\").unwrap(), b"a\r\x1b[A\x03\\");
        assert!(unescape("\\q").is_err());
        assert!(unescape("\\x0").is_err());
    }

    #[test]
    fn a_script_parses_and_a_bad_one_does_not() {
        let steps = parse("# note\n\n< ready\n> go\\r\n= 3\n! bad\n").unwrap();
        assert_eq!(
            steps,
            [
                Step::Expect("ready".into()),
                Step::Send(b"go\r".to_vec()),
                Step::Send(b"xxx".to_vec()),
                Step::Forbid("bad".into()),
            ]
        );
        assert!(parse("? what\n").is_err());
        assert!(parse("<\n").is_err());
    }

    #[test]
    fn expectations_match_in_order() {
        let mut seen = Seen::default();
        seen.add(b"one two one");
        assert!(seen.take("two"));
        assert!(!seen.take("two"), "a match is used up");
        assert!(seen.take("one"));
    }

    #[test]
    fn forbidden_text_is_found_anywhere() {
        let mut seen = Seen::default();
        seen.add(b"early bad");
        seen.forbid("bad");
        assert_eq!(seen.violation(), Some("bad"));
        let mut seen = Seen::default();
        seen.add(b"ZORAYA-PANIC at x");
        assert_eq!(seen.violation(), Some("ZORAYA-PANIC"));
    }
}
