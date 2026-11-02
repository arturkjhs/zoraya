//! Zoraya's own bitmap font (ADR-0095): printable ASCII in 5 × 9 dots,
//! drawn at twice their size in a 12 × 20 pixel cell. Every other character
//! is a hollow box. Each pixel is either the text colour or the background,
//! so a screen can be read back exactly with [`recognise`].

#![no_std]
#![forbid(unsafe_code)]

mod glyphs;

use glyphs::{BOX, COLUMNS, GLYPHS, ROWS};

pub const CELL_WIDTH: usize = 12;
pub const CELL_HEIGHT: usize = 20;
/// Pixels per dot, both ways.
const SCALE: usize = 2;
/// Where the first dot starts in the cell.
const MARGIN: usize = 1;

/// A glyph as rows of dots, bit 4 the leftmost.
pub type Glyph = [u8; ROWS];

const fn parse(rows: &[&str; ROWS]) -> Glyph {
    let mut out = [0; ROWS];
    let mut r = 0;
    while r < ROWS {
        let bytes = rows[r].as_bytes();
        assert!(bytes.len() == COLUMNS, "a glyph row is five dots");
        let mut c = 0;
        while c < COLUMNS {
            match bytes[c] {
                b'#' => out[r] |= 1 << (COLUMNS - 1 - c),
                b'.' => {}
                _ => panic!("a dot is # or ."),
            }
            c += 1;
        }
        r += 1;
    }
    out
}

const fn parse_all() -> [Glyph; 95] {
    let mut out = [[0; ROWS]; 95];
    let mut i = 0;
    while i < 95 {
        out[i] = parse(&GLYPHS[i]);
        i += 1;
    }
    out
}

static TABLE: [Glyph; 95] = parse_all();
static BOX_GLYPH: Glyph = parse(&BOX);

/// The glyph of `c`: its own for printable ASCII, the box otherwise.
pub fn glyph(c: char) -> &'static Glyph {
    match c {
        ' '..='~' => &TABLE[c as usize - 0x20],
        _ => &BOX_GLYPH,
    }
}

/// True if the cell pixel `(x, y)` is part of the glyph.
fn lit(g: &Glyph, x: usize, y: usize) -> bool {
    let (Some(x), Some(y)) = (x.checked_sub(MARGIN), y.checked_sub(MARGIN)) else {
        return false;
    };
    let (column, row) = (x / SCALE, y / SCALE);
    column < COLUMNS && row < ROWS && g[row] & (1 << (COLUMNS - 1 - column)) != 0
}

/// Draws `c` into `target`, rows of `stride` pixels, with the cell's top
/// left at `(x, y)`: every cell pixel `fg` or `bg`. What falls outside
/// `stride` or the slice is not drawn (INV-0095-1).
pub fn draw(target: &mut [u32], stride: usize, x: usize, y: usize, c: char, fg: u32, bg: u32) {
    let g = glyph(c);
    if stride == 0 {
        return;
    }
    let height = target.len() / stride;
    for dy in 0..CELL_HEIGHT {
        let Some(row) = y.checked_add(dy).filter(|r| *r < height) else {
            break;
        };
        for dx in 0..CELL_WIDTH {
            let Some(column) = x.checked_add(dx).filter(|c| *c < stride) else {
                break;
            };
            target[row * stride + column] = if lit(g, dx, dy) { fg } else { bg };
        }
    }
}

/// Reads a cell back: the printable character, or `' '` for an empty cell,
/// whose pixels are exactly what [`draw`] makes with some text colour on
/// `bg`. `pixel(x, y)` gives the cell's pixels. None if no glyph matches
/// exactly: the box, another colour, or noise.
pub fn recognise(pixel: impl Fn(usize, usize) -> u32, bg: u32) -> Option<char> {
    let mut fg = None;
    let mut dots: Glyph = [0; ROWS];
    for y in 0..CELL_HEIGHT {
        for x in 0..CELL_WIDTH {
            let p = pixel(x, y);
            if p == bg {
                continue;
            }
            match fg {
                None => fg = Some(p),
                Some(f) if f != p => return None,
                Some(_) => {}
            }
            let (Some(cx), Some(cy)) = (x.checked_sub(MARGIN), y.checked_sub(MARGIN)) else {
                return None;
            };
            let (column, row) = (cx / SCALE, cy / SCALE);
            if column >= COLUMNS || row >= ROWS {
                return None;
            }
            dots[row] |= 1 << (COLUMNS - 1 - column);
        }
    }
    let found = TABLE.iter().position(|g| *g == dots)?;
    // Every lit dot must be lit whole: compare pixel by pixel.
    let g = &TABLE[found];
    let fg = fg.unwrap_or(!bg);
    for y in 0..CELL_HEIGHT {
        for x in 0..CELL_WIDTH {
            let want = if lit(g, x, y) { fg } else { bg };
            if pixel(x, y) != want {
                return None;
            }
        }
    }
    char::from_u32(0x20 + found as u32)
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
