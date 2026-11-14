//! How many columns a code point takes (ADR-0332 item 9, INV-0332-4).
//!
//! The answers are musl's own `wcwidth`, asked for every code point at
//! generation time (`tools/term-width/gen.sh`): 0 where musl says nonspacing,
//! 2 where it says wide, 1 otherwise, and -1 where it says the code point has
//! no width at all (the controls and the noncharacters). The terminal's grid,
//! `zosh`'s line editor and every ported program (through the same musl)
//! therefore count columns alike. No grapheme clustering, no shaping, no
//! right-to-left layout.

#![no_std]
#![forbid(unsafe_code)]

mod table;

pub use table::RUNS;

/// musl's `wcwidth` for `c`: 0, 1, 2, or -1 for a code point with no width.
pub fn wcwidth(c: u32) -> i8 {
    // The last run that starts at or before `c`; the first run starts at 0.
    let at = RUNS.partition_point(|&(start, _)| start <= c);
    RUNS[at - 1].1
}

/// The columns of `c` in a grid: 0 (joins the cell before it), 1 or 2. A
/// code point musl gives no width (a control, a noncharacter) takes 1: it is
/// drawn as a box where it is drawn at all, and never breaks a layout.
pub fn columns(c: char) -> usize {
    match wcwidth(c as u32) {
        0 => 0,
        2 => 2,
        _ => 1,
    }
}

/// The columns of a string, as [`columns`] counts them.
pub fn str_columns(s: &str) -> usize {
    s.chars().map(columns).sum()
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runs_cover_every_code_point_in_order() {
        assert_eq!(RUNS[0].0, 0);
        assert!(RUNS.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 != w[1].1));
        assert!(
            RUNS.iter()
                .all(|&(s, w)| s <= 0x10ffff && (-1..=2).contains(&w))
        );
    }

    #[test]
    fn known_widths() {
        assert_eq!(wcwidth(0), 0);
        assert_eq!(wcwidth(1), -1);
        assert_eq!(wcwidth(u32::from('a')), 1);
        assert_eq!(wcwidth(0x7f), -1);
        assert_eq!(wcwidth(0x301), 0, "a combining acute");
        assert_eq!(wcwidth(0x4e2d), 2, "a CJK ideograph");
        assert_eq!(wcwidth(0x1f600), 2, "an emoji");
        assert_eq!(wcwidth(0x20000), 2, "plane 2");
        assert_eq!(wcwidth(0xe0100), 0, "a variation selector");
        assert_eq!(wcwidth(0xfffe), -1);
        assert_eq!(wcwidth(0x10ffff), -1);
        assert_eq!(wcwidth(0x10fffd), 1);
        assert_eq!(wcwidth(0x436), 1, "Cyrillic");
    }

    #[test]
    fn columns_map_no_width_to_one() {
        assert_eq!(columns('\x01'), 1);
        assert_eq!(columns('\u{fffe}'), 1);
        assert_eq!(columns('\u{301}'), 0);
        assert_eq!(columns('中'), 2);
        assert_eq!(str_columns("a中\u{301}b"), 4);
    }

    #[test]
    fn the_table_is_small() {
        assert!(RUNS.len() < 1000, "{}", RUNS.len());
    }
}

#[cfg(test)]
zoraya_test_alloc::cap_memory!();
