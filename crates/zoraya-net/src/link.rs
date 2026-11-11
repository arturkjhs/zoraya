//! The frame link under the stack: what `netd` has of the driver, as copies.
//!
//! Frames cross the link by copy in both directions. A frame in a shared slot
//! is another process's memory (the driver can write it while the stack
//! reads it), so the stack parses a private copy: `recv` copies the slot out
//! and gives it back before smoltcp sees a byte. That is the one place frame
//! bytes enter the stack; the length is checked here against the buffer.

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

/// The largest frame the stack takes or makes: an Ethernet header and 1500
/// bytes (`zoraya_proto_nic::MAX_FRAME`).
pub const MAX_FRAME: usize = 1514;

/// What the stack needs of the driver's channel and slots.
pub trait Link {
    /// Copies the next received frame to `buf` and gives its length, or
    /// `None` if there is none. An implementation never returns more than
    /// `buf.len()`; a frame that does not fit is dropped by the link.
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize>;

    /// Whether a received frame is waiting.
    fn has_input(&self) -> bool;

    /// Whether a frame can be sent now (a free transmit slot).
    fn can_send(&self) -> bool;

    /// Sends one frame (`frame.len()` at most [`MAX_FRAME`]). `false` if it
    /// was not taken; the stack counts it as dropped.
    fn send(&mut self, frame: &[u8]) -> bool;

    /// Whether the link is up.
    fn is_up(&self) -> bool;
}

/// Frames the device saw, for `status`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub frames_in: u64,
    pub frames_out: u64,
    pub dropped: u64,
}

/// smoltcp's `Device` over a [`Link`].
pub struct LinkDevice<L: Link> {
    pub link: L,
    rx: Vec<u8>,
    tx: Vec<u8>,
    pub counters: Counters,
}

impl<L: Link> LinkDevice<L> {
    pub fn new(link: L) -> LinkDevice<L> {
        LinkDevice {
            link,
            rx: vec![0; MAX_FRAME],
            tx: vec![0; MAX_FRAME],
            counters: Counters::default(),
        }
    }
}

pub struct Rx<'a>(&'a [u8]);

pub struct Tx<'a, L: Link> {
    link: &'a mut L,
    buf: &'a mut Vec<u8>,
    counters: &'a mut Counters,
}

impl RxToken for Rx<'_> {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(self.0)
    }
}

impl<L: Link> TxToken for Tx<'_, L> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        // smoltcp honours the MTU of `capabilities`; if it did not, a larger
        // buffer is made rather than a slice out of range.
        if self.buf.len() < len {
            self.buf.resize(len, 0);
        }
        let result = f(&mut self.buf[..len]);
        if len <= MAX_FRAME && self.link.send(&self.buf[..len]) {
            self.counters.frames_out += 1;
        } else {
            self.counters.dropped += 1;
        }
        result
    }
}

impl<L: Link> Device for LinkDevice<L> {
    type RxToken<'a>
        = Rx<'a>
    where
        Self: 'a;
    type TxToken<'a>
        = Tx<'a, L>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Rx<'_>, Tx<'_, L>)> {
        // A reply may need a transmit slot: take a frame only with one free.
        if !self.link.can_send() {
            return None;
        }
        let n = self.link.recv(&mut self.rx)?;
        if n > self.rx.len() {
            // A link that lies about the length: drop the frame.
            self.counters.dropped += 1;
            return None;
        }
        self.counters.frames_in += 1;
        Some((
            Rx(&self.rx[..n]),
            Tx {
                link: &mut self.link,
                buf: &mut self.tx,
                counters: &mut self.counters,
            },
        ))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Tx<'_, L>> {
        if !self.link.can_send() {
            return None;
        }
        Some(Tx {
            link: &mut self.link,
            buf: &mut self.tx,
            counters: &mut self.counters,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = MAX_FRAME;
        caps.max_burst_size = None;
        caps
    }
}
