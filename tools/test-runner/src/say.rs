//! Printing that survives a non-blocking terminal.
//!
//! QEMU sets the file description of a terminal it inherits non-blocking,
//! and the runner's stdout and stderr may share that description: a write
//! then fails with `EAGAIN`, and `println!` panics ("failed printing to
//! stdout", docs/backlog.md). These macros write the whole line, waiting
//! briefly and trying again while the terminal would block, and drop the
//! line on any other error, as a print to a closed pipe should not end a
//! test run either.

use std::io::{ErrorKind, Write};
use std::time::Duration;

/// How long to wait before trying a write that would block again.
const RETRY: Duration = Duration::from_millis(1);
/// Tries before giving up on a terminal that never drains (60 s).
const TRIES: u32 = 60_000;

/// Writes all of `bytes` to `out`, retrying while it would block. Returns
/// whether everything was written.
pub fn write_retrying(out: &mut impl Write, mut bytes: &[u8]) -> bool {
    let mut tries = 0;
    while !bytes.is_empty() {
        match out.write(bytes) {
            Ok(0) => return false,
            Ok(n) => {
                bytes = &bytes[n..];
                tries = 0;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock && tries < TRIES => {
                tries += 1;
                std::thread::sleep(RETRY);
            }
            Err(_) => return false,
        }
    }
    loop {
        match out.flush() {
            Ok(()) => return true,
            Err(e) if e.kind() == ErrorKind::WouldBlock && tries < TRIES => {
                tries += 1;
                std::thread::sleep(RETRY);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}

/// One line to stdout.
pub fn line_out(text: &str) {
    let mut line = String::with_capacity(text.len() + 1);
    line.push_str(text);
    line.push('\n');
    write_retrying(&mut std::io::stdout().lock(), line.as_bytes());
}

/// One line to stderr.
pub fn line_err(text: &str) {
    let mut line = String::with_capacity(text.len() + 1);
    line.push_str(text);
    line.push('\n');
    write_retrying(&mut std::io::stderr().lock(), line.as_bytes());
}

/// `println!` that retries on `EAGAIN`.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => { $crate::say::line_out(&format!($($arg)*)) };
}

/// `eprintln!` that retries on `EAGAIN`.
#[macro_export]
macro_rules! esay {
    ($($arg:tt)*) => { $crate::say::line_err(&format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// Would block `blocks` times before each accepted write, and takes at
    /// most `chunk` bytes a write.
    struct Stubborn {
        blocks: u32,
        left: u32,
        chunk: usize,
        written: Vec<u8>,
        fail: bool,
    }

    impl Write for Stubborn {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.fail {
                return Err(io::Error::from(ErrorKind::BrokenPipe));
            }
            if self.left > 0 {
                self.left -= 1;
                return Err(io::Error::from(ErrorKind::WouldBlock));
            }
            self.left = self.blocks;
            let n = buf.len().min(self.chunk);
            self.written.extend_from_slice(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn stubborn(blocks: u32, chunk: usize) -> Stubborn {
        Stubborn {
            blocks,
            left: blocks,
            chunk,
            written: Vec::new(),
            fail: false,
        }
    }

    #[test]
    fn a_write_that_would_block_is_tried_again_until_all_is_written() {
        let mut out = stubborn(3, 5);
        let text = b"[gui-tcg] PASS: 112 steps\n";
        assert!(write_retrying(&mut out, text));
        assert_eq!(out.written, text);
    }

    #[test]
    fn a_plain_writer_gets_everything_at_once() {
        let mut out = stubborn(0, usize::MAX);
        assert!(write_retrying(&mut out, b"abc"));
        assert_eq!(out.written, b"abc");
    }

    #[test]
    fn another_error_gives_up_without_a_panic() {
        let mut out = stubborn(0, 1);
        out.fail = true;
        assert!(!write_retrying(&mut out, b"abc"));
        assert!(out.written.is_empty());
    }

    #[test]
    fn nothing_to_write_is_success() {
        let mut out = stubborn(5, 1);
        assert!(write_retrying(&mut out, b""));
    }
}
