//! Structure-aware: the input is a list of byte edits applied to a valid
//! seed image, optionally with every checksum on the way to the root
//! recomputed (an attacker can do that, ADR-0012 rule 4). Then mount, walk
//! and check.
//!
//! Input: byte 0 = flags (bit 0: recompute checksums); then records of four
//! bytes: block (mod 256), offset (two bytes, mod 4096), value.

#![no_main]

use std::rc::Rc;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use zoraya_fs::BLOCK_SIZE;
use zoraya_fs_testkit::ImageDevice;
use zoraya_fs_testkit::fields::{FieldMap, field_map, fix_checksums, reseal_root};

#[path = "common.rs"]
mod common;

struct Seed {
    img: Vec<u8>,
    map: FieldMap,
}

static SEED: OnceLock<Seed> = OnceLock::new();

fn seed() -> &'static Seed {
    SEED.get_or_init(|| {
        let img = common::seed_image();
        let map = field_map(&mut ImageDevice::new(Rc::new(img.clone()))).expect("seed image map");
        Seed { img, map }
    })
}

fuzz_target!(|data: &[u8]| {
    let Some((&flags, edits)) = data.split_first() else { return };
    let base = Rc::new(seed().img.clone());
    let mut dev = ImageDevice::new(base.clone());
    let map: Option<&FieldMap> = if flags & 1 == 1 { Some(&seed().map) } else { None };
    for e in edits.chunks_exact(4).take(64) {
        let block = u64::from(e[0]) % common::BLOCKS;
        let off = (usize::from(e[1]) | usize::from(e[2]) << 8) % BLOCK_SIZE;
        let old = dev.get_block(block);
        let mut b = old;
        b[off] = e[3];
        dev.set_block(block, b);
        if let Some(m) = map {
            if m.slots.contains(&block) {
                reseal_root(&mut dev, m);
            } else if m.kinds.contains_key(&block) {
                fix_checksums(&mut dev, m, block, &old);
            }
        }
    }
    common::exercise(base, dev);
});
