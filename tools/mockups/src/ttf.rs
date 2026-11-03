//! A small TrueType reader: the tables a `glyf` font needs for rendering
//! Latin and Cyrillic text (`head`, `hhea`, `hmtx`, `maxp`, `loca`, `glyf`,
//! `cmap` formats 4 and 12, and `kern` format 0). Total: every offset is
//! checked and a bad glyph is an empty outline, never a panic (the
//! discipline ADR-0162 asks of `zoraya-text`; this is the illustration).

use std::collections::HashMap;

/// One point of a flattened outline, in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub on_curve: bool,
}

/// A glyph outline: closed contours of quadratic segments, plus metrics.
#[derive(Clone, Debug, Default)]
pub struct Outline {
    pub contours: Vec<Vec<Point>>,
}

pub struct Font {
    data: Vec<u8>,
    loca: (usize, usize, bool), // offset, count+1, long
    glyf: (usize, usize),
    hmtx: (usize, usize),
    num_hmetrics: usize,
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    cmap: HashMap<u32, u16>,
    kern: HashMap<(u16, u16), i16>,
}

fn be16(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
}
fn be32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *d.get(o)?,
        *d.get(o + 1)?,
        *d.get(o + 2)?,
        *d.get(o + 3)?,
    ]))
}

impl Font {
    /// Parses a `glyf` font. Returns a message for anything it cannot use.
    pub fn parse(data: Vec<u8>) -> Result<Font, String> {
        let tag = be32(&data, 0).ok_or("short")?;
        if tag != 0x0001_0000 && tag != u32::from_be_bytes(*b"true") {
            return Err(format!("not a TrueType glyf font (tag {tag:#x})"));
        }
        let n = be16(&data, 4).ok_or("short")? as usize;
        let mut tables: HashMap<[u8; 4], (usize, usize)> = HashMap::new();
        for i in 0..n {
            let r = 12 + 16 * i;
            let t = data.get(r..r + 4).ok_or("short directory")?;
            let off = be32(&data, r + 8).ok_or("short")? as usize;
            let len = be32(&data, r + 12).ok_or("short")? as usize;
            if off.checked_add(len).is_none_or(|e| e > data.len()) {
                return Err(format!("table {} out of range", String::from_utf8_lossy(t)));
            }
            tables.insert([t[0], t[1], t[2], t[3]], (off, len));
        }
        let get = |name: &[u8; 4]| {
            tables
                .get(name)
                .copied()
                .ok_or(format!("no {} table", String::from_utf8_lossy(name)))
        };
        let head = get(b"head")?;
        let units_per_em = be16(&data, head.0 + 18).ok_or("head")?;
        let long_loca = be16(&data, head.0 + 50).ok_or("head")? != 0;
        let hhea = get(b"hhea")?;
        let ascent = be16(&data, hhea.0 + 4).ok_or("hhea")? as i16;
        let descent = be16(&data, hhea.0 + 6).ok_or("hhea")? as i16;
        let line_gap = be16(&data, hhea.0 + 8).ok_or("hhea")? as i16;
        let num_hmetrics = be16(&data, hhea.0 + 34).ok_or("hhea")? as usize;
        let maxp = get(b"maxp")?;
        let num_glyphs = be16(&data, maxp.0 + 4).ok_or("maxp")? as usize;
        let loca = get(b"loca")?;
        let entry = if long_loca { 4 } else { 2 };
        let count = (loca.1 / entry).min(num_glyphs + 1);
        let glyf = get(b"glyf")?;
        let hmtx = get(b"hmtx")?;
        let cmap = parse_cmap(&data, get(b"cmap")?)?;
        let kern = tables
            .get(b"kern")
            .map(|k| parse_kern(&data, *k))
            .unwrap_or_default();
        Ok(Font {
            data,
            loca: (loca.0, count, long_loca),
            glyf,
            hmtx,
            num_hmetrics,
            units_per_em,
            ascent,
            descent,
            line_gap,
            cmap,
            kern,
        })
    }

    pub fn glyph_index(&self, c: char) -> Option<u16> {
        self.cmap.get(&(c as u32)).copied().filter(|&g| g != 0)
    }

    pub fn has(&self, c: char) -> bool {
        self.glyph_index(c).is_some()
    }

    /// Advance width in font units.
    pub fn advance(&self, glyph: u16) -> u16 {
        let i = (glyph as usize).min(self.num_hmetrics.saturating_sub(1));
        be16(&self.data, self.hmtx.0 + 4 * i).unwrap_or(0)
    }

    pub fn kerning(&self, left: u16, right: u16) -> i16 {
        self.kern.get(&(left, right)).copied().unwrap_or(0)
    }

    fn glyph_range(&self, glyph: u16) -> Option<(usize, usize)> {
        let (off, count, long) = self.loca;
        let g = glyph as usize;
        if g + 1 >= count {
            return None;
        }
        let (a, b) = if long {
            (
                be32(&self.data, off + 4 * g)? as usize,
                be32(&self.data, off + 4 * g + 4)? as usize,
            )
        } else {
            (
                be16(&self.data, off + 2 * g)? as usize * 2,
                be16(&self.data, off + 2 * g + 2)? as usize * 2,
            )
        };
        if b <= a || b > self.glyf.1 {
            return None;
        }
        Some((self.glyf.0 + a, self.glyf.0 + b))
    }

    /// The outline of a glyph in font units; empty for a space, a missing
    /// glyph or a damaged one.
    pub fn outline(&self, glyph: u16) -> Outline {
        let mut out = Outline::default();
        self.outline_into(glyph, &mut out, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0);
        out
    }

    fn outline_into(&self, glyph: u16, out: &mut Outline, xf: [f32; 6], depth: u32) {
        if depth > 8 {
            return;
        }
        let Some((start, end)) = self.glyph_range(glyph) else {
            return;
        };
        let d = &self.data[..end];
        let Some(n_contours) = be16(d, start) else {
            return;
        };
        let n_contours = n_contours as i16;
        let apply = |x: f32, y: f32| (xf[0] * x + xf[2] * y + xf[4], xf[1] * x + xf[3] * y + xf[5]);
        if n_contours >= 0 {
            let n = n_contours as usize;
            let mut ends = Vec::with_capacity(n);
            let mut p = start + 10;
            for _ in 0..n {
                let Some(e) = be16(d, p) else { return };
                ends.push(e as usize);
                p += 2;
            }
            let n_points = ends.last().map_or(0, |e| e + 1);
            if n_points > 4096 {
                return;
            }
            let Some(instr_len) = be16(d, p) else { return };
            p += 2 + instr_len as usize;
            // flags
            let mut flags = Vec::with_capacity(n_points);
            while flags.len() < n_points {
                let Some(&f) = d.get(p) else { return };
                p += 1;
                flags.push(f);
                if f & 8 != 0 {
                    let Some(&r) = d.get(p) else { return };
                    p += 1;
                    for _ in 0..r {
                        flags.push(f);
                    }
                }
            }
            flags.truncate(n_points);
            let mut xs = Vec::with_capacity(n_points);
            let mut v: i32 = 0;
            for &f in &flags {
                if f & 2 != 0 {
                    let Some(&b) = d.get(p) else { return };
                    p += 1;
                    v += if f & 16 != 0 { b as i32 } else { -(b as i32) };
                } else if f & 16 == 0 {
                    let Some(w) = be16(d, p) else { return };
                    p += 2;
                    v += w as i16 as i32;
                }
                xs.push(v);
            }
            let mut ys = Vec::with_capacity(n_points);
            v = 0;
            for &f in &flags {
                if f & 4 != 0 {
                    let Some(&b) = d.get(p) else { return };
                    p += 1;
                    v += if f & 32 != 0 { b as i32 } else { -(b as i32) };
                } else if f & 32 == 0 {
                    let Some(w) = be16(d, p) else { return };
                    p += 2;
                    v += w as i16 as i32;
                }
                ys.push(v);
            }
            let mut first = 0;
            for &e in &ends {
                if e < first || e >= n_points {
                    return;
                }
                let mut contour = Vec::with_capacity(e - first + 1);
                for i in first..=e {
                    let (x, y) = apply(xs[i] as f32, ys[i] as f32);
                    contour.push(Point {
                        x,
                        y,
                        on_curve: flags[i] & 1 != 0,
                    });
                }
                out.contours.push(contour);
                first = e + 1;
            }
        } else {
            // composite
            let mut p = start + 10;
            loop {
                let (Some(flags), Some(index)) = (be16(d, p), be16(d, p + 2)) else {
                    return;
                };
                p += 4;
                let (dx, dy);
                if flags & 1 != 0 {
                    let (Some(a), Some(b)) = (be16(d, p), be16(d, p + 2)) else {
                        return;
                    };
                    dx = a as i16 as f32;
                    dy = b as i16 as f32;
                    p += 4;
                } else {
                    let (Some(&a), Some(&b)) = (d.get(p), d.get(p + 1)) else {
                        return;
                    };
                    dx = a as i8 as f32;
                    dy = b as i8 as f32;
                    p += 2;
                }
                let f2 = |o: usize| be16(d, o).map(|v| v as i16 as f32 / 16384.0);
                let mut sub = [1.0, 0.0, 0.0, 1.0, dx, dy];
                if flags & 8 != 0 {
                    let Some(s) = f2(p) else { return };
                    p += 2;
                    sub[0] = s;
                    sub[3] = s;
                } else if flags & 0x40 != 0 {
                    let (Some(a), Some(b)) = (f2(p), f2(p + 2)) else {
                        return;
                    };
                    p += 4;
                    sub[0] = a;
                    sub[3] = b;
                } else if flags & 0x80 != 0 {
                    let (Some(a), Some(b), Some(c), Some(e)) =
                        (f2(p), f2(p + 2), f2(p + 4), f2(p + 6))
                    else {
                        return;
                    };
                    p += 8;
                    sub = [a, b, c, e, dx, dy];
                }
                if flags & 2 == 0 {
                    // point-matching placement is not supported: place at origin
                    sub[4] = 0.0;
                    sub[5] = 0.0;
                }
                // compose: child then parent transform
                let m = [
                    xf[0] * sub[0] + xf[2] * sub[1],
                    xf[1] * sub[0] + xf[3] * sub[1],
                    xf[0] * sub[2] + xf[2] * sub[3],
                    xf[1] * sub[2] + xf[3] * sub[3],
                    xf[0] * sub[4] + xf[2] * sub[5] + xf[4],
                    xf[1] * sub[4] + xf[3] * sub[5] + xf[5],
                ];
                self.outline_into(index, out, m, depth + 1);
                if flags & 0x20 == 0 {
                    break;
                }
            }
        }
    }
}

fn parse_cmap(d: &[u8], (off, len): (usize, usize)) -> Result<HashMap<u32, u16>, String> {
    let n = be16(d, off + 2).ok_or("cmap")? as usize;
    let mut best: Option<(u16, usize)> = None;
    for i in 0..n {
        let r = off + 4 + 8 * i;
        let pid = be16(d, r).ok_or("cmap")?;
        let so = be32(d, r + 4).ok_or("cmap")? as usize;
        if so >= len {
            continue;
        }
        let fmt = be16(d, off + so).ok_or("cmap")?;
        if (pid == 3 || pid == 0) && (fmt == 4 || fmt == 12) && (best.is_none() || fmt == 12) {
            best = Some((fmt, off + so));
        }
    }
    let (fmt, so) = best.ok_or("no usable cmap subtable")?;
    let mut map = HashMap::new();
    if fmt == 4 {
        let segx2 = be16(d, so + 6).ok_or("cmap4")? as usize;
        let seg = segx2 / 2;
        let ends = so + 14;
        let starts = ends + segx2 + 2;
        let deltas = starts + segx2;
        let ranges = deltas + segx2;
        for i in 0..seg {
            let end = be16(d, ends + 2 * i).ok_or("cmap4")? as u32;
            let start = be16(d, starts + 2 * i).ok_or("cmap4")? as u32;
            let delta = be16(d, deltas + 2 * i).ok_or("cmap4")?;
            let ro = be16(d, ranges + 2 * i).ok_or("cmap4")? as usize;
            if start > end || end == 0xFFFF && start == 0xFFFF {
                continue;
            }
            for c in start..=end.min(0xFFFE) {
                let g = if ro == 0 {
                    (c as u16).wrapping_add(delta)
                } else {
                    let gi = ranges + 2 * i + ro + 2 * (c - start) as usize;
                    match be16(d, gi) {
                        Some(0) | None => 0,
                        Some(g) => g.wrapping_add(delta),
                    }
                };
                if g != 0 {
                    map.insert(c, g);
                }
            }
        }
    } else {
        let ngroups = be32(d, so + 12).ok_or("cmap12")? as usize;
        for i in 0..ngroups.min(100_000) {
            let r = so + 16 + 12 * i;
            let a = be32(d, r).ok_or("cmap12")?;
            let b = be32(d, r + 4).ok_or("cmap12")?;
            let g = be32(d, r + 8).ok_or("cmap12")?;
            if b < a || b - a > 0xFFFF {
                continue;
            }
            for (k, c) in (a..=b).enumerate() {
                let gi = g as usize + k;
                if gi != 0 && gi <= 0xFFFF {
                    map.insert(c, gi as u16);
                }
            }
        }
    }
    Ok(map)
}

fn parse_kern(d: &[u8], (off, _len): (usize, usize)) -> HashMap<(u16, u16), i16> {
    let mut map = HashMap::new();
    let Some(n) = be16(d, off + 2) else {
        return map;
    };
    let mut p = off + 4;
    for _ in 0..n {
        let (Some(len), Some(cov)) = (be16(d, p + 2), be16(d, p + 4)) else {
            return map;
        };
        if cov & 0xFF00 == 0 && cov & 1 != 0 {
            let Some(pairs) = be16(d, p + 6) else {
                return map;
            };
            let mut q = p + 14;
            for _ in 0..pairs {
                let (Some(l), Some(r), Some(v)) = (be16(d, q), be16(d, q + 2), be16(d, q + 4))
                else {
                    return map;
                };
                map.insert((l, r), v as i16);
                q += 6;
            }
        }
        p += (len as usize).max(6);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(tag: &[u8; 4], data: &[u8]) -> ([u8; 4], Vec<u8>) {
        let mut d = data.to_vec();
        while !d.len().is_multiple_of(4) {
            d.push(0);
        }
        (*tag, d)
    }

    /// A font with one glyph, a triangle, mapped from 'A' (cmap format 4).
    fn tiny_font() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
        head[50..52].copy_from_slice(&0u16.to_be_bytes()); // short loca
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&800u16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());
        let mut maxp = vec![0u8; 6];
        maxp[4..6].copy_from_slice(&2u16.to_be_bytes());
        let hmtx: Vec<u8> = [500u16, 0, 600, 0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        // glyph 0 empty, glyph 1 a triangle (0,0) (600,0) (300,700)
        let mut glyph = Vec::new();
        glyph.extend_from_slice(&1i16.to_be_bytes()); // contours
        glyph.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // bbox
        glyph.extend_from_slice(&2u16.to_be_bytes()); // end point
        glyph.extend_from_slice(&0u16.to_be_bytes()); // instructions
        glyph.extend_from_slice(&[1, 1, 1]); // flags: on curve, 16-bit coords
        for v in [0i16, 600, -300] {
            glyph.extend_from_slice(&v.to_be_bytes());
        }
        for v in [0i16, 0, 700] {
            glyph.extend_from_slice(&v.to_be_bytes());
        }
        if !glyph.len().is_multiple_of(2) {
            glyph.push(0); // short loca entries are in words
        }
        let loca: Vec<u8> = [0u16, 0, (glyph.len() / 2) as u16]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        // cmap: one format 4 subtable mapping 'A' to glyph 1 (delta 1 - 65)
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&32u16.to_be_bytes()); // length
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&4u16.to_be_bytes()); // segCountX2
        sub.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        sub.extend_from_slice(&65u16.to_be_bytes()); // end A
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes()); // reserved pad
        sub.extend_from_slice(&65u16.to_be_bytes()); // start A
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&((1i16 - 65) as u16).to_be_bytes()); // delta
        sub.extend_from_slice(&1u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes()); // range offsets
        sub.extend_from_slice(&0u16.to_be_bytes());
        let mut cmap = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes());
        cmap.extend_from_slice(&1u16.to_be_bytes());
        cmap.extend_from_slice(&3u16.to_be_bytes());
        cmap.extend_from_slice(&1u16.to_be_bytes());
        cmap.extend_from_slice(&12u32.to_be_bytes());
        cmap.extend_from_slice(&sub);
        let tables = [
            table(b"cmap", &cmap),
            table(b"glyf", &glyph),
            table(b"head", &head),
            table(b"hhea", &hhea),
            table(b"hmtx", &hmtx),
            table(b"loca", &loca),
            table(b"maxp", &maxp),
        ];
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut off = 12 + 16 * tables.len();
        for (tag, d) in &tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&(off as u32).to_be_bytes());
            out.extend_from_slice(&(d.len() as u32).to_be_bytes());
            off += d.len();
        }
        for (_, d) in &tables {
            out.extend_from_slice(d);
        }
        out
    }

    #[test]
    fn reads_metrics_cmap_and_outline() {
        let f = Font::parse(tiny_font()).unwrap();
        assert_eq!(f.units_per_em, 1000);
        assert_eq!(f.ascent, 800);
        assert_eq!(f.glyph_index('A'), Some(1));
        assert_eq!(f.glyph_index('B'), None);
        assert_eq!(f.advance(1), 600);
        let o = f.outline(1);
        assert_eq!(o.contours.len(), 1);
        let pts: Vec<(f32, f32)> = o.contours[0].iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(pts, [(0.0, 0.0), (600.0, 0.0), (300.0, 700.0)]);
        assert!(f.outline(0).contours.is_empty());
        assert!(f.outline(7).contours.is_empty());
    }

    #[test]
    fn truncated_and_mutated_fonts_never_panic() {
        let font = tiny_font();
        for cut in 0..font.len() {
            let _ = Font::parse(font[..cut].to_vec());
        }
        let mut seed = 12345u32;
        for _ in 0..2000 {
            let mut m = font.clone();
            for _ in 0..3 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let i = (seed >> 8) as usize % m.len();
                m[i] = (seed >> 24) as u8;
            }
            if let Ok(f) = Font::parse(m) {
                let _ = f.outline(1);
                let _ = f.advance(1);
                let _ = f.glyph_index('A');
            }
        }
    }

    #[test]
    fn a_triangle_rasterises_inside_its_box() {
        let f = Font::parse(tiny_font()).unwrap();
        let bm = crate::raster::render_glyph(&f, 1, 20.0);
        assert!(
            bm.width >= 12 && bm.height >= 14,
            "{} x {}",
            bm.width,
            bm.height
        );
        let total: u32 = bm.coverage.iter().map(|&c| c as u32).sum();
        // area of the triangle at 20 px/em: 0.5 * 12 * 14 = 84 pixels of coverage
        assert!((70..100).contains(&(total / 255)), "{}", total / 255);
    }
}
