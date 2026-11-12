//! The ZORAYA-TEST line protocol and the pass/fail verdict.
//!
//! ```text
//! ZORAYA-TEST begin <count>
//! ZORAYA-TEST run <name>          (before each test)
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
    /// Quarantined tests and their backlog note: each may fail without
    /// failing the run.
    known_failures: Vec<(String, String)>,
    expect_lines: Vec<String>,
    seen_expect_lines: Vec<bool>,
    begin: Option<usize>,
    names: HashSet<String>,
    /// The test whose `run` line came last and has no result yet.
    running: Option<String>,
    ok: Vec<String>,
    failed: Vec<(String, String)>,
    end: Option<(usize, usize)>,
    errors: Vec<String>,
    /// Problems past `MAX_ERRORS`, counted and not kept: a guest that loops
    /// on a bad line must not fill the runner (docs/backlog.md, 6c).
    more_errors: usize,
    /// See [`Checker::plain`].
    plain: bool,
}

/// The verdict for one QEMU run.
#[derive(Debug)]
pub struct Report {
    pub passed: bool,
    pub ok: usize,
    pub expected_failures: usize,
    /// One line for each quarantined test that failed.
    pub known: Vec<String>,
    pub problems: Vec<String>,
}

/// The most problems a report keeps; the rest are counted.
pub const MAX_ERRORS: usize = 100;
/// The most tests a run may report: the test kernel has a few hundred.
pub const MAX_TESTS: usize = 10_000;

impl Checker {
    /// `finish`'s own findings: a handful, each kept, past the cap too, so
    /// that the reason a run failed is always shown.
    fn verdict(&mut self, problem: String) {
        self.errors.push(problem);
    }

    fn error(&mut self, problem: String) {
        if self.errors.len() < MAX_ERRORS {
            self.errors.push(problem);
        } else {
            self.more_errors += 1;
        }
    }
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

    /// Lets test `name` fail without failing the run; `note` names its
    /// backlog entry.
    pub fn known_failure(mut self, name: String, note: String) -> Checker {
        self.known_failures.push((name, note));
        self
    }

    /// For a kernel that speaks no protocol, such as the normal kernel: only
    /// the expected lines, the absence of panics and a clean power-off count.
    pub fn plain(expect_lines: Vec<String>) -> Checker {
        Checker {
            plain: true,
            ..Checker::new(Vec::new(), expect_lines)
        }
    }

    /// Where the run stands, for the line the runner prints on a timeout, so
    /// that a slow run (many tests passed, the last one still going) can be
    /// told from a hang (few passed, one test stuck).
    pub fn progress(&self) -> String {
        let total = self.begin.map_or(String::new(), |n| format!(" of {n}"));
        let running = match &self.running {
            Some(t) => format!("running test {t}"),
            None => "no test running".to_string(),
        };
        format!(
            "{} tests passed{total}, {} failed, {running}",
            self.ok.len(),
            self.failed.len()
        )
    }

    pub fn feed(&mut self, line: &str) -> Flow {
        let line = line.trim_end_matches(['\r', '\n']);
        for (i, e) in self.expect_lines.iter().enumerate() {
            if line.contains(e.as_str()) {
                self.seen_expect_lines[i] = true;
            }
        }
        if line.starts_with(PANIC) {
            self.error(format!("kernel panic outside a test: {line}"));
            return Flow::Abort;
        }
        if self.plain {
            return Flow::Continue;
        }
        let Some(rest) = line.strip_prefix(TEST) else {
            return Flow::Continue;
        };
        if self.end.is_some() {
            self.error(format!("output after the end line: {line}"));
            return Flow::Continue;
        }
        let (word, args) = rest.split_once(' ').unwrap_or((rest, ""));
        match word {
            "begin" => match (self.begin, args.parse::<usize>()) {
                (None, Ok(n)) => self.begin = Some(n),
                (Some(_), _) => self.error("second begin line".into()),
                (_, Err(_)) => self.error(format!("malformed begin line: {line}")),
            },
            "run" | "ok" | "fail" if self.begin.is_none() => {
                self.error(format!("result before the begin line: {line}"));
            }
            "run" => {
                if let Some(prev) = self.running.replace(args.to_string()) {
                    self.error(format!("{args} started before {prev} reported a result"));
                }
            }
            "ok" => {
                self.check_running(args);
                if self.record_name(args) {
                    self.ok.push(args.to_string());
                }
            }
            "fail" => {
                let (name, msg) = args.split_once(' ').unwrap_or((args, ""));
                self.check_running(name);
                if self.record_name(name) {
                    self.failed.push((name.to_string(), msg.to_string()));
                }
            }
            "end" => match parse_end(args) {
                Some(counts) => {
                    self.end = Some(counts);
                    return Flow::Ended;
                }
                None => self.error(format!("malformed end line: {line}")),
            },
            _ => self.error(format!("unknown protocol line: {line}")),
        }
        Flow::Continue
    }

    fn check_running(&mut self, name: &str) {
        if self.running.take().as_deref() != Some(name) {
            self.error(format!("result for {name} without a run line for it"));
        }
    }

    fn record_name(&mut self, name: &str) -> bool {
        if name.is_empty() || name.contains(char::is_whitespace) {
            self.error(format!("malformed test name {name:?}"));
            return false;
        }
        if self.names.len() >= MAX_TESTS {
            self.error(format!("more than {MAX_TESTS} tests"));
            return false;
        }
        if !self.names.insert(name.to_string()) {
            self.error(format!("test {name} reported twice"));
            return false;
        }
        true
    }

    pub fn finish(mut self, exit: Exit, timeout_secs: u64) -> Report {
        match exit {
            Exit::Timeout => self.verdict(format!(
                "timeout after {timeout_secs} s ({})",
                match (&self.begin, &self.running) {
                    (None, _) => "no begin line".to_string(),
                    (_, Some(t)) => format!("test {t} did not finish"),
                    _ => "no end line".to_string(),
                }
            )),
            Exit::NoPowerOff => {
                self.verdict("the kernel printed the end line but did not power off".into())
            }
            Exit::Status(Some(0)) | Exit::Aborted => {}
            Exit::Status(Some(c)) => self.verdict(format!("QEMU exited with status {c}")),
            Exit::Status(None) => self.verdict("QEMU was killed by a signal".into()),
        }
        let ran = self.ok.len() + self.failed.len();
        match self.begin {
            None if self.plain => {}
            None => self.verdict("no begin line".into()),
            Some(n) if self.end.is_some() && n != ran => {
                self.verdict(format!("begin announced {n} tests, {ran} reported"));
            }
            _ => {}
        }
        match self.end {
            None if matches!(exit, Exit::Status(_)) && !self.plain => {
                self.verdict("QEMU exited without an end line".into());
            }
            Some((p, f)) if (p, f) != (self.ok.len(), self.failed.len()) => {
                self.verdict(format!(
                    "end line says pass={p} fail={f}, lines say pass={} fail={}",
                    self.ok.len(),
                    self.failed.len()
                ));
            }
            _ => {}
        }
        let mut expected_failures = 0;
        let mut known = Vec::new();
        let failed = std::mem::take(&mut self.failed);
        let expect_panic = std::mem::take(&mut self.expect_panic);
        let ok = std::mem::take(&mut self.ok);
        let expect_lines = std::mem::take(&mut self.expect_lines);
        let seen_lines = std::mem::take(&mut self.seen_expect_lines);
        for (name, msg) in &failed {
            if let Some((_, note)) = self.known_failures.iter().find(|(n, _)| n == name) {
                known.push(format!("{name}: known failure, see backlog: {note}"));
            } else if expect_panic.contains(name) {
                if msg.starts_with("panic ") {
                    expected_failures += 1;
                } else {
                    self.verdict(format!(
                        "{name} failed as expected but not with a panic: {msg}"
                    ));
                }
            } else {
                self.verdict(format!("{name} failed: {msg}"));
            }
        }
        if self.end.is_some() {
            for name in &expect_panic {
                if ok.contains(name) {
                    self.verdict(format!("{name} was expected to panic but passed"));
                } else if !failed.iter().any(|(n, _)| n == name) {
                    self.verdict(format!("{name} was expected to panic but did not run"));
                }
            }
        }
        for (e, seen) in expect_lines.iter().zip(&seen_lines) {
            if !seen {
                self.verdict(format!("no output line contains {e:?}"));
            }
        }
        Report {
            passed: self.errors.is_empty(),
            ok: ok.len(),
            expected_failures,
            known,
            problems: {
                let mut p = self.errors;
                if self.more_errors > 0 {
                    p.push(format!("... and {} more problems", self.more_errors));
                }
                p
            },
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
    #[test]
    fn a_guest_that_repeats_a_bad_line_keeps_a_bounded_report() {
        let mut c = super::Checker::new(vec![], vec![]);
        for _ in 0..100_000 {
            let _ = c.feed("ZORAYA-TEST nonsense");
        }
        let r = c.finish(super::Exit::Timeout, 1);
        assert!(!r.passed);
        // 100 kept, finish's own verdicts (here the timeout), and the count.
        assert!(
            r.problems.len() <= super::MAX_ERRORS + 10,
            "{}",
            r.problems.len()
        );
        assert!(r.problems.iter().any(|p| p.contains("more problems")));
        assert!(
            r.problems
                .iter()
                .any(|p| p.contains("timed out") || p.contains("timeout")),
            "the reason is shown"
        );
        let mut c = super::Checker::new(vec![], vec![]);
        let _ = c.feed("ZORAYA-TEST begin 1");
        for i in 0..super::MAX_TESTS + 5 {
            let _ = c.feed(&format!("ZORAYA-TEST run t{i}"));
            let _ = c.feed(&format!("ZORAYA-TEST ok t{i}"));
        }
        let r = c.finish(super::Exit::Timeout, 1);
        assert!(r.ok <= super::MAX_TESTS);
    }

    use super::*;

    #[test]
    fn progress_names_the_running_test_and_the_count() {
        let mut c = Checker::new(vec![], vec![]);
        assert_eq!(c.progress(), "0 tests passed, 0 failed, no test running");
        let _ = c.feed("ZORAYA-TEST begin 3");
        let _ = c.feed("ZORAYA-TEST run a");
        let _ = c.feed("ZORAYA-TEST ok a");
        let _ = c.feed("ZORAYA-TEST run b");
        assert_eq!(
            c.progress(),
            "1 tests passed of 3, 0 failed, running test b"
        );
    }

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
        "ZORAYA-TEST run a",
        "ZORAYA-TEST ok a",
        "ZORAYA-TEST run boom",
        "ZORAYA-TEST fail boom panic at x.rs:1:1: deliberate",
        "ZORAYA-TEST run c",
        "ZORAYA-TEST ok c",
        "ZORAYA-TEST end pass=2 fail=1\r",
    ];

    // Indices into GOOD.
    const OK_A: usize = 3;
    const FAIL_BOOM: usize = 5;
    const OK_C: usize = 7;
    const END: usize = 8;

    #[test]
    fn plain_mode_needs_lines_and_a_clean_exit_only() {
        let mut c = Checker::plain(vec!["done".into()]);
        assert_eq!(c.feed("booting"), Flow::Continue);
        assert_eq!(c.feed("done"), Flow::Continue);
        assert!(c.finish(Exit::Status(Some(0)), 60).passed);

        let c = Checker::plain(vec!["done".into()]);
        assert!(!c.finish(Exit::Status(Some(0)), 60).passed, "line missing");

        let mut c = Checker::plain(vec!["done".into()]);
        assert_eq!(c.feed("ZORAYA-PANIC at x"), Flow::Abort);
        assert!(!c.finish(Exit::Aborted, 60).passed);

        let mut c = Checker::plain(vec![]);
        c.feed("done");
        assert!(!c.finish(Exit::Timeout, 60).passed, "never powered off");
    }

    #[test]
    fn clean_run_passes() {
        let r = run(GOOD, Exit::Status(Some(0)));
        assert!(r.passed, "{:?}", r.problems);
        assert_eq!((r.ok, r.expected_failures), (2, 1));
    }

    #[test]
    fn missing_end_line() {
        let r = run(&GOOD[..END], Exit::Status(Some(0)));
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.contains("without an end line")));
    }

    #[test]
    fn timeout() {
        let r = run(&GOOD[..3], Exit::Timeout);
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.contains("timeout")));
        // The hung test is named.
        let r = run(&GOOD[..5], Exit::Timeout);
        assert!(
            r.problems
                .iter()
                .any(|p| p.contains("test boom did not finish")),
            "{:?}",
            r.problems
        );
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
        lines[OK_A] = "ZORAYA-TEST fail a assertion failed";
        lines[END] = "ZORAYA-TEST end pass=1 fail=2";
        let r = run(&lines, Exit::Status(Some(0)));
        assert!(!r.passed);
        assert!(r.problems.iter().any(|p| p.starts_with("a failed")));
    }

    #[test]
    fn expected_panic_that_passes() {
        let mut lines = GOOD.to_vec();
        lines[FAIL_BOOM] = "ZORAYA-TEST ok boom";
        lines[END] = "ZORAYA-TEST end pass=3 fail=0";
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
        lines[FAIL_BOOM] = "ZORAYA-TEST fail boom something else";
        let r = run(&lines, Exit::Status(Some(0)));
        assert!(!r.passed);
    }

    #[test]
    fn a_known_failure_fails_nothing_else() {
        let mut lines = GOOD.to_vec();
        lines[OK_C] = "ZORAYA-TEST fail c panic at y.rs:2:2: the ASID fault";
        lines[END] = "ZORAYA-TEST end pass=1 fail=2";
        let quarantined = |c: Checker| c.known_failure("c".into(), "ASID fault".into());
        let mut c = quarantined(Checker::new(vec!["boom".into()], vec!["banner".into()]));
        for l in &lines {
            c.feed(l);
        }
        let r = c.finish(Exit::Status(Some(0)), 10);
        assert!(r.passed, "{:?}", r.problems);
        assert_eq!((r.ok, r.expected_failures), (1, 1));
        assert_eq!(r.known, ["c: known failure, see backlog: ASID fault"]);
        // It passing is fine too, and says nothing.
        let mut c = quarantined(Checker::new(vec!["boom".into()], vec!["banner".into()]));
        for l in GOOD {
            c.feed(l);
        }
        let r = c.finish(Exit::Status(Some(0)), 10);
        assert!(r.passed && r.known.is_empty());
        // Another test failing still fails the run.
        let mut lines = GOOD.to_vec();
        lines[OK_A] = "ZORAYA-TEST fail a panic at z.rs:3:3: other";
        lines[END] = "ZORAYA-TEST end pass=1 fail=2";
        let mut c = quarantined(Checker::new(vec!["boom".into()], vec!["banner".into()]));
        for l in &lines {
            c.feed(l);
        }
        let r = c.finish(Exit::Status(Some(0)), 10);
        assert!(!r.passed && r.known.is_empty());
        assert_eq!(r.problems, ["a failed: panic at z.rs:3:3: other"]);
    }

    #[test]
    fn count_mismatches() {
        let mut lines = GOOD.to_vec();
        lines[1] = "ZORAYA-TEST begin 4";
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
        let mut lines = GOOD.to_vec();
        lines[END] = "ZORAYA-TEST end pass=3 fail=0";
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
    }

    #[test]
    fn duplicates_and_garbage() {
        let mut lines = GOOD.to_vec();
        lines[OK_C] = "ZORAYA-TEST ok a";
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
        let mut lines = GOOD.to_vec();
        lines.insert(2, "ZORAYA-TEST maybe x");
        assert!(!run(&lines, Exit::Status(Some(0))).passed);
        // A result without its run line.
        let mut lines = GOOD.to_vec();
        lines.remove(OK_A - 1);
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
