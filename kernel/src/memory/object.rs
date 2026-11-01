//! Memory objects (ADR-0051): frames committed at creation, held by handles,
//! mappings and DMA pins, and freed with the last of them.

use alloc::sync::Arc;
use alloc::vec::Vec;

use zoraya_pmm::{FRAME_SIZE, Frame};

use super::frames;
use super::heap::try_arc;
use zoraya_abi::Status;

enum Frames {
    /// Any frames, in order.
    Scattered(Vec<Frame>),
    /// A run from one allocation.
    Run { first: Frame, count: u64 },
}

pub struct MemoryObject {
    frames: Frames,
}

impl MemoryObject {
    /// Commits `size` bytes, zeroed, or fails having changed nothing
    /// (INV-0051-1). `size` must be a whole number of pages, not 0.
    pub fn new(size: u64, contiguous: bool) -> Result<Arc<MemoryObject>, Status> {
        if size == 0 || !size.is_multiple_of(FRAME_SIZE) {
            return Err(Status::InvalidArgument);
        }
        let count = size / FRAME_SIZE;
        let frames = if contiguous {
            let first = frames::alloc_contig_zeroed(count).map_err(|_| Status::NoMemory)?;
            Frames::Run { first, count }
        } else {
            let mut list = Vec::new();
            let mut object = Frames::Scattered(Vec::new());
            list.try_reserve_exact(count as usize)
                .map_err(|_| Status::NoMemory)?;
            for _ in 0..count {
                match frames::alloc_zeroed() {
                    Ok(frame) => list.push(frame),
                    Err(_) => {
                        object = Frames::Scattered(list);
                        free(&mut object);
                        return Err(Status::NoMemory);
                    }
                }
            }
            drop(object);
            Frames::Scattered(list)
        };
        // On failure the value was dropped, and its `Drop` freed the frames.
        try_arc(MemoryObject { frames }).map_err(|_| Status::NoMemory)
    }

    pub fn size(&self) -> u64 {
        match &self.frames {
            Frames::Scattered(list) => list.len() as u64 * FRAME_SIZE,
            Frames::Run { count, .. } => count * FRAME_SIZE,
        }
    }

    /// The physical address of the page at `offset`, which must be in range
    /// and a page multiple.
    pub fn page(&self, offset: u64) -> u64 {
        let index = offset / FRAME_SIZE;
        match &self.frames {
            Frames::Scattered(list) => list[index as usize].addr(),
            Frames::Run { first, .. } => first.addr() + index * FRAME_SIZE,
        }
    }

    /// True if the frames are one physical run.
    pub fn runs(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        let mut at = 0;
        core::iter::from_fn(move || {
            if at >= self.size() {
                return None;
            }
            let start = self.page(at);
            let mut length = FRAME_SIZE;
            while at + length < self.size() && self.page(at + length) == start + length {
                length += FRAME_SIZE;
            }
            at += length;
            Some((start, length))
        })
    }
}

fn free(object: &mut Frames) {
    match object {
        Frames::Scattered(list) => {
            for frame in list.drain(..) {
                frames::free(frame);
            }
        }
        Frames::Run { first, count } => frames::free_contig(*first, *count),
    }
}

impl Drop for MemoryObject {
    fn drop(&mut self) {
        free(&mut self.frames);
    }
}
