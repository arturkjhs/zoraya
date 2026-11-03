//! Bounds on what a guest may print (docs/backlog.md, the 6c memory entry).
//!
//! Every run's whole output goes to `target/test-logs/<label>.log` (or the
//! directory `ZORAYA_TEST_LOGS` names), up to `LOG_CAP` bytes; past that the
//! run fails as an output flood and the caller kills QEMU. The terminal shows
//! the first `HEAD` and the last `TAIL` lines, with a line saying how many
//! were skipped and where the log is. Lines are read with a length limit:
//! past `LINE_CAP` bytes a line is cut and marked. What the runner keeps in
//! memory to match against is bounded by `LOG_CAP` too.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// The most output a run may produce, and the most any buffer holds.
pub const LOG_CAP: usize = 64 * 1024 * 1024;
/// The longest line kept; the rest of it is dropped.
pub const LINE_CAP: usize = 64 * 1024;
/// Lines the terminal shows from the start and from the end of a run.
pub const HEAD: usize = 200;
pub const TAIL: usize = 200;
/// What a cut line ends with.
pub const CUT: &str = " [line cut at 64 KiB]";

/// The failure every caller reports for too much output.
pub fn flood() -> String {
    format!("output flood: more than {} MiB", LOG_CAP / (1024 * 1024))
}

/// One line read: whether it was cut, and how many bytes it took in all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Read {
    pub cut: bool,
    pub bytes: usize,
}

/// Reads one line: at most `LINE_CAP` bytes of it into `out`. `None` at the
/// end of the output. A longer line is read on and dropped past the cap, so
/// memory stays bounded, but no further than `LOG_CAP` bytes: an endless line
/// comes back then, and its byte count floods the run.
pub fn read_line(reader: &mut impl BufRead, out: &mut Vec<u8>) -> Option<Read> {
    out.clear();
    let mut r = Read {
        cut: false,
        bytes: 0,
    };
    loop {
        let available = match reader.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return (r.bytes > 0).then_some(r),
        };
        if available.is_empty() {
            return (r.bytes > 0).then_some(r);
        }
        let (taken, done) = match available.iter().position(|&b| b == b'\n') {
            Some(at) => (at + 1, true),
            None => (available.len(), false),
        };
        let room = LINE_CAP.saturating_sub(out.len());
        let keep = taken.min(room);
        out.extend_from_slice(&available[..keep]);
        if keep < taken {
            r.cut = true;
        }
        reader.consume(taken);
        r.bytes += taken;
        if done || r.bytes > LOG_CAP {
            return Some(r);
        }
    }
}

/// A run's output: the log file, the terminal's head and tail, the count.
pub struct Output {
    label: String,
    path: PathBuf,
    log: Option<File>,
    logged: usize,
    shown: usize,
    tail: VecDeque<String>,
    skipped: usize,
    /// A line not yet ended, from byte-stream callers, and its bytes in all.
    partial: Vec<u8>,
    partial_cut: bool,
    pending: usize,
    /// Echo to the terminal at all (tests turn it off).
    echo: bool,
}

impl Output {
    pub fn new(label: &str) -> Output {
        let dir = std::env::var_os("ZORAYA_TEST_LOGS")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/test-logs"));
        Output::in_dir(label, dir, true)
    }

    fn in_dir(label: &str, dir: PathBuf, echo: bool) -> Output {
        let _ = std::fs::create_dir_all(&dir);
        let name: String = label
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(format!("{name}.log"));
        let log = File::create(&path).ok();
        Output {
            label: label.to_string(),
            path,
            log,
            logged: 0,
            shown: 0,
            tail: VecDeque::new(),
            skipped: 0,
            partial: Vec::new(),
            partial_cut: false,
            pending: 0,
            echo,
        }
    }

    /// No echo, and the log in the system's temporary directory.
    #[cfg(test)]
    pub fn quiet(label: &str) -> Output {
        Output::in_dir(
            label,
            std::env::temp_dir().join("zoraya-output-test"),
            false,
        )
    }

    /// Takes one line (without its end). Err when the run has printed more
    /// than `LOG_CAP`: the caller kills QEMU and fails with `flood()`.
    #[cfg(test)]
    pub fn line(&mut self, line: &str, cut: bool) -> Result<(), String> {
        self.counted_line(line, cut, line.len() + 1)
    }

    /// `line`, which took `bytes` bytes of output (more than it holds if it
    /// was cut).
    pub fn counted_line(&mut self, line: &str, cut: bool, bytes: usize) -> Result<(), String> {
        let mut text = String::with_capacity(line.len() + CUT.len());
        text.push_str(line);
        if cut {
            text.push_str(CUT);
        }
        self.logged += bytes;
        if self.logged > LOG_CAP {
            return Err(flood());
        }
        if let Some(log) = &mut self.log {
            let _ = writeln!(log, "{text}");
        }
        if !self.echo {
            return Ok(());
        }
        if self.shown < HEAD {
            self.shown += 1;
            println!("[{}] {text}", self.label);
        } else {
            self.tail.push_back(text);
            if self.tail.len() > TAIL {
                self.tail.pop_front();
                self.skipped += 1;
            }
        }
        Ok(())
    }

    /// Takes bytes of a stream, line by line; a line past `LINE_CAP` is cut.
    pub fn bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        for chunk in bytes.split_inclusive(|&b| b == b'\n') {
            let ends = chunk.last() == Some(&b'\n');
            let body = if ends {
                &chunk[..chunk.len() - 1]
            } else {
                chunk
            };
            let room = LINE_CAP.saturating_sub(self.partial.len());
            if body.len() > room {
                self.partial_cut = true;
            }
            self.partial
                .extend_from_slice(&body[..body.len().min(room)]);
            self.pending += chunk.len();
            if ends {
                let line = String::from_utf8_lossy(&self.partial).replace('\r', "");
                let cut = std::mem::take(&mut self.partial_cut);
                let bytes = std::mem::take(&mut self.pending);
                self.partial.clear();
                self.counted_line(&line, cut, bytes)?;
            } else if self.logged + self.pending > LOG_CAP {
                // An unended line counts against the cap as it comes.
                return Err(flood());
            }
        }
        Ok(())
    }

    /// Prints the tail, and where the whole log is if lines were skipped.
    pub fn finish(&mut self) {
        if !self.partial.is_empty() {
            let line = String::from_utf8_lossy(&self.partial).replace('\r', "");
            let cut = self.partial_cut;
            let bytes = std::mem::take(&mut self.pending);
            self.partial.clear();
            let _ = self.counted_line(&line, cut, bytes);
        }
        if !self.echo {
            return;
        }
        if self.skipped > 0 {
            println!(
                "[{}] ... {} lines skipped; the whole output is in {}",
                self.label,
                self.skipped,
                self.path.display()
            );
        }
        for line in self.tail.drain(..) {
            println!("[{}] {line}", self.label);
        }
    }
}

/// What of `text` a message shows: the first `HEAD` and the last `TAIL`
/// lines, with the count of those between.
#[allow(dead_code)] // Reason: the crash test's binary uses it, the runner's does not.
pub fn excerpt(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= HEAD + TAIL {
        return text.to_string();
    }
    let mut out = lines[..HEAD].join("\n");
    out.push_str(&format!(
        "\n... {} lines skipped ...\n",
        lines.len() - HEAD - TAIL
    ));
    out.push_str(&lines[lines.len() - TAIL..].join("\n"));
    out
}

/// Appends to a buffer kept for matching, at most `LOG_CAP` bytes in all.
pub fn keep(buffer: &mut String, more: &str) -> Result<(), String> {
    if buffer.len() + more.len() > LOG_CAP {
        return Err(flood());
    }
    buffer.push_str(more);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn a_long_line_is_cut_and_the_next_one_is_whole() {
        let mut input = vec![b'a'; LINE_CAP * 3];
        input.extend_from_slice(b"\nnext\n");
        let mut reader = BufReader::new(&input[..]);
        let mut line = Vec::new();
        let r = read_line(&mut reader, &mut line).unwrap();
        assert_eq!((r.cut, r.bytes), (true, LINE_CAP * 3 + 1));
        assert_eq!(line.len(), LINE_CAP);
        let r = read_line(&mut reader, &mut line).unwrap();
        assert_eq!((r.cut, r.bytes), (false, 5));
        assert_eq!(line, b"next\n");
        assert_eq!(read_line(&mut reader, &mut line), None);
    }

    #[test]
    fn the_log_and_the_buffers_stop_at_the_cap() {
        let mut o = Output::quiet("cap test");
        let line = "x".repeat(1023);
        let mut lines = 0;
        let r = loop {
            match o.line(&line, false) {
                Ok(()) => lines += 1,
                Err(e) => break e,
            }
        };
        assert_eq!(r, flood());
        assert_eq!(lines, LOG_CAP / 1024);
        let mut kept = String::new();
        assert!(keep(&mut kept, &"y".repeat(LOG_CAP)).is_ok());
        assert_eq!(keep(&mut kept, "z"), Err(flood()));
        let mut o = Output::quiet("endless");
        let chunk = vec![b'e'; 4096];
        let mut sent = 0usize;
        let r = loop {
            if let Err(e) = o.bytes(&chunk) {
                break e;
            }
            sent += chunk.len();
            assert!(sent <= 2 * LOG_CAP, "an endless line must flood too");
        };
        assert_eq!(r, flood());
        assert!(o.partial.len() <= LINE_CAP);
    }

    #[test]
    fn an_excerpt_keeps_the_ends() {
        let text: String = (0..1000).map(|i| format!("{i}\n")).collect();
        let e = excerpt(&text);
        assert!(e.starts_with("0\n1\n") && e.ends_with("999"));
        assert!(e.contains("... 600 lines skipped ..."));
        assert_eq!(e.lines().count(), HEAD + TAIL + 1);
        assert_eq!(excerpt("a\nb\n"), "a\nb\n");
    }
}
