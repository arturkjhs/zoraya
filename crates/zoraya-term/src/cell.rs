//! One cell of the grid, in 16 bytes (ADR-0332 item 6): the character, up to
//! two combining marks, the attributes and the two colours, each a palette
//! index, a direct colour or the default.

/// Drawn with the bold font (SGR 1). Bold is a weight, it does not brighten.
pub const BOLD: u16 = 1 << 0;
/// Half intensity (SGR 2).
pub const FAINT: u16 = 1 << 1;
/// Drawn slanted (SGR 3).
pub const ITALIC: u16 = 1 << 2;
/// Underlined (SGR 4 and 21).
pub const UNDERLINE: u16 = 1 << 3;
/// Stored and not animated (SGR 5 and 6).
pub const BLINK: u16 = 1 << 4;
/// Foreground and background swapped (SGR 7).
pub const REVERSE: u16 = 1 << 5;
/// Drawn as blank (SGR 8).
pub const INVISIBLE: u16 = 1 << 6;
/// Struck through (SGR 9).
pub const STRIKE: u16 = 1 << 7;
/// The left half of a character two cells wide.
pub const WIDE: u16 = 1 << 8;
/// The right half of one: it holds no character of its own.
pub const TAIL: u16 = 1 << 9;
/// The attributes SGR sets, as opposed to the two layout bits.
pub const SGR_ATTRS: u16 = BOLD | FAINT | ITALIC | UNDERLINE | BLINK | REVERSE | INVISIBLE | STRIKE;

/// Where a colour's kind is kept in the attribute word: two bits each.
const FG_KIND: u16 = 10;
const BG_KIND: u16 = 12;

/// A colour a cell asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    /// `term.fg` or `term.bg`, whichever the place calls for.
    Default,
    /// Entries 0 to 15 are the `term.*` tokens' colours, 16 to 255 xterm's
    /// cube and grey ramp (a program may have redefined them with `OSC 4`).
    Index(u8),
    /// A direct colour (SGR 38 or 48 with `2;R;G;B`).
    Rgb(u8, u8, u8),
}

impl Colour {
    fn pack(self) -> (u16, [u8; 3]) {
        match self {
            Colour::Default => (0, [0; 3]),
            Colour::Index(i) => (1, [i, 0, 0]),
            Colour::Rgb(r, g, b) => (2, [r, g, b]),
        }
    }

    fn unpack(kind: u16, v: [u8; 3]) -> Colour {
        match kind {
            1 => Colour::Index(v[0]),
            2 => Colour::Rgb(v[0], v[1], v[2]),
            _ => Colour::Default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// Combining marks (width 0) joined to this cell, as BMP code points; 0
    /// is none. A mark outside the BMP, or a third, is dropped.
    marks: [u16; 2],
    attrs: u16,
    fg: [u8; 3],
    bg: [u8; 3],
}

impl Default for Cell {
    fn default() -> Cell {
        Cell::BLANK
    }
}

impl Cell {
    /// A space in the default colours with no attribute.
    pub const BLANK: Cell = Cell {
        ch: ' ',
        marks: [0; 2],
        attrs: 0,
        fg: [0; 3],
        bg: [0; 3],
    };

    /// `ch` in the default colours with no attribute.
    pub const fn new(ch: char) -> Cell {
        Cell { ch, ..Cell::BLANK }
    }

    /// A blank on background `bg`, as an erase leaves (the terminal has
    /// `bce`).
    pub fn erased(bg: Colour) -> Cell {
        Cell::BLANK.with_bg(bg)
    }

    pub fn fg(&self) -> Colour {
        Colour::unpack(self.attrs >> FG_KIND & 3, self.fg)
    }

    pub fn bg(&self) -> Colour {
        Colour::unpack(self.attrs >> BG_KIND & 3, self.bg)
    }

    pub fn with_fg(mut self, c: Colour) -> Cell {
        let (kind, v) = c.pack();
        self.attrs = self.attrs & !(3 << FG_KIND) | kind << FG_KIND;
        self.fg = v;
        self
    }

    pub fn with_bg(mut self, c: Colour) -> Cell {
        let (kind, v) = c.pack();
        self.attrs = self.attrs & !(3 << BG_KIND) | kind << BG_KIND;
        self.bg = v;
        self
    }

    /// The attribute bits (the constants of this module).
    pub const fn attrs(&self) -> u16 {
        self.attrs & 0x3ff
    }

    pub fn has(&self, flag: u16) -> bool {
        self.attrs & flag != 0
    }

    /// Replaces the attribute bits, leaving the colours.
    pub fn with_attrs(mut self, attrs: u16) -> Cell {
        self.attrs = self.attrs & !0x3ff | attrs & 0x3ff;
        self
    }

    pub fn bold(&self) -> bool {
        self.has(BOLD)
    }

    /// The combining marks joined to this cell.
    pub fn marks(&self) -> impl Iterator<Item = char> + '_ {
        self.marks
            .iter()
            .filter(|m| **m != 0)
            .filter_map(|m| char::from_u32(u32::from(*m)))
    }

    /// Joins a mark; false when it does not fit (outside the BMP, or the
    /// cell holds two already).
    pub fn push_mark(&mut self, mark: char) -> bool {
        let Ok(m) = u16::try_from(u32::from(mark)) else {
            return false;
        };
        match self.marks.iter_mut().find(|slot| **slot == 0) {
            Some(slot) if m != 0 => {
                *slot = m;
                true
            }
            _ => false,
        }
    }

    pub fn clear_marks(&mut self) {
        self.marks = [0; 2];
    }

    /// Whether the cell is drawn without showing its character: invisible,
    /// or the same colour asked for the foreground and the background
    /// (INV-0332-7). The default foreground and the default background are
    /// two colours; two entries that a palette makes alike are for the
    /// caller, which has the palette.
    pub fn hides_its_text(&self) -> bool {
        self.has(INVISIBLE) || (self.fg() == self.bg() && self.fg() != Colour::Default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_is_sixteen_bytes() {
        assert_eq!(size_of::<Cell>(), 16);
    }

    #[test]
    fn colours_and_attributes_do_not_disturb_each_other() {
        let c = Cell::new('x')
            .with_attrs(BOLD | STRIKE | WIDE)
            .with_fg(Colour::Rgb(1, 2, 3))
            .with_bg(Colour::Index(200));
        assert_eq!(c.fg(), Colour::Rgb(1, 2, 3));
        assert_eq!(c.bg(), Colour::Index(200));
        assert_eq!(c.attrs(), BOLD | STRIKE | WIDE);
        let c = c.with_fg(Colour::Default).with_attrs(0);
        assert_eq!(c.fg(), Colour::Default);
        assert_eq!(c.bg(), Colour::Index(200));
        assert_eq!(c.attrs(), 0);
        assert_eq!(Cell::BLANK.fg(), Colour::Default);
        assert_eq!(Cell::BLANK.bg(), Colour::Default);
    }

    #[test]
    fn two_marks_fit_and_no_more() {
        let mut c = Cell::new('e');
        assert!(c.push_mark('\u{301}'));
        assert!(c.push_mark('\u{323}'));
        assert!(!c.push_mark('\u{302}'), "a third is dropped");
        assert!(!Cell::new('e').push_mark('\u{e0100}'), "outside the BMP");
        assert!(!Cell::new('e').push_mark('\0'));
        assert_eq!(
            c.marks().collect::<alloc::vec::Vec<_>>(),
            ['\u{301}', '\u{323}']
        );
        c.clear_marks();
        assert_eq!(c.marks().count(), 0);
    }

    #[test]
    fn a_cell_hides_its_text_when_invisible_or_alike() {
        assert!(
            !Cell::new('x').hides_its_text(),
            "the default colours differ"
        );
        let c = Cell::new('x')
            .with_fg(Colour::Index(1))
            .with_bg(Colour::Index(1));
        assert!(c.hides_its_text());
        let c = Cell::new('x')
            .with_fg(Colour::Index(1))
            .with_bg(Colour::Index(2));
        assert!(!c.hides_its_text());
        assert!(c.with_attrs(INVISIBLE).hides_its_text());
    }
}
