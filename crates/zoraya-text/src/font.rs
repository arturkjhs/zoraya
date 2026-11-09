//! The TrueType reader (ADR-0162): `head`, `hhea`, `hmtx`, `maxp`, `loca`,
//! `glyf` with composite glyphs, `cmap` formats 4 and 12, and `kern` format
//! 0. Total, in ADR-0012's discipline: the table directory is read once,
//! every offset and length is checked before use, and a glyph that cannot
//! be read is an error the caller turns into the box glyph. Nothing here
//! panics on any bytes, and nothing uses floating point.

use alloc::vec::Vec;

/// Why a font or a glyph cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not a TrueType font with `glyf` outlines.
    NotTrueType,
    /// A table the reader needs is missing.
    MissingTable([u8; 4]),
    /// An offset or a length points outside the data.
    OutOfRange,
    /// A value no font has (units per em of 0, an unknown `loca` format).
    BadValue,
    /// No `cmap` subtable of format 4 or 12 for Unicode.
    NoCmap,
    /// The glyph has more points or contours than ADR-0162 allows, or its
    /// composites nest deeper than 8.
    TooComplex,
}

/// ADR-0162's bounds on one glyph.
pub const MAX_POINTS: usize = 4096;
pub const MAX_CONTOURS: usize = 64;
pub const MAX_DEPTH: u32 = 8;
/// Components read for one glyph, all levels together: without it a
/// composite that names itself many times at each of eight levels would
/// take exponential time while adding no points.
pub const MAX_COMPONENTS: u32 = 256;

pub(crate) fn be16(d: &[u8], o: usize) -> Option<u16> {
    let b = d.get(o..o.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

pub(crate) fn be32(d: &[u8], o: usize) -> Option<u32> {
    let b = d.get(o..o.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// A table's place in the data, checked to lie inside it.
#[derive(Debug, Clone, Copy, Default)]
struct Table {
    offset: usize,
    length: usize,
}

/// A parsed font over borrowed bytes.
#[derive(Debug, Clone)]
pub struct Font<'a> {
    data: &'a [u8],
    glyf: Table,
    loca: Table,
    long_loca: bool,
    hmtx: Table,
    num_hmetrics: usize,
    num_glyphs: usize,
    /// The chosen `cmap` subtable: its format (4 or 12) and its offset.
    cmap: (u16, Table),
    /// `kern` format 0 pairs: their offset and count, if present.
    kern: Option<(usize, usize)>,
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
}

/// One point of an outline in font units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    pub on_curve: bool,
}

/// A glyph's outline: points, and where each contour ends (exclusive).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    pub points: Vec<Point>,
    pub ends: Vec<usize>,
}

impl Outline {
    pub fn clear(&mut self) {
        self.points.clear();
        self.ends.clear();
    }

    /// The contours, each a slice of points.
    pub fn contours(&self) -> impl Iterator<Item = &[Point]> {
        let mut start = 0;
        self.ends.iter().map(move |&end| {
            let c = self.points.get(start..end).unwrap_or(&[]);
            start = end;
            c
        })
    }
}

/// A 2×2 matrix in 2.14 fixed point and an offset in font units: a
/// composite component's placement.
#[derive(Debug, Clone, Copy)]
struct Transform {
    a: i32,
    b: i32,
    c: i32,
    d: i32,
    dx: i32,
    dy: i32,
}

const ONE_2_14: i32 = 1 << 14;

impl Transform {
    const IDENTITY: Transform = Transform {
        a: ONE_2_14,
        b: 0,
        c: 0,
        d: ONE_2_14,
        dx: 0,
        dy: 0,
    };

    /// 2.14 multiplication, rounded half away from zero, in i64.
    fn mul(x: i32, y: i32) -> i32 {
        let p = x as i64 * y as i64;
        let r = if p >= 0 {
            (p + (1 << 13)) >> 14
        } else {
            -((-p + (1 << 13)) >> 14)
        };
        r.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    fn apply(&self, x: i32, y: i32) -> (i32, i32) {
        let px = Transform::mul(self.a, x).saturating_add(Transform::mul(self.c, y));
        let py = Transform::mul(self.b, x).saturating_add(Transform::mul(self.d, y));
        (px.saturating_add(self.dx), py.saturating_add(self.dy))
    }

    /// `self` after `inner`: a child's transform within its parent.
    fn then(&self, inner: &Transform) -> Transform {
        let (dx, dy) = self.apply(inner.dx, inner.dy);
        Transform {
            a: Transform::mul(self.a, inner.a).saturating_add(Transform::mul(self.c, inner.b)),
            b: Transform::mul(self.b, inner.a).saturating_add(Transform::mul(self.d, inner.b)),
            c: Transform::mul(self.a, inner.c).saturating_add(Transform::mul(self.c, inner.d)),
            d: Transform::mul(self.b, inner.c).saturating_add(Transform::mul(self.d, inner.d)),
            dx,
            dy,
        }
    }
}

impl<'a> Font<'a> {
    /// Reads the table directory and the header tables.
    pub fn parse(data: &'a [u8]) -> Result<Font<'a>, Error> {
        let version = be32(data, 0).ok_or(Error::NotTrueType)?;
        if version != 0x0001_0000 && version != u32::from_be_bytes(*b"true") {
            return Err(Error::NotTrueType);
        }
        let count = be16(data, 4).ok_or(Error::OutOfRange)? as usize;
        let find = |tag: &[u8; 4]| -> Result<Table, Error> {
            for i in 0..count {
                let record = 12 + 16 * i;
                let t = data.get(record..record + 4).ok_or(Error::OutOfRange)?;
                if t == tag {
                    let offset = be32(data, record + 8).ok_or(Error::OutOfRange)? as usize;
                    let length = be32(data, record + 12).ok_or(Error::OutOfRange)? as usize;
                    let end = offset.checked_add(length).ok_or(Error::OutOfRange)?;
                    if end > data.len() {
                        return Err(Error::OutOfRange);
                    }
                    return Ok(Table { offset, length });
                }
            }
            Err(Error::MissingTable(*tag))
        };
        let in_table = |t: Table, at: usize, size: usize| -> Result<usize, Error> {
            if at.checked_add(size).is_some_and(|e| e <= t.length) {
                Ok(t.offset + at)
            } else {
                Err(Error::OutOfRange)
            }
        };
        let head = find(b"head")?;
        let units_per_em = be16(data, in_table(head, 18, 2)?).ok_or(Error::OutOfRange)?;
        if !(16..=16384).contains(&units_per_em) {
            return Err(Error::BadValue);
        }
        let long_loca = match be16(data, in_table(head, 50, 2)?).ok_or(Error::OutOfRange)? {
            0 => false,
            1 => true,
            _ => return Err(Error::BadValue),
        };
        let hhea = find(b"hhea")?;
        let ascent = be16(data, in_table(hhea, 4, 2)?).ok_or(Error::OutOfRange)? as i16;
        let descent = be16(data, in_table(hhea, 6, 2)?).ok_or(Error::OutOfRange)? as i16;
        let line_gap = be16(data, in_table(hhea, 8, 2)?).ok_or(Error::OutOfRange)? as i16;
        let num_hmetrics = be16(data, in_table(hhea, 34, 2)?).ok_or(Error::OutOfRange)? as usize;
        let maxp = find(b"maxp")?;
        let num_glyphs = be16(data, in_table(maxp, 4, 2)?).ok_or(Error::OutOfRange)? as usize;
        let hmtx = find(b"hmtx")?;
        if num_hmetrics == 0 || num_hmetrics.saturating_mul(4) > hmtx.length {
            return Err(Error::OutOfRange);
        }
        let loca = find(b"loca")?;
        let glyf = find(b"glyf")?;
        let cmap = choose_cmap(data, find(b"cmap")?)?;
        let kern = find(b"kern").ok().and_then(|k| kern_pairs(data, k));
        Ok(Font {
            data,
            glyf,
            loca,
            long_loca,
            hmtx,
            num_hmetrics,
            num_glyphs,
            cmap,
            kern,
            units_per_em,
            ascent,
            descent,
            line_gap,
        })
    }

    /// The font's glyph for `c`, or None (glyph 0 is "missing").
    pub fn glyph_index(&self, c: char) -> Option<u16> {
        let g = match self.cmap.0 {
            4 => cmap4(self.data, self.cmap.1, c as u32),
            _ => cmap12(self.data, self.cmap.1, c as u32),
        }?;
        (g != 0 && (g as usize) < self.num_glyphs).then_some(g)
    }

    /// The number of glyphs (`maxp`).
    pub fn glyph_count(&self) -> usize {
        self.num_glyphs
    }

    /// The advance width of `glyph` in font units.
    pub fn advance(&self, glyph: u16) -> u16 {
        let i = (glyph as usize).min(self.num_hmetrics - 1);
        be16(self.data, self.hmtx.offset + 4 * i).unwrap_or(0)
    }

    /// The `kern` format 0 adjustment between two glyphs, in font units.
    pub fn kerning(&self, left: u16, right: u16) -> i16 {
        let Some((offset, count)) = self.kern else {
            return 0;
        };
        let key = (left as u32) << 16 | right as u32;
        // Pairs are sorted by the key; a font that breaks this only kerns
        // wrongly, never reads out of bounds.
        let (mut lo, mut hi) = (0, count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let at = offset + 6 * mid;
            let Some(k) = be32(self.data, at) else {
                return 0;
            };
            match k.cmp(&key) {
                core::cmp::Ordering::Equal => {
                    return be16(self.data, at + 4).map_or(0, |v| v as i16);
                }
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
            }
        }
        0
    }

    /// Whether the font has `kern` pairs (ADR-0162's open question: Inter
    /// keeps its kerning in GPOS, which this reader does not read).
    pub fn has_kerning(&self) -> bool {
        self.kern.is_some()
    }

    /// The glyph's bytes inside `glyf`, or None for an empty glyph.
    fn glyph_bytes(&self, glyph: u16) -> Result<Option<&'a [u8]>, Error> {
        let g = glyph as usize;
        if g >= self.num_glyphs {
            return Err(Error::OutOfRange);
        }
        let (start, end) = if self.long_loca {
            let at = self.loca.offset + 4 * g;
            if 4 * g + 8 > self.loca.length {
                return Err(Error::OutOfRange);
            }
            (
                be32(self.data, at).ok_or(Error::OutOfRange)? as usize,
                be32(self.data, at + 4).ok_or(Error::OutOfRange)? as usize,
            )
        } else {
            let at = self.loca.offset + 2 * g;
            if 2 * g + 4 > self.loca.length {
                return Err(Error::OutOfRange);
            }
            (
                be16(self.data, at).ok_or(Error::OutOfRange)? as usize * 2,
                be16(self.data, at + 2).ok_or(Error::OutOfRange)? as usize * 2,
            )
        };
        if start == end {
            return Ok(None);
        }
        if start > end || end > self.glyf.length {
            return Err(Error::OutOfRange);
        }
        Ok(self
            .data
            .get(self.glyf.offset + start..self.glyf.offset + end))
    }

    /// The outline of `glyph` in font units, into `out` (cleared first).
    /// An empty glyph (a space) is an empty outline; one that cannot be
    /// read is an error, and `out` is then empty.
    pub fn outline(&self, glyph: u16, out: &mut Outline) -> Result<(), Error> {
        out.clear();
        let mut budget = MAX_COMPONENTS;
        let r = self.outline_into(glyph, out, &Transform::IDENTITY, 0, &mut budget);
        if r.is_err() {
            out.clear();
        }
        r
    }

    fn outline_into(
        &self,
        glyph: u16,
        out: &mut Outline,
        transform: &Transform,
        depth: u32,
        budget: &mut u32,
    ) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            return Err(Error::TooComplex);
        }
        let Some(d) = self.glyph_bytes(glyph)? else {
            return Ok(());
        };
        let contours = be16(d, 0).ok_or(Error::OutOfRange)? as i16;
        if contours >= 0 {
            simple(d, contours as usize, out, transform)
        } else {
            self.composite(d, out, transform, depth, budget)
        }
    }

    fn composite(
        &self,
        d: &[u8],
        out: &mut Outline,
        transform: &Transform,
        depth: u32,
        budget: &mut u32,
    ) -> Result<(), Error> {
        const ARGS_ARE_WORDS: u16 = 0x0001;
        const ARGS_ARE_XY: u16 = 0x0002;
        const SCALE: u16 = 0x0008;
        const MORE: u16 = 0x0020;
        const XY_SCALE: u16 = 0x0040;
        const TWO_BY_TWO: u16 = 0x0080;
        let mut p = 10;
        // Components are bounded by the bytes; each takes at least four.
        loop {
            *budget = budget.checked_sub(1).ok_or(Error::TooComplex)?;
            let flags = be16(d, p).ok_or(Error::OutOfRange)?;
            let index = be16(d, p + 2).ok_or(Error::OutOfRange)?;
            p += 4;
            let (arg1, arg2) = if flags & ARGS_ARE_WORDS != 0 {
                let a = be16(d, p).ok_or(Error::OutOfRange)? as i16 as i32;
                let b = be16(d, p + 2).ok_or(Error::OutOfRange)? as i16 as i32;
                p += 4;
                (a, b)
            } else {
                let a = *d.get(p).ok_or(Error::OutOfRange)? as i8 as i32;
                let b = *d.get(p + 1).ok_or(Error::OutOfRange)? as i8 as i32;
                p += 2;
                (a, b)
            };
            let f2 = |o: usize| be16(d, o).map(|v| v as i16 as i32).ok_or(Error::OutOfRange);
            let mut inner = Transform::IDENTITY;
            if flags & SCALE != 0 {
                let s = f2(p)?;
                p += 2;
                inner.a = s;
                inner.d = s;
            } else if flags & XY_SCALE != 0 {
                inner.a = f2(p)?;
                inner.d = f2(p + 2)?;
                p += 4;
            } else if flags & TWO_BY_TWO != 0 {
                inner.a = f2(p)?;
                inner.b = f2(p + 2)?;
                inner.c = f2(p + 4)?;
                inner.d = f2(p + 6)?;
                p += 8;
            }
            // Point matching (offsets as point numbers) is not supported:
            // the component is placed at the origin, readable if not exact.
            if flags & ARGS_ARE_XY != 0 {
                inner.dx = arg1;
                inner.dy = arg2;
            }
            let combined = transform.then(&inner);
            self.outline_into(index, out, &combined, depth + 1, budget)?;
            if flags & MORE == 0 {
                return Ok(());
            }
        }
    }
}

/// A simple glyph's points and contours, appended to `out`.
fn simple(d: &[u8], contours: usize, out: &mut Outline, t: &Transform) -> Result<(), Error> {
    if contours == 0 {
        return Ok(());
    }
    if out.ends.len() + contours > MAX_CONTOURS {
        return Err(Error::TooComplex);
    }
    let mut p = 10;
    let base = out.points.len();
    let mut last_end: Option<usize> = None;
    let mut ends = [0usize; MAX_CONTOURS];
    for e in ends.iter_mut().take(contours) {
        let end = be16(d, p).ok_or(Error::OutOfRange)? as usize;
        p += 2;
        if last_end.is_some_and(|l| end <= l) {
            return Err(Error::BadValue);
        }
        last_end = Some(end);
        *e = end + 1;
    }
    let count = last_end.map_or(0, |e| e + 1);
    if base + count > MAX_POINTS {
        return Err(Error::TooComplex);
    }
    let instructions = be16(d, p).ok_or(Error::OutOfRange)? as usize;
    p = p.checked_add(2 + instructions).ok_or(Error::OutOfRange)?;
    // Flags first, then the x and y deltas.
    let first_flag = out.points.len();
    while out.points.len() - first_flag < count {
        let flag = *d.get(p).ok_or(Error::OutOfRange)?;
        p += 1;
        let repeat = if flag & 8 != 0 {
            let r = *d.get(p).ok_or(Error::OutOfRange)?;
            p += 1;
            r as usize + 1
        } else {
            1
        };
        for _ in 0..repeat {
            if out.points.len() - first_flag == count {
                break;
            }
            // The flag rides in `y` until the coordinates are read.
            out.points.push(Point {
                x: 0,
                y: flag as i32,
                on_curve: flag & 1 != 0,
            });
        }
    }
    let mut v: i32 = 0;
    for point in &mut out.points[first_flag..] {
        let flag = point.y as u8;
        if flag & 2 != 0 {
            let b = *d.get(p).ok_or(Error::OutOfRange)? as i32;
            p += 1;
            v += if flag & 16 != 0 { b } else { -b };
        } else if flag & 16 == 0 {
            v += be16(d, p).ok_or(Error::OutOfRange)? as i16 as i32;
            p += 2;
        }
        point.x = v;
    }
    v = 0;
    for point in &mut out.points[first_flag..] {
        let flag = point.y as u8;
        if flag & 4 != 0 {
            let b = *d.get(p).ok_or(Error::OutOfRange)? as i32;
            p += 1;
            v += if flag & 32 != 0 { b } else { -b };
        } else if flag & 32 == 0 {
            v += be16(d, p).ok_or(Error::OutOfRange)? as i16 as i32;
            p += 2;
        }
        point.y = v;
    }
    for point in &mut out.points[first_flag..] {
        let (x, y) = t.apply(point.x, point.y);
        point.x = x;
        point.y = y;
    }
    for &e in &ends[..contours] {
        out.ends.push(base + e);
    }
    Ok(())
}

/// The best Unicode subtable: format 12 over format 4.
fn choose_cmap(d: &[u8], t: Table) -> Result<(u16, Table), Error> {
    let count = be16(d, t.offset + 2).ok_or(Error::OutOfRange)? as usize;
    let mut best: Option<(u16, Table)> = None;
    for i in 0..count {
        let record = 4 + 8 * i;
        if record + 8 > t.length {
            return Err(Error::OutOfRange);
        }
        let platform = be16(d, t.offset + record).ok_or(Error::OutOfRange)?;
        let encoding = be16(d, t.offset + record + 2).ok_or(Error::OutOfRange)?;
        let at = be32(d, t.offset + record + 4).ok_or(Error::OutOfRange)? as usize;
        let unicode = platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10));
        if !unicode || at >= t.length {
            continue;
        }
        let Some(format) = be16(d, t.offset + at) else {
            continue;
        };
        if format != 4 && format != 12 {
            continue;
        }
        let sub = Table {
            offset: t.offset + at,
            length: t.length - at,
        };
        if best.is_none_or(|(f, _)| f == 4 && format == 12) {
            best = Some((format, sub));
        }
    }
    best.ok_or(Error::NoCmap)
}

fn cmap4(d: &[u8], t: Table, c: u32) -> Option<u16> {
    if c > 0xFFFF {
        return None;
    }
    let seg2 = be16(d, t.offset + 6)? as usize;
    let segments = seg2 / 2;
    let ends = t.offset + 14;
    let starts = ends + seg2 + 2;
    let deltas = starts + seg2;
    let ranges = deltas + seg2;
    if 16 + 4 * seg2 > t.length {
        return None;
    }
    // The first segment whose end is at least `c` (ends are sorted).
    let (mut lo, mut hi) = (0, segments);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if (be16(d, ends + 2 * mid)? as u32) < c {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == segments {
        return None;
    }
    let i = lo;
    let start = be16(d, starts + 2 * i)? as u32;
    if c < start {
        return None;
    }
    let delta = be16(d, deltas + 2 * i)?;
    let range = be16(d, ranges + 2 * i)? as usize;
    if range == 0 {
        return Some((c as u16).wrapping_add(delta));
    }
    let at = ranges + 2 * i + range + 2 * (c - start) as usize;
    if at + 2 > t.offset + t.length {
        return None;
    }
    match be16(d, at)? {
        0 => None,
        g => Some(g.wrapping_add(delta)),
    }
}

fn cmap12(d: &[u8], t: Table, c: u32) -> Option<u16> {
    let groups = be32(d, t.offset + 12)? as usize;
    if groups.checked_mul(12)?.checked_add(16)? > t.length {
        return None;
    }
    let (mut lo, mut hi) = (0, groups);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let at = t.offset + 16 + 12 * mid;
        let first = be32(d, at)?;
        let last = be32(d, at + 4)?;
        if c < first {
            hi = mid;
        } else if c > last {
            lo = mid + 1;
        } else {
            let g = be32(d, at + 8)?.checked_add(c - first)?;
            return u16::try_from(g).ok();
        }
    }
    None
}

/// The first horizontal format 0 subtable of a version 0 `kern` table.
fn kern_pairs(d: &[u8], t: Table) -> Option<(usize, usize)> {
    if be16(d, t.offset)? != 0 {
        return None;
    }
    let tables = be16(d, t.offset + 2)? as usize;
    let mut at = 4;
    for _ in 0..tables {
        let length = be16(d, t.offset + at + 2)? as usize;
        let coverage = be16(d, t.offset + at + 4)?;
        let format = coverage >> 8;
        let horizontal = coverage & 1 != 0;
        if format == 0 && horizontal && coverage & 0x4 == 0 {
            let pairs = be16(d, t.offset + at + 6)? as usize;
            let first = at + 14;
            if first + 6 * pairs > t.length {
                return None;
            }
            return Some((t.offset + first, pairs));
        }
        if length < 6 {
            return None;
        }
        at += length;
    }
    None
}
