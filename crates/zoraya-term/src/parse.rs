//! The parser (ADR-0332 items 1 to 3): a state machine over bytes, after the
//! DEC parser of Paul Williams, that is total and bounded. Whatever it reads,
//! the cursor stays in the grid and the memory is what [`Term::memory_bound`]
//! says; a well-formed sequence outside the lists of the ADR is consumed and
//! changes nothing (INV-0332-3); and nothing it reads can produce input: the
//! parser has no output path (INV-0096-2, INV-0332-2).

use crate::cell::{
    BLINK, BOLD, Colour, FAINT, INVISIBLE, ITALIC, REVERSE, SGR_ATTRS, STRIKE, UNDERLINE,
};
use crate::colour::parse_spec;
use crate::term::{Charset, Modes, Pen, State, Term};
use crate::{MAX_PARAMS, MAX_STRING};

/// Whether a `CSI` with this private marker (0 for none), first
/// intermediate (0 for none) and final byte is one the terminal acts on
/// (ADR-0332 items 2 and 3). Every other one is consumed and ignored.
pub fn csi_known(marker: u8, intermediate: u8, final_byte: u8) -> bool {
    match (marker, intermediate) {
        (0, 0) => matches!(
            final_byte,
            b'@' | b'A'
                | b'B'
                | b'C'
                | b'D'
                | b'E'
                | b'F'
                | b'G'
                | b'H'
                | b'I'
                | b'J'
                | b'K'
                | b'L'
                | b'M'
                | b'P'
                | b'S'
                | b'T'
                | b'X'
                | b'Z'
                | b'b'
                | b'd'
                | b'f'
                | b'g'
                | b'h'
                | b'l'
                | b'm'
                | b'r'
        ),
        (b'?', 0) => matches!(final_byte, b'h' | b'l'),
        (0, b' ') => final_byte == b'q',
        (0, b'!') => final_byte == b'p',
        _ => false,
    }
}

/// Whether an `ESC` sequence with this intermediate (0 for none) and final
/// byte is one the terminal acts on.
pub fn escape_known(intermediate: u8, final_byte: u8) -> bool {
    match intermediate {
        0 => matches!(
            final_byte,
            b'7' | b'8' | b'D' | b'E' | b'H' | b'M' | b'c' | b'=' | b'>'
        ),
        b'(' | b')' => matches!(final_byte, b'B' | b'0'),
        _ => false,
    }
}

impl Term {
    /// Takes output bytes from the shell.
    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        // Output moves the view back to the live screen.
        if self.view != 0 {
            self.view = 0;
            self.touch_all();
        }
        // CAN and SUB end whatever is going on.
        if (b == 0x18 || b == 0x1a) && self.state != State::Ground {
            self.abort();
            return;
        }
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::EscapeIntermediate => self.escape_intermediate(b),
            State::Csi => self.csi(b),
            State::CsiIgnore => self.csi_ignore(b),
            State::Osc | State::Skip => self.string(b),
        }
    }

    fn abort(&mut self) {
        self.state = State::Ground;
        self.string_escape = false;
        self.osc.clear();
    }

    /// The controls that act wherever they come, a sequence's middle
    /// included.
    fn control(&mut self, b: u8) {
        match b {
            0x08 => {
                self.wrap = false;
                self.x = self.x.saturating_sub(1);
                self.touch(self.y);
            }
            b'\t' => self.tab_forward(1),
            // LF, VT and FF: down one row, the column kept (ADR-0332 item 2).
            b'\n' | 0x0b | 0x0c => {
                self.wrap = false;
                self.index();
            }
            b'\r' => self.carriage_return(),
            0x0e => self.shifted = true,
            0x0f => self.shifted = false,
            _ => {}
        }
    }

    fn ground(&mut self, b: u8) {
        if self.needed > 0 {
            if b & 0xc0 == 0x80 {
                self.utf8 = self.utf8 << 6 | u32::from(b & 0x3f);
                self.needed -= 1;
                if self.needed == 0 {
                    // Overlong encodings, surrogates, values past U+10FFFF
                    // and C1 controls all show as U+FFFD: nothing decoded
                    // is ever a control (ADR-0013 allows UTF-8 names).
                    let c = char::from_u32(self.utf8)
                        .filter(|_| self.utf8 >= self.utf8_min)
                        .filter(|c| !c.is_control())
                        .unwrap_or('\u{fffd}');
                    self.put(c);
                }
                return;
            }
            // A sequence cut short: one replacement, then this byte anew.
            self.needed = 0;
            self.put('\u{fffd}');
        }
        match b {
            0x1b => self.state = State::Escape,
            0x00..=0x1f => self.control(b),
            0x7f => {}
            0x20..=0x7e => {
                let c = self.map_ascii(b);
                self.put(c);
            }
            0xc2..=0xdf => self.lead(b & 0x1f, 1, 0x80),
            0xe0..=0xef => self.lead(b & 0x0f, 2, 0x800),
            0xf0..=0xf4 => self.lead(b & 0x07, 3, 0x1_0000),
            _ => self.put('\u{fffd}'),
        }
    }

    fn lead(&mut self, value: u8, needed: u8, min: u32) {
        self.utf8 = u32::from(value);
        self.needed = needed;
        self.utf8_min = min;
    }

    fn start_sequence(&mut self, state: State) {
        self.state = state;
        self.params = [0; MAX_PARAMS];
        self.sub = 0;
        self.nparams = 0;
        self.cur_open = false;
        self.more = false;
        self.next_is_sub = false;
        self.overflow = false;
        self.marker = 0;
        self.inter = [0; 2];
        self.ninter = 0;
        self.seq_len = 0;
    }

    fn escape(&mut self, b: u8) {
        if self.string_escape {
            // An `ESC` inside a string: `\` ends it (ST), anything else
            // abandons it and starts a new escape sequence.
            self.string_escape = false;
            if b == b'\\' {
                self.finish_string();
                self.state = State::Ground;
                return;
            }
            self.osc.clear();
        }
        match b {
            0x1b => self.state = State::Escape,
            0x00..=0x1f => self.control(b),
            b'[' => self.start_sequence(State::Csi),
            b']' => {
                self.start_sequence(State::Osc);
                self.osc.clear();
            }
            b'P' | b'X' | b'^' | b'_' => self.start_sequence(State::Skip),
            0x20..=0x2f => {
                self.start_sequence(State::EscapeIntermediate);
                self.inter[0] = b;
                self.ninter = 1;
            }
            0x30..=0x7e => {
                self.state = State::Ground;
                self.escape_final(0, b);
            }
            // DEL is ignored, a byte past ASCII ends the sequence and is text.
            0x7f => {}
            _ => {
                self.state = State::Ground;
                self.ground(b);
            }
        }
    }

    fn escape_intermediate(&mut self, b: u8) {
        match b {
            0x1b => self.state = State::Escape,
            0x00..=0x1f => self.control(b),
            // More intermediates: the sequence is not one this terminal knows.
            0x20..=0x2f => self.ninter = 2,
            0x30..=0x7e => {
                self.state = State::Ground;
                let known = self.ninter == 1;
                if known {
                    self.escape_final(self.inter[0], b);
                }
            }
            0x7f => {}
            _ => {
                self.state = State::Ground;
                self.ground(b);
            }
        }
    }

    fn escape_final(&mut self, intermediate: u8, b: u8) {
        if !escape_known(intermediate, b) {
            return;
        }
        match (intermediate, b) {
            (0, b'7') => self.save_cursor(usize::from(self.alt)),
            (0, b'8') => {
                self.restore_cursor(usize::from(self.alt));
                self.touch_all();
            }
            (0, b'D') => {
                self.wrap = false;
                self.index();
            }
            (0, b'E') => {
                self.carriage_return();
                self.index();
            }
            (0, b'H') => {
                if let Some(t) = self.tabs.get_mut(self.x) {
                    *t = true;
                }
            }
            (0, b'M') => {
                self.wrap = false;
                self.reverse_index();
            }
            (0, b'c') => self.full_reset(),
            (0, b'=') => self.modes.app_keypad = true,
            (0, b'>') => self.modes.app_keypad = false,
            (i, set) => {
                let g = usize::from(i == b')');
                self.g[g] = if set == b'0' {
                    Charset::Graphics
                } else {
                    Charset::Ascii
                };
            }
        }
    }

    fn begin_param(&mut self, sub: bool) {
        if self.nparams == MAX_PARAMS {
            self.overflow = true;
            return;
        }
        self.params[self.nparams] = 0;
        if sub {
            self.sub |= 1 << self.nparams;
        }
        self.nparams += 1;
    }

    fn csi(&mut self, b: u8) {
        // A sequence that never ends is cut at the string bound too.
        self.seq_len += 1;
        if self.seq_len >= MAX_STRING {
            self.state = State::Ground;
            return;
        }
        match b {
            b'0'..=b'9' => {
                if self.ninter > 0 {
                    self.state = State::CsiIgnore;
                    return;
                }
                if !self.cur_open {
                    self.begin_param(self.next_is_sub);
                    self.next_is_sub = false;
                    self.cur_open = true;
                }
                if !self.overflow {
                    let p = &mut self.params[self.nparams - 1];
                    *p = p.saturating_mul(10).saturating_add(u16::from(b - b'0'));
                }
            }
            b';' | b':' => {
                if self.ninter > 0 {
                    self.state = State::CsiIgnore;
                    return;
                }
                if !self.cur_open {
                    self.begin_param(self.next_is_sub);
                }
                self.cur_open = false;
                self.more = true;
                self.next_is_sub = b == b':';
            }
            b'<'..=b'?' => {
                if self.nparams == 0 && !self.more && self.marker == 0 && self.ninter == 0 {
                    self.marker = b;
                } else {
                    self.state = State::CsiIgnore;
                }
            }
            0x20..=0x2f => {
                if self.ninter < self.inter.len() {
                    self.inter[self.ninter] = b;
                    self.ninter += 1;
                } else {
                    self.state = State::CsiIgnore;
                }
            }
            0x40..=0x7e => {
                if self.more && !self.cur_open {
                    self.begin_param(self.next_is_sub);
                }
                self.state = State::Ground;
                if !self.overflow && self.ninter <= 1 {
                    self.dispatch(b);
                }
            }
            0x1b => self.state = State::Escape,
            0x00..=0x1f => self.control(b),
            0x7f => {}
            _ => {
                self.state = State::Ground;
                self.ground(b);
            }
        }
    }

    fn csi_ignore(&mut self, b: u8) {
        self.seq_len += 1;
        if self.seq_len >= MAX_STRING {
            self.state = State::Ground;
            return;
        }
        match b {
            0x40..=0x7e => self.state = State::Ground,
            0x1b => self.state = State::Escape,
            0x00..=0x1f => self.control(b),
            0x20..=0x3f | 0x7f => {}
            _ => {
                self.state = State::Ground;
                self.ground(b);
            }
        }
    }

    /// A byte of an `OSC` (collected) or of a `DCS`, `SOS`, `PM` or `APC`
    /// (skipped). Both end at `ST`, `ESC \`; an `OSC` also at BEL.
    fn string(&mut self, b: u8) {
        self.seq_len += 1;
        if self.seq_len >= MAX_STRING {
            // Cut at the bound: nothing is run, the rest is text again.
            self.abort();
            return;
        }
        match b {
            0x1b => {
                self.string_escape = true;
                self.state = State::Escape;
            }
            0x07 if self.state == State::Osc => {
                self.finish_string();
                self.state = State::Ground;
            }
            0x00..=0x1f | 0x7f => {}
            _ => {
                if self.state == State::Osc {
                    self.osc.push(b);
                }
            }
        }
    }

    /// The string ended properly: an `OSC` runs, the others were skipped.
    /// Called with the state still `Osc` or `Skip`, or just left for
    /// `Escape` by the `ESC` of `ST`.
    fn finish_string(&mut self) {
        let osc = core::mem::take(&mut self.osc);
        if self.was_osc(&osc) {
            self.run_osc(&osc);
        }
        self.osc = osc;
        self.osc.clear();
    }

    /// An `OSC` string is the only one collected, so a non-empty buffer
    /// is one. (A `DCS` leaves it empty; an empty `OSC` does nothing.)
    fn was_osc(&self, osc: &[u8]) -> bool {
        !osc.is_empty()
    }

    /// `OSC 4` and `OSC 104`: the palette (ADR-0332 item 3). Every other
    /// number, the titles and the clipboard (`OSC 52`) included, is ignored;
    /// a `?` asks a question and is not answered.
    fn run_osc(&mut self, osc: &[u8]) {
        let mut parts = osc.split(|b| *b == b';');
        let number = parts.next().unwrap_or(&[]);
        match number {
            b"4" => {
                while let (Some(index), Some(spec)) = (parts.next(), parts.next()) {
                    if let (Some(i), Some(rgb)) = (number_of(index), parse_spec(spec))
                        && i < 256
                    {
                        self.overrides[i] = Some(rgb);
                    }
                }
                self.touch_all();
            }
            b"104" => {
                let mut any = false;
                for index in parts {
                    any = true;
                    if let Some(i) = number_of(index)
                        && i < 256
                    {
                        self.overrides[i] = None;
                    }
                }
                if !any {
                    self.overrides = [None; 256];
                }
                self.touch_all();
            }
            _ => {}
        }
    }

    /// Parameter `i`, with 0 and absent meaning `default`.
    fn param(&self, i: usize, default: usize) -> usize {
        match self.params.get(i) {
            Some(&p) if i < self.nparams && p != 0 => usize::from(p),
            _ => default,
        }
    }

    fn dispatch(&mut self, final_byte: u8) {
        let intermediate = self.inter[0];
        if !csi_known(self.marker, intermediate, final_byte) {
            return;
        }
        // The cursor is drawn where it is: its old and new rows change.
        let from = self.y;
        self.dispatch_known(final_byte, intermediate);
        self.touch(from);
        self.touch(self.y);
    }

    fn dispatch_known(&mut self, final_byte: u8, intermediate: u8) {
        match (self.marker, intermediate, final_byte) {
            (b'?', _, b'h') => return self.private_modes(true),
            (b'?', _, b'l') => return self.private_modes(false),
            (_, b' ', _) => {
                let ps = self.param(0, 0);
                if ps <= 6 {
                    self.shape = ps as u8;
                }
                return;
            }
            (_, b'!', _) => return self.soft_reset(),
            (_, _, b'm') => return self.graphics(),
            (_, _, b'h') => return self.ansi_modes(true),
            (_, _, b'l') => return self.ansi_modes(false),
            _ => {}
        }
        let n = self.param(0, 1);
        match final_byte {
            b'A' => self.cursor_up(n),
            b'B' => self.cursor_down(n),
            b'C' => {
                self.x = (self.x + n).min(self.columns - 1);
                self.wrap = false;
            }
            b'D' => {
                self.x = self.x.saturating_sub(n);
                self.wrap = false;
            }
            b'E' => {
                self.cursor_down(n);
                self.x = 0;
            }
            b'F' => {
                self.cursor_up(n);
                self.x = 0;
            }
            b'G' => {
                self.x = (n - 1).min(self.columns - 1);
                self.wrap = false;
            }
            b'd' => {
                let col = self.x;
                self.move_to(n - 1, col);
            }
            b'H' | b'f' => self.move_to(self.param(0, 1) - 1, self.param(1, 1) - 1),
            b'I' => self.tab_forward(n),
            b'Z' => self.tab_back(n),
            b'J' => {
                self.wrap = false;
                self.erase_display(self.param(0, 0));
            }
            b'K' => {
                self.wrap = false;
                self.erase_line(self.param(0, 0));
            }
            b'@' => {
                self.wrap = false;
                self.insert_cells(n);
            }
            b'P' => {
                self.wrap = false;
                self.delete_cells(n);
            }
            b'X' => {
                self.wrap = false;
                self.erase_cells(n);
            }
            b'L' => {
                self.wrap = false;
                self.insert_lines(n);
            }
            b'M' => {
                self.wrap = false;
                self.delete_lines(n);
            }
            // `CSI Ps T` scrolls down; with more parameters it is a mouse
            // highlight that is not ours.
            b'S' => {
                let (top, bottom) = (self.top, self.bottom);
                self.scroll_up_in(top, bottom, n, true);
            }
            b'T' if self.nparams <= 1 => {
                let (top, bottom) = (self.top, self.bottom);
                self.scroll_down_in(top, bottom, n);
            }
            b'b' => {
                if let Some(c) = self.last {
                    for _ in 0..n.min(usize::from(u16::MAX)) {
                        self.put(c);
                    }
                }
            }
            b'g' => match self.param(0, 0) {
                0 => {
                    if let Some(t) = self.tabs.get_mut(self.x) {
                        *t = false;
                    }
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            b'r' => {
                let top = self.param(0, 1) - 1;
                let bottom = self.param(1, self.rows) - 1;
                self.set_region(top, bottom);
            }
            _ => {}
        }
    }

    /// SM and RM: only insert mode (4) acts.
    fn ansi_modes(&mut self, on: bool) {
        if self.marker != 0 {
            return;
        }
        for i in 0..self.nparams {
            if self.params[i] == 4 && self.sub & (1 << i) == 0 {
                self.modes.insert = on;
            }
        }
    }

    /// DECSET and DECRST (ADR-0332 items 2 and 3). Modes 12, 1003, 1005,
    /// 1015, 1034 and 2027 are recognised and do nothing, as does any other.
    fn private_modes(&mut self, on: bool) {
        for i in 0..self.nparams {
            if self.sub & (1 << i) != 0 {
                continue;
            }
            match self.params[i] {
                1 => self.modes.app_cursor = on,
                5 => {
                    self.modes.reverse_screen = on;
                    self.touch_all();
                }
                6 => {
                    self.modes.origin = on;
                    self.move_to(0, 0);
                }
                7 => {
                    self.modes.autowrap = on;
                    if !on {
                        self.wrap = false;
                    }
                }
                25 => self.modes.cursor_visible = on,
                47 | 1047 => {
                    if on {
                        self.enter_alt(false);
                    } else {
                        self.leave_alt();
                    }
                }
                1048 => {
                    if on {
                        self.save_cursor(2);
                    } else {
                        self.restore_cursor(2);
                    }
                }
                1049 => {
                    if on {
                        self.enter_alt(true);
                    } else {
                        self.leave_alt();
                    }
                }
                1000 => self.modes.mouse_press = on,
                1002 => self.modes.mouse_drag = on,
                1004 => self.modes.focus_events = on,
                1006 => self.modes.mouse_sgr = on,
                2004 => self.modes.bracketed_paste = on,
                2026 => self.modes.synchronized = on,
                _ => {}
            }
        }
    }

    /// SGR (ADR-0332 item 2). An extended colour (38 or 48) is consumed as
    /// one unit, in the semicolon form (`38;5;N`, `38;2;R;G;B`) and in the
    /// colon form (`38:5:N`, `38:2::R:G:B`, `38:2:R:G:B`).
    fn graphics(&mut self) {
        if self.nparams == 0 {
            self.pen = Pen::DEFAULT;
            return;
        }
        let n = self.nparams;
        let mut i = 0;
        while i < n {
            let p = self.params[i];
            // Sub-parameters this parser does not use are skipped with their
            // parameter.
            let mut end = i + 1;
            while end < n && self.sub & (1 << end) != 0 {
                end += 1;
            }
            let colon = end > i + 1;
            match p {
                38 | 48 | 58 => {
                    let (colour, used) = if colon {
                        (self.colon_colour(i + 1, end), end - i)
                    } else {
                        self.semicolon_colour(i + 1)
                    };
                    // 58 is the underline's colour: consumed, not drawn.
                    if let Some(c) = colour {
                        match p {
                            38 => self.pen.fg = c,
                            48 => self.pen.bg = c,
                            _ => {}
                        }
                    }
                    i = if colon { end } else { i + 1 + used };
                    continue;
                }
                4 if colon => {
                    // 4:0 off; 4:1 to 4:5 are styles of underline, all drawn alike.
                    if self.params[i + 1] == 0 {
                        self.pen.attrs &= !UNDERLINE;
                    } else {
                        self.pen.attrs |= UNDERLINE;
                    }
                }
                _ => self.sgr_code(p),
            }
            i = end;
        }
    }

    /// The colour after `38` in the semicolon form, from parameter `at`, and
    /// how many parameters it used.
    fn semicolon_colour(&self, at: usize) -> (Option<Colour>, usize) {
        let get = |k: usize| (at + k < self.nparams).then(|| self.params[at + k]);
        match get(0) {
            Some(5) => match get(1) {
                Some(v) => (u8::try_from(v).ok().map(Colour::Index), 2),
                None => (None, 1),
            },
            Some(2) => match (get(1), get(2), get(3)) {
                (Some(r), Some(g), Some(b)) => (
                    match (u8::try_from(r), u8::try_from(g), u8::try_from(b)) {
                        (Ok(r), Ok(g), Ok(b)) => Some(Colour::Rgb(r, g, b)),
                        _ => None,
                    },
                    4,
                ),
                _ => (None, self.nparams.saturating_sub(at)),
            },
            // An unknown colour space: the rest of the sequence is not ours.
            Some(_) => (None, self.nparams.saturating_sub(at)),
            None => (None, 0),
        }
    }

    /// The colour of the colon form, its sub-parameters `from..end`.
    fn colon_colour(&self, from: usize, end: usize) -> Option<Colour> {
        let v = &self.params[from..end];
        match v {
            [5, n, ..] => u8::try_from(*n).ok().map(Colour::Index),
            // `2:R:G:B` or `2:CS:R:G:B`, the colour space ignored.
            [2, r, g, b] | [2, _, r, g, b, ..] => {
                match (u8::try_from(*r), u8::try_from(*g), u8::try_from(*b)) {
                    (Ok(r), Ok(g), Ok(b)) => Some(Colour::Rgb(r, g, b)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn sgr_code(&mut self, p: u16) {
        let pen = &mut self.pen;
        match p {
            0 => *pen = Pen::DEFAULT,
            1 => pen.attrs |= BOLD,
            2 => pen.attrs |= FAINT,
            3 => pen.attrs |= ITALIC,
            4 | 21 => pen.attrs |= UNDERLINE,
            5 | 6 => pen.attrs |= BLINK,
            7 => pen.attrs |= REVERSE,
            8 => pen.attrs |= INVISIBLE,
            9 => pen.attrs |= STRIKE,
            22 => pen.attrs &= !(BOLD | FAINT),
            23 => pen.attrs &= !ITALIC,
            24 => pen.attrs &= !UNDERLINE,
            25 => pen.attrs &= !BLINK,
            27 => pen.attrs &= !REVERSE,
            28 => pen.attrs &= !INVISIBLE,
            29 => pen.attrs &= !STRIKE,
            30..=37 => pen.fg = Colour::Index((p - 30) as u8),
            39 => pen.fg = Colour::Default,
            40..=47 => pen.bg = Colour::Index((p - 40) as u8),
            49 => pen.bg = Colour::Default,
            90..=97 => pen.fg = Colour::Index((p - 90 + 8) as u8),
            100..=107 => pen.bg = Colour::Index((p - 100 + 8) as u8),
            // 26 (proportional spacing) and everything else: nothing.
            _ => {}
        }
        debug_assert!(self.pen.attrs & !SGR_ATTRS == 0);
    }
}

/// A decimal number of at most four digits, from an `OSC`'s field.
fn number_of(field: &[u8]) -> Option<usize> {
    if field.is_empty() || field.len() > 4 || !field.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(field.iter().fold(0, |n, d| n * 10 + usize::from(d - b'0')))
}

impl Modes {
    /// The modes a fresh terminal has.
    pub const fn new() -> Modes {
        Modes::DEFAULT
    }
}

impl Default for Modes {
    fn default() -> Modes {
        Modes::DEFAULT
    }
}
