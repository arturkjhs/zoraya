//! Kernel console on the device tree's UART.
//!
//! The console holds only the UART base address, in an atomic, and a fresh
//! stateless writer is made for every print. There is no lock, so the panic
//! handler can always print, even if it interrupted another print, and two
//! prints can interleave.
//!
//! Input goes the other way, through a fixed ring (ADR-0060), with no line
//! discipline: the kernel gives every byte the same meaning, which is none.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicUsize, Ordering};

use zoraya_abi::{Status, console_signals};
use zoraya_console::InputRing;
use zoraya_events::SourceId;

use crate::pl011::Pl011;
use crate::port;
use crate::sync::SpinLock;

/// PL011 base on QEMU virt. Used only to report a device tree error before
/// halting, never for normal output (docs/design.md, "Boot").
pub const FALLBACK_UART_BASE: usize = 0x0900_0000;

static UART_BASE: AtomicUsize = AtomicUsize::new(0);

static INPUT: SpinLock<InputRing> = SpinLock::new(InputRing::new());
static SOURCE: SpinLock<Option<SourceId>> = SpinLock::new(None);

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

/// Moves the console to another mapping of the same UART.
///
/// # Safety
///
/// `base` must be mapped as Device memory to the UART given to [`init`].
pub unsafe fn rebase(base: usize) {
    UART_BASE.store(base, Ordering::Relaxed);
}

/// The base address of the UART the console prints through (0 before `init`).
pub fn base() -> usize {
    UART_BASE.load(Ordering::Relaxed)
}

/// Makes the console's signal source. Needs the heap.
pub fn init_input() -> Result<(), Status> {
    *SOURCE.lock() = Some(port::new_source(0)?);
    Ok(())
}

/// Starts taking interrupts for input. The test kernel does not: it borrows
/// this interrupt for its own tests, and feeds [`receive`] by hand.
#[cfg(not(feature = "qemu-test"))]
pub fn enable_input(intid: u32, edge: bool) {
    crate::irq::register(intid, edge, drain);
    if let Some(u) = uart() {
        u.enable_receive_interrupt();
    }
}

/// The receive interrupt: runs with interrupts masked, and allocates nothing.
#[cfg(not(feature = "qemu-test"))]
fn drain() {
    if let Some(u) = uart() {
        while let Some(byte) = u.read_byte() {
            receive(byte);
        }
    }
}

/// One byte from the line.
pub fn receive(byte: u8) {
    let mut input = INPUT.lock();
    input.push(byte);
    if let Some(source) = *SOURCE.lock() {
        port::set_signals(source, console_signals::READABLE);
    }
}

/// Moves waiting bytes to `out`.
pub fn read(out: &mut [u8]) -> usize {
    let mut input = INPUT.lock();
    let taken = input.take(out);
    if input.is_empty()
        && let Some(source) = *SOURCE.lock()
    {
        port::clear_signals(source, console_signals::READABLE);
    }
    taken
}

pub fn dropped() -> u64 {
    INPUT.lock().dropped()
}

/// The console's signal source.
pub fn source() -> Option<SourceId> {
    *SOURCE.lock()
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
