//! Memory descriptors (ADR-0413, plan part F3): `memfd_create` and
//! `shm_open(SHM_ANON)` make one, `ftruncate` gives it its memory object,
//! `mmap(MAP_SHARED)` maps it, `SCM_RIGHTS` and `fork` carry it, and the seals
//! are rights: a descriptor sealed with `F_SEAL_FUTURE_WRITE` or `F_SEAL_WRITE`
//! replaces its handle by one without `WRITE`, so every later mapping, copy
//! of the handle, send and fork child is read-only in the kernel's eyes
//! (INV-0413-2).
//!
//! A memory descriptor is the library's own: no service holds it. Its state is
//! [`Memory`], in the description entry, so `dup`ed fds share the position
//! and the seals. Where the memory object stands (INV-0413-1): the object
//! never changes size after the descriptor was first mapped shared, sent,
//! inherited or forked (`pinned`); until then a size change makes a new
//! object and copies, and a `write` past the end does the same in steps that
//! at least double.
//!
//! The decisions of the code are recorded in ADR-0432.

#![forbid(unsafe_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::errno::{
    Call, EACCES, EBADF, EBUSY, EFBIG, EINVAL, EMFILE, ENODEV, ENOMEM, ENOSYS, EPERM,
};
use crate::process::{Desc, Posix, native, o};
use crate::stat::{self, Kind, Meta};
use crate::sys::{Sys, User};

pub const MFD_CLOEXEC: u32 = 1;
pub const MFD_ALLOW_SEALING: u32 = 2;

pub const F_ADD_SEALS: i32 = 1033;
pub const F_GET_SEALS: i32 = 1034;

/// The seals (`fcntl.h`).
pub mod seal {
    pub const SEAL: u32 = 0x01;
    pub const SHRINK: u32 = 0x02;
    pub const GROW: u32 = 0x04;
    pub const WRITE: u32 = 0x08;
    pub const FUTURE_WRITE: u32 = 0x10;
    pub const ALL: u32 = 0x1f;
    /// The seals after which the object's handle is read-only.
    pub const READ_ONLY: u32 = WRITE | FUTURE_WRITE;
}

pub const PROT_READ: i32 = 1;
pub const PROT_WRITE: i32 = 2;
pub const PROT_EXEC: i32 = 4;
pub const MAP_SHARED: i32 = 0x01;
pub const MAP_PRIVATE: i32 = 0x02;
pub const MAP_FIXED: i32 = 0x10;
pub const MAP_ANONYMOUS: i32 = 0x20;

const PAGE: u64 = 4096;
/// The longest name `memfd_create` takes (Linux's `NAME_MAX` less the prefix).
pub const MAX_NAME: usize = 249;
/// The largest descriptor: 1 TiB, far past any address space this system
/// gives; a bigger size is `EFBIG` before a frame is asked for.
pub const MAX_SIZE: u64 = 1 << 40;
/// The most the library maps at once to copy through (ADR-0413 item 3).
const CHUNK: u64 = 1 << 20;

fn round(n: u64) -> u64 {
    n.next_multiple_of(PAGE)
}

/// What `mmap` and `mprotect` may produce (INV-0413-5, ADR-0413 item 6):
/// nothing is executable, so a request for it is refused and no other
/// combination of bits changes that. Pure, so the test runs every `prot`.
pub fn deny_exec(prot: i32) -> Result<(), i32> {
    if prot & PROT_EXEC != 0 {
        Err(EACCES)
    } else {
        Ok(())
    }
}

/// `mprotect`: the library keeps no permission changes (ADR-0100 item 6), so
/// the answer is `ENOSYS`, except that a request for execution is `EACCES`
/// like `mmap`'s (INV-0413-5).
pub fn mprotect_errno(prot: i32) -> i32 {
    deny_exec(prot).err().unwrap_or(ENOSYS)
}

/// What `mmap` of a memory descriptor asks for, checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapRequest {
    pub write: bool,
    pub shared: bool,
}

/// The flags and protection of `mmap(addr, length, prot, flags, fd, off)` for a
/// descriptor: one of `MAP_SHARED` and `MAP_PRIVATE`, no `MAP_FIXED` (the
/// kernel chooses the address), no execution.
pub fn map_request(length: i64, prot: i32, flags: i32) -> Result<MapRequest, i32> {
    deny_exec(prot)?;
    let shared = flags & MAP_SHARED != 0;
    let private = flags & MAP_PRIVATE != 0;
    if shared == private || flags & MAP_FIXED != 0 || length <= 0 {
        return Err(EINVAL);
    }
    Ok(MapRequest {
        write: prot & PROT_WRITE != 0,
        shared,
    })
}

/// A mapping this module made, by address.
#[derive(Debug)]
pub struct MemMap<H> {
    addr: u64,
    length: u64,
    /// The descriptor entry's number, for the writable maps `F_SEAL_WRITE`
    /// counts.
    owner: u64,
    writable: bool,
    /// The copy a `MAP_PRIVATE` mapping made: closed when it is unmapped.
    private: Option<H>,
}

/// A memory descriptor's state, in its description entry.
#[derive(Debug)]
pub struct Memory<H> {
    /// A number of this process for it, to tell its mappings.
    id: u64,
    /// The memory object's handle; `None` while the size is 0.
    object: Option<H>,
    /// The size `fstat` says, in bytes.
    size: u64,
    /// The object's size, in bytes (a page multiple, at least `size` rounded).
    capacity: u64,
    /// `O_RDWR` or `O_RDONLY`: what the descriptor may do.
    access: u32,
    /// The handle has `WRITE` (a sealed descriptor's does not).
    writable: bool,
    seals: u32,
    sealable: bool,
    /// Mapped shared, sent, inherited or forked: the object stays as it is.
    pinned: bool,
    /// A handle with `WRITE` left this process (sent, forked, or arrived
    /// with one): `F_SEAL_WRITE` cannot promise nothing else writes.
    writable_out: bool,
    pos: u64,
    name: Vec<u8>,
}

impl<H: Copy> Memory<H> {
    fn new(id: u64, name: &[u8], sealable: bool) -> Memory<H> {
        Memory {
            id,
            object: None,
            size: 0,
            capacity: 0,
            access: o::RDWR,
            writable: true,
            seals: if sealable { 0 } else { seal::SEAL },
            sealable,
            pinned: false,
            writable_out: false,
            pos: 0,
            name: name.to_vec(),
        }
    }

    /// A descriptor a message brought: its object is somebody else's too, so
    /// its size is fixed and a writable handle is out there.
    pub(crate) fn received(
        id: u64,
        object: H,
        access: u32,
        size: u64,
        seals: u32,
        sealable: bool,
    ) -> Memory<H> {
        Memory {
            id,
            object: Some(object),
            size,
            capacity: round(size),
            access,
            writable: access == o::RDWR,
            seals,
            sealable,
            pinned: true,
            writable_out: access == o::RDWR,
            pos: 0,
            name: Vec::new(),
        }
    }

    /// The name `memfd_create` was given (kept for reports only).
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    /// What the record of a send says (`unix::Kind::Memory`).
    pub(crate) fn wire(&self) -> (u8, u64, u64) {
        let access = if self.writable { 2 } else { 0 };
        let flags = u64::from(self.seals & seal::ALL)
            | if self.sealable {
                crate::unix::MEMORY_SEALABLE
            } else {
                0
            };
        (access, self.size, flags)
    }

    /// The object's handle (the library's own, never given out), or `None`
    /// for a descriptor that has no object yet (size 0).
    pub fn object(&self) -> Option<H> {
        self.object
    }

    /// The send happened: the object has left, and its size stays.
    pub(crate) fn sent(&mut self) {
        self.pinned = true;
        self.writable_out |= self.writable;
    }

    /// A `fork` copied the handle (the child's copy is at the same slot).
    pub(crate) fn forked(&mut self) {
        self.sent();
    }

    /// Whether the object may be replaced (INV-0413-1).
    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// `O_RDWR`, or `O_RDONLY` for a descriptor that arrived read-only.
    pub fn access(&self) -> u32 {
        self.access
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn seals(&self) -> u32 {
        self.seals
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    /// What `ftruncate(length)` does to the object.
    fn resize_plan(&self, length: u64) -> Result<Resize, i32> {
        if self.access != o::RDWR {
            return Err(EINVAL);
        }
        if length > MAX_SIZE {
            return Err(EFBIG);
        }
        if length == self.size {
            return Ok(Resize::Keep);
        }
        if length > self.size && self.seals & seal::GROW != 0 {
            return Err(EPERM);
        }
        if length < self.size && self.seals & seal::SHRINK != 0 {
            return Err(EPERM);
        }
        if self.pinned {
            return Err(EBUSY);
        }
        Ok(Resize::New)
    }

    /// The capacity a `write` that ends at `end` needs, in steps that at
    /// least double (ADR-0413 item 2).
    fn grown_capacity(&self, end: u64) -> u64 {
        round(end).max(self.capacity.saturating_mul(2))
    }
}

enum Resize {
    Keep,
    New,
}

impl<S: Sys> Posix<S> {
    fn memory(&self, fd: i32) -> Result<&Memory<S::H>, i32> {
        match self.fds.get(fd) {
            Some(Desc::Memory(m)) => Ok(m),
            Some(_) => Err(EINVAL),
            None => Err(EBADF),
        }
    }

    fn memory_mut(&mut self, fd: i32) -> Result<&mut Memory<S::H>, i32> {
        match self.fds.get_mut(fd) {
            Some(Desc::Memory(m)) => Ok(m),
            Some(_) => Err(EINVAL),
            None => Err(EBADF),
        }
    }

    fn next_memory_id(&mut self) -> u64 {
        self.memory_ids += 1;
        self.memory_ids
    }

    /// The number a received descriptor takes.
    pub(crate) fn memory_id(&mut self) -> u64 {
        self.next_memory_id()
    }

    /// `memfd_create(name, flags)` (ADR-0413 item 1).
    pub fn memfd_create(&mut self, name: &[u8], flags: u32) -> Result<i32, i32> {
        if flags & !(MFD_CLOEXEC | MFD_ALLOW_SEALING) != 0 || name.len() > MAX_NAME {
            return Err(EINVAL);
        }
        let id = self.next_memory_id();
        let m = Memory::new(id, name, flags & MFD_ALLOW_SEALING != 0);
        self.install_memory(m, flags & MFD_CLOEXEC != 0)
    }

    /// `shm_open(name, oflag, mode)`: only `SHM_ANON` (`anon`), which makes a
    /// descriptor that accepts seals, close-on-exec as musl's is
    /// (ADR-0413 item 1; INV-0413-3). A name is `EACCES`.
    pub fn shm_open(&mut self, anon: bool, oflag: u32) -> Result<i32, i32> {
        if !anon {
            return Err(EACCES);
        }
        let allowed = o::ACCMODE | o::CREAT | o::EXCL | o::TRUNC | o::CLOEXEC | o::NONBLOCK;
        if oflag & !allowed != 0 || oflag & o::ACCMODE != o::RDWR {
            return Err(EINVAL);
        }
        let id = self.next_memory_id();
        let m = Memory::new(id, b"", true);
        self.install_memory(m, true)
    }

    fn install_memory(&mut self, m: Memory<S::H>, cloexec: bool) -> Result<i32, i32> {
        match self.fds.install(0, Desc::Memory(Box::new(m)), cloexec) {
            Ok(fd) => Ok(fd as i32),
            Err(_) => Err(EMFILE),
        }
    }

    // ------------------------------------------------------------ copying

    /// Maps `length` bytes of `object` from the page at `offset` to copy
    /// through; the caller unmaps.
    fn copy_map(
        &mut self,
        object: S::H,
        offset: u64,
        length: u64,
        write: bool,
    ) -> Result<u64, i32> {
        self.sys
            .memory_map_at(object, offset, length, write, false)
            .map_err(|s| native(s, Call::Memory))
    }

    /// Reads `into.length` bytes at `offset` of the object.
    fn object_read(&mut self, object: S::H, offset: u64, into: User) -> Result<(), i32> {
        let mut done = 0usize;
        while done < into.length {
            let at = offset + done as u64;
            let (page, delta) = (at / PAGE * PAGE, at % PAGE);
            let n = (into.length - done).min((CHUNK - delta) as usize);
            let length = round(delta + n as u64);
            let address = self.copy_map(object, page, length, false)?;
            self.sys
                .copy(into.part(done, n), User::new(address + delta, n));
            let _ = self.sys.memory_unmap(address, length);
            done += n;
        }
        Ok(())
    }

    /// Writes `from` at `offset` of the object (the handle must be writable).
    fn object_write(&mut self, object: S::H, offset: u64, from: User) -> Result<(), i32> {
        let mut done = 0usize;
        while done < from.length {
            let at = offset + done as u64;
            let (page, delta) = (at / PAGE * PAGE, at % PAGE);
            let n = (from.length - done).min((CHUNK - delta) as usize);
            let length = round(delta + n as u64);
            let address = self.copy_map(object, page, length, true)?;
            self.sys
                .copy(User::new(address + delta, n), from.part(done, n));
            let _ = self.sys.memory_unmap(address, length);
            done += n;
        }
        Ok(())
    }

    /// Copies the first `bytes` of `from` into `to`, which is a new object.
    fn object_copy(&mut self, from: S::H, to: S::H, bytes: u64) -> Result<(), i32> {
        let mut done = 0;
        while done < bytes {
            let n = (bytes - done).min(CHUNK);
            let length = round(n);
            let a = self.copy_map(from, done, length, false)?;
            let b = match self.copy_map(to, done, length, true) {
                Ok(b) => b,
                Err(e) => {
                    let _ = self.sys.memory_unmap(a, length);
                    return Err(e);
                }
            };
            self.sys
                .copy(User::new(b, n as usize), User::new(a, n as usize));
            let _ = self.sys.memory_unmap(a, length);
            let _ = self.sys.memory_unmap(b, length);
            done += n;
        }
        Ok(())
    }

    /// A new object of `capacity` bytes holding the first `keep` bytes of the
    /// descriptor's old one, which is closed. A refusal leaves the descriptor
    /// as it was. The new handle is made read-only again if a seal says so.
    fn replace_object(&mut self, fd: i32, capacity: u64, keep: u64) -> Result<(), i32> {
        let (old, seals) = {
            let m = self.memory(fd)?;
            (m.object, m.seals)
        };
        let new = self
            .sys
            .memory_create(capacity)
            .map_err(|s| native(s, Call::Memory))?;
        if let Some(old) = old
            && keep > 0
            && let Err(e) = self.object_copy(old, new, keep)
        {
            self.sys.close(new);
            return Err(e);
        }
        let mut handle = new;
        let mut writable = true;
        if seals & seal::READ_ONLY != 0 {
            match self.sys.duplicate_read_only(new) {
                Ok(ro) => {
                    self.sys.close(new);
                    handle = ro;
                    writable = false;
                }
                Err(s) => {
                    self.sys.close(new);
                    return Err(native(s, Call::Descriptor));
                }
            }
        }
        if let Some(old) = old {
            self.sys.close(old);
        }
        let m = self.memory_mut(fd)?;
        m.object = Some(handle);
        m.capacity = capacity;
        m.writable = writable;
        Ok(())
    }

    // ------------------------------------------------------------ size

    /// `ftruncate` (ADR-0413 item 2).
    pub fn memory_ftruncate(&mut self, fd: i32, length: u64) -> Result<(), i32> {
        if let Resize::Keep = self.memory(fd)?.resize_plan(length)? {
            return Ok(());
        }
        let (size, object) = {
            let m = self.memory(fd)?;
            (m.size, m.object)
        };
        if length == 0 {
            if let Some(h) = object {
                self.sys.close(h);
            }
            let m = self.memory_mut(fd)?;
            m.object = None;
            m.size = 0;
            m.capacity = 0;
            return Ok(());
        }
        self.replace_object(fd, round(length), size.min(length))?;
        self.memory_mut(fd)?.size = length;
        Ok(())
    }

    /// Makes room for a write that ends at `end`: a new, larger object while
    /// the descriptor may still change (ADR-0413 item 2), else `EFBIG`.
    fn memory_room(&mut self, fd: i32, end: u64) -> Result<(), i32> {
        let m = self.memory(fd)?;
        if end <= m.size {
            return Ok(());
        }
        if end > MAX_SIZE {
            return Err(EFBIG);
        }
        if m.seals & seal::GROW != 0 {
            return Err(EPERM);
        }
        if m.pinned {
            return Err(EFBIG);
        }
        if end > m.capacity {
            let (capacity, size) = (m.grown_capacity(end), m.size);
            self.replace_object(fd, capacity, size)?;
        }
        self.memory_mut(fd)?.size = end;
        Ok(())
    }

    // ------------------------------------------------------------ data

    pub fn memory_read(&mut self, fd: i32, into: User, at: Option<u64>) -> Result<usize, i32> {
        let m = self.memory(fd)?;
        let offset = at.unwrap_or(m.pos);
        let n = (m.size.saturating_sub(offset)).min(into.length as u64) as usize;
        let Some(object) = m.object else {
            return Ok(0);
        };
        if n == 0 {
            return Ok(0);
        }
        self.object_read(object, offset, into.part(0, n))?;
        if at.is_none() {
            self.memory_mut(fd)?.pos = offset + n as u64;
        }
        Ok(n)
    }

    pub fn memory_write(&mut self, fd: i32, from: User, at: Option<u64>) -> Result<usize, i32> {
        let m = self.memory(fd)?;
        if m.access != o::RDWR {
            return Err(EBADF);
        }
        if m.seals & seal::READ_ONLY != 0 {
            return Err(EPERM);
        }
        if from.length == 0 {
            return Ok(0);
        }
        let offset = at.unwrap_or(m.pos);
        let end = offset.checked_add(from.length as u64).ok_or(EFBIG)?;
        let mut n = from.length;
        if end > m.size {
            match self.memory_room(fd, end) {
                Ok(()) => {}
                // A descriptor that cannot grow takes what fits.
                Err(EFBIG) if offset < self.memory(fd)?.size => {
                    n = (self.memory(fd)?.size - offset) as usize;
                }
                Err(e) => return Err(e),
            }
        }
        let object = self.memory(fd)?.object.ok_or(EBADF)?;
        self.object_write(object, offset, from.part(0, n))?;
        if at.is_none() {
            self.memory_mut(fd)?.pos = offset + n as u64;
        }
        Ok(n)
    }

    pub fn memory_lseek(&mut self, fd: i32, offset: i64, from: i32) -> Result<i64, i32> {
        let m = self.memory_mut(fd)?;
        let base = match from {
            0 => 0,
            1 => m.pos as i64,
            2 => m.size as i64,
            _ => return Err(EINVAL),
        };
        let to = base.checked_add(offset).ok_or(EINVAL)?;
        if to < 0 {
            return Err(EINVAL);
        }
        m.pos = to as u64;
        Ok(to)
    }

    pub fn memory_fstat(&self, fd: i32) -> Result<[u8; stat::SIZE], i32> {
        let m = self.memory(fd)?;
        let meta = Meta {
            mode: 0o600,
            size: m.size,
            ..Meta::default()
        };
        Ok(stat::encode(
            Kind::File,
            &meta,
            self.facts.uid,
            self.facts.gid,
        ))
    }

    // ------------------------------------------------------------ seals

    /// `fcntl(fd, F_ADD_SEALS, seals)` (ADR-0413 item 4).
    pub fn memory_add_seals(&mut self, fd: i32, seals: u32) -> Result<(), i32> {
        let m = self.memory(fd)?;
        if seals & !seal::ALL != 0 {
            return Err(EINVAL);
        }
        if !m.sealable || m.seals & seal::SEAL != 0 || m.access != o::RDWR {
            return Err(EPERM);
        }
        let (id, writable_out) = (m.id, m.writable_out);
        if seals & seal::WRITE != 0 && m.seals & seal::WRITE == 0 {
            let mapped = self.memory_maps.iter().any(|x| x.owner == id && x.writable);
            if writable_out || mapped {
                return Err(EBUSY);
            }
        }
        if seals & seal::READ_ONLY != 0 {
            self.memory_downgrade(fd)?;
        }
        let m = self.memory_mut(fd)?;
        m.seals |= seals;
        Ok(())
    }

    pub fn memory_get_seals(&self, fd: i32) -> Result<u32, i32> {
        Ok(self.memory(fd)?.seals)
    }

    /// Replaces a writable handle by one without `WRITE`.
    fn memory_downgrade(&mut self, fd: i32) -> Result<(), i32> {
        let m = self.memory(fd)?;
        let Some(h) = m.object else {
            return Ok(());
        };
        if !m.writable {
            return Ok(());
        }
        let ro = self
            .sys
            .duplicate_read_only(h)
            .map_err(|s| native(s, Call::Descriptor))?;
        self.sys.close(h);
        let m = self.memory_mut(fd)?;
        m.object = Some(ro);
        m.writable = false;
        Ok(())
    }

    // ------------------------------------------------------------ mapping

    /// `mmap` of a memory descriptor (ADR-0413 item 3): shared, a mapping of
    /// the object itself (the descriptor then never changes size), or private,
    /// a copy.
    pub fn memory_mmap(
        &mut self,
        fd: i32,
        length: i64,
        prot: i32,
        flags: i32,
        offset: i64,
    ) -> Result<u64, i32> {
        let request = map_request(length, prot, flags)?;
        let Ok(offset) = u64::try_from(offset) else {
            return Err(EINVAL);
        };
        if !offset.is_multiple_of(PAGE) {
            return Err(EINVAL);
        }
        let length = round(length as u64);
        let m = match self.fds.get(fd) {
            Some(Desc::Memory(m)) => m,
            Some(_) => return Err(ENODEV),
            None => return Err(EBADF),
        };
        let Some(object) = m.object else {
            return Err(EINVAL);
        };
        if offset
            .checked_add(length)
            .is_none_or(|end| end > round(m.size))
        {
            return Err(EINVAL);
        }
        if m.access == o::WRONLY {
            return Err(EACCES);
        }
        let id = m.id;
        if !request.shared {
            return self.memory_mmap_private(object, id, offset, length);
        }
        if request.write {
            // A descriptor that is not open for writing is `EACCES`; one
            // sealed against writing is `EPERM`, as Linux has it.
            if m.access != o::RDWR {
                return Err(EACCES);
            }
            if m.seals & seal::READ_ONLY != 0 {
                return Err(EPERM);
            }
            if !m.writable {
                return Err(EACCES);
            }
        }
        if self.memory_maps.len() >= MAX_MAPS {
            return Err(ENOMEM);
        }
        let address = self
            .sys
            .memory_map_at(object, offset, length, request.write, true)
            .map_err(|s| native(s, Call::Memory))?;
        self.memory_maps.push(MemMap {
            addr: address,
            length,
            owner: id,
            writable: request.write,
            private: None,
        });
        self.memory_mut(fd)?.pinned = true;
        Ok(address)
    }

    /// `MAP_PRIVATE`: a copy of the pages, in a memory object of its own.
    fn memory_mmap_private(
        &mut self,
        object: S::H,
        owner: u64,
        offset: u64,
        length: u64,
    ) -> Result<u64, i32> {
        if self.memory_maps.len() >= MAX_MAPS {
            return Err(ENOMEM);
        }
        let copy = self
            .sys
            .memory_create(length)
            .map_err(|s| native(s, Call::Memory))?;
        let address = match self.sys.memory_map_at(copy, 0, length, true, false) {
            Ok(a) => a,
            Err(s) => {
                self.sys.close(copy);
                return Err(native(s, Call::Memory));
            }
        };
        let source = match self.copy_map(object, offset, length, false) {
            Ok(a) => a,
            Err(e) => {
                let _ = self.sys.memory_unmap(address, length);
                self.sys.close(copy);
                return Err(e);
            }
        };
        self.sys.copy(
            User::new(address, length as usize),
            User::new(source, length as usize),
        );
        let _ = self.sys.memory_unmap(source, length);
        self.memory_maps.push(MemMap {
            addr: address,
            length,
            owner,
            writable: false,
            private: Some(copy),
        });
        Ok(address)
    }

    /// `munmap` of a mapping `memory_mmap` made: `None` if `address` is not
    /// one of them.
    pub fn memory_munmap(&mut self, address: u64, length: u64) -> Option<Result<(), i32>> {
        let at = self.memory_maps.iter().position(|m| m.addr == address)?;
        if self.memory_maps[at].length != round(length) {
            return Some(Err(EINVAL));
        }
        let m = &self.memory_maps[at];
        if let Err(s) = self.sys.memory_unmap(m.addr, m.length) {
            return Some(Err(native(s, Call::Memory)));
        }
        let m = self.memory_maps.remove(at);
        if let Some(h) = m.private {
            self.sys.close(h);
        }
        Some(Ok(()))
    }

    /// How many mappings of memory descriptors the process holds.
    pub fn memory_map_count(&self) -> usize {
        self.memory_maps.len()
    }

    // ------------------------------------------------------------ fork

    /// After a `fork`, in the parent and in the child: every memory
    /// descriptor's object is held by two processes, so its size stays and a
    /// writable handle is out (INV-0413-1).
    pub(crate) fn memory_after_fork(&mut self) {
        for d in self.fds.descriptions_mut() {
            if let Desc::Memory(m) = d {
                m.forked();
            }
        }
    }

    /// The send of these fds happened.
    pub(crate) fn memory_sent(&mut self, fds: &[i32]) {
        for &fd in fds {
            if let Some(Desc::Memory(m)) = self.fds.get_mut(fd) {
                m.sent();
            }
        }
    }
}

/// The most mappings of memory descriptors a process keeps (the kernel's own
/// limit is higher; this one bounds the library's table).
pub const MAX_MAPS: usize = 1024;
