//! Memory objects (ADR-0051): frames committed at creation, held by handles,
//! mappings and DMA pins, and freed with the last of them.

use alloc::sync::Arc;
use alloc::vec::Vec;

use zoraya_pmm::{FRAME_SIZE, Frame};

use super::{frames, phys_to_virt};
use crate::charge::{self, Paid};
use crate::job::Job;
use zoraya_abi::Status;

enum Frames {
    /// Any frames, in order.
    Scattered(Vec<Frame>),
    /// A run from one allocation.
    Run { first: Frame, count: u64 },
}

pub struct MemoryObject {
    frames: Frames,
    /// Its creator pays for its frames and its record for as long as it lives,
    /// however many jobs map or hold it (ADR-0261 point 8).
    _paid: Paid,
}

impl MemoryObject {
    /// Commits `size` bytes, zeroed, or fails having changed nothing
    /// (INV-0051-1). `size` must be a whole number of pages, not 0. `payer`
    /// is the creating job: `None` for the kernel's own objects.
    pub fn new(
        size: u64,
        contiguous: bool,
        payer: Option<&Arc<Job>>,
    ) -> Result<Arc<MemoryObject>, Status> {
        MemoryObject::make(size, contiguous, true, payer)
    }

    /// As [`MemoryObject::new`], scattered and not zeroed: for a copy that
    /// writes every byte before anything can reach the object (a snapshot's
    /// mapped objects, ADR-0102 item 2). The caller must do so.
    pub fn new_for_copy(size: u64, payer: Option<&Arc<Job>>) -> Result<Arc<MemoryObject>, Status> {
        MemoryObject::make(size, false, false, payer)
    }

    fn make(
        size: u64,
        contiguous: bool,
        zeroed: bool,
        payer: Option<&Arc<Job>>,
    ) -> Result<Arc<MemoryObject>, Status> {
        if size == 0 || !size.is_multiple_of(FRAME_SIZE) {
            return Err(Status::InvalidArgument);
        }
        let count = size / FRAME_SIZE;
        // The charge comes first: a refusal allocates nothing. The frames, then
        // the record and the list of frames a scattered object keeps.
        let mut paid = Paid::new(payer);
        paid.user(size)?;
        let list = if contiguous {
            0
        } else {
            charge::vec_cost::<Frame>(count as usize)
        };
        paid.kernel(charge::arc_cost::<MemoryObject>().saturating_add(list))?;
        let frames = if contiguous {
            let first = frames::alloc_contig_zeroed(count).map_err(|_| Status::NoMemory)?;
            Frames::Run { first, count }
        } else {
            let mut list = Vec::new();
            let mut object = Frames::Scattered(Vec::new());
            list.try_reserve_exact(count as usize)
                .map_err(|_| Status::NoMemory)?;
            for _ in 0..count {
                let frame = if zeroed {
                    frames::alloc_zeroed()
                } else {
                    frames::alloc()
                };
                match frame {
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
        // On failure the value was dropped, and its `Drop` freed the frames
        // and gave the charge back.
        charge::prepaid_arc(MemoryObject {
            frames,
            _paid: paid,
        })
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

    /// Copies the bytes from `offset` on into `out`, or gives `false` if they
    /// are not all inside the object.
    pub fn read(&self, offset: u64, out: &mut [u8]) -> bool {
        let end = offset.checked_add(out.len() as u64);
        if end.is_none_or(|end| end > self.size()) {
            return false;
        }
        let mut done = 0;
        while done < out.len() {
            let at = offset + done as u64;
            let in_page = (at % FRAME_SIZE) as usize;
            let length = (FRAME_SIZE as usize - in_page).min(out.len() - done);
            let from = phys_to_virt(self.page(at - in_page as u64)) + in_page as u64;
            // SAFETY: a frame of this object, in the direct map, alive while
            // `self` is; `length` stays inside its page and inside `out`. A
            // process may write it through a mapping, but not while the
            // kernel runs on the one CPU, so nothing changes under the copy.
            unsafe {
                core::ptr::copy_nonoverlapping(from as *const u8, out[done..].as_mut_ptr(), length);
            }
            done += length;
        }
        true
    }

    /// Copies `bytes` into the object at `offset`, for tests that hand a
    /// process prepared contents. False if they do not fit.
    #[cfg(feature = "qemu-test")]
    pub fn write(&self, offset: u64, bytes: &[u8]) -> bool {
        let end = offset.checked_add(bytes.len() as u64);
        if end.is_none_or(|end| end > self.size()) {
            return false;
        }
        for (i, byte) in bytes.iter().enumerate() {
            let at = offset + i as u64;
            let page = phys_to_virt(self.page(at - at % FRAME_SIZE));
            // SAFETY: a frame of this object in the direct map; `at % FRAME_SIZE`
            // is inside it.
            unsafe { ((page + at % FRAME_SIZE) as *mut u8).write(*byte) };
        }
        true
    }

    /// The physically contiguous runs, each `(address, length)`.
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
