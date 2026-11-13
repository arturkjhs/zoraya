//! System calls (ADR-0032). Nothing here trusts a register: every value comes
//! from user space, and every failure is a [`Status`]. `zoraya-abi` decodes
//! the registers and calls the [`Handlers`] here (ADR-0044).

use alloc::sync::Arc;
use alloc::vec::Vec;

use zoraya_abi::quota::{JobLimits, LIMITS_SIZE, USAGE_SIZE, USAGE_SIZE_THREADS};
use zoraya_abi::snapshot::{MAX_SNAPSHOT_SIZE, SnapshotRequest};
use zoraya_abi::spawn::{MAX_SPAWN_SIZE, SpawnRequest, Transfer};
use zoraya_abi::{
    Address, Buffer, Bytes, ChannelEntry, ChannelObject, ChannelSizes, Cookie, Count,
    DEADLINE_INFINITE, Detail, DeviceObject, DirectoryEntry, Disposition, EntryPoint, EventObject,
    ExitCode, ExitReason, ExitRecord, Handle, Handlers, ImageObject, InterruptObject, JobFlags,
    JobObject, MapFlags, MemoryFlags, MemoryObject as MemoryType, Mode, ObjectType, Opaque, Out,
    Packet, PacketBuffer, PathBuffer, PinObject, PortObject, ProcessObject, RangeIndex,
    RelativePath, RequestKind, Rights, SignalSet, Size, SnapshotBuffer, SpawnBuffer, Status,
    StreamReadObject, StreamWriteObject, UsageFlags, Word32, request_value,
};
use zoraya_console::INPUT_SIZE;
use zoraya_elf::{EHDR_SIZE, Header, Program};
use zoraya_pagetable::{Attributes, MemoryType as PageMemory};
use zoraya_pmm::FRAME_SIZE;
use zoraya_ramdisk::NAME_SIZE;
use zoraya_time::{Deadline, Instant};

use crate::arch::{self, Frame};
use crate::channel::{self, MAX_MESSAGE, MAX_MESSAGE_HANDLES, Pair};
use crate::charge::{self, Paid};
use crate::device::Pin;
use crate::job::Job;
use crate::memory::heap;
use crate::memory::object::MemoryObject;
use crate::memory::user::{AddressSpace, Copied, Holder, MappingError, SnapshotBuild, USER_HIGH};
use crate::object::{self, Event, HandleTable, Image, MemoryImage, Object};
use crate::port::Port;
use crate::process::{self, Loadable, PROGRAM_HIGH, PROGRAM_LOW, Process, Snapshot, Start};
use crate::stream::Stream;
use crate::thread::{self, MapGuard};
use crate::{console, port, print, ramdisk, sched, steps, timer};

/// Longest `console_write`.
///
/// System calls run with interrupts masked, and the UART is polled, so this
/// bounds how long one call can hold off everything else. 4 KiB took about
/// 60 ms under QEMU (from 690 ms measured for 64 KiB).
const MAX_WRITE: u64 = 4 * 1024;

/// Packets one `port_wait` returns at most. They sit on the kernel stack.
const MAX_PACKETS: usize = 64;

/// A range in the user half. The only way to reach user memory (ADR-0031).
pub struct UserSlice {
    address: u64,
    len: usize,
}

impl UserSlice {
    pub fn new(address: u64, len: u64) -> Result<UserSlice, Status> {
        match address.checked_add(len) {
            Some(end) if end <= USER_HIGH => Ok(UserSlice {
                address,
                len: len as usize,
            }),
            _ => Err(Status::BadAddress),
        }
    }

    fn of(buffer: Buffer) -> Result<UserSlice, Status> {
        UserSlice::new(buffer.address, buffer.length)
    }

    /// The part of the range from `offset` on.
    fn at(&self, offset: u64) -> Result<UserSlice, Status> {
        let len = (self.len as u64)
            .checked_sub(offset)
            .ok_or(Status::InvalidArgument)?;
        UserSlice::new(self.address + offset, len)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Copies `out.len()` bytes from `at` in the range into `out`.
    pub fn read_into(&self, at: u64, out: &mut [u8]) -> Result<(), Status> {
        let end = at.checked_add(out.len() as u64);
        if end.is_none_or(|end| end > self.len as u64) {
            return Err(Status::InvalidArgument);
        }
        // SAFETY: `out` is writable for its length, and any user address is
        // safe: the access is checked as EL0's, and a fault returns an error.
        unsafe { arch::copy_from_user(out, self.address + at) }.map_err(|_| Status::BadAddress)
    }

    /// Copies `bytes` to `at` in the range, which must hold them.
    pub fn write_at(&self, at: u64, bytes: &[u8]) -> Result<(), Status> {
        self.at(at)?.write(bytes)
    }

    /// Copies the range into a new vector.
    pub fn read(&self) -> Result<Vec<u8>, Status> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.len)
            .map_err(|_| Status::NoMemory)?;
        bytes.resize(self.len, 0);
        // SAFETY: `bytes` has `len` bytes, and any user address is safe: the
        // access is checked as EL0's, and a fault returns an error.
        unsafe { arch::copy_from_user(&mut bytes, self.address) }
            .map_err(|_| Status::BadAddress)?;
        Ok(bytes)
    }

    /// Copies `bytes`, which must fit, to the start of the range.
    pub fn write(&self, bytes: &[u8]) -> Result<(), Status> {
        if bytes.len() > self.len {
            return Err(Status::InvalidArgument);
        }
        // SAFETY: any user address is safe: the access is checked as EL0's,
        // and a fault returns an error.
        unsafe { arch::copy_to_user(self.address, bytes) }.map_err(|_| Status::BadAddress)
    }
}

fn deadline(raw: zoraya_abi::Deadline) -> Deadline {
    match raw.0 {
        DEADLINE_INFINITE => Deadline::Infinite,
        nanos => Deadline::At(Instant::from_nanos(nanos)),
    }
}

/// The calling process, for one call. A call that ends the process leaves its
/// record in `exit`: `process::exit_current` does not return, so it must run
/// after this has been dropped, or the count here would never go.
struct Calls<'f> {
    process: Arc<Process>,
    exit: Option<ExitRecord>,
    /// `thread_exit` ran: the calling thread leaves after the call, and the
    /// process goes on unless it was the last (ADR-0300 item 3).
    end_thread: bool,
    /// The caller's registers at its `svc`, for `snapshot_process`.
    frame: &'f Frame,
}

/// The handles a `Move` list holds from its check until the child exists
/// (ADR-0313 item 7): held handles are seen, duplicated, closed and moved by
/// no sibling, so the checked entry is the one that leaves, however long the
/// call pauses. Dropping the claim gives them all back; `finish` takes them.
struct MoveClaim {
    process: Arc<Process>,
    held: Vec<Handle>,
    taken: Vec<zoraya_handle::Entry<Object>>,
    /// The table keeps a room for the child's handle (ADR-0313 item 8).
    kept: bool,
}

impl MoveClaim {
    /// Room for `room` handles, so that `hold` and `finish` allocate nothing.
    fn new(process: &Arc<Process>, room: usize) -> Result<MoveClaim, Status> {
        let mut held = Vec::new();
        held.try_reserve_exact(room).map_err(|_| Status::NoMemory)?;
        let mut taken = Vec::new();
        taken
            .try_reserve_exact(room)
            .map_err(|_| Status::NoMemory)?;
        Ok(MoveClaim {
            process: process.clone(),
            held,
            taken,
            kept: false,
        })
    }

    /// Keeps a room in the caller's table for the child's handle, so that the
    /// insert after the pause cannot fail with the child already made.
    fn keep_room(&mut self, handles: &mut HandleTable) -> Result<(), Status> {
        handles.keep_room()?;
        self.kept = true;
        Ok(())
    }

    /// The child's handle, into the kept room: cannot fail.
    fn insert_child(&mut self, child: Arc<Process>) -> Result<Handle, Status> {
        self.kept = false;
        self.process.handles.lock().insert_kept(
            Object::Process(child),
            ProcessObject::RIGHTS,
            object::next_id(),
        )
    }

    /// Checks `handle` for `rights` and holds it. At most `room` times.
    fn hold(
        &mut self,
        handles: &mut HandleTable,
        handle: Handle,
        rights: Rights,
    ) -> Result<zoraya_handle::Entry<Object>, Status> {
        let entry = handles.begin_move(handle, rights)?;
        self.held.push(handle);
        Ok(entry)
    }

    /// The held entries, out of the table in one hold of its lock.
    fn finish(&mut self) -> Vec<zoraya_handle::Entry<Object>> {
        let mut handles = self.process.handles.lock();
        for handle in self.held.drain(..) {
            if let Some(entry) = handles.finish_move(handle) {
                self.taken.push(entry);
            }
        }
        core::mem::take(&mut self.taken)
    }
}

impl Drop for MoveClaim {
    fn drop(&mut self) {
        if self.held.is_empty() && !self.kept {
            return;
        }
        let mut handles = self.process.handles.lock();
        for handle in self.held.drain(..) {
            handles.cancel_move(handle);
        }
        if self.kept {
            handles.drop_kept();
        }
    }
}

/// What the last `snapshot_process` cost: nanoseconds with interrupts
/// masked, and pages copied. For the measurements of TASK-16.
pub static LAST_SNAPSHOT: crate::sync::SpinLock<(u64, u64)> = crate::sync::SpinLock::new((0, 0));

/// The name a process from a memory image gets: its `argv[0]`, if that is
/// short UTF-8, since the image itself has no name.
fn image_name<'a>(request: &SpawnRequest<'a>) -> &'a str {
    request
        .argv
        .clone()
        .next()
        .filter(|a| !a.is_empty() && a.len() <= 32)
        .and_then(|a| core::str::from_utf8(a).ok())
        .unwrap_or("image")
}

impl Calls<'_> {
    fn object(&self, handle: Handle, rights: Rights) -> Result<Object, Status> {
        let handles = self.process.handles.lock();
        Ok(handles.check(handle, rights)?.object.clone())
    }

    fn other_process(&self, handle: Handle, rights: Rights) -> Result<Arc<Process>, Status> {
        match self.object(handle, rights)? {
            Object::Process(process) => Ok(process),
            _ => Err(Status::BadHandle),
        }
    }

    fn job(&self, handle: Handle, rights: Rights) -> Result<Arc<Job>, Status> {
        match self.object(handle, rights)? {
            Object::Job(job) => Ok(job),
            _ => Err(Status::BadHandle),
        }
    }

    fn port(&self, handle: Handle, rights: Rights) -> Result<Arc<Port>, Status> {
        match self.object(handle, rights)? {
            Object::Port(port) => Ok(port),
            _ => Err(Status::BadHandle),
        }
    }

    /// The child's table for `request`, from the caller's, which stays as it
    /// is: every check comes before anything is allocated (ADR-0102 items 3,
    /// 4). Also reserves the caller's room for the child's handle.
    fn snapshot_table(
        &self,
        request: &SnapshotRequest<'_>,
        claim: &mut MoveClaim,
    ) -> Result<(HandleTable, Option<Handle>), Status> {
        let mut handles = self.process.handles.lock();
        claim.keep_room(&mut handles)?;
        if handles
            .entries()
            .any(|(_, e)| matches!(e.object, Object::Pin(_)))
        {
            return Err(Status::BadState);
        }
        for moved in request.moves() {
            let entry = handles.check(moved.handle, Rights::TRANSFER)?;
            if let Object::Channel(pair, end) = &entry.object
                && pair.holds_handles(*end)
            {
                return Err(Status::BadState);
            }
        }
        let self_slot = request.self_slot;
        let top = request.moves().map(|m| m.slot).chain(self_slot).max();
        let mut table = handles.empty_like(top.unwrap_or(0))?;
        let named = |slot: u32| Some(slot) == self_slot || request.moves().any(|m| m.slot == slot);
        let moved = |h: Handle| request.moves().any(|m| m.handle == h);
        for (handle, entry) in handles.entries() {
            let duplicable = entry.object.duplicable() && entry.rights.contains(Rights::DUPLICATE);
            if duplicable && !named(handle.slot()) && !moved(handle) {
                table.place(
                    handle,
                    entry.object.clone(),
                    entry.rights,
                    object::next_id(),
                )?;
            }
        }
        for m in request.moves() {
            let entry = handles.check(m.handle, Rights::TRANSFER)?;
            let at = Handle::from_parts(m.slot, handles.generation_of(m.slot))
                .ok_or(Status::InvalidArgument)?;
            table.place(at, entry.object.clone(), entry.rights, object::next_id())?;
        }
        let self_handle = match self_slot {
            Some(slot) => Some(
                Handle::from_parts(slot, handles.generation_of(slot))
                    .ok_or(Status::InvalidArgument)?,
            ),
            None => None,
        };
        // Last, after every check: the moved handles are held until the child
        // exists, as in `spawn`. (A handle listed twice never gets here: the
        // decode refuses it, ADR-0313 item 7.)
        for m in request.moves() {
            claim.hold(&mut handles, m.handle, Rights::TRANSFER)?;
        }
        Ok((table, self_handle))
    }

    /// INV-0295-3: the hold of a transport device ends with the last handle
    /// of this process that can write it. Called with the table unlocked
    /// after a handle to `object` left it, by a close or by a `Move` (spawn,
    /// channel_write, snapshot). Nothing here pauses, so no sibling adds a
    /// handle meanwhile; and the kernel is not preempted on its one CPU
    /// (design.md, Scheduler), so the process a moved handle went to runs
    /// only after the hold has ended.
    fn end_hold_if_unwritable(&self, object: &Object) {
        let Object::Device(device) = object else {
            return;
        };
        let writable = self.process.handles.lock().entries().any(|(_, e)| {
            e.rights.contains(Rights::WRITE)
                && matches!(&e.object, Object::Device(d) if Arc::ptr_eq(d, device))
        });
        if writable {
            return;
        }
        let claim = self
            .process
            .space
            .lock()
            .as_mut()
            .and_then(|s| s.let_go(device));
        drop(claim);
    }

    /// Unmaps the mapping that is exactly `[va, va + length)`, in steps, with
    /// the mapping lock held by the caller.
    fn unmap_exact(&self, va: u64, length: u64) -> Result<(), Status> {
        {
            let space = self.process.space.lock();
            let space = space.as_ref().ok_or(Status::BadState)?;
            if !space.is_mapped_exactly(va, length) {
                return Err(Status::InvalidArgument);
            }
        }
        // Unmapping goes on to its end whatever the process was asked
        // (ADR-0270): a pause after each step, no cancel.
        let step = (steps::STEP as u64) * FRAME_SIZE;
        let mut done = 0;
        while done < length {
            let n = step.min(length - done);
            {
                let mut space = self.process.space.lock();
                let space = space.as_mut().ok_or(Status::BadState)?;
                space.unmap_range(va, done, n);
            }
            done += n;
            if done < length {
                steps::pause();
            }
        }
        let holder = {
            let mut space = self.process.space.lock();
            let space = space.as_mut().ok_or(Status::BadState)?;
            space
                .unmap_commit(va, length)
                .map_err(|_| Status::InvalidArgument)?
        };
        drop(holder);
        Ok(())
    }

    fn insert(&self, object: Object, rights: Rights) -> Result<u64, Status> {
        let handle = self
            .process
            .handles
            .lock()
            .insert(object, rights, object::next_id())?;
        Ok(handle.raw())
    }
}

impl Handlers for Calls<'_> {
    fn process_exit(&mut self, code: ExitCode) -> Result<u64, Status> {
        self.exit = Some(ExitRecord {
            code: code.0,
            reason: ExitReason::Exited,
            detail: 0,
            request_kind: None,
        });
        Ok(0)
    }

    fn thread_yield(&mut self) -> Result<u64, Status> {
        sched::yield_now();
        Ok(0)
    }

    fn console_write(&mut self, console: Handle, bytes: Bytes) -> Result<u64, Status> {
        let Object::Console = self.object(console, Rights::WRITE)? else {
            return Err(Status::BadHandle);
        };
        if bytes.0.length > MAX_WRITE {
            return Err(Status::InvalidArgument);
        }
        let bytes = UserSlice::of(bytes.0)?.read()?;
        let text = core::str::from_utf8(&bytes).map_err(|_| Status::InvalidArgument)?;
        print!("{text}");
        Ok(bytes.len() as u64)
    }

    fn clock_monotonic(&mut self) -> Result<u64, Status> {
        Ok(timer::now().as_nanos())
    }

    fn sleep_until(&mut self, at: zoraya_abi::Deadline) -> Result<u64, Status> {
        // 1 when `thread_interrupt` ended it early, else 0 (ADR-0300 item 5).
        thread::sleep_until(deadline(at))
    }

    fn handle_close(&mut self, handle: Handle) -> Result<u64, Status> {
        let entry = self
            .process
            .handles
            .lock()
            .remove(handle)
            .ok_or(Status::BadHandle)?;
        self.end_hold_if_unwritable(&entry.object);
        object::close(entry);
        Ok(0)
    }

    fn handle_duplicate(&mut self, handle: Handle, rights: Rights) -> Result<u64, Status> {
        let id = object::next_id();
        let new = self.process.handles.lock().duplicate(handle, rights, id)?;
        Ok(new.raw())
    }

    fn job_create(&mut self, parent: Handle, flags: JobFlags) -> Result<u64, Status> {
        let parent = self.job(parent, Rights::CREATE_PROCESS)?;
        self.process.handles.lock().reserve()?;
        let job = Job::create(&parent, flags.is_posix())?;
        self.insert(Object::Job(job), JobObject::RIGHTS)
    }

    fn spawn(&mut self, args: SpawnBuffer) -> Result<u64, Status> {
        if args.0.length > MAX_SPAWN_SIZE as u64 {
            return Err(Status::InvalidArgument);
        }
        // The only read of this buffer: from here on the kernel works on its
        // own copy (INV-0042-1).
        let bytes = UserSlice::of(args.0)?.read()?;
        let request = SpawnRequest::decode(&bytes)?;
        let job = self.job(request.job, Rights::CREATE_PROCESS)?;
        let job = match request.group {
            None => job,
            Some(group) => {
                let group = self.job(group, Rights::CREATE_PROCESS)?;
                if !group.is_within(&job) {
                    return Err(Status::InvalidArgument);
                }
                group
            }
        };
        let Object::Image(image) = self.object(request.image, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        let (name, loadable) = match &image {
            Image::Ramdisk { name, file } => (*name, Loadable::from_file(file)?),
            Image::Memory(image) => (image_name(&request), Loadable::from_image(image)),
        };
        // Nothing of the caller's changes until the child exists (INV-0042-3).
        let mut table = HandleTable::new();
        // The moved handles stay held from the check until the child exists:
        // `process::create` pauses, and a sibling must not move one away
        // meanwhile (a second copy of a handle without DUPLICATE).
        let mut claim = MoveClaim::new(&self.process, request.handle_count())?;
        {
            let mut handles = self.process.handles.lock();
            claim.keep_room(&mut handles)?;
            for listed in request.handles() {
                let entry = match listed.transfer {
                    Transfer::Move => claim.hold(&mut handles, listed.handle, Rights::TRANSFER)?,
                    Transfer::Duplicate => handles.check(listed.handle, Rights::DUPLICATE)?.clone(),
                };
                if !entry.rights.contains(listed.rights) {
                    return Err(Status::AccessDenied);
                }
                table.insert_at(
                    listed.slot,
                    entry.object.clone(),
                    listed.rights,
                    object::next_id(),
                )?;
            }
        }
        let child = process::create(Start {
            name,
            image: loadable,
            job,
            handles: table,
            argv: request.argv,
            envp: request.envp,
            dispositions: request.dispositions,
            suspended: request.suspended,
        })?;
        for entry in claim.finish() {
            steps::before(object::weight(&entry));
            steps::count(1);
            object::end_subscriptions(&entry);
            self.end_hold_if_unwritable(&entry.object);
        }
        Ok(claim.insert_child(child)?.raw())
    }

    fn process_resume(&mut self, process: Handle) -> Result<u64, Status> {
        self.other_process(process, Rights::RESUME)?.resume()?;
        Ok(0)
    }

    fn process_exit_with_reason(
        &mut self,
        reason: ExitReason,
        detail: Detail,
    ) -> Result<u64, Status> {
        if reason != ExitReason::Terminated {
            return Err(Status::InvalidArgument);
        }
        self.exit = Some(ExitRecord {
            code: 0,
            reason,
            detail: detail.0,
            request_kind: None,
        });
        Ok(0)
    }

    fn process_exit_record(&mut self, process: Handle, record: Bytes) -> Result<u64, Status> {
        let process = self.other_process(process, Rights::WAIT)?;
        let target = UserSlice::of(record.0)?;
        let record = process.exit_record().ok_or(Status::BadState)?;
        target.write(&record.to_bytes())?;
        Ok(0)
    }

    fn process_signal(
        &mut self,
        process: Handle,
        kind: RequestKind,
        detail: Detail,
    ) -> Result<u64, Status> {
        let process = self.other_process(process, Rights::SIGNAL)?;
        process.signal(kind, detail.0)?;
        Ok(0)
    }

    fn process_set_disposition(
        &mut self,
        kind: RequestKind,
        disposition: Disposition,
    ) -> Result<u64, Status> {
        self.process.set_disposition(kind, disposition);
        Ok(0)
    }

    fn process_take_request(&mut self) -> Result<u64, Status> {
        let (kind, detail) = self.process.take_request().ok_or(Status::NotFound)?;
        Ok(request_value(kind, detail))
    }

    fn job_signal(
        &mut self,
        job: Handle,
        kind: RequestKind,
        detail: Detail,
    ) -> Result<u64, Status> {
        self.job(job, Rights::SIGNAL)?.signal(kind, detail.0)?;
        Ok(0)
    }

    fn job_kill(&mut self, job: Handle) -> Result<u64, Status> {
        self.job(job, Rights::KILL)?.kill();
        Ok(0)
    }

    fn port_create(&mut self) -> Result<u64, Status> {
        self.process.handles.lock().reserve()?;
        self.insert(
            Object::Port(Port::new(Some(self.process.job()))?),
            PortObject::RIGHTS,
        )
    }

    fn port_subscribe(
        &mut self,
        port: Handle,
        handle: Handle,
        signals: SignalSet,
        key: Opaque,
        mode: Mode,
    ) -> Result<u64, Status> {
        let port = self.port(port, Rights::WRITE)?;
        let (object, owner) = {
            let handles = self.process.handles.lock();
            let entry = handles.check(handle, Rights::WAIT)?;
            (entry.object.clone(), entry.id)
        };
        let (source, known) = object.signals().ok_or(Status::BadHandle)?;
        if signals.0 == 0 || signals.0 & !known != 0 {
            return Err(Status::InvalidArgument);
        }
        // The holder pays for the subscription until it ends (ADR-0006 rule 2).
        let mut paid = Paid::new(Some(self.process.job()));
        paid.kernel(charge::SUBSCRIPTION_COST)?;
        port.subscribe(source, signals.0, key.0, mode, owner, paid)?;
        Ok(0)
    }

    fn port_unsubscribe(&mut self, port: Handle, key: Opaque) -> Result<u64, Status> {
        self.port(port, Rights::WRITE)?.unsubscribe(key.0)?;
        Ok(0)
    }

    fn port_queue(&mut self, port: Handle, key: Opaque, data: Opaque) -> Result<u64, Status> {
        self.port(port, Rights::WRITE)?.queue(key.0, data.0)?;
        Ok(0)
    }

    fn port_wait(
        &mut self,
        port: Handle,
        at: zoraya_abi::Deadline,
        packets: PacketBuffer,
    ) -> Result<u64, Status> {
        let port = self.port(port, Rights::WAIT)?;
        if packets.0.length == 0 {
            return Err(Status::InvalidArgument);
        }
        let room = packets.0.length.min(MAX_PACKETS as u64) as usize;
        let target = UserSlice::new(packets.0.address, (room * Packet::SIZE) as u64)?;
        // A bad buffer must fail before packets leave the port.
        target.write(&[0; MAX_PACKETS * Packet::SIZE][..room * Packet::SIZE])?;
        let mut taken = [Packet::default(); MAX_PACKETS];
        let count = port.wait(deadline(at), &mut taken[..room])?;
        for (i, packet) in taken[..count].iter().enumerate() {
            let place = target.at((i * Packet::SIZE) as u64)?;
            place.write(&packet.to_bytes())?;
        }
        Ok(count as u64)
    }

    fn directory_lookup(&mut self, directory: Handle, name: PathBuffer) -> Result<u64, Status> {
        let Object::Directory = self.object(directory, Rights::LOOKUP)? else {
            return Err(Status::BadHandle);
        };
        if name.0.length > NAME_SIZE as u64 {
            return Err(Status::NotFound);
        }
        let bytes = UserSlice::of(name.0)?.read()?;
        let text = core::str::from_utf8(&bytes).map_err(|_| Status::InvalidArgument)?;
        let path = RelativePath::new(text).ok_or(Status::InvalidArgument)?;
        let (name, file) = ramdisk::find(path.as_str()).ok_or(Status::NotFound)?;
        self.insert(
            Object::Image(Image::Ramdisk { name, file }),
            ImageObject::RIGHTS,
        )
    }

    fn console_read(&mut self, console: Handle, bytes: Bytes) -> Result<u64, Status> {
        let Object::Console = self.object(console, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        // User memory is reached before any byte leaves the ring, so that a
        // bad address loses no input.
        let target = UserSlice::of(bytes.0)?;
        let mut buffer = [0; INPUT_SIZE];
        let room = &mut buffer[..bytes.0.length.min(INPUT_SIZE as u64) as usize];
        target.write(room)?;
        let taken = console::read(room);
        target.write(&buffer[..taken])?;
        Ok(taken as u64)
    }

    fn console_dropped(&mut self, console: Handle) -> Result<u64, Status> {
        let Object::Console = self.object(console, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        Ok(console::dropped())
    }

    fn directory_entry(
        &mut self,
        directory: Handle,
        cookie: Cookie,
        entry: Bytes,
    ) -> Result<u64, Status> {
        let Object::Directory = self.object(directory, Rights::LOOKUP)? else {
            return Err(Status::BadHandle);
        };
        let target = UserSlice::of(entry.0)?;
        let disk = ramdisk::get();
        let Some((name, _)) = disk.files().nth(cookie.0 as usize) else {
            return Ok(0);
        };
        let next = cookie.0 + 1;
        let record = DirectoryEntry {
            next: if (next as usize) < disk.len() {
                next
            } else {
                0
            },
            name: name.as_bytes(),
        };
        let mut buffer = [0; DirectoryEntry::HEADER_SIZE + NAME_SIZE];
        let size = record.encode(&mut buffer).ok_or(Status::InvalidArgument)?;
        if entry.0.length < size as u64 {
            return Err(Status::InvalidArgument);
        }
        target.write(&buffer[..size])?;
        Ok(size as u64)
    }

    fn memory_create(&mut self, size: Size, flags: MemoryFlags) -> Result<u64, Status> {
        let object = MemoryObject::new(size.0, flags.is_contiguous(), Some(self.process.job()))?;
        self.insert(Object::Memory(object), MemoryType::RIGHTS)
    }

    fn memory_map(
        &mut self,
        object: Handle,
        offset: Size,
        length: Size,
        flags: MapFlags,
    ) -> Result<u64, Status> {
        let needed = if flags.is_write() {
            Rights::READ.union(Rights::WRITE)
        } else {
            Rights::READ
        };
        let Object::Memory(object) = self.object(object, needed)? else {
            return Err(Status::BadHandle);
        };
        let (offset, length) = (offset.0, length.0);
        let end = offset.checked_add(length).ok_or(Status::InvalidArgument)?;
        let whole = |v: u64| v.is_multiple_of(FRAME_SIZE);
        if length == 0 || !whole(offset) || !whole(length) || end > object.size() {
            return Err(Status::InvalidArgument);
        }
        // Never executable (INV-0051-2): this ADR has no right that could say so.
        let attributes = Attributes {
            memory: PageMemory::Normal,
            write: flags.is_write(),
            execute: false,
            user: true,
        };
        let frames = object.clone();
        // In steps of `STEP` pages with no lock held between them
        // (ADR-0270): the range is in the table of no mapping until the last
        // step, and a `job_kill` between steps unmaps what was done. The
        // mapping lock is held, so no other thread maps or unmaps meanwhile.
        let to_status = |e| match e {
            MappingError::Limit => Status::LimitReached,
            MappingError::Quota => Status::OverQuota,
            _ => Status::NoMemory,
        };
        // The mapping lock for the whole call (ADR-0301 item 6): another
        // thread of the process may not map between `map_reserve` and
        // `map_commit`. Dropped on every return, `Cancelled` ones included.
        let _map = MapGuard::acquire(&self.process)?;
        let va = {
            let mut space = self.process.space.lock();
            let space = space.as_mut().ok_or(Status::BadState)?;
            space.map_reserve(length).map_err(to_status)?
        };
        let page = |at: u64| frames.page(offset + at);
        let step = (steps::STEP as u64) * FRAME_SIZE;
        let mut done = 0;
        while done < length {
            let n = step.min(length - done);
            // Room for the step's tables in the space's list, mapped ahead
            // with no lock held (INV-0270-1).
            let growth = {
                let space = self.process.space.lock();
                space.as_ref().map_or(0, AddressSpace::step_growth)
            };
            heap::prepare(growth);
            {
                let mut space = self.process.space.lock();
                let space = space.as_mut().ok_or(Status::BadState)?;
                space.reserve_step();
                if let Err(e) = space.map_range(va, done, n, attributes, &page) {
                    if done > 0 {
                        space.unmap_range(va, 0, done);
                    }
                    return Err(to_status(e));
                }
            }
            done += n;
            if done < length
                && let Err(status) = steps::between()
            {
                let mut space = self.process.space.lock();
                if let Some(space) = space.as_mut() {
                    space.unmap_range(va, 0, done);
                }
                return Err(status);
            }
        }
        let mut space = self.process.space.lock();
        let space = space.as_mut().ok_or(Status::BadState)?;
        space.map_commit(va, length, attributes, Holder::Memory(object));
        Ok(va)
    }

    fn memory_unmap(&mut self, address: Address, length: Size) -> Result<u64, Status> {
        let _map = MapGuard::acquire(&self.process)?;
        self.unmap_exact(address.0, length.0)?;
        Ok(0)
    }

    fn channel_create(&mut self, ends: Bytes) -> Result<u64, Status> {
        if ends.0.length != 16 {
            return Err(Status::InvalidArgument);
        }
        let target = UserSlice::of(ends.0)?;
        target.write(&[0; 16])?;
        let pair = Pair::new(Some(self.process.job()))?;
        heap::pause_if_asked();
        let mut handles = self.process.handles.lock();
        handles.reserve_many(2)?;
        let mut record = [0; 16];
        let mut made = [None; 2];
        for end in 0..2 {
            let object = Object::Channel(pair.clone(), end);
            let handle = handles.insert(object, ChannelObject::RIGHTS, object::next_id())?;
            made[end] = Some(handle);
            record[end * 8..end * 8 + 8].copy_from_slice(&handle.raw().to_le_bytes());
        }
        // `Pair::new` may have paused, and a sibling may have unmapped `ends`
        // since: if the record cannot be written, the handles come out again.
        if let Err(e) = target.write(&record) {
            let gone: [_; 2] = made.map(|h| h.and_then(|h| handles.remove(h)));
            drop(handles);
            for entry in gone.into_iter().flatten() {
                object::close(entry);
            }
            return Err(e);
        }
        Ok(0)
    }

    fn channel_write(&mut self, end: Handle, bytes: Bytes, list: Bytes) -> Result<u64, Status> {
        let Object::Channel(pair, side) = self.object(end, Rights::WRITE)? else {
            return Err(Status::BadHandle);
        };
        let entries = list.0.length as usize / ChannelEntry::SIZE;
        if bytes.0.length > MAX_MESSAGE as u64
            || entries > MAX_MESSAGE_HANDLES
            || !(list.0.length as usize).is_multiple_of(ChannelEntry::SIZE)
        {
            return Err(Status::InvalidArgument);
        }
        let body = UserSlice::of(bytes.0)?.read()?;
        let raw = UserSlice::of(list.0)?.read()?;
        let mut listed = [None; MAX_MESSAGE_HANDLES];
        for (slot, chunk) in listed.iter_mut().zip(raw.as_chunks().0) {
            *slot = Some(ChannelEntry::from_bytes(chunk)?);
        }
        let listed = listed.iter().flatten().copied();

        // Every check and every allocation first: a failed write has no
        // effect (INV-0050-1).
        let mut handles = self.process.handles.lock();
        for (i, entry) in listed.clone().enumerate() {
            let held = handles.check(entry.handle, channel::needed_right(entry.transfer))?;
            if !held.rights.contains(entry.rights) {
                return Err(Status::AccessDenied);
            }
            if entry.transfer == Transfer::Move
                && listed.clone().skip(i + 1).any(|e| e.handle == entry.handle)
                || entry.transfer == Transfer::Duplicate
                    && listed
                        .clone()
                        .skip(i + 1)
                        .any(|e| e.handle == entry.handle && e.transfer == Transfer::Move)
            {
                return Err(Status::InvalidArgument);
            }
            if let Object::Channel(other, other_end) = &held.object
                && !zoraya_channel::may_travel(
                    Arc::ptr_eq(other, &pair),
                    other.holds_handles(*other_end),
                )
            {
                return Err(Status::BadState);
            }
        }
        let mut carried = Vec::new();
        carried
            .try_reserve_exact(entries)
            .map_err(|_| Status::NoMemory)?;
        // The moved entries' old forms, whose subscriptions end after the
        // handle-table lock is dropped, a pause at a time (review round 2,
        // error 1): the old owner id is dead, so nothing can need them.
        let mut moved_out = Vec::new();
        moved_out
            .try_reserve_exact(entries)
            .map_err(|_| Status::NoMemory)?;
        // The writer pays for the message while it is queued (ADR-0261 point 7).
        let mut paid = Paid::new(Some(self.process.job()));
        paid.kernel(channel::message_cost(body.capacity(), carried.capacity()))?;
        let mut message = channel::message(body, Vec::new(), paid)?;
        pair.prepare(side, message.bytes.len())?;
        let mut failed = None;
        for entry in listed.clone() {
            let object = match entry.transfer {
                Transfer::Duplicate => match handles.check(entry.handle, Rights::NONE) {
                    Ok(held) => held.object.clone(),
                    Err(status) => {
                        failed = Some(status);
                        break;
                    }
                },
                Transfer::Move => match handles.remove(entry.handle) {
                    Some(taken) => {
                        let object = taken.object.clone();
                        moved_out.push(taken);
                        object
                    }
                    None => {
                        failed = Some(Status::BadHandle);
                        break;
                    }
                },
            };
            carried.push(zoraya_handle::Entry {
                object,
                rights: entry.rights,
                id: object::next_id(),
            });
        }
        drop(handles);
        if let Some(status) = failed {
            // Defensive, unreachable: the entries were validated above under
            // the same lock, which is still the only way to change them (one
            // `handles` guard). Kept so that a later change cannot leak a
            // subscription.
            for taken in &moved_out {
                object::end_subscriptions(taken);
            }
            return Err(status);
        }
        // Before the write: the reader holds the handles only after it.
        for (entry, moved) in carried.iter().zip(listed) {
            if moved.transfer == Transfer::Move {
                self.end_hold_if_unwritable(&entry.object);
            }
        }
        message.handles = carried;
        // `write` follows `prepare` with nothing that can pause between them
        // (channel.rs): a pause there lets the peer close, and the message
        // would be queued into a closed end (review round 3, error 1).
        pair.write(side, message);
        // Only now the moved entries' subscriptions end, a pause at a time
        // with no lock held. The old ids are dead (`next_id`), so the order
        // against the write does not matter.
        // Each is dropped after its own announcement: the reader may have
        // closed them meanwhile, and these are then the last references, so
        // holding them all to the end would free them in one interval (review
        // round 4, error 1).
        for taken in moved_out {
            steps::before(object::weight(&taken));
            steps::count(1);
            object::end_subscriptions(&taken);
            drop(taken);
        }
        Ok(0)
    }

    fn channel_read(
        &mut self,
        end: Handle,
        bytes: Bytes,
        list: Bytes,
        sizes: Out,
    ) -> Result<u64, Status> {
        let Object::Channel(pair, side) = self.object(end, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        let sizes = UserSlice::new(sizes.0, ChannelSizes::SIZE as u64)?;
        let target = UserSlice::of(bytes.0)?;
        let room = list.0.length / 8;
        if !list.0.length.is_multiple_of(8) {
            return Err(Status::InvalidArgument);
        }
        let values = UserSlice::of(list.0)?;
        let Some((length, count)) = pair.peek(side)? else {
            return Err(Status::WouldBlock);
        };
        let record = ChannelSizes {
            bytes: length as u32,
            handles: count as u32,
        };
        sizes.write(&record.to_bytes())?;
        if length as u64 > bytes.0.length || count as u64 > room {
            return Err(Status::BufferTooSmall);
        }
        let mut handles = self.process.handles.lock();
        handles.reserve_many(count)?;
        // User memory is reached before the message leaves the queue, so a bad
        // address loses nothing.
        let mut zeros = [0u8; 8 * MAX_MESSAGE_HANDLES];
        values.write(&zeros[..count * 8])?;
        pair.copy_front(side, |bytes| target.write(bytes))?;
        let Some(mut message) = pair.read(side) else {
            return Err(Status::WouldBlock);
        };
        let mut at = 0;
        for entry in message.handles.drain(..) {
            let handle = handles.insert(entry.object, entry.rights, entry.id)?;
            zeros[at..at + 8].copy_from_slice(&handle.raw().to_le_bytes());
            at += 8;
        }
        drop(handles);
        // This range was just written, and no step point lies between (the
        // kernel is not preempted), so no sibling has unmapped it.
        values.write(&zeros[..at])?;
        Ok(0)
    }

    fn device_map(&mut self, device: Handle, index: RangeIndex, out: Bytes) -> Result<u64, Status> {
        let Object::Device(device) = self.object(device, Rights::MAP)? else {
            return Err(Status::BadHandle);
        };
        if out.0.length < 16 {
            return Err(Status::InvalidArgument);
        }
        let target = UserSlice::of(out.0)?;
        target.write(&[0; 16])?;
        let offset = device.offset(index.0).ok_or(Status::InvalidArgument)?;
        // Every mapping call takes the mapping lock (ADR-0301 item 6): a
        // sibling's stepped map holds a reserved address between its parts.
        let _map = MapGuard::acquire(&self.process)?;
        let mapping = device.begin_mapping()?;
        // Device-nGnRnE, never executable (INV-0054-1).
        let attributes = Attributes {
            memory: PageMemory::Device,
            write: true,
            execute: false,
            user: true,
        };
        let base = device.base();
        let mut space = self.process.space.lock();
        let space = space.as_mut().ok_or(Status::BadState)?;
        let address = space
            .map_object(FRAME_SIZE, attributes, Holder::Device(mapping), |_| base)
            .map_err(|e| match e {
                MappingError::Limit => Status::LimitReached,
                MappingError::Quota => Status::OverQuota,
                _ => Status::NoMemory,
            })?;
        let mut record = [0; 16];
        record[..8].copy_from_slice(&address.to_le_bytes());
        record[8..].copy_from_slice(&offset.to_le_bytes());
        target.write(&record)?;
        Ok(0)
    }

    fn device_interrupt(&mut self, device: Handle, index: RangeIndex) -> Result<u64, Status> {
        let Object::Device(device) = self.object(device, Rights::INTERRUPT)? else {
            return Err(Status::BadHandle);
        };
        let interrupt = device.interrupt(index.0)?;
        self.insert(Object::Interrupt(interrupt), InterruptObject::RIGHTS)
    }

    /// ADR-0295: the device of one transport of a page, for a holder of the
    /// page with `MAP`. The new handle has the page handle's rights that a
    /// device handle has, but `MAP` (a transport device is never mapped), so
    /// it passes where the page's would, to the one driver whose transport it
    /// is.
    fn device_transport(&mut self, device: Handle, index: RangeIndex) -> Result<u64, Status> {
        let (object, held) = {
            let handles = self.process.handles.lock();
            let entry = handles.check(device, Rights::MAP)?;
            (entry.object.clone(), entry.rights)
        };
        let Object::Device(device) = object else {
            return Err(Status::BadHandle);
        };
        let transport = device.transport(index.0)?;
        // Rights only decrease (ADR-0040): the page handle's rights that a
        // device handle has, and no `MAP`: a transport device is never
        // mapped (INV-0295-3).
        let rights = Rights::from_raw(held.raw() & DeviceObject::RIGHTS.raw())
            .ok_or(Status::InvalidArgument)?
            .without(Rights::MAP);
        self.insert(Object::Device(transport), rights)
    }

    /// ADR-0295: a register of a transport device. The offset is the
    /// caller's and `Device::read_register` bounds it.
    fn device_read(&mut self, device: Handle, offset: Size, width: Size) -> Result<u64, Status> {
        let Object::Device(device) = self.object(device, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        device.read_register(offset.0, width.0).map(u64::from)
    }

    fn device_write(
        &mut self,
        device: Handle,
        offset: Size,
        value: Opaque,
        width: Size,
    ) -> Result<u64, Status> {
        let Object::Device(device) = self.object(device, Rights::WRITE)? else {
            return Err(Status::BadHandle);
        };
        // A value wider than its access is refused, not cut.
        if width.0 > 4 || value.0 >> (8 * width.0) != 0 {
            return Err(Status::InvalidArgument);
        }
        let value = value.0 as u32;
        device.check_register(offset.0, width.0)?;
        // The first write makes this process the transport's holder, and a
        // second holder is refused (INV-0295-3). Nothing pauses here, and the
        // space's lock is held, so two threads cannot both become the holder.
        let mut space = self.process.space.lock();
        let space = space.as_mut().ok_or(Status::BadState)?;
        let held = space.hold(&device)?;
        device.write_register(held, offset.0, width.0, value)?;
        Ok(0)
    }

    fn interrupt_ack(&mut self, interrupt: Handle) -> Result<u64, Status> {
        let Object::Interrupt(interrupt) = self.object(interrupt, Rights::WRITE)? else {
            return Err(Status::BadHandle);
        };
        interrupt.ack();
        Ok(0)
    }

    fn dma_pin(&mut self, memory: Handle, device: Handle, out: Bytes) -> Result<u64, Status> {
        // A POSIX process makes no pin (ADR-0130, the architect's decision,
        // INV-0130-3), so none can hide in a queued message at a snapshot.
        if self.process.job().is_posix() {
            return Err(Status::AccessDenied);
        }
        let Object::Memory(memory) = self.object(memory, Rights::WRITE)? else {
            return Err(Status::BadHandle);
        };
        let Object::Device(device) = self.object(device, Rights::DMA)? else {
            return Err(Status::BadHandle);
        };
        let target = UserSlice::of(out.0)?;
        let needed = memory.runs().count() as u64 * 16;
        if needed > out.0.length {
            return Err(Status::InvalidArgument);
        }
        // Everything that can fail comes before the pin: a pin that ends
        // without `dma_unpin` is quarantined, so a failed call must make none.
        for at in (0..needed).step_by(16) {
            target.at(at)?.write(&[0; 16])?;
        }
        let mut handles = self.process.handles.lock();
        handles.reserve()?;
        let pin = Pin::new(device, memory.clone())?;
        let handle = handles.insert(Object::Pin(pin), PinObject::RIGHTS, object::next_id())?;
        drop(handles);
        for (i, (address, length)) in memory.runs().enumerate() {
            let mut record = [0; 16];
            record[..8].copy_from_slice(&address.to_le_bytes());
            record[8..].copy_from_slice(&length.to_le_bytes());
            // This range was just written, and no step point lies between (the
            // kernel is not preempted), so no sibling has unmapped it.
            target.at(i as u64 * 16)?.write(&record)?;
        }
        Ok(handle.raw())
    }

    fn dma_unpin(&mut self, pin: Handle) -> Result<u64, Status> {
        let Object::Pin(held) = self.object(pin, Rights::DMA)? else {
            return Err(Status::BadHandle);
        };
        held.unpin();
        let entry = self.process.handles.lock().remove(pin);
        object::close(entry.ok_or(Status::BadHandle)?);
        Ok(0)
    }

    fn dma_release_quarantine(&mut self, device: Handle) -> Result<u64, Status> {
        let Object::Device(device) = self.object(device, Rights::DMA)? else {
            return Err(Status::BadHandle);
        };
        device.release_quarantine()?;
        Ok(0)
    }

    fn event_create(&mut self) -> Result<u64, Status> {
        self.process.handles.lock().reserve()?;
        let paid = Paid::new(Some(self.process.job()));
        let event = charge::arc(paid, 0, |_paid| Event { _paid })?;
        self.insert(Object::Event(event), EventObject::RIGHTS)
    }

    fn object_same(&mut self, a: Handle, b: Handle) -> Result<u64, Status> {
        let a = self.object(a, Rights::NONE)?;
        let b = self.object(b, Rights::NONE)?;
        Ok(u64::from(a.same(&b)))
    }

    fn stream_create(&mut self, ends: Bytes) -> Result<u64, Status> {
        if ends.0.length != 16 {
            return Err(Status::InvalidArgument);
        }
        let target = UserSlice::of(ends.0)?;
        target.write(&[0; 16])?;
        // The table's lock is not held while the stream is built: its
        // memory object prepares the heap, which may pause (ADR-0270
        // point 2). A sibling thread runs at that pause: it may use up free
        // slots or unmap `ends`. So the room is made after the last pause,
        // and the record is written with the table locked, and both handles
        // are taken out again if it cannot be (the caller is told of none).
        let (reader, writer) = Stream::create(Some(self.process.job()))?;
        let mut handles = self.process.handles.lock();
        handles.reserve_many(2)?;
        let read = Object::StreamRead(reader);
        let read = handles.insert(read, StreamReadObject::RIGHTS, object::next_id())?;
        let write = Object::StreamWrite(writer);
        let write = handles.insert(write, StreamWriteObject::RIGHTS, object::next_id())?;
        let mut record = [0; 16];
        record[..8].copy_from_slice(&read.raw().to_le_bytes());
        record[8..].copy_from_slice(&write.raw().to_le_bytes());
        if let Err(e) = target.write(&record) {
            let gone = [handles.remove(read), handles.remove(write)];
            drop(handles);
            for entry in gone.into_iter().flatten() {
                object::close(entry);
            }
            return Err(e);
        }
        Ok(0)
    }

    fn stream_read(&mut self, end: Handle, bytes: Bytes) -> Result<u64, Status> {
        let Object::StreamRead(end) = self.object(end, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        Ok(end.read(&UserSlice::of(bytes.0)?)? as u64)
    }

    fn stream_write(&mut self, end: Handle, bytes: Bytes) -> Result<u64, Status> {
        let Object::StreamWrite(end) = self.object(end, Rights::WRITE)? else {
            return Err(Status::BadHandle);
        };
        Ok(end.write(&UserSlice::of(bytes.0)?)? as u64)
    }

    fn snapshot_process(&mut self, args: SnapshotBuffer) -> Result<u64, Status> {
        let started = timer::now();
        // Only for `posix` jobs: `dispatch` refuses it for others, as every
        // `#[personality_only]` call (INV-0002-2, INV-0003-2).
        if args.0.length > MAX_SNAPSHOT_SIZE as u64 {
            return Err(Status::InvalidArgument);
        }
        // The only read of this buffer (ADR-0042).
        let bytes = UserSlice::of(args.0)?.read()?;
        let request = SnapshotRequest::decode(&bytes)?;
        let job = self.job(request.job, Rights::CREATE_PROCESS)?;
        // As `spawn`, the handle's `CREATE_PROCESS` is the authority; the
        // caller's job has the `posix` bit (`dispatch`) and so must the
        // target's, so a copy never lands outside POSIX (ADR-0192). A fork
        // child's process job sits beside its parent's (ADR-0104).
        if !job.is_posix() {
            return Err(Status::AccessDenied);
        }
        let job = match request.group {
            None => job,
            Some(group) => {
                let group = self.job(group, Rights::CREATE_PROCESS)?;
                if !group.is_within(&job) {
                    return Err(Status::InvalidArgument);
                }
                group
            }
        };
        let mut claim = MoveClaim::new(&self.process, request.move_count())?;
        let (table, self_handle) = self.snapshot_table(&request, &mut claim)?;
        // The child's slot first, then its full copy, both paid by the child's
        // job (ADR-0261 point 6): a limit refuses the call with no effect.
        let mut count = Paid::new(Some(&job));
        count.process()?;
        // Then the child's first thread, in the order processes, threads,
        // memory (ADR-0307 item 1).
        let first = sched::reserve_thread(&job)?;
        // The mapping lock for the whole call, and the other threads of the
        // caller parked at their next return to EL0, so that the copy sees one
        // memory and no one maps or unmaps meanwhile (ADR-0306 item 2). Both
        // are released on every return.
        let _map = MapGuard::acquire(&self.process)?;
        let _freeze = thread::Freeze::new(&self.process);
        let mut copied = Copied::default();
        // The copy in steps (ADR-0270): the plan, the child and its objects
        // with no lock held, then up to `STEP` pages at a time under the
        // parent's lock, with the call open to a `job_kill` between them. The
        // child is in no table until the end; a cancelled call drops it.
        let plan = {
            let space = self.process.space.lock();
            space.as_ref().ok_or(Status::BadState)?.snapshot_plan()?
        };
        let mut build = SnapshotBuild::new(plan, Some(&job))?;
        loop {
            build.prepare_step();
            let done = {
                let space = self.process.space.lock();
                space
                    .as_ref()
                    .ok_or(Status::BadState)?
                    .snapshot_step(&mut build, &mut copied)?
            };
            if done {
                break;
            }
            steps::between()?;
        }
        let space = build.finish()?;
        let child = process::create_snapshot(Snapshot {
            parent: &self.process,
            space,
            job,
            count,
            first,
            handles: table,
            self_handle,
            frame: self.frame,
            tpidr_el0: arch::tpidr_el0(),
            suspended: request.suspended,
        })?;
        // The child exists: only now do the moved handles leave (INV-0102-2).
        for entry in claim.finish() {
            steps::before(object::weight(&entry));
            steps::count(1);
            object::end_subscriptions(&entry);
            self.end_hold_if_unwritable(&entry.object);
        }
        // `snapshot_table` kept this room, so a sibling cannot have filled it.
        let handle = claim.insert_child(child)?;
        *LAST_SNAPSHOT.lock() = (timer::now().nanos_since(started), copied.pages);
        Ok(handle.raw())
    }

    fn image_create(&mut self, memory: Handle, length: Size) -> Result<u64, Status> {
        let Object::Memory(memory) = self.object(memory, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        let length = length.0;
        if length > memory.size() {
            return Err(Status::InvalidArgument);
        }
        let mut ehdr = [0; EHDR_SIZE];
        let head = &mut ehdr[..(length as usize).min(EHDR_SIZE)];
        // In range: `head` is at most `length`, which is at most the size.
        let read = memory.read(0, head);
        debug_assert!(read);
        let header = Header::parse(head).map_err(|_| Status::InvalidArgument)?;
        let end = header.table_offset as u64 + header.table_length as u64;
        if end > length {
            return Err(Status::InvalidArgument);
        }
        let mut table = Vec::new();
        table
            .try_reserve_exact(header.table_length)
            .map_err(|_| Status::NoMemory)?;
        table.resize(header.table_length, 0);
        // In range: `end <= length <= size`, checked above.
        let read = memory.read(header.table_offset as u64, &mut table);
        debug_assert!(read);
        // The same parser and range as the ramdisk's (INV-0108-1).
        let program = Program::from_table(&header, &table, length, PROGRAM_LOW, PROGRAM_HIGH)
            .map_err(|_| Status::InvalidArgument)?;
        let mut handles = self.process.handles.lock();
        handles.reserve()?;
        let image = charge::arc(
            Paid::new(Some(self.process.job())),
            charge::vec_cost::<u8>(table.capacity()),
            |paid| MemoryImage {
                memory,
                program,
                table,
                _paid: paid,
            },
        )?;
        let image = Object::Image(Image::Memory(image));
        Ok(handles
            .insert(image, ImageObject::RIGHTS, object::next_id())?
            .raw())
    }

    fn thread_create(
        &mut self,
        entry: EntryPoint,
        stack: Address,
        argument: Opaque,
        tls: Address,
    ) -> Result<u64, Status> {
        thread::create(&self.process, entry.0, stack.0, argument.0, tls.0)
    }

    fn thread_exit(
        &mut self,
        clear: Address,
        unmap_base: Address,
        unmap_length: Size,
    ) -> Result<u64, Status> {
        // In this order (ADR-0300 item 3). A mismatch or a bad address is
        // ignored: the thread is ending.
        if unmap_length.0 != 0
            && let Ok(_map) = MapGuard::acquire(&self.process)
        {
            let _ = self.unmap_exact(unmap_base.0, unmap_length.0);
        }
        // The figures go back before the store and the wake: a joiner woken
        // by the store sees them refunded, and the wake itself may yield
        // (INV-0313-3). `end_thread_record` finds nothing left to give.
        sched::current().release_thread();
        if clear.0 != 0
            && clear.0.is_multiple_of(4)
            && let Ok(word) = UserSlice::new(clear.0, 4)
            && word.write(&0u32.to_le_bytes()).is_ok()
        {
            let _ = thread::futex_wake(&self.process, clear.0, Count::MAX);
        }
        self.end_thread = true;
        Ok(0)
    }

    fn thread_self(&mut self) -> Result<u64, Status> {
        thread::this(&self.process)
    }

    fn thread_interrupt(&mut self, target: Handle) -> Result<u64, Status> {
        let Object::Thread(object) = self.object(target, Rights::SIGNAL)? else {
            return Err(Status::BadHandle);
        };
        thread::interrupt(&object)?;
        Ok(0)
    }

    fn futex_wait(
        &mut self,
        address: Address,
        expected: Word32,
        at: zoraya_abi::Deadline,
    ) -> Result<u64, Status> {
        thread::futex_wait(&self.process, address.0, expected.0, deadline(at))
    }

    fn futex_wake(&mut self, address: Address, count: Count) -> Result<u64, Status> {
        thread::futex_wake(&self.process, address.0, count.0)
    }

    fn job_set_limits(&mut self, job: Handle, limits: Bytes) -> Result<u64, Status> {
        let job = self.job(job, Rights::LIMIT)?;
        if limits.0.length != LIMITS_SIZE as u64 {
            return Err(Status::InvalidArgument);
        }
        let bytes = UserSlice::of(limits.0)?.read()?;
        job.set_limits(JobLimits::decode(&bytes)?)?;
        Ok(0)
    }

    fn job_get_usage(&mut self, job: Handle, out: Bytes, flags: UsageFlags) -> Result<u64, Status> {
        let job = self.job(job, Rights::WAIT)?;
        let target = UserSlice::of(out.0)?;
        if target.len() < USAGE_SIZE {
            return Err(Status::BufferTooSmall);
        }
        // Written first, then cleared: a bad address clears nothing.
        // The first version's 88 bytes, the record with the CPU words, or the
        // one with the thread words too (ADR-0307 item 2).
        let usage = job.usage(false).encode();
        target.write(&usage[..target.len().min(USAGE_SIZE_THREADS)])?;
        if flags.clears() {
            job.usage(true);
        }
        Ok(0)
    }

    fn job_suspend(&mut self, job: Handle) -> Result<u64, Status> {
        self.job(job, Rights::KILL)?.suspend();
        Ok(0)
    }

    fn job_resume(&mut self, job: Handle) -> Result<u64, Status> {
        self.job(job, Rights::KILL)?.resume();
        Ok(0)
    }

    fn object_get_signals(&mut self, handle: Handle) -> Result<u64, Status> {
        let object = self.object(handle, Rights::WAIT)?;
        let (source, known) = object.signals().ok_or(Status::BadHandle)?;
        Ok(u64::from(port::signals(source) & known))
    }

    fn handle_type(&mut self, handle: Handle) -> Result<u64, Status> {
        Ok(self.object(handle, Rights::NONE)?.type_index())
    }

    fn power_off(&mut self, power: Handle) -> Result<u64, Status> {
        let Object::Power = self.object(power, Rights::POWER)? else {
            return Err(Status::BadHandle);
        };
        crate::shutdown()
    }

    fn power_reset(&mut self, power: Handle) -> Result<u64, Status> {
        let Object::Power = self.object(power, Rights::POWER)? else {
            return Err(Status::BadHandle);
        };
        Err(crate::reset())
    }
}

/// Calls that exist only in the test kernel, outside `zoraya-abi`.
#[cfg(feature = "qemu-test")]
pub mod test_calls {
    /// Returns 1 if the timer is on.
    pub const TIMER_IS_ON: u64 = 0x7000;
    /// Overflows the kernel stack of the calling thread.
    pub const OVERFLOW_STACK: u64 = 0x7001;
    /// Copies from the user address in x0 to an unmapped kernel address, and
    /// returns 1 if the fault was not resumed as a bad user pointer.
    pub const COPY_TO_BAD_KERNEL: u64 = 0x7002;
    /// Copies from an unmapped kernel address to the user address in x0, and
    /// returns 1 if the fault was not resumed.
    pub const COPY_FROM_BAD_KERNEL: u64 = 0x7003;
    /// Asks that the user address in x0 be read as this process ends.
    pub const WATCH: u64 = 0x7004;
    /// Makes the line of the first interrupt object pending.
    pub const TRIGGER_INTERRUPT: u64 = 0x7005;
    /// Returns the free frames.
    pub const FREE_FRAMES: u64 = 0x7006;
    /// Returns how many device mappings the caller has, or `u64::MAX` if one
    /// breaks INV-0054-1.
    pub const DEVICE_MAPPINGS: u64 = 0x7007;
    /// Returns how many pages of memory objects the caller has mapped, or
    /// `u64::MAX` if one is executable (INV-0051-2).
    pub const OBJECT_MAPPINGS: u64 = 0x7008;
    /// Returns the line of the first interrupt object, or 0.
    pub const FIRST_LINE: u64 = 0x7009;
    /// Returns 1 if the line in x0 is enabled.
    pub const LINE_ENABLED: u64 = 0x700a;
    /// Returns the kernel heap's live bytes.
    pub const HEAP_LIVE: u64 = 0x700b;
    /// Takes frames until x0 are free, and returns how many it holds.
    pub const HOLD_FRAMES: u64 = 0x700c;
    /// Gives back what `HOLD_FRAMES` took, and returns how many. With x0
    /// not 0, gives back at most x0 of them.
    pub const RELEASE_FRAMES: u64 = 0x700d;
    /// Returns the last `snapshot_process`'s time with interrupts masked, in
    /// nanoseconds.
    pub const SNAPSHOT_NS: u64 = 0x700e;
    /// Returns the pages the last `snapshot_process` copied.
    pub const SNAPSHOT_PAGES: u64 = 0x700f;
    /// Returns the raw handle in the caller's slot x0, or 0 if it is empty.
    pub const HANDLE_AT: u64 = 0x7010;
    /// Returns the rights of the handle in the caller's slot x0, or 0.
    pub const RIGHTS_AT: u64 = 0x7011;
    /// Returns the generation the caller's slot x0 is at.
    pub const GENERATION_AT: u64 = 0x7012;
    /// Returns the longest time IRQ stayed masked since the last reset, in
    /// nanoseconds (INV-0270-1); x0 not 0 resets it.
    pub const MASKED_MAX_NS: u64 = 0x7013;
    /// Returns the live event sources and ports, as `sources << 32 | ports`.
    pub const EVENTS_LIVE: u64 = 0x7014;
    /// Returns the most units of work (frames, messages, objects) done in
    /// one interval with IRQ masked since the last reset (INV-0270-1); x0 not
    /// 0 resets it.
    pub const MASKED_MAX_UNITS: u64 = 0x7015;
    /// Returns how many times a step point ended a call with `Cancelled`
    /// because its process was killed (INV-0270-2).
    pub const CANCELLED: u64 = 0x7016;
    /// Returns the bytes the kernel heap has taken from the frames so far
    /// (they never go back), once the reaper is idle.
    pub const HEAP_BACKING: u64 = 0x7017;
    /// Sets the CPU period to x0 nanoseconds; 0 restores the default
    /// (ADR-0264's measurement). The harness restores it between tests.
    pub const SET_CPU_PERIOD: u64 = 0x7018;
    /// Prints where the units of the busiest masked interval came from.
    pub const DIAGNOSE_UNITS: u64 = 0x7019;
    /// Makes every step point switch away (x0 = 1) or not (0): the walks of
    /// different processes then interleave at each one (`t_suspend`). While
    /// it is on, user threads also get [`FORCED_SLICE_NS`] slices.
    pub const FORCE_SWITCH: u64 = 0x701a;
    /// The slice while `FORCE_SWITCH` is on. Each step point of a walk then
    /// runs every other ready thread before the walk goes on, and a process
    /// that spins at EL0 keeps the CPU for its whole slice: with the default
    /// 10 ms, `t_suspend`'s two forced cases (700 jobs, up to 18 spinning
    /// processes) took about 235 s of a 240 s `t_suspend` under TCG on the
    /// development host and pushed every CI test-qemu run past its 300 s;
    /// with 1 ms the whole of `t_suspend` took 39 s there.
    pub const FORCED_SLICE_NS: u64 = 1_000_000;
    /// Makes `heap::prepare` pause once on every call (x0 = 1) or not (0):
    /// a creating call then always lets a sibling run before it makes its
    /// handles. The test that sets it clears it.
    pub const PAUSE_IN_PREPARE: u64 = 0x701b;
}

#[cfg(feature = "qemu-test")]
fn test_call(number: u64, a0: u64) -> Option<Result<u64, Status>> {
    match number {
        test_calls::COPY_TO_BAD_KERNEL => Some(Ok(crate::testing::copy_to_bad_kernel(a0))),
        test_calls::COPY_FROM_BAD_KERNEL => Some(Ok(crate::testing::copy_from_bad_kernel(a0))),
        test_calls::WATCH => {
            if let Some(process) = sched::current().process() {
                process.watch(a0);
            }
            Some(Ok(0))
        }
        test_calls::TIMER_IS_ON => Some(Ok(u64::from(arch::timer::is_enabled()))),
        test_calls::TRIGGER_INTERRUPT => {
            crate::device::trigger_first_line();
            Some(Ok(0))
        }
        test_calls::FIRST_LINE => Some(Ok(u64::from(crate::device::first_line()))),
        test_calls::LINE_ENABLED => Some(Ok(u64::from(crate::gic::is_enabled(a0 as u32)))),
        test_calls::FREE_FRAMES => Some(Ok(crate::memory::frames::settled_stats().free)),
        test_calls::HEAP_BACKING => Some(Ok(crate::memory::heap::settled_stats().backing as u64)),
        test_calls::HEAP_LIVE => Some(Ok(crate::memory::heap::settled_stats().live as u64)),
        test_calls::HOLD_FRAMES => Some(Ok(crate::testing::hold_frames(a0))),
        test_calls::RELEASE_FRAMES => Some(Ok(crate::testing::release_frames(a0))),
        test_calls::MASKED_MAX_NS => Some(Ok(crate::steps::masked_max_ns(a0 != 0))),
        test_calls::MASKED_MAX_UNITS => Some(Ok(crate::steps::masked_max_units(a0 != 0))),
        test_calls::CANCELLED => Some(Ok(crate::steps::cancelled())),
        // Below 1 ms a low ceiling's budget is zero and the release deadline
        // would storm; 0 restores the default.
        test_calls::SET_CPU_PERIOD if a0 != 0 && a0 < 1_000_000 => {
            Some(Err(Status::InvalidArgument))
        }
        test_calls::FORCE_SWITCH => {
            crate::steps::set_always_switch(a0 != 0);
            // 0 restores the default slice; the harness also restores it
            // before every test.
            sched::set_test_slice(if a0 != 0 {
                test_calls::FORCED_SLICE_NS
            } else {
                0
            });
            Some(Ok(0))
        }
        test_calls::PAUSE_IN_PREPARE => {
            crate::memory::heap::PAUSE_IN_PREPARE
                .store(a0 != 0, core::sync::atomic::Ordering::Relaxed);
            Some(Ok(0))
        }
        test_calls::DIAGNOSE_UNITS => {
            crate::steps::diagnose_units();
            Some(Ok(0))
        }
        test_calls::SET_CPU_PERIOD => {
            crate::cpu::set_test_period(a0);
            Some(Ok(0))
        }
        test_calls::EVENTS_LIVE => {
            let (sources, ports) = port::EVENTS.lock().live();
            Some(Ok(((sources as u64) << 32) | ports as u64))
        }
        test_calls::SNAPSHOT_NS => Some(Ok(LAST_SNAPSHOT.lock().0)),
        test_calls::SNAPSHOT_PAGES => Some(Ok(LAST_SNAPSHOT.lock().1)),
        test_calls::HANDLE_AT | test_calls::RIGHTS_AT | test_calls::GENERATION_AT => {
            let thread = sched::current();
            let handles = thread.process()?.handles.lock();
            let slot = u32::try_from(a0).ok()?;
            let generation = handles.generation_of(slot);
            let entry = Handle::from_parts(slot, generation).and_then(|h| handles.get(h));
            Some(Ok(match number {
                test_calls::HANDLE_AT => entry.map_or(0, |_| {
                    Handle::from_parts(slot, generation).map_or(0, Handle::raw)
                }),
                test_calls::RIGHTS_AT => entry.map_or(0, |e| u64::from(e.rights.raw())),
                _ => generation,
            }))
        }
        test_calls::OBJECT_MAPPINGS => {
            let thread = sched::current();
            let process = thread.process()?;
            let space = process.space.lock();
            let (mut count, mut good) = (0, true);
            space.as_ref()?.for_each_mapping(&mut |m| {
                if m.va >= crate::memory::user::MAP_LOW
                    && m.va < crate::memory::user::MAP_HIGH
                    && m.attributes.memory == PageMemory::Normal
                {
                    count += 1;
                    good &= !m.attributes.execute;
                }
            });
            Some(Ok(if good { count } else { u64::MAX }))
        }
        test_calls::DEVICE_MAPPINGS => {
            let thread = sched::current();
            let process = thread.process()?;
            let space = process.space.lock();
            let (mut count, mut good) = (0, true);
            space.as_ref()?.for_each_mapping(&mut |m| {
                if m.attributes.memory == PageMemory::Device {
                    count += 1;
                    good &= !m.attributes.execute;
                }
            });
            Some(Ok(if good { count } else { u64::MAX }))
        }
        test_calls::OVERFLOW_STACK => {
            crate::testing::overflow_stack();
            Some(Ok(0))
        }
        _ => None,
    }
}

/// `ESR_EL1.ISS` of an `svc`: its immediate, which must be 0 (ADR-0032).
const fn svc_immediate(esr: u64) -> u64 {
    esr & 0xffff
}

/// Handles the `svc` in `frame`.
pub fn dispatch(frame: &mut Frame) {
    let number = frame.x[8];
    #[cfg(feature = "qemu-test")]
    if let Some(result) = test_call(number, frame.x[0]) {
        frame.x[0] = match result {
            Ok(_) => Status::Ok as u64,
            Err(status) => status as u64,
        };
        frame.x[1] = result.unwrap_or(0);
        return;
    }
    let registers = [
        frame.x[0], frame.x[1], frame.x[2], frame.x[3], frame.x[4], frame.x[5],
    ];
    let process = sched::current().process().cloned();
    let mut exit = None;
    let mut end_thread = false;
    let result = match process {
        Some(process) if svc_immediate(frame.esr) == 0 => {
            // INV-0002-2: every `#[personality_only]` call is refused here
            // outside a `posix` job, whatever its handler checks.
            if zoraya_abi::is_personality_only(number) && !process.job().is_posix() {
                Err(Status::AccessDenied)
            } else {
                let mut calls = Calls {
                    process,
                    exit: None,
                    end_thread: false,
                    frame,
                };
                let result = zoraya_abi::dispatch(&mut calls, number, &registers);
                exit = calls.exit;
                end_thread = calls.end_thread;
                result
            }
        }
        _ => Err(Status::BadSyscall),
    };
    if let Some(record) = exit {
        process::exit_current(record);
    }
    if end_thread {
        process::leave_current();
    }
    let (status, value) = match result {
        Ok(value) => (Status::Ok, value),
        Err(status) => (status, charge::failure_value(status)),
    };
    frame.x[0] = status as u64;
    frame.x[1] = value;
}
