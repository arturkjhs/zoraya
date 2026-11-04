//! The job table and its notices (ADR-0158 rules 1, 5, 6 and 7), without
//! the kernel: `zosh` keeps its handles in each entry's payload.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::ExitRecord;

use crate::record;

/// Whether a job runs or is stopped (rule 5's `jobs` words).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Stopped,
}

/// One job: a command line's pipeline (rule 1).
#[derive(Debug)]
pub struct Entry<P> {
    pub number: u32,
    /// The line as typed, without its `&`.
    pub line: String,
    /// What it was given, as `grants` words it (rule 5): empty for nothing
    /// but its streams.
    pub given: String,
    pub state: State,
    pub payload: P,
}

/// Where the shell is when it reads a job's end (rule 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    /// In `kill` or `wait`, which report ends as they come.
    Reporting,
    /// Anywhere else: editing a line, running one, waiting for a
    /// foreground job. The notice waits for the next prompt.
    Elsewhere,
}

/// The jobs and the notices not yet shown.
#[derive(Debug)]
pub struct Jobs<P> {
    entries: Vec<Entry<P>>,
    notices: Vec<String>,
    /// `exit` was refused once and nothing else ran since (rule 7).
    exit_warned: bool,
}

impl<P> Default for Jobs<P> {
    fn default() -> Self {
        Jobs {
            entries: Vec::new(),
            notices: Vec::new(),
            exit_warned: false,
        }
    }
}

/// What `exit` does (rule 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    /// No job is left, or this is the second `exit` in a row: end them all.
    Leave,
    /// The first `exit` with live jobs: this, and nothing ends.
    Warn(String),
}

impl<P> Jobs<P> {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The smallest free number from 1 (rule 1).
    pub fn free_number(&self) -> u32 {
        (1..)
            .find(|n| self.entries.iter().all(|e| e.number != *n))
            .unwrap_or(1)
    }

    /// Adds a running job under the smallest free number, and gives it.
    pub fn add(&mut self, line: &str, given: String, payload: P) -> u32 {
        let number = self.free_number();
        self.entries.push(Entry {
            number,
            line: line.trim_end_matches(['&', ' ']).into(),
            given,
            state: State::Running,
            payload,
        });
        number
    }

    pub fn get(&self, number: u32) -> Option<&Entry<P>> {
        self.entries.iter().find(|e| e.number == number)
    }

    pub fn get_mut(&mut self, number: u32) -> Option<&mut Entry<P>> {
        self.entries.iter_mut().find(|e| e.number == number)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry<P>> {
        self.entries.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Entry<P>> {
        self.entries.iter_mut()
    }

    /// `%N`, or the newest job when no word is given (rule 5).
    pub fn which(&self, word: Option<&str>) -> Result<u32, String> {
        match word {
            None => self
                .entries
                .iter()
                .map(|e| e.number)
                .max()
                .ok_or_else(|| "no jobs".into()),
            Some(w) => {
                let n = w
                    .strip_prefix('%')
                    .and_then(zoraya_describe::parse_number)
                    .filter(|n| *n > 0 && *n <= i64::from(u32::MAX))
                    .ok_or_else(|| format!("{w}: not a job; write %N"))?;
                let n = n as u32;
                if self.get(n).is_some() {
                    Ok(n)
                } else {
                    Err(format!("{w}: no such job"))
                }
            }
        }
    }

    /// The newest stopped job, for `bg` without a word.
    pub fn newest_stopped(&self) -> Option<u32> {
        self.entries
            .iter()
            .filter(|e| e.state == State::Stopped)
            .map(|e| e.number)
            .max()
    }

    /// `jobs`' listing (rule 5): `[N]  running   LINE   (GRANTS)`.
    pub fn list(&self) -> String {
        let mut s = String::new();
        for e in &self.entries {
            let state = match e.state {
                State::Running => "running",
                State::Stopped => "stopped",
            };
            s.push_str(&format!("[{}]  {state:<9} {}", e.number, e.line));
            if !e.given.is_empty() {
                s.push_str(&format!("   ({})", e.given));
            }
            s.push('\n');
        }
        s
    }

    /// Job `number` ended with `record` (its pipeline's status): removes
    /// it. At a point that reports, gives its notice to show now; else
    /// queues it for the next prompt. Each end is so reported once
    /// (INV-0158-4).
    pub fn ended(
        &mut self,
        number: u32,
        record: &ExitRecord,
        at: At,
    ) -> Option<(Entry<P>, Option<String>)> {
        let i = self.entries.iter().position(|e| e.number == number)?;
        let entry = self.entries.remove(i);
        let text = notice(entry.number, &entry.line, record);
        match at {
            At::Reporting => Some((entry, Some(text))),
            At::Elsewhere => {
                self.notices.push(text);
                Some((entry, None))
            }
        }
    }

    /// Removes job `number` without a notice: a foreground job's end is
    /// reported by the line that ran it.
    pub fn remove(&mut self, number: u32) -> Option<Entry<P>> {
        let i = self.entries.iter().position(|e| e.number == number)?;
        Some(self.entries.remove(i))
    }

    /// Queues other output for the next prompt (`[N] stopped: LINE`, a
    /// background job's lines).
    pub fn queue(&mut self, text: String) {
        self.notices.push(text);
    }

    /// The notices to show before the prompt is drawn, or above the line
    /// being edited when it is drawn again; each is given once.
    pub fn take_notices(&mut self) -> String {
        let mut s = String::new();
        for n in self.notices.drain(..) {
            s.push_str(&n);
        }
        s
    }

    pub fn has_notices(&self) -> bool {
        !self.notices.is_empty()
    }

    /// A line other than `exit` ran: the next `exit` warns again.
    pub fn other_line(&mut self) {
        self.exit_warned = false;
    }

    /// `exit` (rule 7, INV-0158-3).
    pub fn exit(&mut self) -> Exit {
        if self.entries.is_empty() || self.exit_warned {
            return Exit::Leave;
        }
        self.exit_warned = true;
        let n = self.entries.len();
        Exit::Warn(if n == 1 {
            "1 job is still running; exit again to end it\n".into()
        } else {
            format!("{n} jobs are still running; exit again to end them\n")
        })
    }
}

/// A job's end in rule 6's words: `[N] done: LINE`, or `[N] LINE: WHAT`.
pub fn notice(number: u32, line: &str, record: &ExitRecord) -> String {
    if record::is_success(record) {
        format!("[{number}] done: {line}\n")
    } else {
        format!("[{number}] {line}: {}\n", record::what(record))
    }
}
