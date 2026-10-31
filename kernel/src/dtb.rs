//! Finding the device tree blob.
//!
//! QEMU's virt board passes a bare-metal ELF image no boot arguments; it
//! places the blob at the start of RAM, 0x40000000 (QEMU `hw/arm/boot.c`,
//! `arm_setup_direct_kernel_boot`; documented in QEMU's
//! `docs/system/arm/virt.rst`). The kernel also accepts an address in `x0`, as
//! the Linux arm64 boot protocol passes it, if it lies in RAM below the
//! image. The candidates are checked in that order: `x0`, then the start of
//! RAM. What QEMU actually passes in `x0` is recorded in docs/design.md.
//!
//! [`locate`] reads through the boot identity map. Once the final tables are
//! on, [`rebase`] gives the same blob through the direct map, and nothing may
//! keep a reference from the first parse.

use core::sync::atomic::{AtomicUsize, Ordering};

use zoraya_dtb::{Fdt, HEADER_SIZE, Header};

use crate::memory;

/// Start of RAM on QEMU virt.
pub const RAM_BASE: usize = 0x4000_0000;

/// Where the blob was found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// The address the loader left in `x0`.
    Register,
    /// The start of RAM.
    StartOfRam,
}

#[derive(Clone, Copy, Debug)]
pub enum DtbError {
    /// No candidate address holds the FDT magic number.
    NotFound,
    /// The header's `totalsize` runs into the kernel image.
    TooLarge(usize),
    /// The blob failed validation.
    Invalid(zoraya_dtb::Error),
}

impl core::fmt::Display for DtbError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DtbError::NotFound => write!(f, "no device tree blob in x0 or at {RAM_BASE:#x}"),
            DtbError::TooLarge(n) => write!(f, "device tree totalsize {n} overlaps the kernel"),
            DtbError::Invalid(e) => write!(f, "device tree is invalid: {e}"),
        }
    }
}

static DTB_ADDR: AtomicUsize = AtomicUsize::new(0);
static DTB_LEN: AtomicUsize = AtomicUsize::new(0);

/// The blob at physical address `addr`, which must lie in
/// `[RAM_BASE, kernel start)`, read through the boot identity map.
fn blob_at(addr: usize, limit: usize) -> Result<&'static [u8], DtbError> {
    if addr < RAM_BASE || !addr.is_multiple_of(8) || addr.saturating_add(HEADER_SIZE) > limit {
        return Err(DtbError::NotFound);
    }
    // SAFETY: [RAM_BASE, limit) is RAM below the kernel image: QEMU loaded
    // the image 2 MiB above RAM_BASE, and virt RAM is one contiguous range
    // from RAM_BASE. The boot tables map it at the same address until the
    // caller's limit (see `locate`), and nothing writes below the image.
    let header = unsafe { core::slice::from_raw_parts(addr as *const u8, HEADER_SIZE) };
    let header = Header::read(header).map_err(|_| DtbError::NotFound)?;
    let len = header.total_size as usize;
    if len > limit - addr {
        return Err(DtbError::TooLarge(len));
    }
    // SAFETY: as above; the range [addr, addr + len) was just checked to
    // stay below `limit`, and the memory is never written at all.
    Ok(unsafe { core::slice::from_raw_parts(addr as *const u8, len) })
}

/// Finds and validates the device tree, and remembers it for [`rebase`].
///
/// # Safety
///
/// The boot identity map must be active. Despite the `'static` in the result,
/// it and everything borrowed from it are valid only until that map is
/// removed; after that, use [`rebase`].
pub unsafe fn locate(x0: usize) -> Result<(Fdt<'static>, Source), DtbError> {
    let limit = memory::kernel_image_phys().start as usize;
    let (bytes, source) = match blob_at(x0, limit) {
        Ok(b) => (b, Source::Register),
        Err(_) => (blob_at(RAM_BASE, limit)?, Source::StartOfRam),
    };
    let fdt = Fdt::new(bytes).map_err(DtbError::Invalid)?;
    DTB_ADDR.store(bytes.as_ptr() as usize, Ordering::Relaxed);
    DTB_LEN.store(bytes.len(), Ordering::Relaxed);
    Ok((fdt, source))
}

/// Physical address and size of the blob found by [`locate`].
pub fn phys() -> (usize, usize) {
    (
        DTB_ADDR.load(Ordering::Relaxed),
        DTB_LEN.load(Ordering::Relaxed),
    )
}

/// The blob found by [`locate`], through the direct map.
///
/// # Safety
///
/// [`locate`] succeeded and the final tables are on.
pub unsafe fn rebase() -> &'static [u8] {
    let (addr, len) = phys();
    // SAFETY: addr and len describe the blob `locate` validated. The final
    // tables map it read-only in the direct map, and it is reserved from the
    // frame allocator, so it stays valid and unmodified for the kernel's
    // lifetime.
    unsafe { core::slice::from_raw_parts(memory::phys_to_virt(addr as u64) as *const u8, len) }
}

/// The blob, for tests.
#[cfg(feature = "qemu-test")]
pub fn blob() -> &'static [u8] {
    // SAFETY: tests run after boot has finished with the final tables on.
    unsafe { rebase() }
}
