//! Plays keys to a kernel on its serial line, and checks what comes back.
//!
//! A script is lines, each an instruction, run in order:
//!
//! ```text
//! # a comment
//! < TEXT          wait until TEXT appears after the last match
//! > KEYS          send KEYS
//! = N             send N bytes of 'x' at once
//! ! TEXT          fail if TEXT appears anywhere in the output, before or after
//! x TEXT          fail if TEXT lies between the last two matches
//! o TEXT          fail unless TEXT has appeared exactly once so far
//! ~ MS TEXT       for MS milliseconds, fail if TEXT arrives
//! @ KEYS | TEXT   send KEYS every 200 ms until TEXT appears
//! * N KEYS | TEXT N times: send KEYS and wait for TEXT
//! % NAME          print `RUNNER-TIME NAME SECONDS`, the time since the
//!                 previous `%` (or the start), for the stage 6 tables
//! ```
//!
//! TEXT and KEYS take the escapes \r \n \e \\ and \xHH.
//!
//! A run passes if every step passed, no forbidden text appeared, and QEMU
//! then exited with status 0, which only the kernel's power-off gives.

use std::io::{Read, Write};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;

use crate::output;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Expect(String),
    Send(Vec<u8>),
    Forbid(String),
    /// Not between the last two matches.
    Absent(String),
    Once(String),
    Quiet(u64, String),
    Until(Vec<u8>, String),
    Repeat(usize, Vec<u8>, String),
    /// A time mark: the seconds since the last one, measured on the host.
    Mark(String),
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
            Some(b'n') => out.push(b'\n'),
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

fn text_of(raw: &str) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&unescape(raw)?).into_owned())
}

fn keys_and_text(rest: &str) -> Result<(Vec<u8>, String), String> {
    let (keys, text) = rest.split_once(" | ").ok_or("no ` | `")?;
    Ok((unescape(keys)?, text_of(text)?))
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
            "<" => Step::Expect(text_of(rest).map_err(|e| format!("line {number}: {e}"))?),
            ">" => Step::Send(unescape(rest).map_err(|e| format!("line {number}: {e}"))?),
            "x" => Step::Absent(text_of(rest).map_err(|e| format!("line {number}: {e}"))?),
            "o" => Step::Once(text_of(rest).map_err(|e| format!("line {number}: {e}"))?),
            "~" => {
                let (ms, text) = rest
                    .split_once(' ')
                    .ok_or(format!("line {number}: no text"))?;
                let ms = ms
                    .parse()
                    .map_err(|_| format!("line {number}: not a time"))?;
                Step::Quiet(ms, text.to_string())
            }
            "@" => {
                let (keys, text) =
                    keys_and_text(rest).map_err(|e| format!("line {number}: {e}"))?;
                Step::Until(keys, text)
            }
            "*" => {
                let (n, rest) = rest
                    .split_once(' ')
                    .ok_or(format!("line {number}: no keys"))?;
                let n = n
                    .parse()
                    .map_err(|_| format!("line {number}: not a count"))?;
                let (keys, text) =
                    keys_and_text(rest).map_err(|e| format!("line {number}: {e}"))?;
                Step::Repeat(n, keys, text)
            }
            "=" => {
                let n = rest
                    .parse()
                    .map_err(|_| format!("line {number}: not a count"))?;
                Step::Send(vec![b'x'; n])
            }
            "!" => Step::Forbid(rest.to_string()),
            "%" => Step::Mark(rest.to_string()),
            _ => return Err(format!("line {number}: unknown step")),
        });
    }
    Ok(steps)
}

/// What has been seen so far, and where the next expectation starts. At
/// most `output::LOG_CAP` bytes of it (`add` fails past that).
#[derive(Default)]
pub struct Seen {
    text: String,
    /// Where the last match began its search, and where it ended.
    before: usize,
    from: usize,
    forbidden: Vec<String>,
    /// The first forbidden text found, and how far the text was searched.
    found: Option<String>,
    searched: usize,
    /// The text `take` last looked for, and how far it has looked.
    looking: Option<(String, usize)>,
    /// The start of an escape sequence cut at the end of the last read.
    pending: Vec<u8>,
}

/// Drops SGR sequences (`ESC [ digits ; ... m`): colour and faint are not
/// text, so scripts match what a person reads (ADR-0154). Gives the bytes
/// kept, and the start of a sequence the input ends inside.
pub fn strip_sgr(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            let mut j = i + 1;
            if j == bytes.len() {
                return (out, bytes[i..].to_vec());
            }
            if bytes[j] == b'[' {
                j += 1;
                while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                    j += 1;
                }
                if j == bytes.len() {
                    return (out, bytes[i..].to_vec());
                }
                if bytes[j] == b'm' {
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    (out, Vec::new())
}

impl Seen {
    pub fn add(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut all = core::mem::take(&mut self.pending);
        all.extend_from_slice(bytes);
        let (kept, pending) = strip_sgr(&all);
        self.pending = pending;
        output::keep(&mut self.text, &String::from_utf8_lossy(&kept))
    }

    pub fn forbid(&mut self, text: &str) {
        self.forbidden.push(text.to_string());
        // A new rule covers the output before it too.
        if self.found.is_none() && self.text.contains(text) {
            self.found = Some(text.to_string());
        }
    }

    /// True once `text` has appeared after the last match, and moves past it.
    /// Only what is new since the last look for the same text is searched,
    /// with an overlap, so waiting through a long output stays linear.
    pub fn take(&mut self, text: &str) -> bool {
        let mut start = match &self.looking {
            Some((t, upto)) if t == text => upto.saturating_sub(text.len()).max(self.from),
            _ => self.from,
        };
        // A match begins on a character boundary: moving up skips none.
        while !self.text.is_char_boundary(start) {
            start += 1;
        }
        match self.text[start..].find(text) {
            Some(at) => {
                self.before = self.from;
                self.from = start + at + text.len();
                self.looking = None;
                true
            }
            None => {
                self.looking = Some((text.to_string(), self.text.len()));
                false
            }
        }
    }

    /// How many times `text` has appeared.
    pub fn count(&self, text: &str) -> usize {
        self.text.matches(text).count()
    }

    /// True if `text` lies between the last two matches.
    pub fn between(&self, text: &str) -> bool {
        self.text[self.before..self.from].contains(text)
    }

    /// Output after the last match.
    pub fn since_match(&self) -> &str {
        &self.text[self.from..]
    }

    /// The first forbidden text in the whole output. Only what is new since
    /// the last call is searched, with an overlap for a text split between
    /// two arrivals, so a long output costs a linear search, not a square.
    pub fn violation(&mut self) -> Option<&str> {
        if self.found.is_none() {
            let longest = ALWAYS_FORBIDDEN
                .iter()
                .map(|t| t.len())
                .chain(self.forbidden.iter().map(String::len))
                .max()
                .unwrap_or(0);
            let mut start = self.searched.saturating_sub(longest);
            while !self.text.is_char_boundary(start) {
                start += 1;
            }
            let fresh = &self.text[start..];
            self.found = ALWAYS_FORBIDDEN
                .iter()
                .copied()
                .chain(self.forbidden.iter().map(String::as_str))
                .find(|t| fresh.contains(t))
                .map(str::to_string);
            self.searched = self.text.len();
        }
        self.found.as_deref()
    }
}

fn pump(child: &mut Child) -> mpsc::Receiver<Vec<u8>> {
    let mut stdout = child.stdout.take().expect("stdout is piped");
    // Bounded: a guest that prints faster than it is matched waits.
    let (tx, rx) = mpsc::sync_channel(256);
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

/// The output so far, and the way more of it arrives.
struct Link<'a> {
    rx: mpsc::Receiver<Vec<u8>>,
    seen: &'a mut Seen,
    deadline: Instant,
    out: output::Output,
}

impl Link<'_> {
    /// Takes what arrives before `limit`: false if the output has ended,
    /// and an error past `output::LOG_CAP` of it.
    fn receive(&mut self, limit: Instant) -> Result<bool, String> {
        let wait = limit
            .min(self.deadline)
            .saturating_duration_since(Instant::now());
        match self.rx.recv_timeout(wait) {
            Ok(bytes) => {
                self.out.bytes(&bytes)?;
                self.seen.add(&bytes)?;
                Ok(true)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(true),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(false),
        }
    }

    fn expect(&mut self, text: &str, limit: Instant) -> Result<bool, String> {
        while !self.seen.take(text) {
            if let Some(bad) = self.seen.violation() {
                return Err(format!("forbidden output: {bad}"));
            }
            let now = Instant::now();
            if now >= self.deadline {
                return Err(format!("timed out waiting for {text:?}"));
            }
            if now >= limit {
                return Ok(false);
            }
            if !self.receive(limit)? {
                return Err(format!("output ended before {text:?}"));
            }
        }
        Ok(true)
    }
}

fn send(stdin: &mut impl Write, bytes: &[u8]) -> Result<(), String> {
    stdin
        .write_all(bytes)
        .and_then(|()| stdin.flush())
        .map_err(|e| format!("send: {e}"))
}

fn drive(
    steps: &[Step],
    child: &mut Child,
    seen: &mut Seen,
    deadline: Instant,
    label: &str,
) -> Result<(), String> {
    let stdin = child.stdin.take().ok_or("stdin is not piped")?;
    let rx = pump(child);
    let mut link = Link {
        rx,
        seen,
        deadline,
        // The unit tests keep their logs out of the tree.
        #[cfg(not(test))]
        out: output::Output::new(label),
        #[cfg(test)]
        out: output::Output::quiet(label),
    };
    let result = steps_then_drain(steps, child, &mut link, stdin);
    link.out.finish();
    result
}

fn steps_then_drain(
    steps: &[Step],
    child: &mut Child,
    link: &mut Link<'_>,
    mut stdin: std::process::ChildStdin,
) -> Result<(), String> {
    let deadline = link.deadline;
    let far = deadline + Duration::from_secs(1);
    let mut mark = Instant::now();
    for step in steps {
        match step {
            Step::Mark(name) => {
                let now = Instant::now();
                let line = format!(
                    "RUNNER-TIME {name} {:.3}",
                    now.duration_since(mark).as_secs_f64()
                );
                link.out.counted_line(&line, false, line.len() + 1)?;
                mark = now;
            }
            Step::Forbid(text) => link.seen.forbid(text),
            Step::Send(bytes) => send(&mut stdin, bytes)?,
            Step::Expect(text) => {
                link.expect(text, far)?;
            }
            Step::Absent(text) => {
                if link.seen.between(text) {
                    return Err(format!("{text:?} appeared where it must not"));
                }
            }
            Step::Once(text) => {
                let count = link.seen.count(text);
                if count != 1 {
                    return Err(format!("{text:?} appeared {count} times"));
                }
            }
            Step::Quiet(ms, text) => {
                let until = Instant::now() + Duration::from_millis(*ms);
                let start = link.seen.since_match().len();
                while Instant::now() < until {
                    if !link.receive(until)? {
                        break;
                    }
                }
                if link.seen.since_match()[start..].contains(text.as_str()) {
                    return Err(format!("{text:?} arrived in a quiet time"));
                }
            }
            Step::Until(keys, text) => loop {
                send(&mut stdin, keys)?;
                let limit = Instant::now() + Duration::from_millis(200);
                if link.expect(text, limit)? {
                    break;
                }
            },
            Step::Repeat(n, keys, text) => {
                for _ in 0..*n {
                    send(&mut stdin, keys)?;
                    link.expect(text, far)?;
                }
            }
        }
    }
    // The script's last step should have ended the machine: drain the rest.
    drop(stdin);
    while link.receive(deadline)? && Instant::now() < deadline {}
    if let Some(bad) = link.seen.violation() {
        return Err(format!("forbidden output: {bad}"));
    }
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
    match play(&steps, timeout, command, label) {
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

fn play(steps: &[Step], timeout: u64, command: &[String], label: &str) -> Result<(), String> {
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", command[0]))?;
    let mut seen = Seen::default();
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let result = drive(steps, &mut child, &mut seen, deadline, label);
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_is_dropped_even_when_cut() {
        assert_eq!(strip_sgr(b"a\x1b[31mb\x1b[mc\x1b[2Kd").0, b"abc\x1b[2Kd");
        let mut seen = Seen::default();
        seen.add(b"x\x1b[2").unwrap();
        seen.add(b"2my\x1b").unwrap();
        seen.add(b"[mz").unwrap();
        assert!(seen.take("xyz"));
    }

    #[test]
    fn escapes_become_bytes() {
        assert_eq!(
            unescape("a\\r\\e[A\\x03\\\\\\n").unwrap(),
            b"a\r\x1b[A\x03\\\n"
        );
        assert!(unescape("\\q").is_err());
        assert!(unescape("\\x0").is_err());
    }

    #[test]
    fn a_script_parses_and_a_bad_one_does_not() {
        let steps = parse("# note\n\n< ready\n> go\\r\n= 3\n! bad\n% w1 make\n").unwrap();
        assert_eq!(
            steps,
            [
                Step::Expect("ready".into()),
                Step::Send(b"go\r".to_vec()),
                Step::Send(b"xxx".to_vec()),
                Step::Forbid("bad".into()),
                Step::Mark("w1 make".into()),
            ]
        );
        assert!(parse("? what\n").is_err());
        assert!(parse("<\n").is_err());
    }

    #[test]
    fn expectations_match_in_order() {
        let mut seen = Seen::default();
        seen.add(b"one two one").unwrap();
        assert!(seen.take("two"));
        assert!(!seen.take("two"), "a match is used up");
        assert!(seen.take("one"));
    }

    fn shell(script: &str, steps: &str, timeout: u64) -> Result<(), String> {
        let command = ["sh", "-c", script].map(String::from);
        play(&parse(steps).unwrap(), timeout, &command, "script-test")
    }

    #[test]
    fn a_clean_run_passes() {
        assert_eq!(shell("echo ready", "< ready\n", 5), Ok(()));
    }

    #[test]
    fn a_time_mark_passes_and_changes_nothing() {
        assert_eq!(
            shell("echo ready", "% start\n< ready\n% ready\n", 5),
            Ok(())
        );
    }

    #[test]
    fn a_bad_exit_status_fails() {
        assert!(shell("echo ready; exit 3", "< ready\n", 5).is_err());
    }

    #[test]
    fn forbidden_text_after_the_last_step_fails() {
        let steps = "! late\n< ready\n";
        assert!(shell("echo ready; sleep 0.2; echo late", steps, 5).is_err());
    }

    #[test]
    fn no_power_off_fails() {
        assert!(shell("echo ready; sleep 30", "< ready\n", 1).is_err());
    }

    #[test]
    fn a_missing_line_fails() {
        assert!(shell("echo other", "< ready\n", 5).is_err());
    }

    #[test]
    fn absent_looks_between_the_last_two_matches() {
        let script = "printf 'a b c'";
        assert!(shell(script, "< a\n< c\nx b\n", 5).is_err());
        assert_eq!(shell(script, "< b\n< c\nx a\n", 5), Ok(()));
    }

    #[test]
    fn once_counts_the_whole_output() {
        assert_eq!(shell("echo a b", "< b\no a\n", 5), Ok(()));
        assert!(shell("echo a a b", "< b\no a\n", 5).is_err());
    }

    #[test]
    fn quiet_fails_on_new_text_only() {
        let script = "echo go; sleep 0.1; echo tick";
        assert!(shell(script, "< go\n~ 400 tick\n", 5).is_err());
        assert_eq!(shell("echo go", "< go\n~ 100 tick\n", 5), Ok(()));
    }

    #[test]
    fn until_sends_again() {
        let script = "read a; read b; echo done";
        assert_eq!(shell(script, "@ x\\n | done\n", 5), Ok(()));
    }

    #[test]
    fn forbidden_text_is_found_anywhere() {
        let mut seen = Seen::default();
        seen.add(b"early bad").unwrap();
        seen.forbid("bad");
        assert_eq!(seen.violation(), Some("bad"));
        let mut seen = Seen::default();
        seen.add(b"ZORAYA-PANIC at x").unwrap();
        assert_eq!(seen.violation(), Some("ZORAYA-PANIC"));
    }
}
