//! Physical address ranges and fixed-capacity sets of them.

use crate::FRAME_SIZE;

/// The half-open byte range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: u64,
    pub end: u64,
}

impl Range {
    /// The range of `size` bytes at `start`, clamped at the top of the
    /// address space.
    pub const fn at(start: u64, size: u64) -> Range {
        Range {
            start,
            end: start.saturating_add(size),
        }
    }

    pub const fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub const fn overlaps(&self, other: &Range) -> bool {
        self.start < other.end && other.start < self.end && !self.is_empty() && !other.is_empty()
    }

    /// The whole frames inside this range. Empty if there are none.
    pub const fn frames_inside(&self) -> Range {
        let start = match self.start.checked_next_multiple_of(FRAME_SIZE) {
            Some(s) => s,
            None => return Range { start: 0, end: 0 },
        };
        let end = self.end - self.end % FRAME_SIZE;
        Range { start, end }
    }

    /// The frames this range touches. A range ending in the last frame of
    /// the address space ends at the last frame boundary below 2^64 rounded
    /// to `u64::MAX`, which still covers that frame.
    pub const fn frames_around(&self) -> Range {
        if self.is_empty() {
            return Range { start: 0, end: 0 };
        }
        let start = self.start - self.start % FRAME_SIZE;
        let end = match self.end.checked_next_multiple_of(FRAME_SIZE) {
            Some(e) => e,
            None => u64::MAX,
        };
        Range { start, end }
    }
}

/// A [`RangeSet`] needs more than its capacity. The set is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangesFull;

/// Up to `N` sorted, disjoint, non-adjacent, non-empty ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeSet<const N: usize> {
    items: [Range; N],
    len: usize,
}

impl<const N: usize> RangeSet<N> {
    pub const fn new() -> RangeSet<N> {
        RangeSet {
            items: [Range { start: 0, end: 0 }; N],
            len: 0,
        }
    }

    pub fn as_slice(&self) -> &[Range] {
        &self.items[..self.len]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn insert(&mut self, at: usize, r: Range) -> Result<(), RangesFull> {
        if self.len == N {
            return Err(RangesFull);
        }
        self.items.copy_within(at..self.len, at + 1);
        self.items[at] = r;
        self.len += 1;
        Ok(())
    }

    fn delete(&mut self, at: usize) {
        self.items.copy_within(at + 1..self.len, at);
        self.len -= 1;
    }

    /// Adds `r`, merging it with ranges it overlaps or touches.
    pub fn add(&mut self, r: Range) -> Result<(), RangesFull> {
        if r.is_empty() {
            return Ok(());
        }
        let mut merged = r;
        let mut i = 0;
        let mut first_merged = None;
        let mut last_merged = None;
        while i < self.len {
            let cur = self.items[i];
            if cur.end >= r.start && cur.start <= r.end {
                merged.start = merged.start.min(cur.start);
                merged.end = merged.end.max(cur.end);
                first_merged.get_or_insert(i);
                last_merged = Some(i);
            } else if cur.start > r.end {
                break;
            }
            i += 1;
        }
        match (first_merged, last_merged) {
            (Some(first), Some(last)) => {
                self.items[first] = merged;
                for _ in first..last {
                    self.delete(first + 1);
                }
                Ok(())
            }
            _ => self.insert(i, merged),
        }
    }

    /// Removes `r`, splitting a range if `r` falls inside it.
    pub fn remove(&mut self, r: Range) -> Result<(), RangesFull> {
        if r.is_empty() {
            return Ok(());
        }
        let before = *self;
        let mut i = 0;
        while i < self.len {
            let cur = self.items[i];
            if !cur.overlaps(&r) {
                i += 1;
                continue;
            }
            let left = Range {
                start: cur.start,
                end: r.start,
            };
            let right = Range {
                start: r.end,
                end: cur.end,
            };
            match (left.is_empty(), right.is_empty()) {
                (true, true) => self.delete(i),
                (false, true) => {
                    self.items[i] = left;
                    i += 1;
                }
                (true, false) => {
                    self.items[i] = right;
                    i += 1;
                }
                (false, false) => {
                    self.items[i] = left;
                    if self.insert(i + 1, right).is_err() {
                        *self = before;
                        return Err(RangesFull);
                    }
                    i += 2;
                }
            }
        }
        Ok(())
    }
}

impl<const N: usize> Default for RangeSet<N> {
    fn default() -> Self {
        RangeSet::new()
    }
}
