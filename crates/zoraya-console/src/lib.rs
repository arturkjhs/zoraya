//! Bytes from the console on their way to a reader (ADR-0060). A fixed ring:
//! the crate does not link `alloc`, because the receive interrupt fills it.

#![cfg_attr(not(test), no_std)]

/// Bytes the ring holds.
pub const INPUT_SIZE: usize = 256;

pub struct InputRing {
    bytes: [u8; INPUT_SIZE],
    first: usize,
    count: usize,
    dropped: u64,
}

impl InputRing {
    pub const fn new() -> InputRing {
        InputRing {
            bytes: [0; INPUT_SIZE],
            first: 0,
            count: 0,
            dropped: 0,
        }
    }

    /// Adds a byte, or drops and counts it if the ring is full.
    pub fn push(&mut self, byte: u8) {
        if self.count == INPUT_SIZE {
            self.dropped += 1;
            return;
        }
        self.bytes[(self.first + self.count) % INPUT_SIZE] = byte;
        self.count += 1;
    }

    /// Moves bytes to the front of `out`, oldest first, and gives how many.
    pub fn take(&mut self, out: &mut [u8]) -> usize {
        let taken = self.count.min(out.len());
        for slot in &mut out[..taken] {
            *slot = self.bytes[self.first];
            self.first = (self.first + 1) % INPUT_SIZE;
        }
        self.count -= taken;
        taken
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Bytes dropped so far.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

impl Default for InputRing {
    fn default() -> Self {
        InputRing::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_keep_their_order_across_the_wrap() {
        let mut ring = InputRing::new();
        let mut out = [0; INPUT_SIZE];
        for round in 0..3 * INPUT_SIZE {
            ring.push(round as u8);
            ring.push((round + 1) as u8);
            assert_eq!(ring.take(&mut out[..2]), 2);
            assert_eq!(out[..2], [round as u8, (round + 1) as u8]);
        }
        assert!(ring.is_empty());
    }

    #[test]
    fn a_full_ring_drops_the_new_bytes_and_counts_them() {
        let mut ring = InputRing::new();
        for i in 0..INPUT_SIZE + 10 {
            ring.push(i as u8);
        }
        assert_eq!(ring.dropped(), 10);
        let mut out = [0; INPUT_SIZE + 10];
        assert_eq!(ring.take(&mut out), INPUT_SIZE);
        assert_eq!(out[INPUT_SIZE - 1], (INPUT_SIZE - 1) as u8);
        assert!(ring.is_empty());
        assert_eq!(ring.dropped(), 10, "taking does not reset the count");
    }

    #[test]
    fn a_short_buffer_takes_part_and_an_empty_ring_gives_nothing() {
        let mut ring = InputRing::new();
        let mut out = [0; 4];
        assert_eq!(ring.take(&mut out), 0);
        ring.push(1);
        ring.push(2);
        assert_eq!(ring.take(&mut out[..1]), 1);
        assert_eq!(ring.take(&mut out), 1);
        assert_eq!(out[0], 2);
    }
}
