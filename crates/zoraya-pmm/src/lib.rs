//! Physical frame allocator (ADR-0021).
//!
//! A two-bit-per-frame bitmap over RAM regions. The bitmap lives in storage
//! the caller provides, outside the managed frames, so a stray write to a
//! free frame cannot damage the allocator.
//!
//! For each region the storage holds two bit arrays of equal length:
//! - `used`: 1 if the frame is reserved or allocated, and for the padding
//!   bits past the region's last frame, which makes word-at-a-time search
//!   simple;
//! - `reserved`: 1 if the frame may never be allocated.
//!
//! Nothing here panics on any input, and nothing allocates.

#![no_std]
#![forbid(unsafe_code)]

mod ranges;

pub use ranges::{Range, RangeSet, RangesFull};

/// Size of a frame in bytes.
pub const FRAME_SIZE: u64 = 4096;

/// Most separate RAM ranges managed, after merging.
pub const MAX_REGIONS: usize = 16;

const WORD_BITS: u64 = u64::BITS as u64;

/// A 4 KiB physical frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frame(u64);

impl Frame {
    /// The frame at physical address `addr`, if it is aligned.
    pub const fn from_addr(addr: u64) -> Option<Frame> {
        if addr.is_multiple_of(FRAME_SIZE) {
            Some(Frame(addr))
        } else {
            None
        }
    }

    pub const fn addr(self) -> u64 {
        self.0
    }
}

/// No frames (or no suitable run of frames) are free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfFrames;

/// Why a free was refused. The allocator's state did not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeError {
    /// The address is not in managed RAM.
    OutOfRange,
    /// The frame may never be allocated.
    Reserved,
    /// The frame is already free.
    NotAllocated,
}

/// Why the allocator could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitError {
    /// More than [`MAX_REGIONS`] RAM ranges after merging.
    TooManyRegions,
    /// `memory` holds no whole frame.
    NoMemory,
    /// The storage is smaller than [`storage_words`] asks for.
    StorageTooSmall,
}

/// What is known about one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameState {
    Free,
    Allocated,
    Reserved,
    /// Not in managed RAM.
    Outside,
}

/// Frame counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Frames of managed RAM.
    pub total: u64,
    /// Frames that may never be allocated.
    pub reserved: u64,
    /// Frames free now.
    pub free: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Region {
    /// Frame number of the first frame.
    first: u64,
    /// Frames in the region.
    frames: u64,
    /// Index of the `used` array in the storage.
    used: usize,
    /// Index of the `reserved` array in the storage.
    reserved: usize,
}

impl Region {
    fn words(&self) -> usize {
        words_for(self.frames)
    }

    fn contains(&self, frame_number: u64) -> bool {
        frame_number >= self.first && frame_number - self.first < self.frames
    }
}

const fn words_for(frames: u64) -> usize {
    frames.div_ceil(WORD_BITS) as usize
}

/// Word index and bit mask for frame `index` of a region.
const fn bit(index: u64) -> (usize, u64) {
    ((index / WORD_BITS) as usize, 1 << (index % WORD_BITS))
}

fn whole_frames(memory: &[Range]) -> Result<RangeSet<MAX_REGIONS>, InitError> {
    let mut set = RangeSet::new();
    for r in memory {
        let inner = r.frames_inside();
        if !inner.is_empty() {
            set.add(inner).map_err(|_| InitError::TooManyRegions)?;
        }
    }
    if set.is_empty() {
        return Err(InitError::NoMemory);
    }
    Ok(set)
}

/// The `u64` words of storage needed to manage `memory`.
pub fn storage_words(memory: &[Range]) -> Result<usize, InitError> {
    let set = whole_frames(memory)?;
    Ok(set
        .as_slice()
        .iter()
        .map(|r| 2 * words_for(r.len() / FRAME_SIZE))
        .sum())
}

/// A place for `words` words of metadata: the frames at the start of the
/// largest range of `memory` left free by `reserved`.
pub fn place_metadata(memory: &[Range], reserved: &[Range], words: usize) -> Option<Range> {
    let mut free = whole_frames(memory).ok()?;
    for r in reserved {
        free.remove(r.frames_around()).ok()?;
    }
    let largest = free.as_slice().iter().max_by_key(|r| r.len())?;
    let bytes = (words as u64)
        .checked_mul(8)?
        .checked_next_multiple_of(FRAME_SIZE)?;
    (bytes <= largest.len()).then_some(Range {
        start: largest.start,
        end: largest.start + bytes,
    })
}

/// The allocator. `'m` is the lifetime of its storage.
#[derive(Debug)]
pub struct FrameAllocator<'m> {
    regions: [Region; MAX_REGIONS],
    region_count: usize,
    storage: &'m mut [u64],
    stats: Stats,
    /// Next-fit position: region index and frame index within the region.
    hint: (usize, u64),
}

impl<'m> FrameAllocator<'m> {
    /// Builds the allocator over the whole frames of `memory`, minus every
    /// frame touched by `reserved` and frame 0 (address 0 stays usable as "no
    /// frame" for callers).
    pub fn new(
        memory: &[Range],
        reserved: &[Range],
        storage: &'m mut [u64],
    ) -> Result<FrameAllocator<'m>, InitError> {
        let set = whole_frames(memory)?;
        let mut regions = [Region::default(); MAX_REGIONS];
        let mut next = 0;
        let mut total = 0;
        for (slot, r) in regions.iter_mut().zip(set.as_slice()) {
            let frames = r.len() / FRAME_SIZE;
            let words = words_for(frames);
            *slot = Region {
                first: r.start / FRAME_SIZE,
                frames,
                used: next,
                reserved: next + words,
            };
            next += 2 * words;
            total += frames;
        }
        let storage = storage.get_mut(..next).ok_or(InitError::StorageTooSmall)?;
        storage.fill(0);

        let mut a = FrameAllocator {
            regions,
            region_count: set.as_slice().len(),
            storage,
            stats: Stats {
                total,
                reserved: 0,
                free: total,
            },
            hint: (0, 0),
        };
        a.seal_padding();
        for r in reserved {
            a.reserve(r.frames_around());
        }
        a.reserve(Range {
            start: 0,
            end: FRAME_SIZE,
        });
        Ok(a)
    }

    /// Marks the bits past each region's last frame as used.
    fn seal_padding(&mut self) {
        for region in &self.regions[..self.region_count] {
            let tail = region.frames % WORD_BITS;
            if tail != 0 {
                let last = region.used + region.words() - 1;
                self.storage[last] |= !0 << tail;
            }
        }
    }

    fn reserve(&mut self, range: Range) {
        for ri in 0..self.region_count {
            let region = self.regions[ri];
            let start = (range.start / FRAME_SIZE).max(region.first);
            // `frames_around` keeps `end` aligned or at the top of the
            // address space.
            let end = (range.end.div_ceil(FRAME_SIZE)).min(region.first + region.frames);
            for number in start..end {
                let (word, mask) = bit(number - region.first);
                if self.storage[region.reserved + word] & mask == 0 {
                    self.storage[region.reserved + word] |= mask;
                    self.storage[region.used + word] |= mask;
                    self.stats.reserved += 1;
                    self.stats.free -= 1;
                }
            }
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The managed RAM ranges, in address order.
    pub fn regions(&self) -> impl Iterator<Item = Range> + '_ {
        self.regions[..self.region_count].iter().map(|r| Range {
            start: r.first * FRAME_SIZE,
            end: (r.first + r.frames) * FRAME_SIZE,
        })
    }

    fn find(&self, frame_number: u64) -> Option<(Region, u64)> {
        self.regions[..self.region_count]
            .iter()
            .find(|r| r.contains(frame_number))
            .map(|r| (*r, frame_number - r.first))
    }

    pub fn state(&self, frame: Frame) -> FrameState {
        match self.find(frame.0 / FRAME_SIZE) {
            None => FrameState::Outside,
            Some((region, index)) => {
                let (word, mask) = bit(index);
                if self.storage[region.reserved + word] & mask != 0 {
                    FrameState::Reserved
                } else if self.storage[region.used + word] & mask != 0 {
                    FrameState::Allocated
                } else {
                    FrameState::Free
                }
            }
        }
    }

    /// Allocates one frame. Next-fit: the search continues after the last
    /// frame returned. The frame's contents are whatever was left in it.
    pub fn alloc(&mut self) -> Result<Frame, OutOfFrames> {
        if self.stats.free == 0 {
            return Err(OutOfFrames);
        }
        let (first_region, first_index) = self.hint;
        // One extra step: the wrap-around revisits the start word of the
        // region the search began in.
        for step in 0..=self.region_count {
            let ri = (first_region + step) % self.region_count;
            let region = self.regions[ri];
            let start_word = if step == 0 {
                (first_index / WORD_BITS) as usize
            } else {
                0
            };
            for w in start_word..region.words() {
                let word = self.storage[region.used + w];
                if word != u64::MAX {
                    let index = w as u64 * WORD_BITS + u64::from((!word).trailing_zeros());
                    self.storage[region.used + w] |= 1 << (index % WORD_BITS);
                    self.stats.free -= 1;
                    self.hint = (ri, index);
                    return Ok(Frame((region.first + index) * FRAME_SIZE));
                }
            }
        }
        Err(OutOfFrames)
    }

    /// Allocates `count` contiguous frames starting at a frame number that
    /// is a multiple of `align` (a power of two, in frames).
    pub fn alloc_contig(&mut self, count: u64, align: u64) -> Result<Frame, OutOfFrames> {
        if count == 0 || !align.is_power_of_two() || count > self.stats.free {
            return Err(OutOfFrames);
        }
        for ri in 0..self.region_count {
            let region = self.regions[ri];
            let mut number = region.first.next_multiple_of(align);
            let limit = region.first + region.frames;
            while number + count <= limit {
                if let Some(busy) = self.first_used(&region, number, number + count) {
                    number = (busy + 1).next_multiple_of(align);
                    continue;
                }
                for n in number..number + count {
                    let (word, mask) = bit(n - region.first);
                    self.storage[region.used + word] |= mask;
                }
                self.stats.free -= count;
                return Ok(Frame(number * FRAME_SIZE));
            }
        }
        Err(OutOfFrames)
    }

    /// The first frame number in `[start, end)` that is not free.
    fn first_used(&self, region: &Region, start: u64, end: u64) -> Option<u64> {
        (start..end).find(|n| {
            let (word, mask) = bit(n - region.first);
            self.storage[region.used + word] & mask != 0
        })
    }

    fn check_free(&self, frame_number: u64) -> Result<(Region, u64), FreeError> {
        let (region, index) = self.find(frame_number).ok_or(FreeError::OutOfRange)?;
        let (word, mask) = bit(index);
        if self.storage[region.reserved + word] & mask != 0 {
            return Err(FreeError::Reserved);
        }
        if self.storage[region.used + word] & mask == 0 {
            return Err(FreeError::NotAllocated);
        }
        Ok((region, index))
    }

    /// Frees one frame.
    pub fn free(&mut self, frame: Frame) -> Result<(), FreeError> {
        self.free_contig(frame, 1)
    }

    /// Frees `count` frames starting at `frame`. All or nothing: if any frame
    /// cannot be freed, none is.
    pub fn free_contig(&mut self, frame: Frame, count: u64) -> Result<(), FreeError> {
        let first = frame.0 / FRAME_SIZE;
        let end = first.checked_add(count).ok_or(FreeError::OutOfRange)?;
        for number in first..end {
            self.check_free(number)?;
        }
        for number in first..end {
            let (region, index) = self.check_free(number)?;
            let (word, mask) = bit(index);
            self.storage[region.used + word] &= !mask;
        }
        self.stats.free += count;
        Ok(())
    }
}
