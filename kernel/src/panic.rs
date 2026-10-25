//! Panic handler: prints the message and location on one line, then halts.
//! In the test kernel, a panic inside a test is reported as that test's
//! failure and the remaining tests run (see testing.rs).

use core::fmt::Write;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{arch, console};

static PANICKING: AtomicBool = AtomicBool::new(false);

/// Marks the end of panic handling, for the test kernel's recovery path.
#[cfg(feature = "qemu-test")]
pub fn clear() {
    PANICKING.store(false, Ordering::Relaxed);
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    // One CPU and interrupts masked: a plain load and store cannot race.
    if PANICKING.load(Ordering::Relaxed) {
        crate::print!("\nZORAYA-PANIC nested panic; halting\n");
        arch::halt();
    }
    PANICKING.store(true, Ordering::Relaxed);

    #[cfg(feature = "qemu-test")]
    crate::testing::on_panic(info);

    if console::base() == 0 {
        console::print_fallback(format_args!("\nZORAYA-PANIC {}\n", Report(info)));
    } else {
        crate::print!("\nZORAYA-PANIC ");
        let _ = write!(console::OneLine, "{}", Report(info));
        crate::print!("\n");
    }
    arch::halt()
}

/// "at file:line:col: message".
pub struct Report<'a, 'b>(pub &'a PanicInfo<'b>);

impl core::fmt::Display for Report<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0.location() {
            Some(l) => write!(f, "at {}:{}:{}: ", l.file(), l.line(), l.column())?,
            None => f.write_str("at unknown location: ")?,
        }
        write!(f, "{}", self.0.message())
    }
}
