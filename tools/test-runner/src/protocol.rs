//! The ZORAYA-TEST line protocol and the pass/fail verdict.
//!
//! ```text
//! ZORAYA-TEST begin <count>
//! ZORAYA-TEST ok <name>
//! ZORAYA-TEST fail <name> <message>
//! ZORAYA-TEST end pass=<n> fail=<m>
//! ```
//!
//! A kernel panic outside a test prints a line starting `ZORAYA-PANIC`.

use std::collections::HashSet;

const TEST: &str = "ZORAYA-TEST ";
const PANIC: &str = "ZORAYA-PANIC";

/// What the caller should do after a line.
#[derive(Debug, PartialEq, Eq)]
pub enum Flow {
    /// Keep reading.
    Continue,
    /// The `end` line arrived; wait for the machine to power off.
    Ended,
    /// Nothing more is worth waiting for (panic outside a test).
    Abort,
}

/// How the QEMU process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Exited on its own with this status (None: killed by a signal).
    Status(Option<i32>),
    /// The overall timeout expired; the process was killed.
    Timeout,
    /// The `end` line arrived but the machine did not power off in time;
    /// the process was killed.
    NoPowerOff,
    /// Killed after a panic outside a test.
    Aborted,
}

#[derive(Debug, Default)]
pub struct Checker {
    expect_panic: Vec<String>,
    expect_lines: Vec<String>,
    seen_expect_lines: Vec<bool>,
    begin: Option<usize>,
    names: HashSet<String>,
    ok: Vec<String>,
    failed: Vec<(String, String)>,
    end: Option<(usize, usize)>,
    errors: Vec<String>,
}

/// The verdict for one QEMU run.
#[derive(Debug)]
pub struct Report {
    pub passed: bool,
    pub ok: usize,
    pub expected_failures: usize,
    pub problems: Vec<String>,
}

impl Checker {
    /// `expect_panic`: tests that must fail with a panic. `expect_lines`:
    /// substrings that must each appear on some output line.
    pub fn new(expect_panic: Vec<String>, expect_lines: Vec<String>) -> Checker {
        Checker {
            seen_expect_lines: vec![false; expect_lines.len()],
            expect_panic,
            expect_lines,
            ..Checker::default()
        }
    }

    pub fn feed(&mut self, line: &str) -> Flow {
        let line = line.trim_end_matches(['\r', '\n']);
        for (i, e) in self.expect_lines.iter().enumerate() {
            if line.contains(e.as_str()) {
                self.seen_expect_lines[i] = true;
            }
        }
        if line.starts_with(PANIC) {
            self.errors
                .push(format!("kernel panic outside a test: {line}"));
            return Flow::Abort;
        }
        let Some(rest) = line.strip_prefix(TEST) else {
            return Flow::Continue;
        };
        if self.end.is_some() {
            self.errors
                .push(format!("output after the end line: {line}"));
            return Flow::Continue;
        }
        let (word, args) = rest.split_once(' ').unwrap_or((rest, ""));
        match word {
            "begin" => match (self.begin, args.parse::<usize>()) {
                (None, Ok(n)) => self.begin = Some(n),
                (Some(_), _) => self.errors.push("second begin line".into()),
                (_, Err(_)) => self.errors.push(format!("malformed begin line: {line}")),
            },
            "ok" | "fail" if self.begin.is_none() => {
                self.errors
                    .push(format!("result before the begin line: {line}"));
            }
            "ok" => {
                if self.record_name(args) {
                    self.ok.push(args.to_string());
                }
            }
            "fail" => {
                let (name, msg) = args.split_once(' ').unwrap_or((args, ""));
                if self.record_name(name) {
                    self.failed.push((name.to_string(), msg.to_string()));
                }
            }
            "end" => match parse_end(args) {
                Some(counts) => {
                    self.end = Some(counts);
                    return Flow::Ended;
                }
                None => self.errors.push(format!("malformed end line: {line}")),
            },
            _ => self.errors.push(format!("unknown protocol line: {line}")),
        }
        Flow::Continue
    }

    fn record_name(&mut self, name: &str) -> bool {
        if name.is_empty() || name.contains(char::is_whitespace) {
            self.errors.push(format!("malformed test name {name:?}"));
            return false;
        }
        if !self.names.insert(name.to_string()) {
            self.errors.push(format!("test {name} reported twice"));
            return false;
        }
        true
    }

    pub fn finish(mut self, exit: Exit, timeout_secs: u64) -> Report {
        match exit {
            Exit::Timeout => self.errors.push(format!(
                "timeout after {timeout_secs} s ({})",
                if self.begin.is_none() {
                    "no begin line"
                } else {
                    "no end line"
                }
            )),
            Exit::NoPowerOff => self
                .errors
                .push("the kernel printed the end line but did not power off".into()),
            Exit::Status(Some(0)) | Exit::Aborted => {}
            Exit::Status(Some(c)) => self.errors.push(format!("QEMU exited with status {c}")),
            Exit::Status(None) => self.errors.push("QEMU was killed by a signal".into()),
        }
        let ran = self.ok.len() + self.failed.len();
        match self.begin {
            None => self.errors.push("no begin line".into()),
            Some(n) if self.end.is_some() && n != ran => {
                self.errors
                    .push(format!("begin announced {n} tests, {ran} reported"));
            }
            _ => {}
        }
        match self.end {
            None if matches!(exit, Exit::Status(_)) => {
                self.errors.push("QEMU exited without an end line".into());
            }
            Some((p, f)) if (p, f) != (self.ok.len(), self.failed.len()) => {
                self.errors.push(format!(
                    "end line says pass={p} fail={f}, lines say pass={} fail={}",
                    self.ok.len(),
                    self.failed.len()
                ));
            }
            _ => {}
        }
        let mut expected_failures = 0;
        for (name, msg) in &self.failed {
            if self.expect_panic.contains(name) {
                if msg.starts_with("panic ") {
                    expected_failures += 1;
                } else {
                    self.errors.push(format!(
                        "{name} failed as expected but not with a panic: {msg}"
                    ));
                }
            } else {
                self.errors.push(format!("{name} failed: {msg}"));
            }
        }
        if self.end.is_some() {
            for name in &self.expect_panic {
                if self.ok.contains(name) {
                    self.errors
                        .push(format!("{name} was expected to panic but passed"));
                } else if !self.failed.iter().any(|(n, _)| n == name) {
                    self.errors
                        .push(format!("{name} was expected to panic but did not run"));
                }
            }
        }
        for (e, seen) in self.expect_lines.iter().zip(&self.seen_expect_lines) {
            if !seen {
                self.errors.push(format!("no output line contains {e:?}"));
            }
        }
        Report {
            passed: self.errors.is_empty(),
            ok: self.ok.len(),
            expected_failures,
            problems: self.errors,
        }
    }
}

fn parse_end(args: &str) -> Option<(usize, usize)> {
    let (p, f) = args.split_once(' ')?;
    let p = p.strip_prefix("pass=")?.parse().ok()?;
    let f = f.strip_prefix("fail=")?.parse().ok()?;
    Some((p, f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str], exit: Exit) -> Report {
        let mut c = Checker::new(vec!["boom".into()], vec!["banner".into()]);
        for l in lines {
            if c.feed(l) == Flow::Abort {
                return c.finish(Exit::Aborted, 10);
            }
        }
        c.finish(exit, 10)
    }

    const GOOD: &[&str] = &[
        "banner line\r",
        "ZORAYA-TEST begin 3",
        "ZORAYA-TEST ok a",
        "ZORAYA-TEST fail boom panic at x.rs:1:1: deliberate",
        "ZORAYA-TEST ok c",
        "ZORAYA-TEST end pass=2 fail=1\r",
    ];

    #[test]
    fn clean_run_passes() {
        let r = run(GOOD, Exit::Status(Some(0)));
        assert!(r.passed, "{:?}", r.problems);
        assert_eq!((r.ok, r.expected_failures), (2, 1));
    }

    #[test]
    fn missing_end_line() {
        let r = run(&GOOD[..5], Exit::Status(Some(0)));
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.contains("without an end line")));
    }

    #[test]
    fn timeout() {
        let r = run(&GOOD[..3], Exit::Timeout);
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.contains("timeout")));
    }

    #[test]
    fn no_power_off() {
        let r = run(GOOD, Exit::NoPowerOff);
        assert!(!r.passed);
    }

    #[test]
    fn panic_outside_test_aborts() {
        let r = run(
            &[
                "banner",
                "ZORAYA-TEST begin 1",
                "ZORAYA-PANIC at a.rs:1:1: x",
            ],
            Exit::Status(Some(0)),
        );
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.contains("outside a test")));
    }

    #[test]
    fn unexpected_failure() {
        let mut lines = GOOD.to_vec();
        lines[2] = "ZORAYA-TEST fail a assertion failed";
        lines[5] = "ZORAYA-TEST end pass=1 fail=2";
        let r = run(&lines, Exit::Status(Some(0)));
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.starts_with("a failed")));
    }

    #[test]
    fn expected_panic_that_passes() {
        let mut lines = GOOD.to_vec();
        lines[3] = "ZORAYA-TEST ok boom";
        lines[5] = "ZORAYA-TEST end pass=3 fail=0";
        let r = run(&lines, Exit::Status(Some(0)));
        assert!(!r.passed);
        assert!(
            r.problems
                .iter()
                .any(|p| p.contains("expected to panic but passed"))
        );
    }

    #[test]
    fn expected_failure_without_panic() {
        let mut lines = GOOD.to_vec();
        lines[3] = "ZORAYA-TEST fail boom something else";
        let r = run(&lines, Exit::Status(Some(0)));
        assert!(!r.passed);
    }

    #[test]
    fn count_mismatches() {
        let mut lines = GOOD.to_vec();
        lines[1] = "ZORAYA-TEST begin 4";
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
        let mut lines = GOOD.to_vec();
        lines[5] = "ZORAYA-TEST end pass=3 fail=0";
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
    }

    #[test]
    fn duplicates_and_garbage() {
        let mut lines = GOOD.to_vec();
        lines[4] = "ZORAYA-TEST ok a";
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
        let mut lines = GOOD.to_vec();
        lines.insert(2, "ZORAYA-TEST maybe x");
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
        let mut lines = GOOD.to_vec();
        lines.push("ZORAYA-TEST ok late");
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
    }

    #[test]
    fn missing_expected_line() {
        let r = run(&GOOD[1..], Exit::Status(Some(0)));
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.contains("banner")));
    }

    #[test]
    fn qemu_error_status() {
        assert!(!run(GOOD, Exit::Status(Some(1))).passed);
        assert!(!run(GOOD, Exit::Status(None)).passed);
    }
}
