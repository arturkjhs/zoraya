//! Unstructured: the input is XORed onto the first bytes of the seed image
//! (the eight root slots and the first blocks after them), so an all-zero
//! input is the valid image and every byte of the root slots is reachable.
//! No checksum is recomputed: this target exercises root record parsing,
//! slot selection and the checksum checks on arbitrary bytes.

#![no_main]

use std::rc::Rc;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use zoraya_fs_testkit::ImageDevice;

#[path = "common.rs"]
mod common;

static SEED: OnceLock<Vec<u8>> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let seed = SEED.get_or_init(common::seed_image);
    let mut img = seed.clone();
    for (b, d) in img.iter_mut().zip(data) {
        *b ^= d;
    }
    let img = Rc::new(img);
    common::exercise(img.clone(), ImageDevice::new(img));
});
