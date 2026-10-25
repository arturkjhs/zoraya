//! Kernel console on the device tree's UART.
//!
//! The console holds only the UART base address. It is stored in an atomic
//! with plain loads and stores (no read-modify-write, so no exclusive access
//! instructions, which need not work while the MMU is off), and a fresh
//! stateless writer is made for every print. The panic handler can therefore
//! always print, even if it interrupted another print.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::pl011::Pl011;

/// PL011 base on QEMU virt. Used only to report a device tree error before
/// halting, never for normal output (docs/design.md, "Boot").
pub const FALLBACK_UART_BASE: usize = 0x0900_0000;

static UART_BASE: AtomicUsize = AtomicUsize::new(0);

/// Sets the console UART and enables it.
///
/// # Safety
///
/// `base` must be the MMIO base of a PL011, as required by [`Pl011::new`].
pub unsafe fn init(base: usize) {
    // SAFETY: forwarded from the caller.
    let uart = unsafe { Pl011::new(base) };
    uart.init();
    UART_BASE.store(base, Ordering::Relaxed);
}

/// The base address of the UART the console prints through (0 before `init`).
pub fn base() -> usize {
    UART_BASE.load(Ordering::Relaxed)
}

fn uart() -> Option<Pl011> {
    match base() {
        0 => None,
        // SAFETY: a non-zero base was stored by `init`, whose caller
        // guaranteed it is a PL011.
        b => Some(unsafe { Pl011::new(b) }),
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    if let Some(mut u) = uart() {
        let _ = u.write_fmt(args);
    }
}

/// Prints through the fallback UART, for the one error message that may come
/// before the console exists.
pub fn print_fallback(args: fmt::Arguments<'_>) {
    // SAFETY: QEMU virt has a PL011 at FALLBACK_UART_BASE; it is used only
    // when the device tree could not provide one.
    let mut u = unsafe { Pl011::new(FALLBACK_UART_BASE) };
    u.init();
    let _ = u.write_fmt(args);
}

/// Writes through the console, replacing newlines with spaces, so that a
/// message stays on one protocol line.
pub struct OneLine;

impl Write for OneLine {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let Some(mut u) = uart() else { return Ok(()) };
        for part in s.split(['\n', '\r']).enumerate() {
            if part.0 > 0 {
                u.write_byte(b' ');
            }
            u.write_str(part.1)?;
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::console::_print(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => { $crate::console::_print(format_args!("{}\n", format_args!($($arg)*))) };
}
