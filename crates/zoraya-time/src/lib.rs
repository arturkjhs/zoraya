//! Monotonic time arithmetic and the deadline queue (ADR-0023).
//!
//! The hardware counter ticks at a rate only known at run time. [`Clock`]
//! converts between ticks and nanoseconds so that a deadline never fires
//! early: ticks to time rounds down, time to ticks rounds up.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// A point on the monotonic clock, in nanoseconds since the clock started.
/// `u64` nanoseconds last 584 years.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(u64);

impl Instant {
    pub const ZERO: Instant = Instant(0);

    pub const fn from_nanos(nanos: u64) -> Instant {
        Instant(nanos)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// This instant plus `nanos`, saturating.
    pub const fn plus_nanos(self, nanos: u64) -> Instant {
        Instant(self.0.saturating_add(nanos))
    }

    /// Nanoseconds from `earlier` to this instant, 0 if it is later.
    pub const fn nanos_since(self, earlier: Instant) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

/// When a blocking operation gives up (ADR-0006 rule 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Deadline {
    At(Instant),
    Infinite,
}

/// Converts between counter ticks and [`Instant`] for one counter frequency
/// and start value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    hz: u64,
    start: u64,
}

impl Clock {
    /// `None` if `hz` is 0.
    pub const fn new(hz: u64, start_ticks: u64) -> Option<Clock> {
        if hz == 0 {
            None
        } else {
            Some(Clock {
                hz,
                start: start_ticks,
            })
        }
    }

    pub const fn hz(&self) -> u64 {
        self.hz
    }

    /// The instant at counter value `ticks`, rounded down. A counter value
    /// before the start gives [`Instant::ZERO`].
    pub const fn instant(&self, ticks: u64) -> Instant {
        let elapsed = ticks.saturating_sub(self.start) as u128;
        let nanos = elapsed * NANOS_PER_SEC / self.hz as u128;
        Instant(if nanos > u64::MAX as u128 {
            u64::MAX
        } else {
            nanos as u64
        })
    }

    /// The first counter value at which [`Clock::instant`] is at or past
    /// `at`. Saturates at `u64::MAX`.
    pub const fn ticks_at(&self, at: Instant) -> u64 {
        let elapsed = (at.0 as u128 * self.hz as u128).div_ceil(NANOS_PER_SEC);
        let ticks = elapsed + self.start as u128;
        if ticks > u64::MAX as u128 {
            u64::MAX
        } else {
            ticks as u64
        }
    }
}

/// Names an entry of a [`DeadlineQueue`]. Ids are not reused: the low 16
/// bits are the entry's slot, the rest a sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerId(u64);

/// The queue holds its maximum number of entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFull;

#[derive(Debug, Clone, Copy)]
struct Entry<T> {
    at: Instant,
    /// The insertion sequence number: the tie-break, and half of the id.
    seq: u64,
    value: T,
    /// Where this entry's slot is in `heap` (the back-index, ADR-0301 item
    /// 5), so that a cancel finds it without a scan.
    pos: u16,
}

/// At most `N` (below 65,536) pending deadlines in a binary min-heap by
/// `(deadline, insertion order)`: insert, cancel and pop move at most
/// `2 * log2(N)` entries, with interrupts masked (ADR-0301 item 5,
/// INV-0301-7). Entries with equal deadlines keep insertion order.
///
/// `slots` holds the entries where they were put and never moves them;
/// `heap` holds slot numbers in heap order; `free` is a stack of unused slots.
#[derive(Debug)]
pub struct DeadlineQueue<T, const N: usize> {
    slots: [Option<Entry<T>>; N],
    heap: [u16; N],
    free: [u16; N],
    len: usize,
    free_len: usize,
    next_seq: u64,
    /// Heap positions written by the last operation (for the move bound).
    moves: usize,
}

impl<T: Copy, const N: usize> DeadlineQueue<T, N> {
    pub const fn new() -> DeadlineQueue<T, N> {
        assert!(N < 1 << 16);
        let mut free = [0u16; N];
        let mut i = 0;
        while i < N {
            free[i] = (N - 1 - i) as u16;
            i += 1;
        }
        DeadlineQueue {
            slots: [None; N],
            heap: [0; N],
            free,
            len: 0,
            free_len: N,
            next_seq: 0,
            moves: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The heap positions the last `insert`, `cancel` or `pop_due` wrote.
    pub const fn last_moves(&self) -> usize {
        self.moves
    }

    fn entry(&self, position: usize) -> Entry<T> {
        // The heap holds only slots in use.
        self.slots[usize::from(self.heap[position])].expect("a heap slot is in use")
    }

    fn key(&self, position: usize) -> (Instant, u64) {
        let e = self.entry(position);
        (e.at, e.seq)
    }

    fn place(&mut self, position: usize, slot: u16) {
        self.heap[position] = slot;
        if let Some(e) = self.slots[usize::from(slot)].as_mut() {
            e.pos = position as u16;
        }
        self.moves += 1;
    }

    fn sift_up(&mut self, mut position: usize) {
        let slot = self.heap[position];
        let key = self.key(position);
        while position > 0 {
            let parent = (position - 1) / 2;
            if self.key(parent) <= key {
                break;
            }
            let up = self.heap[parent];
            self.place(position, up);
            position = parent;
        }
        self.place(position, slot);
    }

    fn sift_down(&mut self, mut position: usize) {
        let slot = self.heap[position];
        let key = self.key(position);
        loop {
            let left = 2 * position + 1;
            if left >= self.len {
                break;
            }
            let right = left + 1;
            let child = if right < self.len && self.key(right) < self.key(left) {
                right
            } else {
                left
            };
            if self.key(child) >= key {
                break;
            }
            let up = self.heap[child];
            self.place(position, up);
            position = child;
        }
        self.place(position, slot);
    }

    pub fn insert(&mut self, at: Instant, value: T) -> Result<TimerId, QueueFull> {
        if self.free_len == 0 {
            return Err(QueueFull);
        }
        self.moves = 0;
        self.free_len -= 1;
        let slot = self.free[self.free_len];
        let seq = self.next_seq;
        self.next_seq += 1;
        let position = self.len;
        self.slots[usize::from(slot)] = Some(Entry {
            at,
            seq,
            value,
            pos: position as u16,
        });
        self.heap[position] = slot;
        self.len += 1;
        self.sift_up(position);
        Ok(TimerId(seq << 16 | u64::from(slot)))
    }

    /// Takes the entry at heap `position` out and restores the heap order.
    fn remove_at(&mut self, position: usize) -> Entry<T> {
        self.moves = 0;
        let slot = self.heap[position];
        let gone = self.slots[usize::from(slot)]
            .take()
            .expect("a heap slot is in use");
        self.free[self.free_len] = slot;
        self.free_len += 1;
        self.len -= 1;
        if position < self.len {
            let last = self.heap[self.len];
            self.place(position, last);
            // The moved entry may belong above or below this position.
            self.sift_up(position);
            let now = usize::from(self.slots[usize::from(last)].map_or(0, |e| e.pos));
            self.sift_down(now);
        }
        gone
    }

    /// Removes the entry, if it is still pending.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        let slot = (id.0 & 0xffff) as usize;
        match self.slots.get(slot).copied().flatten() {
            Some(e) if e.seq == id.0 >> 16 => {
                self.remove_at(usize::from(e.pos));
                true
            }
            _ => false,
        }
    }

    /// The earliest pending deadline.
    pub fn earliest(&self) -> Option<Instant> {
        (self.len > 0).then(|| self.entry(0).at)
    }

    /// Removes and returns the earliest entry whose deadline is at or before
    /// `now`.
    pub fn pop_due(&mut self, now: Instant) -> Option<T> {
        if self.len == 0 || self.entry(0).at > now {
            return None;
        }
        Some(self.remove_at(0).value)
    }
}

impl<T: Copy, const N: usize> Default for DeadlineQueue<T, N> {
    fn default() -> Self {
        DeadlineQueue::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [u64; 6] = [
        1,
        24_000_000,
        62_500_000,
        1_000_000_000,
        3_000_000_007,
        u64::MAX,
    ];

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
    }

    #[test]
    fn zero_hz_is_refused() {
        assert_eq!(Clock::new(0, 0), None);
    }

    #[test]
    fn known_rates() {
        let c = Clock::new(62_500_000, 1000).unwrap();
        assert_eq!(c.instant(1000), Instant::ZERO);
        assert_eq!(
            c.instant(1000 + 62_500_000),
            Instant::from_nanos(1_000_000_000)
        );
        assert_eq!(c.instant(1001), Instant::from_nanos(16));
        assert_eq!(c.ticks_at(Instant::from_nanos(16)), 1001);
        assert_eq!(c.ticks_at(Instant::from_nanos(17)), 1002, "rounds up");
        assert_eq!(c.instant(5), Instant::ZERO, "before the start");
    }

    #[test]
    fn a_deadline_converted_to_ticks_is_never_early() {
        let mut rng = Rng(0xabcdef);
        for hz in RATES {
            let start = rng.next() >> 8;
            let c = Clock::new(hz, start).unwrap();
            for _ in 0..200_000 {
                let at = Instant::from_nanos(rng.next() >> (rng.next() % 64));
                let t = c.ticks_at(at);
                if t == u64::MAX {
                    continue;
                }
                assert!(c.instant(t) >= at, "hz {hz} at {at:?}");
                // And it is the first such tick.
                if t > start {
                    assert!(c.instant(t - 1) < at, "hz {hz} at {at:?}");
                }
            }
        }
    }

    #[test]
    fn the_clock_never_decreases() {
        let mut rng = Rng(0x1234);
        for hz in RATES {
            let c = Clock::new(hz, 0).unwrap();
            let mut t = 0u64;
            let mut last = Instant::ZERO;
            for _ in 0..200_000 {
                t = t.saturating_add(rng.next() >> 40);
                let now = c.instant(t);
                assert!(now >= last);
                last = now;
            }
        }
    }

    #[test]
    fn extremes_saturate_instead_of_overflowing() {
        let c = Clock::new(1, 0).unwrap();
        assert_eq!(c.instant(u64::MAX), Instant::from_nanos(u64::MAX));
        let c = Clock::new(u64::MAX, u64::MAX).unwrap();
        assert_eq!(c.ticks_at(Instant::from_nanos(u64::MAX)), u64::MAX);
        assert_eq!(
            Instant::from_nanos(u64::MAX).plus_nanos(5),
            Instant::from_nanos(u64::MAX)
        );
        assert_eq!(Instant::ZERO.nanos_since(Instant::from_nanos(5)), 0);
    }

    #[test]
    fn deadline_order_puts_infinite_last() {
        assert!(Deadline::At(Instant::from_nanos(u64::MAX)) < Deadline::Infinite);
    }

    fn t(n: u64) -> Instant {
        Instant::from_nanos(n)
    }

    #[test]
    fn queue_orders_by_time_then_insertion() {
        let mut q = DeadlineQueue::<u32, 8>::new();
        q.insert(t(30), 1).unwrap();
        q.insert(t(10), 2).unwrap();
        q.insert(t(20), 3).unwrap();
        q.insert(t(10), 4).unwrap();
        assert_eq!(q.earliest(), Some(t(10)));
        assert_eq!(q.pop_due(t(5)), None);
        assert_eq!(q.pop_due(t(100)), Some(2));
        assert_eq!(q.pop_due(t(100)), Some(4));
        assert_eq!(q.pop_due(t(10)), None, "20 is not due at 10");
        assert_eq!(q.pop_due(t(20)), Some(3));
        assert_eq!(q.pop_due(t(30)), Some(1));
        assert!(q.is_empty());
        assert_eq!(q.earliest(), None);
    }

    #[test]
    fn queue_reports_full_and_recovers() {
        let mut q = DeadlineQueue::<u32, 2>::new();
        q.insert(t(1), 1).unwrap();
        let b = q.insert(t(2), 2).unwrap();
        assert_eq!(q.insert(t(3), 3), Err(QueueFull));
        assert_eq!(q.len(), 2);
        assert!(q.cancel(b));
        assert!(!q.cancel(b), "already gone");
        q.insert(t(0), 3).unwrap();
        assert_eq!(q.pop_due(t(9)), Some(3));
    }

    #[test]
    fn queue_matches_a_model() {
        let mut rng = Rng(0x77);
        let mut q = DeadlineQueue::<u64, 16>::new();
        let mut model: [Option<(TimerId, u64, u64)>; 16] = [None; 16];
        let mut now = 0;
        let mut seq = 0;
        for _ in 0..50_000 {
            match rng.next() % 4 {
                0 | 1 => {
                    let at = now + rng.next() % 64;
                    seq += 1;
                    match q.insert(t(at), seq) {
                        Ok(id) => {
                            let slot = model.iter_mut().find(|m| m.is_none()).unwrap();
                            *slot = Some((id, at, seq));
                        }
                        Err(QueueFull) => assert!(model.iter().all(Option::is_some)),
                    }
                }
                2 => {
                    let i = (rng.next() % 16) as usize;
                    if let Some((id, ..)) = model[i].take() {
                        assert!(q.cancel(id));
                    }
                }
                _ => {
                    now += rng.next() % 8;
                    while let Some(v) = q.pop_due(t(now)) {
                        let (i, (_, at, _)) = model
                            .iter()
                            .enumerate()
                            .filter_map(|(i, m)| m.map(|m| (i, m)))
                            .min_by_key(|(_, (_, at, s))| (*at, *s))
                            .unwrap();
                        assert!(at <= now);
                        assert_eq!(model[i].unwrap().2, v);
                        model[i] = None;
                    }
                }
            }
            let earliest = model.iter().flatten().map(|m| m.1).min();
            assert_eq!(q.earliest(), earliest.map(t));
        }
    }

    /// INV-0301-7: no insert, cancel or pop writes more than 12 heap
    /// positions at 1024 entries, and the queue still matches the order.
    #[test]
    fn heap_operations_move_at_most_twelve_entries_at_1024() {
        let mut rng = Rng(0x5eed);
        let mut q = DeadlineQueue::<u64, 1024>::new();
        let mut ids: std::vec::Vec<TimerId> = std::vec::Vec::new();
        let mut worst = 0;
        let mut now = 0;
        for round in 0..60_000u64 {
            match rng.next() % 5 {
                0..=2 => {
                    if let Ok(id) = q.insert(t(now + rng.next() % 5000), round) {
                        ids.push(id);
                    }
                    worst = worst.max(q.last_moves());
                }
                3 => {
                    if !ids.is_empty() {
                        let i = (rng.next() as usize) % ids.len();
                        let id = ids.swap_remove(i);
                        // It may have expired already.
                        q.cancel(id);
                        worst = worst.max(q.last_moves());
                    }
                }
                _ => {
                    now += rng.next() % 40;
                    let mut last = 0;
                    while q.pop_due(t(now)).is_some() {
                        worst = worst.max(q.last_moves());
                        last += 1;
                    }
                    let _ = last;
                }
            }
        }
        assert!(worst <= 12, "{worst}");
        assert!(worst > 0);
    }

    #[test]
    fn a_cancelled_id_is_never_reused_for_another_entry() {
        let mut q = DeadlineQueue::<u32, 2>::new();
        let a = q.insert(t(5), 1).unwrap();
        assert!(q.cancel(a));
        let b = q.insert(t(5), 2).unwrap();
        assert!(!q.cancel(a), "the old id names nothing now");
        assert!(q.cancel(b));
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
