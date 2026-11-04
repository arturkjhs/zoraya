//! The line discipline (ADR-0105 item 2): canonical mode with echo, ERASE,
//! KILL and EOF, `ICRNL` on input and `ONLCR` on output, and the three
//! signal characters under `ISIG`. Non-canonical mode is here (a read takes
//! what there is) but nothing at stage 6 sets it.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use zoraya_proto_tty::NCCS;

/// musl's AArch64 (Linux's generic) `termios` bits.
pub mod bits {
    pub const ICRNL: u32 = 0o400;
    pub const INLCR: u32 = 0o100;
    pub const IGNCR: u32 = 0o200;
    pub const IXON: u32 = 0o2000;
    pub const IUTF8: u32 = 0o40000;

    pub const OPOST: u32 = 0o1;
    pub const ONLCR: u32 = 0o4;

    pub const B38400: u32 = 0o17;
    pub const CS8: u32 = 0o60;
    pub const CREAD: u32 = 0o200;

    pub const ISIG: u32 = 0o1;
    pub const ICANON: u32 = 0o2;
    pub const ECHO: u32 = 0o10;
    pub const ECHOE: u32 = 0o20;
    pub const ECHOK: u32 = 0o40;
    pub const ECHONL: u32 = 0o100;
    pub const NOFLSH: u32 = 0o200;
    pub const TOSTOP: u32 = 0o400;
    pub const ECHOCTL: u32 = 0o1000;
    pub const ECHOKE: u32 = 0o4000;
    pub const IEXTEN: u32 = 0o100000;

    pub const VINTR: usize = 0;
    pub const VQUIT: usize = 1;
    pub const VERASE: usize = 2;
    pub const VKILL: usize = 3;
    pub const VEOF: usize = 4;
    pub const VTIME: usize = 5;
    pub const VMIN: usize = 6;
    pub const VSUSP: usize = 10;
    pub const VEOL: usize = 11;
}

use bits::*;

/// Ctrl-H, the second erase character (ADR-0105 item 2).
const BACKSPACE: u8 = 0x08;
/// The most a line holds; past it, characters are dropped (Linux's 4095 and
/// the newline).
pub const MAX_LINE: usize = 4095;
/// The most completed input held before the reader takes it.
pub const MAX_PENDING: usize = 16 * 1024;

/// The `termios` subset (ADR-0105 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termios {
    pub iflag: u32,
    pub oflag: u32,
    pub cflag: u32,
    pub lflag: u32,
    pub cc: [u8; NCCS],
}

impl Default for Termios {
    /// Linux's defaults for a new terminal, `TOSTOP` off.
    fn default() -> Termios {
        let mut cc = [0u8; NCCS];
        cc[VINTR] = 0x03;
        cc[VQUIT] = 0x1c;
        cc[VERASE] = 0x7f;
        cc[VKILL] = 0x15;
        cc[VEOF] = 0x04;
        cc[VTIME] = 0;
        cc[VMIN] = 1;
        cc[VSUSP] = 0x1a;
        Termios {
            iflag: ICRNL | IXON | IUTF8,
            oflag: OPOST | ONLCR,
            cflag: B38400 | CS8 | CREAD,
            lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | ECHOKE | IEXTEN,
            cc,
        }
    }
}

/// A signal character (ADR-0105 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Quit,
    Suspend,
}

impl Signal {
    pub const fn number(self) -> u32 {
        match self {
            Signal::Interrupt => 2,
            Signal::Quit => 3,
            Signal::Suspend => 20,
        }
    }
}

/// One terminal's input state.
#[derive(Debug)]
pub struct Discipline {
    pub attr: Termios,
    /// The line being edited.
    line: Vec<u8>,
    /// Completed input: whole lines in canonical mode, each ending with its
    /// newline, or with nothing after an EOF character.
    ready: VecDeque<u8>,
    /// Where each completed line ends in `ready`, counted from its front;
    /// a zero-length line is an end of file.
    ends: VecDeque<usize>,
    pub dropped: u64,
}

impl Default for Discipline {
    fn default() -> Discipline {
        Discipline::new()
    }
}

impl Discipline {
    pub fn new() -> Discipline {
        Discipline {
            attr: Termios::default(),
            line: Vec::new(),
            ready: VecDeque::new(),
            ends: VecDeque::new(),
            dropped: 0,
        }
    }

    fn canonical(&self) -> bool {
        self.attr.lflag & ICANON != 0
    }

    fn echoing(&self) -> bool {
        self.attr.lflag & ECHO != 0
    }

    /// Takes one byte from the backend. What to show goes onto `echo`, as
    /// output (`ONLCR` already applied); a signal character returns its
    /// signal, after flushing the input unless `NOFLSH`.
    pub fn input(&mut self, byte: u8, echo: &mut Vec<u8>) -> Option<Signal> {
        let byte = match byte {
            b'\r' if self.attr.iflag & IGNCR != 0 => return None,
            b'\r' if self.attr.iflag & ICRNL != 0 => b'\n',
            b'\n' if self.attr.iflag & INLCR != 0 => b'\r',
            b => b,
        };
        let cc = self.attr.cc;
        if self.attr.lflag & ISIG != 0 {
            let signal = if byte == cc[VINTR] && byte != 0 {
                Some(Signal::Interrupt)
            } else if byte == cc[VQUIT] && byte != 0 {
                Some(Signal::Quit)
            } else if byte == cc[VSUSP] && byte != 0 {
                Some(Signal::Suspend)
            } else {
                None
            };
            if let Some(s) = signal {
                if self.attr.lflag & NOFLSH == 0 {
                    self.flush();
                }
                if self.echoing() {
                    self.echo_control(byte, echo);
                }
                return Some(s);
            }
        }
        if !self.canonical() {
            self.push_ready(&[byte]);
            if self.echoing() {
                self.echo_byte(byte, echo);
            }
            return None;
        }
        if byte == cc[VERASE] || byte == BACKSPACE {
            self.erase(echo);
        } else if byte == cc[VKILL] && byte != 0 {
            let lflag = self.attr.lflag;
            while !self.line.is_empty() {
                if lflag & ECHOKE != 0 && lflag & ECHOE != 0 && self.echoing() {
                    self.erase(echo);
                } else {
                    self.line.clear();
                }
            }
            if lflag & ECHOKE == 0 && lflag & ECHOK != 0 && self.echoing() {
                self.output(b"\n", echo);
            }
        } else if byte == cc[VEOF] && byte != 0 {
            // The line so far, without a newline; at the start of a line,
            // an end of file.
            let line = core::mem::take(&mut self.line);
            self.complete(&line);
        } else if byte == b'\n' || (byte == cc[VEOL] && byte != 0) {
            if self.echoing() || self.attr.lflag & ECHONL != 0 {
                self.output(b"\n", echo);
            }
            let mut line = core::mem::take(&mut self.line);
            line.push(byte);
            self.complete(&line);
        } else if self.line.len() < MAX_LINE {
            self.line.push(byte);
            if self.echoing() {
                self.echo_byte(byte, echo);
            }
        } else {
            self.dropped += 1;
        }
        None
    }

    fn push_ready(&mut self, bytes: &[u8]) -> bool {
        if self.ready.len() + bytes.len() > MAX_PENDING {
            self.dropped += bytes.len() as u64;
            return false;
        }
        self.ready.extend(bytes.iter().copied());
        true
    }

    fn complete(&mut self, line: &[u8]) {
        if self.push_ready(line) {
            self.ends.push_back(line.len());
        }
    }

    /// One character off the line being edited: a UTF-8 sequence whole.
    fn erase(&mut self, echo: &mut Vec<u8>) {
        let Some(mut last) = self.line.pop() else {
            return;
        };
        while last & 0xc0 == 0x80 {
            match self.line.pop() {
                Some(b) => last = b,
                None => break,
            }
        }
        if self.echoing() && self.attr.lflag & ECHOE != 0 {
            let width = if last < 0x20 && self.attr.lflag & ECHOCTL != 0 {
                2
            } else {
                1
            };
            for _ in 0..width {
                self.output(b"\x08 \x08", echo);
            }
        }
    }

    fn echo_byte(&self, byte: u8, echo: &mut Vec<u8>) {
        if byte < 0x20 && byte != b'\n' && byte != b'\t' {
            self.echo_control(byte, echo);
        } else {
            self.output(&[byte], echo);
        }
    }

    fn echo_control(&self, byte: u8, echo: &mut Vec<u8>) {
        if self.attr.lflag & ECHOCTL != 0 && byte < 0x20 {
            echo.extend_from_slice(&[b'^', byte + 0x40]);
        } else if byte == 0x7f && self.attr.lflag & ECHOCTL != 0 {
            echo.extend_from_slice(b"^?");
        } else {
            self.output(&[byte], echo);
        }
    }

    /// Output processing: `ONLCR` under `OPOST`.
    pub fn output(&self, bytes: &[u8], out: &mut Vec<u8>) {
        if self.attr.oflag & OPOST != 0 && self.attr.oflag & ONLCR != 0 {
            for &b in bytes {
                if b == b'\n' {
                    out.push(b'\r');
                }
                out.push(b);
            }
        } else {
            out.extend_from_slice(bytes);
        }
    }

    /// Throws away the line being edited and every completed line.
    pub fn flush(&mut self) {
        self.line.clear();
        self.ready.clear();
        self.ends.clear();
    }

    /// True if a read would not block.
    pub fn readable(&self) -> bool {
        if self.canonical() {
            !self.ends.is_empty()
        } else {
            !self.ready.is_empty()
        }
    }

    /// Up to `max` bytes of the next line (canonical) or of what there is;
    /// `None` when a read must wait. `Some` of nothing is an end of file.
    pub fn read(&mut self, max: usize) -> Option<Vec<u8>> {
        if !self.canonical() {
            if self.ready.is_empty() {
                return None;
            }
            let n = max.min(self.ready.len());
            return Some(self.ready.drain(..n).collect());
        }
        let end = *self.ends.front()?;
        if end == 0 {
            self.ends.pop_front();
            return Some(Vec::new());
        }
        let n = max.min(end);
        if max == 0 {
            return Some(Vec::new());
        }
        let bytes: Vec<u8> = self.ready.drain(..n).collect();
        if n == end {
            self.ends.pop_front();
        } else if let Some(front) = self.ends.front_mut() {
            *front -= n;
        }
        Some(bytes)
    }

    /// Changes the attributes. Leaving canonical mode makes the line being
    /// edited readable; entering it starts a line from what is unread.
    pub fn set(&mut self, attr: Termios) {
        let was = self.canonical();
        self.attr = attr;
        let now = self.canonical();
        if was && !now {
            let line = core::mem::take(&mut self.line);
            self.push_ready(&line);
            self.ends.clear();
        } else if !was && now && !self.ready.is_empty() {
            let rest: Vec<u8> = self.ready.drain(..).collect();
            self.line = rest;
            self.line.truncate(MAX_LINE);
        }
    }
}
