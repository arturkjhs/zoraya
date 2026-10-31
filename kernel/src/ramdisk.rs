//! The initial ramdisk, embedded in the image (ADR-0033). It is in `.rodata`,
//! so it has one read-only mapping. The build gives an empty one if no user
//! programs were built.

use zoraya_ramdisk::Ramdisk;

static BYTES: &[u8] = include_bytes!(env!("ZORAYA_RAMDISK_PATH"));

/// The ramdisk. Panics on a malformed one: the build made it.
pub fn get() -> Ramdisk<'static> {
    Ramdisk::new(BYTES).expect("embedded ramdisk")
}
