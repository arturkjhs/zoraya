//! User address spaces (ADR-0031).

use alloc::sync::Arc;
use alloc::vec::Vec;

#[cfg(feature = "qemu-test")]
use zoraya_pagetable::MemoryType;
use zoraya_pagetable::{Attributes, Half, MapError, PageTable, TableMemory};
use zoraya_pmm::{FRAME_SIZE, Frame};

use super::object::MemoryObject;
use super::phys_to_virt;
use super::{frames, heap};
use crate::arch::mmu;
use crate::charge::Paid;
use crate::device::{Device, DeviceClaim, DeviceMapping};
use crate::job::Job;
use crate::reaper;
use crate::steps::{self, STEP};
use crate::sync::SpinLock;
use zoraya_abi::Status;

/// Lowest address a user mapping may have.
pub const USER_LOW: u64 = 0x40_0000;
/// One past the highest user address.
pub const USER_HIGH: u64 = 1 << 47;

/// Frames one step may add to a space's list: its pages, and the tables that
/// map them (a step of 64 pages crosses at most one boundary of each level).
pub const STEP_FRAMES: usize = STEP + 8;

/// Mappings a process may hold (ADR-0051).
pub const MAX_MAPPINGS: usize = 256;
/// Where mappings of objects go: the kernel chooses inside this range.
pub const MAP_LOW: u64 = 1 << 46;
pub const MAP_HIGH: u64 = 3 << 45;

/// ASIDs in use; ASID 0 belongs to no process.
static ASIDS: SpinLock<[u64; 4]> = SpinLock::new([1, 0, 0, 0]);

fn alloc_asid() -> Option<u16> {
    let mut asids = ASIDS.lock();
    for (i, word) in asids.iter_mut().enumerate() {
        if *word != u64::MAX {
            let bit = word.trailing_ones();
            *word |= 1 << bit;
            return Some(i as u16 * 64 + bit as u16);
        }
    }
    None
}

fn free_asid(asid: u16) {
    ASIDS.lock()[usize::from(asid / 64)] &= !(1 << (asid % 64));
}

/// Table memory from frames, which the address space owns along with every
/// frame it maps.
struct UserMemory {
    asid: u16,
    frames: Vec<Frame>,
    /// What the space's job pays for its frames: user frames and the tables
    /// that map them (ADR-0261 point 5), given back when it drops.
    paid: Paid,
    /// A charge was refused since this was last taken: how a table allocation
    /// that returns only `None` says why.
    refused: bool,
}

impl UserMemory {
    /// Keeps `frame`, or frees it and fails.
    fn adopt(&mut self, frame: Frame) -> Result<(), Status> {
        if self.frames.len() == self.frames.capacity() {
            self.count_list_copy();
        }
        if self.frames.try_reserve(1).is_err() {
            frames::free(frame);
            return Err(Status::NoMemory);
        }
        self.frames.push(frame);
        Ok(())
    }

    /// Counts the copy that growing the list makes (`GlobalAlloc::realloc` is
    /// allocate, copy, free): `len * 8` bytes, in units of a page, so that the
    /// test kernel sees it where it runs in a masked interval. It runs there
    /// when the list grows under the space's lock (INV-0270-1, ADR-0270 open
    /// questions).
    fn count_list_copy(&self) {
        steps::count(self.frames.len() * size_of::<Frame>() / FRAME_SIZE as usize);
    }

    /// The capacity the list of frames needs to take `more`, if it must grow:
    /// doubled, as `Vec` would, so that one growth serves many steps.
    fn capacity_for(&self, more: usize) -> Option<usize> {
        let need = self.frames.len().saturating_add(more);
        (need > self.frames.capacity()).then(|| need.max(self.frames.capacity() * 2))
    }

    /// Charges one frame of user memory, remembering a refusal.
    fn pay_user(&mut self) -> Result<(), Status> {
        self.paid
            .user(FRAME_SIZE)
            .inspect_err(|_| self.refused = true)
    }

    /// The status for a failure just seen: a refusal if one was made.
    fn failure(&mut self) -> Status {
        if core::mem::take(&mut self.refused) {
            Status::OverQuota
        } else {
            Status::NoMemory
        }
    }
}

impl TableMemory for UserMemory {
    fn alloc_table(&mut self) -> Option<u64> {
        // A page table is a kernel frame of the job's.
        if self.paid.kernel(FRAME_SIZE).is_err() {
            self.refused = true;
            return None;
        }
        let Ok(frame) = frames::alloc_zeroed() else {
            self.paid.release_kernel(FRAME_SIZE);
            return None;
        };
        if self.adopt(frame).is_err() {
            self.paid.release_kernel(FRAME_SIZE);
            return None;
        }
        Some(frame.addr())
    }

    fn read(&self, table: u64, index: usize) -> u64 {
        // SAFETY: `table` is a frame this space allocated for a table, which
        // the direct map holds read-write, and `index` is below 512.
        unsafe { ((phys_to_virt(table) as *const u64).add(index)).read_volatile() }
    }

    fn write(&mut self, table: u64, index: usize, value: u64) {
        // SAFETY: as in `read`; the owner holds the process's `space` lock while
        // it changes this table, or it is still being built by one thread.
        unsafe { ((phys_to_virt(table) as *mut u64).add(index)).write_volatile(value) }
    }

    fn invalidate(&mut self, _va: u64, _size: u64) {
        mmu::invalidate_asid(self.asid);
    }

    fn sync(&mut self) {
        mmu::sync_tables();
    }
}

impl Holder {
    /// The work of dropping it, for `steps::before`.
    pub fn weight(&self) -> usize {
        match self {
            Holder::Memory(object) => super::object::weight(object),
            Holder::Device(_) => 1,
        }
    }
}

/// What a mapping keeps alive.
pub enum Holder {
    // Read by nothing: holding them is the point.
    #[allow(dead_code)]
    Memory(Arc<MemoryObject>),
    #[allow(dead_code)]
    Device(DeviceMapping),
}

struct Mapping {
    va: u64,
    length: u64,
    attributes: Attributes,
    holder: Holder,
}

/// Why a mapping was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingError {
    /// The process has `MAX_MAPPINGS`.
    Limit,
    Memory,
    /// The job's limit refused the page tables.
    Quota,
    /// No mapping is exactly that range.
    NotMapped,
}

/// One process's memory. Dropping it frees every frame and its ASID.
pub struct AddressSpace {
    table: PageTable,
    memory: UserMemory,
    /// By address.
    mappings: Vec<Mapping>,
    /// The transport devices this process holds (INV-0295-3): one entry
    /// each, ended when it holds no handle that can write one, or ends.
    claims: Vec<DeviceClaim>,
}

impl AddressSpace {
    /// The bytes the list of frames would allocate to take a step of
    /// [`STEP_FRAMES`] more, or 0 if it has room: what [`heap::prepare`]
    /// maps ahead before a step, with no lock held.
    pub fn step_growth(&self) -> usize {
        self.memory
            .capacity_for(STEP_FRAMES)
            .map_or(0, |capacity| capacity * size_of::<Frame>())
    }

    /// Makes room in the list of frames for a step, after
    /// `heap::prepare(step_growth())`, so that the step's frames and tables
    /// do not grow the list, and with it the heap, in the step's masked
    /// interval (INV-0270-1). The list grows with what the space holds: its
    /// doubling was a growth of the heap proportional to the space. Best
    /// effort: if the heap has no room, the step's first frame reports it.
    pub fn reserve_step(&mut self) {
        if let Some(capacity) = self.memory.capacity_for(STEP_FRAMES) {
            let more = capacity - self.memory.frames.len();
            self.memory.count_list_copy();
            let _ = self.memory.frames.try_reserve_exact(more);
        }
    }

    /// The length and capacity of the list of frames, for the test kernel.
    #[cfg(feature = "qemu-test")]
    pub fn list_state(&self) -> (usize, usize) {
        (self.memory.frames.len(), self.memory.frames.capacity())
    }

    /// [`heap::prepare`] and [`AddressSpace::reserve_step`], for a space no
    /// lock holds (one being built: `spawn`, a snapshot's child).
    ///
    /// The copy the list's growth makes is work (counted, `count_list_copy`)
    /// and gets an interval of its own: a step point after it, so that the
    /// step's pages are not added to it (INV-0270-1). No lock may be held.
    pub fn prepare_step(&mut self) {
        let grows = self.step_growth() > 0;
        heap::prepare(self.step_growth());
        self.reserve_step();
        if grows {
            steps::pause();
        }
    }

    /// An empty space whose frames `payer` pays for (`None`: the kernel's).
    pub fn new(payer: Option<&Arc<Job>>) -> Result<AddressSpace, Status> {
        let asid = alloc_asid().ok_or(Status::NoMemory)?;
        let mut memory = UserMemory {
            asid,
            frames: Vec::new(),
            paid: Paid::new(payer),
            refused: false,
        };
        match PageTable::new(&mut memory, Half::Lower) {
            Ok(table) => Ok(AddressSpace {
                table,
                memory,
                mappings: Vec::new(),
                claims: Vec::new(),
            }),
            Err(_) => {
                let failure = memory.failure();
                // The frames it took go back through the memory's own list.
                for frame in memory.frames.drain(..) {
                    frames::free(frame);
                }
                free_asid(asid);
                Err(failure)
            }
        }
    }

    /// The value for `TTBR0_EL1`.
    pub fn ttbr0(&self) -> u64 {
        self.table.root() | (u64::from(self.memory.asid) << 48)
    }

    /// Maps a new zeroed page at `va` and returns the address the kernel can
    /// fill it through.
    pub fn map_new_page(&mut self, va: u64, attributes: Attributes) -> Result<*mut u8, MapFailed> {
        self.memory.pay_user().map_err(|_| MapFailed::Quota)?;
        let frame = frames::alloc_zeroed().map_err(|_| {
            self.memory.paid.release_user(FRAME_SIZE);
            MapFailed::Memory
        })?;
        self.memory.adopt(frame).map_err(|_| {
            self.memory.paid.release_user(FRAME_SIZE);
            MapFailed::Memory
        })?;
        self.table
            .map_pages(&mut self.memory, va, frame.addr(), FRAME_SIZE, attributes)
            .map_err(|e| {
                if core::mem::take(&mut self.memory.refused) {
                    MapFailed::Quota
                } else {
                    MapFailed::Table(e)
                }
            })?;
        Ok(phys_to_virt(frame.addr()) as *mut u8)
    }

    /// The lowest place for `length` bytes, with an unmapped page after what
    /// comes before it, so that running off a mapping faults.
    fn find_gap(&self, length: u64) -> Option<(usize, u64)> {
        let mut at = MAP_LOW;
        for (i, m) in self.mappings.iter().enumerate() {
            if at.checked_add(length)? < m.va {
                return Some((i, at));
            }
            at = m.va + m.length + FRAME_SIZE;
        }
        (at.checked_add(length)? <= MAP_HIGH).then_some((self.mappings.len(), at))
    }

    /// Maps pages given by `page`, a function of their offset, and keeps
    /// `holder` until they are unmapped.
    pub fn map_object(
        &mut self,
        length: u64,
        attributes: Attributes,
        holder: Holder,
        page: impl Fn(u64) -> u64,
    ) -> Result<u64, MappingError> {
        if self.mappings.len() >= MAX_MAPPINGS {
            return Err(MappingError::Limit);
        }
        self.mappings
            .try_reserve(1)
            .map_err(|_| MappingError::Memory)?;
        let (index, va) = self.find_gap(length).ok_or(MappingError::Memory)?;
        for offset in (0..length).step_by(FRAME_SIZE as usize) {
            let mapped = self.table.map_pages(
                &mut self.memory,
                va + offset,
                page(offset),
                FRAME_SIZE,
                attributes,
            );
            if mapped.is_err() {
                if offset > 0 {
                    let _ = self.table.unmap(&mut self.memory, va, offset);
                }
                return Err(match self.memory.failure() {
                    Status::OverQuota => MappingError::Quota,
                    _ => MappingError::Memory,
                });
            }
        }
        self.mappings.insert(
            index,
            Mapping {
                va,
                length,
                attributes,
                holder,
            },
        );
        Ok(va)
    }

    /// The first of the three parts of a mapping made in steps (ADR-0270):
    /// checks the room and finds the address. Nothing is changed.
    pub fn map_reserve(&mut self, length: u64) -> Result<u64, MappingError> {
        if self.mappings.len() >= MAX_MAPPINGS {
            return Err(MappingError::Limit);
        }
        self.mappings
            .try_reserve(1)
            .map_err(|_| MappingError::Memory)?;
        let (_, va) = self.find_gap(length).ok_or(MappingError::Memory)?;
        Ok(va)
    }

    /// The second part: maps the pages of `[offset, offset + length)` of the
    /// range at `va`. On failure it unmaps what it did of this range, and the
    /// caller what it did before (see [`AddressSpace::unmap_range`]).
    pub fn map_range(
        &mut self,
        va: u64,
        offset: u64,
        length: u64,
        attributes: Attributes,
        page: &impl Fn(u64) -> u64,
    ) -> Result<(), MappingError> {
        for at in (offset..offset + length).step_by(FRAME_SIZE as usize) {
            let mapped =
                self.table
                    .map_pages(&mut self.memory, va + at, page(at), FRAME_SIZE, attributes);
            if mapped.is_err() {
                if at > offset {
                    let _ = self.table.unmap(&mut self.memory, va + offset, at - offset);
                }
                return Err(match self.memory.failure() {
                    Status::OverQuota => MappingError::Quota,
                    _ => MappingError::Memory,
                });
            }
        }
        Ok(())
    }

    /// Unmaps `[va + offset, va + offset + length)` of a range being built or
    /// ended, without touching the list of mappings.
    pub fn unmap_range(&mut self, va: u64, offset: u64, length: u64) {
        let _ = self.table.unmap(&mut self.memory, va + offset, length);
    }

    /// The third part: the range is mapped; records it and keeps `holder`.
    pub fn map_commit(&mut self, va: u64, length: u64, attributes: Attributes, holder: Holder) {
        let index = self.mappings.partition_point(|m| m.va < va);
        // The room was reserved by `map_reserve`, and nothing else maps in
        // between: the process's one thread is in this call.
        self.mappings.insert(
            index,
            Mapping {
                va,
                length,
                attributes,
                holder,
            },
        );
    }

    /// Whether a mapping is exactly `[va, va + length)`.
    pub fn is_mapped_exactly(&self, va: u64, length: u64) -> bool {
        self.mappings
            .iter()
            .any(|m| m.va == va && m.length == length)
    }

    /// Removes the record of the mapping that is exactly `[va, va + length)`
    /// after its pages were unmapped, and gives its holder back.
    pub fn unmap_commit(&mut self, va: u64, length: u64) -> Result<Holder, MappingError> {
        let index = self
            .mappings
            .iter()
            .position(|m| m.va == va && m.length == length)
            .ok_or(MappingError::NotMapped)?;
        Ok(self.mappings.remove(index).holder)
    }

    /// Takes the last mapping off the list and gives its holder, for an exit
    /// that drops them in steps. The pages stay in the table, which nothing
    /// runs on again: the table goes with the space.
    /// Makes this process the holder of transport device `device`, if it
    /// is not already (INV-0295-3).
    pub fn hold(&mut self, device: &Arc<Device>) -> Result<&DeviceClaim, Status> {
        let at = match self
            .claims
            .iter()
            .position(|c| Arc::ptr_eq(c.device(), device))
        {
            Some(at) => at,
            None => {
                // Not charged, as a pin is not (ADR-0261 point 6): a
                // driver's, and at most one per transport device, which are
                // fixed at boot.
                self.claims.try_reserve(1).map_err(|_| Status::NoMemory)?;
                self.claims.push(device.claim()?);
                self.claims.len() - 1
            }
        };
        Ok(&self.claims[at])
    }

    /// Ends this process's hold of `device`, if it has one.
    pub fn let_go(&mut self, device: &Arc<Device>) -> Option<DeviceClaim> {
        let at = self
            .claims
            .iter()
            .position(|c| Arc::ptr_eq(c.device(), device))?;
        Some(self.claims.swap_remove(at))
    }

    pub fn pop_mapping(&mut self) -> Option<Holder> {
        self.mappings.pop().map(|m| m.holder)
    }

    /// True if any mapping is of a device window (ADR-0054).
    pub fn has_device_mapping(&self) -> bool {
        self.mappings
            .iter()
            .any(|m| matches!(m.holder, Holder::Device(_)))
    }

    /// What a snapshot of this space needs to know before it starts: the
    /// range, length and attributes of each mapping of an object. A device
    /// mapping refuses it with `BadState` (ADR-0102 item 2).
    pub fn snapshot_plan(&self) -> Result<Vec<(u64, u64, Attributes)>, Status> {
        if self.has_device_mapping() {
            return Err(Status::BadState);
        }
        let mut plan = Vec::new();
        plan.try_reserve_exact(self.mappings.len())
            .map_err(|_| Status::NoMemory)?;
        plan.extend(self.mappings.iter().map(|m| (m.va, m.length, m.attributes)));
        Ok(plan)
    }

    /// One step of a snapshot's copy (ADR-0102 item 2, ADR-0270): up to
    /// `STEP` pages from where `build` stopped, each into a fresh frame of the
    /// child or a page of the copy of its mapped object, with the same
    /// contents and attributes. Gives `true` when the whole space was copied.
    /// The caller holds this space's lock for the step only.
    pub fn snapshot_step(
        &self,
        build: &mut SnapshotBuild,
        copied: &mut Copied,
    ) -> Result<bool, Status> {
        let mut failed = None;
        let mut taken = 0;
        let mut next = build.next;
        let reached_end = self
            .table
            .for_each_mapping_from(&self.memory, build.next, &mut |leaf| {
                for offset in (0..leaf.size).step_by(FRAME_SIZE as usize) {
                    let (va, pa) = (leaf.va + offset, leaf.pa + offset);
                    // Mappings are kept by address, and do not overlap.
                    let i = self.mappings.partition_point(|m| m.va + m.length <= va);
                    let target = match self.mappings.get(i) {
                        Some(m) if m.va <= va => build.objects[i].page(va - m.va),
                        _ => match build.child.copy_page(va, leaf.attributes) {
                            Ok(pa) => pa,
                            Err(e) => {
                                failed = Some(e);
                                return false;
                            }
                        },
                    };
                    // SAFETY: both are whole frames of ordinary RAM in the
                    // direct map: `pa` is mapped by this space, which the
                    // caller's lock keeps, and `target` was just taken for the
                    // copy and is reached by nothing else yet.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            phys_to_virt(pa) as *const u8,
                            phys_to_virt(target) as *mut u8,
                            FRAME_SIZE as usize,
                        );
                    }
                    if leaf.attributes.execute {
                        // SAFETY: the frame just written, in the direct map.
                        unsafe { mmu::clean_to_unification(phys_to_virt(target), FRAME_SIZE) };
                        build.executable = true;
                    }
                    copied.pages += 1;
                    taken += 1;
                }
                next = leaf.va + leaf.size;
                taken < STEP
            });
        if let Some(e) = failed {
            return Err(e);
        }
        build.next = next;
        Ok(reached_end)
    }

    /// Maps a new frame at `va` for [`AddressSpace::snapshot_step`] to fill: not
    /// zeroed, since the whole page is written before anything can read it.
    fn copy_page(&mut self, va: u64, attributes: Attributes) -> Result<u64, Status> {
        self.memory.pay_user()?;
        let frame = frames::alloc().map_err(|_| {
            self.memory.paid.release_user(FRAME_SIZE);
            Status::NoMemory
        })?;
        self.memory.adopt(frame).inspect_err(|_| {
            self.memory.paid.release_user(FRAME_SIZE);
        })?;
        self.table
            .map_pages(&mut self.memory, va, frame.addr(), FRAME_SIZE, attributes)
            .map_err(|_| self.memory.failure())?;
        Ok(frame.addr())
    }

    /// The 64-bit value at `va`, which must be mapped and aligned.
    #[cfg(feature = "qemu-test")]
    pub fn peek(&self, va: u64) -> Option<u64> {
        let mut found = None;
        self.for_each_mapping(&mut |m| {
            let normal = m.attributes.memory == MemoryType::Normal;
            if normal && (m.va..m.va + m.size).contains(&va) {
                found = Some(m.pa + (va - m.va));
            }
        });
        // SAFETY: RAM of this space or of an object it maps, which the direct
        // map holds: a device window has no direct-map entry, and is left out.
        found.map(|pa| unsafe { (phys_to_virt(pa) as *const u64).read_volatile() })
    }

    /// Calls `f` for every mapping.
    #[cfg(feature = "qemu-test")]
    pub fn for_each_mapping(&self, f: &mut impl FnMut(zoraya_pagetable::Mapping)) {
        self.table.for_each_mapping(&self.memory, f);
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        mmu::invalidate_asid(self.memory.asid);
        // Only now can nothing reach what the mappings held.
        self.mappings.clear();
        // A large space's frames go to the reaper with the charge, which
        // stays until they are freed (ADR-0270 point 4); the ASID is free
        // at once, since its entries are gone and nothing runs on the root.
        let frames = core::mem::take(&mut self.memory.frames);
        let paid = core::mem::take(&mut self.memory.paid);
        reaper::release(frames, paid);
        free_asid(self.memory.asid);
    }
}

/// A snapshot being built in steps (ADR-0102, ADR-0270): the child space, the
/// copy of each mapped object, and where the copy of the parent stopped. It is
/// in no table: dropping it, as a cancelled call does, gives everything back.
pub struct SnapshotBuild {
    child: AddressSpace,
    plan: Vec<(u64, u64, Attributes)>,
    objects: Vec<Arc<MemoryObject>>,
    /// The address the next step of the copy resumes at.
    next: u64,
    executable: bool,
}

impl SnapshotBuild {
    /// The child and a fresh object, not zeroed, for every mapping of `plan`.
    /// The objects are made in steps (`MemoryObject::new_for_copy`).
    pub fn new(
        plan: Vec<(u64, u64, Attributes)>,
        payer: Option<&Arc<Job>>,
    ) -> Result<SnapshotBuild, Status> {
        let mut child = AddressSpace::new(payer)?;
        child
            .mappings
            .try_reserve_exact(plan.len())
            .map_err(|_| Status::NoMemory)?;
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(plan.len())
            .map_err(|_| Status::NoMemory)?;
        for (_, length, _) in &plan {
            // Every page is written by the copy before the child exists.
            objects.push(MemoryObject::new_for_copy(*length, payer)?);
        }
        Ok(SnapshotBuild {
            child,
            plan,
            objects,
            next: 0,
            executable: false,
        })
    }

    /// Before a step of the copy: room for its frames in the child's list,
    /// mapped ahead in steps (`AddressSpace::prepare_step`). No lock held.
    pub fn prepare_step(&mut self) {
        self.child.prepare_step();
    }

    /// Maps the copies of the objects into the child, in steps, and gives the
    /// child. The copy of the parent must be complete.
    pub fn finish(mut self) -> Result<AddressSpace, Status> {
        if self.executable {
            // After every clean: fetches through the child's mappings then
            // see the copies (Arm ARM D7.5.9).
            mmu::invalidate_instruction_cache();
        }
        let mut pages = 0usize;
        let plan = core::mem::take(&mut self.plan);
        let objects = core::mem::take(&mut self.objects);
        for ((va, length, attributes), object) in plan.into_iter().zip(objects) {
            for offset in (0..length).step_by(FRAME_SIZE as usize) {
                if pages.is_multiple_of(STEP) {
                    if pages > 0 {
                        steps::between()?;
                    }
                    self.child.prepare_step();
                }
                pages += 1;
                self.child
                    .table
                    .map_pages(
                        &mut self.child.memory,
                        va + offset,
                        object.page(offset),
                        FRAME_SIZE,
                        attributes,
                    )
                    .map_err(|_| self.child.memory.failure())?;
            }
            self.child.mappings.push(Mapping {
                va,
                length,
                attributes,
                holder: Holder::Memory(object),
            });
        }
        Ok(self.child)
    }
}

/// What a snapshot copied, for its measurement.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Copied {
    /// Pages copied: the process's own and those of mapped objects.
    pub pages: u64,
}

/// Why a page could not be mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapFailed {
    Memory,
    /// The job's limit refused the frame or a table for it.
    Quota,
    Table(MapError),
}
