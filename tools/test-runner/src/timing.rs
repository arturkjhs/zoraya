//! How long each test of a ZORAYA-TEST run took, by the host's clock: the
//! slowest are printed after the verdict, and the running test's time at a
//! timeout, so that a slow suite can be told from a hung test.

use std::time::{Duration, Instant};

/// How many of the slowest tests are kept and printed.
pub const SLOWEST: usize = 5;

#[derive(Debug, Default)]
pub struct Timings {
    running: Option<(String, Instant)>,
    /// The slowest tests so far, slowest first, at most [`SLOWEST`].
    slowest: Vec<(Duration, String)>,
}

impl Timings {
    /// Notes a line of output that arrived at `now`.
    pub fn line(&mut self, line: &str, now: Instant) {
        let Some(rest) = line.trim_end().strip_prefix("ZORAYA-TEST ") else {
            return;
        };
        let mut words = rest.split(' ');
        match (words.next(), words.next()) {
            (Some("run"), Some(name)) => self.running = Some((name.to_string(), now)),
            (Some("ok" | "fail"), Some(name)) => {
                if let Some((running, since)) = self.running.take()
                    && running == name
                {
                    self.add(now.saturating_duration_since(since), running);
                }
            }
            _ => {}
        }
    }

    fn add(&mut self, took: Duration, name: String) {
        let at = self.slowest.partition_point(|(d, _)| *d >= took);
        if at < SLOWEST {
            self.slowest.insert(at, (took, name));
            self.slowest.truncate(SLOWEST);
        }
    }

    /// "a 38.0 s, b 23.1 s", or None before any test ended.
    pub fn slowest(&self) -> Option<String> {
        if self.slowest.is_empty() {
            return None;
        }
        let parts: Vec<String> = self
            .slowest
            .iter()
            .map(|(d, n)| format!("{n} {:.1} s", d.as_secs_f64()))
            .collect();
        Some(parts.join(", "))
    }

    /// "x for 12.3 s", for the test whose result has not come at `now`.
    pub fn running(&self, now: Instant) -> Option<String> {
        self.running.as_ref().map(|(n, since)| {
            format!(
                "{n} for {:.1} s",
                now.saturating_duration_since(*since).as_secs_f64()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_slowest_in_order() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut t = Timings::default();
        let mut now = 0;
        for (i, took) in [3u64, 1, 7, 2, 9, 4, 5].iter().enumerate() {
            t.line(&format!("ZORAYA-TEST run t{i}"), at(now));
            now += took;
            t.line(&format!("ZORAYA-TEST ok t{i}\r"), at(now));
        }
        assert_eq!(
            t.slowest().unwrap(),
            "t4 9.0 s, t2 7.0 s, t6 5.0 s, t5 4.0 s, t0 3.0 s"
        );
        assert_eq!(t.running(at(now)), None);
    }

    #[test]
    fn a_failed_test_counts_and_other_lines_do_not() {
        let t0 = Instant::now();
        let mut t = Timings::default();
        t.line("ZORAYA-TEST begin 2", t0);
        t.line("ZORAYA-TEST run a", t0);
        t.line("t_cpu: a line of the test", t0 + Duration::from_secs(1));
        t.line(
            "ZORAYA-TEST fail a panic at x.rs:1:1: boom",
            t0 + Duration::from_secs(2),
        );
        t.line("ZORAYA-TEST run b", t0 + Duration::from_secs(2));
        assert_eq!(t.slowest().unwrap(), "a 2.0 s");
        assert_eq!(
            t.running(t0 + Duration::from_millis(4500)).unwrap(),
            "b for 2.5 s"
        );
    }

    #[test]
    fn nothing_without_the_protocol() {
        let mut t = Timings::default();
        t.line("hello from EL0", Instant::now());
        t.line("ZORAYA-TEST ok never-run", Instant::now());
        assert_eq!(t.slowest(), None);
        assert_eq!(t.running(Instant::now()), None);
    }
}
