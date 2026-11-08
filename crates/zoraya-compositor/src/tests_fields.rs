use alloc::vec;
use alloc::vec::Vec;

use zoraya_tokens::LIGHT;

use crate::fields::*;
use crate::rect::Rect;

const SIZES: Sizes = Sizes {
    padding: 24,
    gap: 12,
    radius: 12,
};
const W: i64 = 400;
const H: i64 = 300;
/// Between a 28-pixel bar and a 70-pixel dock strip.
const WORK: Rect = Rect::new(0, 28, W, H - 28 - 70);

struct Rng(u64);

impl Rng {
    fn next(&mut self, n: i64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as i64
    }
}

fn which(shapes: &[Shape], rooms: usize, x: i64, y: i64) -> Vec<usize> {
    (0..rooms)
        .filter(|r| in_field(shapes, *r, WORK, SIZES, x, y))
        .collect()
}

/// The exact distance check, in floating point, the slow way.
fn distance(r: &Rect, x: i64, y: i64) -> f64 {
    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
    let dx = (r.x as f64 - px).max(px - r.right() as f64).max(0.0);
    let dy = (r.y as f64 - py).max(py - r.bottom() as f64).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

#[test]
fn fields_keep_their_distance_over_random_windows() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..40 {
        let rooms = 2 + rng.next(2) as usize;
        let n = 2 + rng.next(4) as usize;
        let shapes: Vec<Shape> = (0..n)
            .map(|i| Shape {
                room: i % rooms,
                rect: Rect::new(
                    rng.next(W) - 40,
                    rng.next(H) - 40,
                    20 + rng.next(160),
                    20 + rng.next(120),
                ),
            })
            .collect();
        let mut owner = vec![None; (W * H) as usize];
        for y in 0..H {
            for x in 0..W {
                let w = which(&shapes, rooms, x, y);
                // INV-0172-8: no pixel in two fields.
                assert!(w.len() <= 1, "({x}, {y}) in {w:?}");
                if let Some(&r) = w.first() {
                    // INV-0172-7: within the padding of one of its windows,
                    // and never in the bar's or the dock's strip.
                    assert!(WORK.contains(x, y));
                    let near = shapes
                        .iter()
                        .filter(|s| s.room == r)
                        .any(|s| distance(&s.rect, x, y) <= SIZES.padding as f64 + 0.01);
                    assert!(near, "({x}, {y}) of room {r} is far from its windows");
                    owner[(y * W + x) as usize] = Some(r);
                }
            }
        }
        // INV-0172-8: at least `gap` pixels of neither between two fields,
        // along rows and columns.
        let gap = SIZES.gap as usize;
        for y in 0..H as usize {
            let row: Vec<Option<usize>> =
                (0..W as usize).map(|x| owner[y * W as usize + x]).collect();
            seams(&row, gap);
        }
        for x in 0..W as usize {
            let col: Vec<Option<usize>> =
                (0..H as usize).map(|y| owner[y * W as usize + x]).collect();
            seams(&col, gap);
        }
    }
}

fn seams(line: &[Option<usize>], gap: usize) {
    let mut last: Option<(usize, usize)> = None;
    for (i, o) in line.iter().enumerate() {
        if let Some(r) = o {
            if let Some((j, q)) = last
                && q != *r
            {
                assert!(i - j > gap, "a seam of {} between {q} and {r}", i - j - 1);
            }
            last = Some((i, *r));
        }
    }
}

#[test]
fn a_room_alone_is_its_padded_windows_and_none_is_empty() {
    let win = Rect::new(100, 100, 80, 60);
    let shapes = [Shape { room: 0, rect: win }];
    // Inside, on the padding's edge straight out, and past it.
    assert!(in_field(&shapes, 0, WORK, SIZES, 140, 130));
    assert!(in_field(&shapes, 0, WORK, SIZES, 100 - 24, 130));
    assert!(!in_field(&shapes, 0, WORK, SIZES, 100 - 25, 130));
    assert!(in_field(&shapes, 0, WORK, SIZES, 180 + 23, 130));
    assert!(!in_field(&shapes, 0, WORK, SIZES, 180 + 24, 130));
    // The corner is rounded: radius plus padding.
    assert!(!in_field(&shapes, 0, WORK, SIZES, 100 - 22, 100 - 22));
    assert!(in_field(&shapes, 0, WORK, SIZES, 100 - 10, 100 - 10));
    // `gap` from the work area's edges.
    let top = Shape {
        room: 0,
        rect: Rect::new(10, 30, 50, 50),
    };
    assert!(!in_field(&[top], 0, WORK, SIZES, 30, 28 + 11));
    assert!(in_field(&[top], 0, WORK, SIZES, 30, 28 + 12));
    assert!(!in_field(&[top], 0, WORK, SIZES, 11, 50));
    assert!(!in_field(&[], 0, WORK, SIZES, 140, 130));
    assert!(bounds(&[], 0, WORK, SIZES).is_empty());
}

#[test]
fn painting_tints_only_coloured_rooms_and_reports_what_it_changed() {
    let back = LIGHT.bg_desktop;
    let mut px = vec![back.pixel(); (W * H) as usize];
    let shapes = [
        Shape {
            room: 0,
            rect: Rect::new(40, 60, 100, 80),
        },
        Shape {
            room: 1,
            rect: Rect::new(250, 60, 100, 80),
        },
        Shape {
            room: 2,
            rect: Rect::new(150, 160, 60, 30),
        },
    ];
    let colours = [
        Some((LIGHT.room(1), LIGHT.room(1).blend(LIGHT.wallpaper_a, 128))),
        None,
        Some((LIGHT.room_untrusted, LIGHT.room_untrusted)),
    ];
    let changed = paint(&mut px, W as usize, &shapes, &colours, WORK, SIZES);
    assert_eq!(changed.len(), 2);
    let at = |x: i64, y: i64| px[(y * W + x) as usize];
    assert_eq!(at(90, 100), back.blend(LIGHT.room(1), GROUND_ALPHA).pixel());
    assert_eq!(at(300, 100), back.pixel(), "home has no field");
    assert_eq!(
        at(180, 175),
        back.blend(LIGHT.room_untrusted, GROUND_ALPHA).pixel()
    );
    // The line: the field's outermost pixel straight out of the window.
    assert_eq!(
        at(40 - 24, 100),
        LIGHT.room(1).blend(LIGHT.wallpaper_a, 128).pixel()
    );
    for y in 0..H {
        for x in 0..W {
            if at(x, y) != back.pixel() {
                assert!(changed.iter().any(|r| r.contains(x, y)));
            }
        }
    }
    // Moving a window and painting again gives the field of the new place
    // (INV-0172-9's computation): the same as painting it there at first.
    let moved = [
        shapes[0],
        shapes[1],
        Shape {
            room: 2,
            rect: Rect::new(20, 170, 60, 30),
        },
    ];
    let mut a = vec![back.pixel(); (W * H) as usize];
    paint(&mut a, W as usize, &moved, &colours, WORK, SIZES);
    let mut b = vec![back.pixel(); (W * H) as usize];
    paint(
        &mut b,
        W as usize,
        &[moved[2], moved[0], moved[1]],
        &colours,
        WORK,
        SIZES,
    );
    assert_eq!(a, b, "the order of windows does not matter");
    assert_ne!(a, px);
}
