//! The pointer and the terminal (ADR-0332 items 3 and 8, ADR-0385). With a
//! program's mouse reporting off, or with Shift held, a drag selects text;
//! with reporting on (1000, and 1002 for motion with a button down) the
//! pointer's presses, releases, drags and wheel turns are reported in the SGR
//! encoding (1006), the only one there is: `CSI < b ; x ; y M` for a press
//! and a motion, `m` for a release, cells counted from 1. Focus in and out
//! are `CSI I` and `CSI O` while 1004 is set. Every report is made of the
//! person's own gesture and numbers; none depends on what a program printed
//! except through the modes it set (INV-0332-2).

use alloc::vec::Vec;

use crate::keys::Mods;
use crate::term::Term;

/// The buttons of `zoraya-proto-gui`'s `buttons` (left 1, right 2, middle 4).
pub const LEFT: u32 = 1;
pub const RIGHT: u32 = 2;
pub const MIDDLE: u32 = 4;

/// The most reports one turn of the wheel makes (a fast wheel gathers up to
/// 64 notches in one round; a program gets a few, not a flood).
pub const MAX_WHEEL_REPORTS: u32 = 8;

/// One pointer event: the cell it is over and the buttons held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEvent {
    pub column: usize,
    pub row: usize,
    pub buttons: u32,
    pub mods: Mods,
}

/// What the pointer did since its last event.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pointer {
    /// The buttons held at the last event.
    buttons: u32,
    /// The cell of the last event.
    at: Option<(usize, usize)>,
    /// A selection is being made, by the left button.
    selecting: bool,
    /// The buttons whose press was reported: only their releases are.
    reported: u32,
}

fn push_decimal(out: &mut Vec<u8>, mut n: usize) {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[i..]);
}

/// `CSI < code ; column ; row M` (`m` for a release), cells from 1.
fn report(out: &mut Vec<u8>, code: u32, column: usize, row: usize, press: bool) {
    out.extend_from_slice(b"\x1b[<");
    push_decimal(out, code as usize);
    out.push(b';');
    push_decimal(out, column + 1);
    out.push(b';');
    push_decimal(out, row + 1);
    out.push(if press { b'M' } else { b'm' });
}

/// The modifiers of a report's button code: Alt 8, Ctrl 16 (Shift, which
/// would add 4, selects instead and is never reported).
const fn code_mods(mods: Mods) -> u32 {
    (if mods.alt { 8 } else { 0 }) | (if mods.ctrl { 16 } else { 0 })
}

/// A button's number in a report: left 0, middle 1, right 2.
const fn number(button: u32) -> u32 {
    match button {
        LEFT => 0,
        MIDDLE => 1,
        _ => 2,
    }
}

impl Term {
    /// Whether the program has asked for the pointer to be reported.
    pub const fn reports_mouse(&self) -> bool {
        self.modes().mouse_press || self.modes().mouse_drag
    }
}

/// The bytes of a focus event: `CSI I` or `CSI O` while 1004 is set, else
/// none.
pub fn focus_bytes(term: &Term, focused: bool) -> &'static [u8] {
    match (term.modes().focus_events, focused) {
        (false, _) => b"",
        (true, true) => b"\x1b[I",
        (true, false) => b"\x1b[O",
    }
}

impl Pointer {
    /// Takes one pointer event: a selection moves, or reports are appended to
    /// `out`.
    pub fn event(&mut self, term: &mut Term, e: PointerEvent, out: &mut Vec<u8>) {
        let cell = (e.column, e.row);
        let moved = self.at != Some(cell);
        let pressed = e.buttons & !self.buttons;
        let released = self.buttons & !e.buttons;
        if self.selecting {
            // A selection ends with the left button, whatever else happens.
            if e.buttons & LEFT != 0 {
                if moved {
                    term.select_extend(e.column, e.row);
                }
            } else {
                term.select_extend(e.column, e.row);
                term.select_end();
                self.selecting = false;
            }
        } else if term.reports_mouse() && !e.mods.shift {
            let mods = code_mods(e.mods);
            for button in [LEFT, MIDDLE, RIGHT] {
                if pressed & button != 0 {
                    report(out, number(button) | mods, e.column, e.row, true);
                    self.reported |= button;
                }
            }
            if term.modes().mouse_drag && e.buttons != 0 && moved && pressed == 0 && released == 0 {
                // Motion with a button down: the lowest of those held.
                if let Some(button) = [LEFT, MIDDLE, RIGHT]
                    .into_iter()
                    .find(|b| e.buttons & b != 0)
                {
                    report(out, number(button) | mods | 32, e.column, e.row, true);
                }
            }
            for button in [LEFT, MIDDLE, RIGHT] {
                if released & button != 0 && self.reported & button != 0 {
                    report(out, number(button) | mods, e.column, e.row, false);
                    self.reported &= !button;
                }
            }
        } else if pressed & LEFT != 0 {
            term.select_start(e.column, e.row);
            self.selecting = true;
        }
        if !term.reports_mouse() {
            // Nothing is reported any more, whatever was pressed.
            self.reported = 0;
        }
        self.buttons = e.buttons;
        self.at = Some(cell);
    }

    /// The wheel turned `notches` (above 0: toward the person, the view moving
    /// down the content) over a cell. With reporting on and no Shift, each
    /// notch is a report (buttons 64 away, 65 toward), at most
    /// [`MAX_WHEEL_REPORTS`], and this gives `true`; otherwise nothing is
    /// appended and `false` says the view scrolls.
    pub fn wheel(
        &mut self,
        term: &Term,
        column: usize,
        row: usize,
        notches: i32,
        mods: Mods,
        out: &mut Vec<u8>,
    ) -> bool {
        if !term.reports_mouse() || mods.shift {
            return false;
        }
        let code = 64 | u32::from(notches > 0) | code_mods(mods);
        for _ in 0..notches.unsigned_abs().min(MAX_WHEEL_REPORTS) {
            report(out, code, column, row, true);
        }
        true
    }
}
