//! The terminal functions that are arithmetic on musl's `struct termios`
//! (ADR-0342 item 4, ADR-0389): `cfmakeraw`, the four speed functions, the
//! `when` and `queue` arguments of `tcsetattr` and `tcflush`, and the names
//! `ttyname` and `ctermid` give. The requests to the terminal are in
//! `ffi/procs.rs`; nothing here touches a handle.

#![forbid(unsafe_code)]

use crate::errno::{EINVAL, ERANGE};

/// Bytes of musl's `struct termios`: four flag words, `c_line`, `c_cc[32]`,
/// the two speeds.
pub const SIZE: usize = 60;

const IFLAG: usize = 0;
const OFLAG: usize = 4;
const CFLAG: usize = 8;
const LFLAG: usize = 12;
const CC: usize = 17;
const ISPEED: usize = 52;
const OSPEED: usize = 56;

const VTIME: usize = 5;
const VMIN: usize = 6;

// Linux's values, which musl's header has.
const IGNBRK: u32 = 0o1;
const BRKINT: u32 = 0o2;
const PARMRK: u32 = 0o10;
const ISTRIP: u32 = 0o40;
const INLCR: u32 = 0o100;
const IGNCR: u32 = 0o200;
const ICRNL: u32 = 0o400;
const IXON: u32 = 0o2000;
const OPOST: u32 = 0o1;
const CSIZE: u32 = 0o60;
const CS8: u32 = 0o60;
const PARENB: u32 = 0o400;
const ISIG: u32 = 0o1;
const ICANON: u32 = 0o2;
const ECHO: u32 = 0o10;
const ECHONL: u32 = 0o100;
const IEXTEN: u32 = 0o100000;
/// The baud rate bits of `c_cflag`.
pub const CBAUD: u32 = 0o10017;

/// `tcsetattr`'s `optional_actions`.
pub const TCSANOW: i32 = 0;
pub const TCSADRAIN: i32 = 1;
pub const TCSAFLUSH: i32 = 2;

/// `tcflush`'s `queue_selector` and `tcflow`'s `action`.
pub const TCIFLUSH: i32 = 0;
pub const TCOFLUSH: i32 = 1;
pub const TCIOFLUSH: i32 = 2;
pub const TCOOFF: i32 = 0;
pub const TCOON: i32 = 1;
pub const TCIOFF: i32 = 2;
pub const TCION: i32 = 3;

/// `ioctl` requests that a terminal always refuses (ADR-0331 item 7,
/// ADR-0338 item 2).
pub const TIOCSTI: i64 = 0x5412;
pub const TIOCSWINSZ: i64 = 0x5414;

/// What a terminal answers to a request it never carries out: `EPERM` for
/// the size (only the window sets it), `ENOTTY` for input injection
/// (`TIOCSTI` is not offered; INV-0338-1).
pub fn ioctl_refusal(request: i64) -> Option<i32> {
    match request {
        TIOCSWINSZ => Some(crate::errno::EPERM),
        TIOCSTI => Some(crate::errno::ENOTTY),
        _ => None,
    }
}

/// The name of the controlling terminal (ADR-0342 item 4, ADR-0105 item 6).
pub const TTY_NAME: &[u8; 9] = b"/dev/tty\0";

fn word(t: &[u8; SIZE], at: usize) -> u32 {
    u32::from_le_bytes([t[at], t[at + 1], t[at + 2], t[at + 3]])
}

fn put(t: &mut [u8; SIZE], at: usize, w: u32) {
    t[at..at + 4].copy_from_slice(&w.to_le_bytes());
}

/// `cfmakeraw`: musl's own arithmetic.
pub fn make_raw(t: &mut [u8; SIZE]) {
    let i = word(t, IFLAG) & !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    put(t, IFLAG, i);
    let o = word(t, OFLAG) & !OPOST;
    put(t, OFLAG, o);
    let l = word(t, LFLAG) & !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
    put(t, LFLAG, l);
    let c = (word(t, CFLAG) & !(CSIZE | PARENB)) | CS8;
    put(t, CFLAG, c);
    t[CC + VMIN] = 1;
    t[CC + VTIME] = 0;
}

/// `cfgetospeed` and `cfgetispeed` (one speed, `c_cflag`'s).
pub fn speed(t: &[u8; SIZE]) -> u32 {
    word(t, CFLAG) & CBAUD
}

/// `cfsetospeed`: stored, without effect (ADR-0342 item 4); a value that is
/// not a baud rate is `EINVAL`.
pub fn set_speed(t: &mut [u8; SIZE], speed: u32) -> Result<(), i32> {
    if speed & !CBAUD != 0 {
        return Err(EINVAL);
    }
    let c = (word(t, CFLAG) & !CBAUD) | speed;
    put(t, CFLAG, c);
    put(t, ISPEED, speed);
    put(t, OSPEED, speed);
    Ok(())
}

/// `cfsetispeed`: musl's: zero means "the output speed" and changes nothing.
pub fn set_input_speed(t: &mut [u8; SIZE], speed: u32) -> Result<(), i32> {
    if speed == 0 {
        Ok(())
    } else {
        set_speed(t, speed)
    }
}

/// `tcsetattr`'s `optional_actions` or `EINVAL`.
pub fn check_action(when: i32) -> Result<i32, i32> {
    match when {
        TCSANOW | TCSADRAIN | TCSAFLUSH => Ok(when),
        _ => Err(EINVAL),
    }
}

/// `tcflush`'s queue: whether the unread input is flushed (`TCIFLUSH` and
/// `TCIOFLUSH`), or `EINVAL`. Output is a stream the library has already
/// written into (ADR-0331 item 5), so `TCOFLUSH` has nothing to drop.
pub fn flushes_input(queue: i32) -> Result<bool, i32> {
    match queue {
        TCIFLUSH | TCIOFLUSH => Ok(true),
        TCOFLUSH => Ok(false),
        _ => Err(EINVAL),
    }
}

/// `tcflow`'s action or `EINVAL`.
pub fn check_flow(action: i32) -> Result<(), i32> {
    match action {
        TCOOFF | TCOON | TCIOFF | TCION => Ok(()),
        _ => Err(EINVAL),
    }
}

/// `ttyname_r`'s copy: the name and its NUL into a buffer of `size` bytes, or
/// `ERANGE`.
pub fn name_into(buf: &mut [u8], size: usize) -> Result<(), i32> {
    if size < TTY_NAME.len() || buf.len() < TTY_NAME.len() {
        return Err(ERANGE);
    }
    buf[..TTY_NAME.len()].copy_from_slice(TTY_NAME);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_tty::Termios;

    fn musl(t: &Termios) -> [u8; SIZE] {
        let mut b = [0u8; SIZE];
        for (i, w) in [t.iflag, t.oflag, t.cflag, t.lflag].iter().enumerate() {
            put(&mut b, i * 4, *w);
        }
        b[CC..CC + 32].copy_from_slice(&t.cc);
        b
    }

    #[test]
    fn make_raw_is_what_the_disciplines_raw_is() {
        let mut t = musl(&Termios::default());
        make_raw(&mut t);
        let r = Termios::raw();
        // The same flags the host names `raw()` ... with 8-bit characters,
        // no parity and VMIN 1, VTIME 0 on top.
        assert_eq!(
            word(&t, IFLAG) & !(IGNBRK | BRKINT | PARMRK | ISTRIP),
            r.iflag
        );
        assert_eq!(word(&t, OFLAG), r.oflag);
        assert_eq!(word(&t, LFLAG), r.lflag);
        assert_eq!(word(&t, CFLAG) & CSIZE, CS8);
        assert_eq!((t[CC + VMIN], t[CC + VTIME]), (1, 0));
    }

    #[test]
    fn make_raw_clears_exactly_musls_bits_and_nothing_else() {
        // Every other bit of every word, and the other bytes, stay.
        let mut t = [0xffu8; SIZE];
        make_raw(&mut t);
        let w = |at| word(&t, at);
        assert_eq!(
            w(IFLAG),
            !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON)
        );
        assert_eq!(w(OFLAG), !OPOST);
        assert_eq!(w(LFLAG), !(ECHO | ECHONL | ICANON | ISIG | IEXTEN));
        assert_eq!(w(CFLAG), !(CSIZE | PARENB) | CS8);
        for (i, b) in t.iter().enumerate() {
            match i {
                0..=15 => {}
                x if x == CC + VMIN => assert_eq!(*b, 1),
                x if x == CC + VTIME => assert_eq!(*b, 0),
                _ => assert_eq!(*b, 0xff, "byte {i}"),
            }
        }
    }

    #[test]
    fn speeds_are_stored_and_a_bad_one_is_refused() {
        let mut t = [0u8; SIZE];
        assert_eq!(set_speed(&mut t, 0o15), Ok(()));
        assert_eq!(speed(&t), 0o15);
        assert_eq!(set_speed(&mut t, 0o20), Err(EINVAL));
        assert_eq!(speed(&t), 0o15, "a refused speed changes nothing");
        assert_eq!(set_input_speed(&mut t, 0), Ok(()));
        assert_eq!(speed(&t), 0o15);
        assert_eq!(set_input_speed(&mut t, 0o12), Ok(()));
        assert_eq!(speed(&t), 0o12);
    }

    #[test]
    fn the_arguments_are_checked() {
        assert_eq!(check_action(TCSANOW), Ok(0));
        assert_eq!(check_action(TCSADRAIN), Ok(1));
        assert_eq!(check_action(TCSAFLUSH), Ok(2));
        assert_eq!(check_action(3), Err(EINVAL));
        assert_eq!(check_action(-1), Err(EINVAL));
        assert_eq!(flushes_input(TCIFLUSH), Ok(true));
        assert_eq!(flushes_input(TCIOFLUSH), Ok(true));
        assert_eq!(flushes_input(TCOFLUSH), Ok(false));
        assert_eq!(flushes_input(3), Err(EINVAL));
        for a in 0..4 {
            assert_eq!(check_flow(a), Ok(()));
        }
        assert_eq!(check_flow(4), Err(EINVAL));
    }

    #[test]
    fn no_request_injects_input_and_only_the_window_sets_the_size() {
        // INV-0338-1.
        assert_eq!(ioctl_refusal(TIOCSTI), Some(crate::errno::ENOTTY));
        assert_eq!(ioctl_refusal(TIOCSWINSZ), Some(crate::errno::EPERM));
        assert_eq!(ioctl_refusal(0x5413), None, "TIOCGWINSZ is answered");
        assert_eq!(ioctl_refusal(0x5401), None, "TCGETS is answered");
    }

    #[test]
    fn the_name_fits_or_is_erange() {
        let mut b = [9u8; 16];
        assert_eq!(name_into(&mut b, 8), Err(ERANGE));
        assert_eq!(b[0], 9, "nothing written on ERANGE");
        assert_eq!(name_into(&mut b, 9), Ok(()));
        assert_eq!(&b[..9], b"/dev/tty\0");
        let mut small = [0u8; 4];
        assert_eq!(name_into(&mut small, 9), Err(ERANGE));
    }
}
