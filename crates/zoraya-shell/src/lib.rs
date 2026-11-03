//! What `zosh` does without the kernel: edit a line, parse it, describe an
//! exit (TASK-11). No allocation: fixed buffers, so a flood of input costs
//! nothing but the keys it drops.

#![cfg_attr(not(test), no_std)]

use core::fmt::{self, Write};

use zoraya_abi::{ExitReason, ExitRecord, RequestKind};

/// Longest line.
pub const LINE_SIZE: usize = 128;
/// Lines of history.
pub const HISTORY: usize = 32;
/// Most words in a command.
pub const MAX_WORDS: usize = 16;

pub const CTRL_C: u8 = 3;
pub const CTRL_U: u8 = 21;
pub const CTRL_BACKSLASH: u8 = 28;
const BACKSPACE: u8 = 8;
const DELETE: u8 = 127;
const ESCAPE: u8 = 27;

#[derive(Clone, Copy)]
struct Line {
    bytes: [u8; LINE_SIZE],
    length: usize,
}

impl Line {
    const EMPTY: Line = Line {
        bytes: [0; LINE_SIZE],
        length: 0,
    };

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    None,
    /// After Esc.
    Started,
    /// After `Esc [`, up to a final byte.
    Csi,
    /// After `Esc O`: one more byte.
    Single,
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Nothing for the shell: any echo has been written.
    Edited,
    /// Enter: [`Editor::line`] holds the line.
    Line,
    /// Ctrl-C: the line was cleared.
    Interrupt,
}

/// A line being typed, and the lines before it. The shell echoes, so each key
/// writes what the screen needs to `out`.
pub struct Editor {
    line: Line,
    history: [Line; HISTORY],
    /// Lines ever kept. The newest is at `(kept - 1) % HISTORY`.
    kept: usize,
    /// How far back Up has gone: 0 is the line being typed.
    back: usize,
    escape: Escape,
}

fn is_printable(byte: u8) -> bool {
    (0x20..0x7f).contains(&byte)
}

impl Editor {
    pub const fn new() -> Editor {
        Editor {
            line: Line::EMPTY,
            history: [Line::EMPTY; HISTORY],
            kept: 0,
            back: 0,
            escape: Escape::None,
        }
    }

    /// The line Enter finished.
    pub fn line(&self) -> &str {
        // Only printable ASCII reaches a line.
        core::str::from_utf8(self.line.as_bytes()).unwrap_or("")
    }

    /// Clears the line, and keeps it in history if it holds anything.
    pub fn finish(&mut self) {
        if !self.line().trim().is_empty() {
            self.history[self.kept % HISTORY] = self.line;
            self.kept += 1;
        }
        self.line.length = 0;
        self.back = 0;
    }

    /// Takes one input byte.
    pub fn key(&mut self, byte: u8, out: &mut impl Write) -> Result<Key, fmt::Error> {
        // Ctrl-C always means Ctrl-C, whatever was typed before it.
        if byte == CTRL_C {
            self.escape = Escape::None;
        }
        match self.escape {
            Escape::Started => {
                self.escape = match byte {
                    b'[' => Escape::Csi,
                    b'O' => Escape::Single,
                    _ => Escape::None,
                };
                return Ok(Key::Edited);
            }
            Escape::Single => {
                self.escape = Escape::None;
                return Ok(Key::Edited);
            }
            Escape::Csi => {
                // Parameters and intermediates run until a final byte.
                if (0x40..=0x7e).contains(&byte) {
                    self.escape = Escape::None;
                    match byte {
                        b'A' => self.older(out)?,
                        b'B' => self.newer(out)?,
                        _ => {}
                    }
                }
                return Ok(Key::Edited);
            }
            Escape::None => {}
        }
        match byte {
            b'\r' | b'\n' => {
                out.write_str("\n")?;
                Ok(Key::Line)
            }
            CTRL_C => {
                out.write_str("^C\n")?;
                self.line.length = 0;
                self.back = 0;
                Ok(Key::Interrupt)
            }
            CTRL_U => {
                self.replace_with(&[], out)?;
                self.back = 0;
                Ok(Key::Edited)
            }
            BACKSPACE | DELETE => {
                if self.line.length > 0 {
                    self.line.length -= 1;
                    out.write_str("\x08 \x08")?;
                }
                Ok(Key::Edited)
            }
            ESCAPE => {
                self.escape = Escape::Started;
                Ok(Key::Edited)
            }
            byte if is_printable(byte) && self.line.length < LINE_SIZE => {
                self.line.bytes[self.line.length] = byte;
                self.line.length += 1;
                out.write_char(byte as char)?;
                Ok(Key::Edited)
            }
            _ => Ok(Key::Edited),
        }
    }

    fn stored(&self, back: usize) -> Line {
        self.history[(self.kept - back) % HISTORY]
    }

    fn older(&mut self, out: &mut impl Write) -> fmt::Result {
        if self.back < self.kept.min(HISTORY) {
            self.back += 1;
            let line = self.stored(self.back);
            self.replace_with(line.as_bytes(), out)?;
        }
        Ok(())
    }

    fn newer(&mut self, out: &mut impl Write) -> fmt::Result {
        if self.back > 1 {
            self.back -= 1;
            let line = self.stored(self.back);
            self.replace_with(line.as_bytes(), out)
        } else if self.back == 1 {
            self.back = 0;
            self.replace_with(&[], out)
        } else {
            Ok(())
        }
    }

    fn replace_with(&mut self, text: &[u8], out: &mut impl Write) -> fmt::Result {
        for _ in 0..self.line.length {
            out.write_str("\x08 \x08")?;
        }
        self.line.bytes[..text.len()].copy_from_slice(text);
        self.line.length = text.len();
        for &byte in text {
            out.write_char(byte as char)?;
        }
        Ok(())
    }
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

/// A line split at spaces.
#[derive(Debug, PartialEq, Eq)]
pub struct Words<'a> {
    words: [&'a str; MAX_WORDS],
    count: usize,
    /// It ended with `&`.
    pub background: bool,
}

impl<'a> Words<'a> {
    pub fn as_slice(&self) -> &[&'a str] {
        &self.words[..self.count]
    }
}

/// The line has more than [`MAX_WORDS`] words.
#[derive(Debug, PartialEq, Eq)]
pub struct TooManyWords;

pub fn split(line: &str) -> Result<Words<'_>, TooManyWords> {
    let mut words = [""; MAX_WORDS];
    let mut count = 0;
    let mut background = false;
    let line = match line.trim_end().strip_suffix('&') {
        Some(rest) => {
            background = true;
            rest
        }
        None => line,
    };
    for word in line.split_ascii_whitespace() {
        *words.get_mut(count).ok_or(TooManyWords)? = word;
        count += 1;
    }
    Ok(Words {
        words,
        count,
        background,
    })
}

/// `%n`, as a job number.
pub fn job_number(word: &str) -> Option<u32> {
    word.strip_prefix('%')?.parse().ok()
}

/// What `kill` sends, from its flag.
pub fn request_flag(flag: &str) -> Option<RequestKind> {
    Some(match flag {
        "-int" => RequestKind::Interrupt,
        "-term" => RequestKind::Terminate,
        "-hup" => RequestKind::Hangup,
        "-quit" => RequestKind::Quit,
        _ => return None,
    })
}

/// The name of a kind, for reports.
pub struct KindName(pub RequestKind);

impl fmt::Display for KindName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            RequestKind::Interrupt => f.write_str("Interrupt"),
            RequestKind::Terminate => f.write_str("Terminate"),
            RequestKind::Hangup => f.write_str("Hangup"),
            RequestKind::Quit => f.write_str("Quit"),
            RequestKind::Custom(n) => write!(f, "Custom({n})"),
        }
    }
}

fn fault_name(class: u32) -> &'static str {
    match class {
        0x00 => "unknown instruction",
        0x15 => "system call",
        0x20 | 0x21 => "instruction abort",
        0x22 => "pc alignment",
        0x24 | 0x25 => "data abort",
        0x26 => "stack alignment",
        0x3c => "breakpoint",
        _ => "exception",
    }
}

/// An exit record in its short form. `ADR-0034`'s `detail` is the exception
/// class, so a fault report has no address: the kernel's own line has it.
pub struct Report(pub ExitRecord);

impl Report {
    /// True if a clean exit: the shell prints nothing for it.
    pub fn is_clean(&self) -> bool {
        self.0.reason == ExitReason::Exited && self.0.code == 0
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let record = &self.0;
        match (record.reason, record.request_kind) {
            (ExitReason::Exited, _) => write!(f, "[exited {}]", record.code),
            (ExitReason::Terminated, Some(kind)) => {
                write!(f, "[terminated: {}]", KindName(kind))
            }
            (ExitReason::Terminated, None) => write!(f, "[terminated: {}]", record.detail),
            (ExitReason::Killed, _) => f.write_str("[killed]"),
            (ExitReason::Fault, _) => write!(
                f,
                "[fault: {}, class {:#x}]",
                fault_name(record.detail),
                record.detail
            ),
            (ExitReason::Limit, _) => f.write_str("[limit]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(editor: &mut Editor, keys: &[u8]) -> (String, Option<Key>) {
        let mut out = String::new();
        let mut last = None;
        for &byte in keys {
            last = Some(editor.key(byte, &mut out).unwrap());
        }
        (out, last)
    }

    #[test]
    fn printable_keys_echo_and_enter_ends_the_line() {
        let mut e = Editor::new();
        let (out, last) = typed(&mut e, b"ls\r");
        assert_eq!((out.as_str(), last), ("ls\n", Some(Key::Line)));
        assert_eq!(e.line(), "ls");
    }

    #[test]
    fn backspace_rubs_out_and_stops_at_the_start() {
        let mut e = Editor::new();
        let (out, _) = typed(&mut e, b"ab\x7f\x08\x08c");
        assert_eq!(out, "ab\x08 \x08\x08 \x08c");
        assert_eq!(e.line(), "c");
    }

    #[test]
    fn ctrl_u_kills_the_line() {
        let mut e = Editor::new();
        let (out, _) = typed(&mut e, &[b'a', b'b', CTRL_U, b'c']);
        assert_eq!(out, "ab\x08 \x08\x08 \x08c");
        assert_eq!(e.line(), "c");
    }

    #[test]
    fn ctrl_c_clears_and_says_so() {
        let mut e = Editor::new();
        let (_, last) = typed(&mut e, &[b'a', CTRL_C]);
        assert_eq!(last, Some(Key::Interrupt));
        assert_eq!(e.line(), "");
    }

    #[test]
    fn control_bytes_and_high_bytes_go_nowhere() {
        let mut e = Editor::new();
        let (out, _) = typed(&mut e, &[1, 0x80, 0xff, 0, 9]);
        assert_eq!(out, "");
        assert_eq!(e.line(), "");
    }

    #[test]
    fn a_long_line_stops_at_its_size() {
        let mut e = Editor::new();
        let (out, _) = typed(&mut e, &[b'x'; LINE_SIZE + 5]);
        assert_eq!(out.len(), LINE_SIZE);
        assert_eq!(e.line().len(), LINE_SIZE);
    }

    fn enter(e: &mut Editor, line: &str) {
        typed(e, line.as_bytes());
        typed(e, b"\r");
        e.finish();
    }

    const UP: &[u8] = b"\x1b[A";
    const DOWN: &[u8] = b"\x1b[B";

    #[test]
    fn up_and_down_walk_the_history() {
        let mut e = Editor::new();
        enter(&mut e, "one");
        enter(&mut e, "two");
        typed(&mut e, UP);
        assert_eq!(e.line(), "two");
        typed(&mut e, UP);
        assert_eq!(e.line(), "one");
        typed(&mut e, UP);
        assert_eq!(e.line(), "one", "the oldest stays");
        typed(&mut e, DOWN);
        assert_eq!(e.line(), "two");
        typed(&mut e, DOWN);
        assert_eq!(e.line(), "");
        typed(&mut e, DOWN);
        assert_eq!(e.line(), "");
    }

    #[test]
    fn history_keeps_the_last_32_and_no_blank_lines() {
        let mut e = Editor::new();
        for i in 0..HISTORY + 8 {
            enter(&mut e, &format!("line{i}"));
        }
        enter(&mut e, "   ");
        for _ in 0..HISTORY + 8 {
            typed(&mut e, UP);
        }
        assert_eq!(e.line(), "line8");
        for _ in 0..HISTORY - 1 {
            typed(&mut e, DOWN);
        }
        assert_eq!(e.line(), format!("line{}", HISTORY + 7));
    }

    #[test]
    fn an_unknown_escape_is_swallowed() {
        let mut e = Editor::new();
        let (out, _) = typed(&mut e, b"\x1b[Zk\x1bOAx\x1b[3~y");
        assert_eq!(out, "kxy");
    }

    #[test]
    fn ctrl_c_gets_through_an_escape() {
        let mut e = Editor::new();
        let (_, last) = typed(&mut e, &[27, CTRL_C]);
        assert_eq!(last, Some(Key::Interrupt));
        let (out, _) = typed(&mut e, b"a");
        assert_eq!(out, "a");
    }

    #[test]
    fn newline_ends_a_line_too() {
        let mut e = Editor::new();
        let (_, last) = typed(&mut e, b"x\n");
        assert_eq!(last, Some(Key::Line));
    }

    #[test]
    fn ctrl_c_and_enter_return_to_the_newest_line() {
        let mut e = Editor::new();
        enter(&mut e, "one");
        enter(&mut e, "two");
        typed(&mut e, UP);
        typed(&mut e, UP);
        typed(&mut e, &[CTRL_C]);
        typed(&mut e, UP);
        assert_eq!(e.line(), "two");
        typed(&mut e, UP);
        typed(&mut e, b"\r");
        e.finish();
        typed(&mut e, UP);
        assert_eq!(e.line(), "one", "the line just entered");
    }

    #[test]
    fn split_finds_words_and_the_background_mark() {
        let w = split("  run a  b &").unwrap();
        assert_eq!(w.as_slice(), ["run", "a", "b"]);
        assert!(w.background);
        let w = split("sleep 1&").unwrap();
        assert_eq!(w.as_slice(), ["sleep", "1"]);
        assert!(w.background);
        assert!(!split("echo a").unwrap().background);
        assert!(split("").unwrap().as_slice().is_empty());
        assert_eq!(split(&"x ".repeat(MAX_WORDS + 1)), Err(TooManyWords));
    }

    #[test]
    fn job_numbers_and_flags() {
        assert_eq!(job_number("%3"), Some(3));
        assert_eq!(job_number("3"), None);
        assert_eq!(job_number("%x"), None);
        assert_eq!(request_flag("-int"), Some(RequestKind::Interrupt));
        assert_eq!(request_flag("-term"), Some(RequestKind::Terminate));
        assert_eq!(request_flag("-hup"), Some(RequestKind::Hangup));
        assert_eq!(request_flag("-quit"), Some(RequestKind::Quit));
        assert_eq!(request_flag("-kill"), None);
    }

    fn report(reason: ExitReason, code: i32, detail: u32, kind: Option<RequestKind>) -> String {
        Report(ExitRecord {
            code,
            reason,
            detail,
            request_kind: kind,
        })
        .to_string()
    }

    #[test]
    fn reports_have_their_short_form() {
        assert_eq!(report(ExitReason::Exited, 3, 0, None), "[exited 3]");
        assert_eq!(
            report(ExitReason::Terminated, 0, 2, Some(RequestKind::Interrupt)),
            "[terminated: Interrupt]"
        );
        assert_eq!(
            report(ExitReason::Terminated, 0, 0, Some(RequestKind::Custom(9))),
            "[terminated: Custom(9)]"
        );
        assert_eq!(
            report(ExitReason::Terminated, 0, 2, None),
            "[terminated: 2]"
        );
        assert_eq!(report(ExitReason::Killed, 0, 0, None), "[killed]");
        assert_eq!(report(ExitReason::Limit, 0, 0, None), "[limit]");
        assert_eq!(
            report(ExitReason::Fault, 0, 0x24, None),
            "[fault: data abort, class 0x24]"
        );
    }

    #[test]
    fn only_exit_0_is_clean() {
        let record = |reason, code| {
            Report(ExitRecord {
                code,
                reason,
                detail: 0,
                request_kind: None,
            })
        };
        assert!(record(ExitReason::Exited, 0).is_clean());
        assert!(!record(ExitReason::Exited, 1).is_clean());
        assert!(!record(ExitReason::Killed, 0).is_clean());
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
