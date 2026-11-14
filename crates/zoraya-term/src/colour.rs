//! The 256-colour table and the colour specifications of `OSC 4`.

/// xterm's colour `i` for 16 to 255: the 6 × 6 × 6 cube, then 24 greys.
/// Entries 0 to 15 are the terminal's tokens and are not here.
pub fn xterm_rgb(i: u8) -> (u8, u8, u8) {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match i {
        16..=231 => {
            let n = usize::from(i - 16);
            (LEVELS[n / 36], LEVELS[n / 6 % 6], LEVELS[n % 6])
        }
        232..=255 => {
            let g = 8 + 10 * (i - 232);
            (g, g, g)
        }
        _ => (0, 0, 0),
    }
}

/// One to four hexadecimal digits as a value scaled to 8 bits.
fn hex_channel(s: &[u8]) -> Option<u8> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut v: u32 = 0;
    for &b in s {
        v = v << 4 | char::from(b).to_digit(16)?;
    }
    let max = (1u32 << (4 * s.len())) - 1;
    Some(((v * 255 + max / 2) / max) as u8)
}

/// A colour specification as `OSC 4` takes it: `rgb:R/G/B` with one to four
/// hexadecimal digits a channel, or `#RGB`, `#RRGGBB`, `#RRRGGGBBB`,
/// `#RRRRGGGGBBBB`. A query (`?`) is not a colour: nothing is answered.
pub fn parse_spec(s: &[u8]) -> Option<(u8, u8, u8)> {
    if let Some(rest) = s.strip_prefix(b"rgb:") {
        let mut parts = rest.split(|b| *b == b'/');
        let (r, g, b) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        return Some((hex_channel(r)?, hex_channel(g)?, hex_channel(b)?));
    }
    let digits = s.strip_prefix(b"#")?;
    if digits.is_empty() || digits.len() % 3 != 0 || digits.len() > 12 {
        return None;
    }
    let n = digits.len() / 3;
    Some((
        hex_channel(&digits[..n])?,
        hex_channel(&digits[n..2 * n])?,
        hex_channel(&digits[2 * n..])?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cube_and_the_greys() {
        assert_eq!(xterm_rgb(16), (0, 0, 0));
        assert_eq!(xterm_rgb(17), (0, 0, 95));
        assert_eq!(xterm_rgb(21), (0, 0, 255));
        assert_eq!(xterm_rgb(46), (0, 255, 0));
        assert_eq!(xterm_rgb(196), (255, 0, 0));
        assert_eq!(xterm_rgb(231), (255, 255, 255));
        assert_eq!(xterm_rgb(232), (8, 8, 8));
        assert_eq!(xterm_rgb(255), (238, 238, 238));
        assert_eq!(xterm_rgb(67), (95, 135, 175));
    }

    #[test]
    fn colour_specifications() {
        assert_eq!(parse_spec(b"rgb:ff/00/80"), Some((255, 0, 128)));
        assert_eq!(parse_spec(b"rgb:f/0/8"), Some((255, 0, 136)));
        assert_eq!(parse_spec(b"rgb:ffff/0000/8080"), Some((255, 0, 128)));
        assert_eq!(parse_spec(b"#ff0080"), Some((255, 0, 128)));
        assert_eq!(parse_spec(b"#f08"), Some((255, 0, 136)));
        assert_eq!(parse_spec(b"#fff000000"), Some((255, 0, 0)));
        assert_eq!(parse_spec(b"?"), None, "a query is not a colour");
        assert_eq!(parse_spec(b"rgb:ff/00"), None);
        assert_eq!(parse_spec(b"rgb:ff/00/00/00"), None);
        assert_eq!(parse_spec(b"rgb:fg/00/00"), None);
        assert_eq!(parse_spec(b"#ff"), None);
        assert_eq!(parse_spec(b"red"), None);
        assert_eq!(parse_spec(b"rgb:fffff/0/0"), None);
    }
}
