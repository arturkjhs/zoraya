//! Minimal ARM PL011 UART driver: transmit polled, receive by interrupt.
//!
//! Register offsets and bits from the PL011 Technical Reference Manual
//! (ARM DDI 0183). The baud rate is not programmed: QEMU ignores it, and the
//! UART clock (needed to compute the divisor) is not read at stage 0.

// The test kernel borrows the receive interrupt for its own tests, so the
// receive path is used only in the normal kernel.
#![cfg_attr(feature = "qemu-test", allow(dead_code))]

use core::ptr;

const DR: usize = 0x00;
const FR: usize = 0x18;
const LCR_H: usize = 0x2c;
const CR: usize = 0x30;
const IMSC: usize = 0x38;
const ICR: usize = 0x44;

const FR_TXFF: u32 = 1 << 5;
const FR_RXFE: u32 = 1 << 4;
const FR_BUSY: u32 = 1 << 3;
const LCR_H_FEN: u32 = 1 << 4;
const LCR_H_WLEN_8: u32 = 0b11 << 5;
const CR_UARTEN: u32 = 1 << 0;
const CR_TXE: u32 = 1 << 8;
const CR_RXE: u32 = 1 << 9;
/// Receive and receive timeout: the second fires for a FIFO below its level.
const INT_RX: u32 = 1 << 4 | 1 << 6;

/// A PL011 at a fixed MMIO base address.
#[derive(Clone, Copy)]
pub struct Pl011 {
    base: usize,
}

impl Pl011 {
    /// # Safety
    ///
    /// `base` must be the address of a PL011 register block that is mapped
    /// (or, with the MMU off, physically present) as device memory and that
    /// nothing else drives in a conflicting way.
    pub const unsafe fn new(base: usize) -> Pl011 {
        Pl011 { base }
    }

    fn read(&self, off: usize) -> u32 {
        // SAFETY: `new`'s contract makes base + off a PL011 register, and
        // device registers are accessed with volatile 32-bit operations.
        unsafe { ptr::read_volatile((self.base + off) as *const u32) }
    }

    fn write(&self, off: usize, v: u32) {
        // SAFETY: as in `read`.
        unsafe { ptr::write_volatile((self.base + off) as *mut u32, v) }
    }

    /// Enables the UART for 8N1 with FIFOs: disable, drain, set the line
    /// control, enable transmit and receive.
    pub fn init(&self) {
        self.write(CR, 0);
        while self.read(FR) & FR_BUSY != 0 {}
        self.write(LCR_H, LCR_H_WLEN_8 | LCR_H_FEN);
        self.write(CR, CR_UARTEN | CR_TXE | CR_RXE);
    }

    /// Asks for an interrupt when bytes arrive.
    pub fn enable_receive_interrupt(&self) {
        self.write(ICR, INT_RX);
        self.write(IMSC, INT_RX);
    }

    /// The next received byte, if there is one. Emptying the FIFO ends the
    /// interrupt.
    pub fn read_byte(&self) -> Option<u8> {
        if self.read(FR) & FR_RXFE != 0 {
            return None;
        }
        Some(self.read(DR) as u8)
    }

    pub fn write_byte(&self, b: u8) {
        while self.read(FR) & FR_TXFF != 0 {}
        self.write(DR, u32::from(b));
    }
}

impl core::fmt::Write for Pl011 {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
        Ok(())
    }
}
