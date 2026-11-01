//! System calls (ADR-0032). Nothing here trusts a register: every value comes
//! from user space, and every failure is a [`Status`]. `zoraya-abi` decodes
//! the registers and calls the [`Handlers`] here (ADR-0044).

use alloc::sync::Arc;
use alloc::vec::Vec;

use zoraya_abi::spawn::{MAX_SPAWN_SIZE, SpawnRequest, Transfer};
use zoraya_abi::{
    Buffer, Bytes, Cookie, DEADLINE_INFINITE, Detail, DirectoryEntry, Disposition, ExitCode,
    ExitReason, ExitRecord, Handle, Handlers, ImageObject, JobFlags, JobObject, Mode, ObjectType,
    Opaque, Packet, PacketBuffer, PathBuffer, PortObject, ProcessObject, RelativePath, RequestKind,
    Rights, SignalSet, SpawnBuffer, Status, request_value,
};
use zoraya_console::INPUT_SIZE;
use zoraya_ramdisk::NAME_SIZE;
use zoraya_time::{Deadline, Instant};

use crate::arch::{self, Frame};
use crate::job::Job;
use crate::memory::user::USER_HIGH;
use crate::object::{self, HandleTable, Image, Object};
use crate::port::Port;
use crate::process::{self, Process, Start};
use crate::{console, print, ramdisk, sched, timer};

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
struct Calls {
    process: Arc<Process>,
    exit: Option<ExitRecord>,
}

impl Calls {
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

    fn insert(&self, object: Object, rights: Rights) -> Result<u64, Status> {
        let handle = self
            .process
            .handles
            .lock()
            .insert(object, rights, object::next_id())?;
        Ok(u64::from(handle.raw()))
    }
}

impl Handlers for Calls {
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
        sched::sleep_until(deadline(at)).map_err(|_| Status::NoMemory)?;
        Ok(0)
    }

    fn handle_close(&mut self, handle: Handle) -> Result<u64, Status> {
        let entry = self.process.handles.lock().remove(handle);
        object::close(entry.ok_or(Status::BadHandle)?);
        Ok(0)
    }

    fn handle_duplicate(&mut self, handle: Handle, rights: Rights) -> Result<u64, Status> {
        let id = object::next_id();
        let new = self.process.handles.lock().duplicate(handle, rights, id)?;
        Ok(u64::from(new.raw()))
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
        // Nothing of the caller's changes until the child exists (INV-0042-3).
        let mut table = HandleTable::new();
        {
            let mut handles = self.process.handles.lock();
            handles.reserve()?;
            for listed in request.handles() {
                let needed = match listed.transfer {
                    Transfer::Move => Rights::TRANSFER,
                    Transfer::Duplicate => Rights::DUPLICATE,
                };
                let entry = handles.check(listed.handle, needed)?;
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
            name: image.name,
            file: image.file,
            job,
            handles: table,
            argv: request.argv,
            envp: request.envp,
            dispositions: request.dispositions,
            suspended: request.suspended,
        })?;
        for listed in request.handles() {
            if listed.transfer == Transfer::Move {
                let entry = self.process.handles.lock().remove(listed.handle);
                if let Some(entry) = entry {
                    object::end_subscriptions(&entry);
                }
            }
        }
        self.insert(Object::Process(child), ProcessObject::RIGHTS)
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
        self.insert(Object::Port(Port::new()?), PortObject::RIGHTS)
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
        port.subscribe(source, signals.0, key.0, mode, owner)?;
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
        self.insert(Object::Image(Image { name, file }), ImageObject::RIGHTS)
    }

    fn console_read(&mut self, console: Handle, bytes: Bytes) -> Result<u64, Status> {
        let Object::Console = self.object(console, Rights::READ)? else {
            return Err(Status::BadHandle);
        };
        if bytes.0.length > INPUT_SIZE as u64 {
            return Err(Status::InvalidArgument);
        }
        // User memory is reached before any byte leaves the ring, so that a
        // bad address loses no input.
        let target = UserSlice::of(bytes.0)?;
        let mut buffer = [0; INPUT_SIZE];
        let room = &mut buffer[..bytes.0.length as usize];
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

    fn power_off(&mut self, power: Handle) -> Result<u64, Status> {
        let Object::Power = self.object(power, Rights::POWER)? else {
            return Err(Status::BadHandle);
        };
        crate::shutdown()
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
    let result = match process {
        Some(process) if svc_immediate(frame.esr) == 0 => {
            let mut calls = Calls {
                process,
                exit: None,
            };
            let result = zoraya_abi::dispatch(&mut calls, number, &registers);
            exit = calls.exit;
            result
        }
        _ => Err(Status::BadSyscall),
    };
    if let Some(record) = exit {
        process::exit_current(record);
    }
    let (status, value) = match result {
        Ok(value) => (Status::Ok, value),
        Err(status) => (status, 0),
    };
    frame.x[0] = status as u64;
    frame.x[1] = value;
}
