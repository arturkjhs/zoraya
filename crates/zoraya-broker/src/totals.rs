//! What the session's processes report (ADR-0110 item 7): the sum of their
//! counters, the largest of the `max_` ones, the union of the functions they
//! called, and the broker's own peak of live pids. The broker prints them at
//! the session's end as `POSIX-STATS name=value` lines and one `POSIX-CALLS`
//! line of hex, which `tools/posix-stats` reads from the runner's log.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_proto_posix::stats::{MAX_CALL_BYTES, NAMES};

/// A report the broker refuses: a counter list not a multiple of 8 bytes or
/// longer than the names, or a bitmap past the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Malformed;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Totals {
    /// Reports received: one per image that exited or exec'd normally.
    pub images: u64,
    pub counters: Vec<u64>,
    /// Bit `i % 8` of byte `i / 8` for function `i` of the call log.
    pub calls: Vec<u8>,
    /// The most pids live at once, the shell's included.
    pub peak_pids: usize,
}

impl Totals {
    /// Adds one report: `counters` in the order of [`NAMES`], eight bytes
    /// each. A malformed report (a length not a multiple of 8, more counters
    /// than names, a bitmap past the limit) changes nothing.
    pub fn add(&mut self, counters: &[u8], calls: &[u8]) -> Result<(), Malformed> {
        if !counters.len().is_multiple_of(8)
            || counters.len() / 8 > NAMES.len()
            || calls.len() > MAX_CALL_BYTES
        {
            return Err(Malformed);
        }
        if self.counters.len() < NAMES.len() {
            self.counters.resize(NAMES.len(), 0);
        }
        for (i, c) in counters.as_chunks::<8>().0.iter().enumerate() {
            let v = u64::from_le_bytes(*c);
            let total = &mut self.counters[i];
            *total = if NAMES[i].starts_with("max_") {
                (*total).max(v)
            } else {
                total.saturating_add(v)
            };
        }
        if self.calls.len() < calls.len() {
            self.calls.resize(calls.len(), 0);
        }
        for (t, c) in self.calls.iter_mut().zip(calls) {
            *t |= c;
        }
        self.images += 1;
        Ok(())
    }

    /// Notes `live` pids at once.
    pub fn live(&mut self, live: usize) {
        self.peak_pids = self.peak_pids.max(live);
    }

    /// The lines the broker prints: the images, every counter, the peaks,
    /// the bitmap in hex (two digits a byte, byte 0 first). `peak_tmp` is the
    /// temporary service's peak use in bytes, when it answered.
    pub fn lines(&self, peak_tmp: Option<u64>) -> Vec<String> {
        let mut out = Vec::with_capacity(NAMES.len() + 4);
        out.push(format!("POSIX-STATS images={}", self.images));
        for (i, name) in NAMES.iter().enumerate() {
            let v = self.counters.get(i).copied().unwrap_or(0);
            out.push(format!("POSIX-STATS {name}={v}"));
        }
        out.push(format!("POSIX-STATS peak_pids={}", self.peak_pids));
        if let Some(t) = peak_tmp {
            out.push(format!("POSIX-STATS peak_tmp_bytes={t}"));
        }
        let mut hex = String::with_capacity(self.calls.len() * 2);
        for b in &self.calls {
            hex.push_str(&format!("{b:02x}"));
        }
        out.push(format!("POSIX-CALLS {hex}"));
        out
    }
}
