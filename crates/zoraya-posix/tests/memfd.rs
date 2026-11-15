//! Memory descriptors over the fake kernel (ADR-0413, plan part F3):
//! `memfd_create`, `shm_open(SHM_ANON)`, `ftruncate`, reads and writes,
//! `mmap(MAP_SHARED)` and `MAP_PRIVATE`, seals as rights, `SCM_RIGHTS` and
//! `fork`. INV-0413-1 (the object never changes size once mapped or sent,
//! in every order), INV-0413-2 (a handle or mapping after a seal is not
//! writable, here and in the receiver), INV-0413-3 (`shm_open` of a name is
//! refused), INV-0413-5 (no `prot` gives an executable mapping).

mod kernel;

use kernel::*;
use zoraya_abi::posix::Facts;
use zoraya_posix::errno::*;
use zoraya_posix::memfd::{
    F_ADD_SEALS, F_GET_SEALS, MAP_FIXED, MAP_PRIVATE, MAP_SHARED, MFD_ALLOW_SEALING, MFD_CLOEXEC,
    PROT_EXEC, PROT_READ, PROT_WRITE, deny_exec, map_request, mprotect_errno, seal,
};
use zoraya_posix::process::{Desc, F_GETFD, F_GETFL, FD_CLOEXEC, Posix, o};
use zoraya_posix::sys::Sys;
use zoraya_posix::unix::{AF_UNIX, SOCK_SEQPACKET, SOCK_STREAM};

zoraya_test_alloc::cap_memory!();

const PAGE: usize = 4096;
const RW: i32 = PROT_READ | PROT_WRITE;

fn memfd(r: &mut Rig, flags: u32) -> i32 {
    r.p.memfd_create(b"test", flags).unwrap()
}

/// A descriptor of `size` bytes, sealable.
fn sized(r: &mut Rig, size: usize) -> i32 {
    let fd = memfd(r, MFD_ALLOW_SEALING);
    r.p.ftruncate(fd, size as i64).unwrap();
    fd
}

fn object(r: &Rig, fd: i32) -> u32 {
    let Some(Desc::Memory(m)) = r.p.fds.get(fd) else {
        panic!("fd {fd} is no memory descriptor");
    };
    m.object().expect("an object")
}

fn size_of_object(r: &Rig, fd: i32) -> usize {
    r.net
        .borrow()
        .memory_bytes(object(r, fd))
        .expect("memory")
        .len()
}

fn map(r: &mut Rig, fd: i32, len: usize, prot: i32, flags: i32, off: i64) -> Result<u64, i32> {
    r.p.memory_mmap(fd, len as i64, prot, flags, off)
}

fn bytes_at(r: &Rig, address: u64, n: usize) -> Vec<u8> {
    r.net.borrow_mut().memory(address, n).to_vec()
}

fn put_at(r: &Rig, address: u64, bytes: &[u8]) {
    r.net
        .borrow_mut()
        .memory(address, bytes.len())
        .copy_from_slice(bytes);
}

/// A second process over the same kernel.
fn second(r: &Rig) -> Rig {
    let net = r.net.clone();
    let mut fake = Fake::new(net.clone(), r.p.sys.services.clone());
    let arena = fake.memory_create(ARENA as u64).unwrap();
    let base = fake.memory_map(arena, ARENA as u64).unwrap();
    let p = Posix::new(
        fake,
        Facts {
            uid: 1000,
            gid: 100,
            umask: 0o022,
            ..Default::default()
        },
    );
    Rig {
        p,
        net,
        arena: Arena { base },
        root: r.root,
        reports: r.reports,
    }
}

fn send(r: &mut Rig, sock: i32, rights: &[i32]) -> Result<usize, i32> {
    let u = r.poke(0, b"m");
    r.p.sendmsg(sock, &[u], rights, 0)
}

/// Receives on `sock` (a packet socket of `r`): the new fds.
fn receive(r: &mut Rig, sock: i32) -> Vec<i32> {
    let u = r.arena.user(ARENA / 2, 16);
    let got = r.p.recvmsg(sock, &[u], 8, 0).unwrap();
    got.fds
}

/// Moves the packet socket `fd` of `a` to `b`, as a spawner would.
fn give(a: &mut Rig, fd: i32, b: &mut Rig) -> i32 {
    use zoraya_posix::socket::{Socket, State};
    let Some(Desc::Socket(s)) = a.p.fds.get_mut(fd) else {
        panic!("no socket");
    };
    let mut t = Socket::fresh_unix(s.kind, s.nonblock);
    t.state = State::Connected;
    t.read = s.read.take();
    t.write = s.write.take();
    t.ctl = s.ctl.take();
    s.state = State::Dead;
    a.p.close(fd).unwrap();
    match b.p.fds.install(0, Desc::Socket(Box::new(t)), false) {
        Ok(fd) => fd as i32,
        Err(_) => panic!("table full"),
    }
}

// ------------------------------------------------------------ making one

#[test]
fn memfd_create_takes_its_flags_and_names() {
    let mut r = rig();
    for bad in [4u32, 8, 0x8000_0000, MFD_CLOEXEC | 4] {
        assert_eq!(r.p.memfd_create(b"x", bad), Err(EINVAL), "flags {bad:#x}");
    }
    assert_eq!(r.p.memfd_create(&[b'a'; 250], 0), Err(EINVAL));
    let fd = r.p.memfd_create(&[b'a'; 249], MFD_CLOEXEC).unwrap();
    // The name is kept, for reports.
    let Some(Desc::Memory(m)) = r.p.fds.get(fd) else {
        panic!("no memory descriptor");
    };
    assert_eq!(m.name(), &[b'a'; 249][..]);
    assert_eq!(
        (m.size(), m.seals(), m.is_pinned(), m.is_writable()),
        (0, seal::SEAL, false, true)
    );
    assert_eq!(r.p.fcntl(fd, F_GETFD, 0), Ok(FD_CLOEXEC));
    let plain = memfd(&mut r, 0);
    assert_eq!(r.p.fcntl(plain, F_GETFD, 0), Ok(0));
    // A descriptor made without MFD_ALLOW_SEALING is sealed against sealing.
    assert_eq!(r.p.fcntl(plain, F_GET_SEALS, 0), Ok(seal::SEAL as i32));
    let sealable = memfd(&mut r, MFD_ALLOW_SEALING);
    assert_eq!(r.p.fcntl(sealable, F_GET_SEALS, 0), Ok(0));
    assert_eq!(r.p.fcntl(sealable, F_GETFL, 0), Ok(o::RDWR as i32));
    // Size 0: no object, so nothing to read, and nothing to charge.
    assert_eq!(r.read(fd, 10).unwrap(), b"");
    assert_eq!(r.p.fstat(fd).unwrap().len(), zoraya_posix::stat::SIZE);
}

/// INV-0413-3.
#[test]
fn shm_open_succeeds_only_for_the_anonymous_name() {
    let mut r = rig();
    let base = r.p.fds.count();
    for oflag in [
        o::RDWR,
        o::RDWR | o::CREAT,
        o::RDWR | o::CREAT | o::EXCL | o::TRUNC,
    ] {
        assert_eq!(
            r.p.shm_open(false, oflag),
            Err(EACCES),
            "a name, {oflag:#o}"
        );
    }
    assert_eq!(r.p.shm_open(false, o::RDONLY), Err(EACCES));
    for flag in [o::CREAT, o::EXCL, o::TRUNC, o::CLOEXEC, o::NONBLOCK] {
        let fd = r.p.shm_open(true, o::RDWR | flag).unwrap();
        r.p.close(fd).unwrap();
    }
    let fd = r.p.shm_open(true, o::RDWR).unwrap();
    // musl's shm_open closes on exec, and it takes seals.
    assert_eq!(r.p.fcntl(fd, F_GETFD, 0), Ok(FD_CLOEXEC));
    assert_eq!(r.p.fcntl(fd, F_GET_SEALS, 0), Ok(0));
    r.p.ftruncate(fd, 100).unwrap();
    assert_eq!(r.p.shm_open(true, o::RDONLY), Err(EINVAL));
    assert_eq!(r.p.shm_open(true, o::WRONLY), Err(EINVAL));
    assert_eq!(r.p.shm_open(true, o::RDWR | o::APPEND), Err(EINVAL));
    // Refusals made no descriptor.
    assert_eq!(r.p.fds.count(), base + 1);
}

// ------------------------------------------------------------ size and data

#[test]
fn the_first_ftruncate_makes_the_object_and_a_later_one_replaces_it() {
    let mut r = rig();
    let fd = memfd(&mut r, 0);
    let before = r.handles();
    r.p.ftruncate(fd, 5000).unwrap();
    assert_eq!(size_of_object(&r, fd), 2 * PAGE);
    assert_eq!(r.handles(), before + 1);
    // The same size changes nothing.
    let h = object(&r, fd);
    r.p.ftruncate(fd, 5000).unwrap();
    assert_eq!(object(&r, fd), h);
    // Unused, it can change: the bytes stay.
    assert_eq!(r.write(fd, b"hello").unwrap(), 5);
    r.p.ftruncate(fd, 3 * PAGE as i64).unwrap();
    assert_eq!(size_of_object(&r, fd), 3 * PAGE);
    r.p.lseek(fd, 0, 0).unwrap();
    assert_eq!(r.read(fd, 5).unwrap(), b"hello");
    // Smaller keeps the head.
    r.p.ftruncate(fd, 3).unwrap();
    r.p.lseek(fd, 0, 0).unwrap();
    assert_eq!(r.read(fd, 100).unwrap(), b"hel");
    assert_eq!(size_of_object(&r, fd), PAGE);
    // Zero gives the object back.
    let held = r.handles();
    r.p.ftruncate(fd, 0).unwrap();
    assert_eq!(r.handles(), held - 1);
    assert_eq!(r.read(fd, 10).unwrap(), b"");
    // A size no one can have is `EFBIG`, a negative one `EINVAL`, and
    // neither changes anything.
    assert_eq!(r.p.ftruncate(fd, 1 << 41), Err(EFBIG));
    assert_eq!(r.p.ftruncate(fd, -1), Err(EINVAL));
    r.p.close(fd).unwrap();
    assert_eq!(r.handles(), before);
}

#[test]
fn a_write_past_the_end_grows_it_in_doubling_steps_until_it_is_used() {
    let mut r = rig();
    let fd = memfd(&mut r, 0);
    let mut replaced = 0;
    let mut last = None;
    for i in 0..200u32 {
        let byte = [i as u8; 100];
        assert_eq!(r.write(fd, &byte).unwrap(), 100);
        let h = object(&r, fd);
        if last != Some(h) {
            replaced += 1;
            last = Some(h);
        }
    }
    // A write that ends exactly at the object's capacity needs no new object.
    let fd2 = memfd(&mut r, 0);
    r.write(fd2, &[1u8; 100]).unwrap();
    let h2 = object(&r, fd2);
    assert_eq!(r.write(fd2, &[2u8; PAGE - 100]), Ok(PAGE - 100));
    assert_eq!(object(&r, fd2), h2, "filled to the capacity, not past it");
    assert_eq!(r.write(fd2, b"x"), Ok(1));
    assert_ne!(object(&r, fd2), h2, "one byte past the capacity");
    // 20,000 bytes in 100-byte writes: a new object each time would be 200;
    // doubling makes it about log2(20000 / 4096) + 1.
    assert!(replaced <= 4, "{replaced} objects");
    r.p.lseek(fd, 0, 0).unwrap();
    let all = r.read(fd, 20_000).unwrap();
    assert_eq!(all.len(), 20_000);
    assert!(
        all.chunks(100)
            .enumerate()
            .all(|(i, c)| c == [i as u8; 100])
    );
    let stat = r.p.fstat(fd).unwrap();
    assert_eq!(u64::from_le_bytes(stat[48..56].try_into().unwrap()), 20_000);
    // pread and pwrite leave the position.
    let u = r.poke(0, b"ZZ");
    assert_eq!(r.p.pwrite(fd, u, 10), Ok(2));
    let into = r.arena.user(ARENA / 2, 4);
    assert_eq!(r.p.pread(fd, into, 9), Ok(4));
    assert_eq!(r.peek(ARENA / 2, 4)[..], [0, b'Z', b'Z', 0][..]);
    assert_eq!(r.p.lseek(fd, 0, 1), Ok(20_000));
    // Seek, then write past the end: the gap is zeros.
    r.p.lseek(fd, 30_000, 0).unwrap();
    r.write(fd, b"!").unwrap();
    r.p.lseek(fd, 25_000, 0).unwrap();
    assert_eq!(r.read(fd, 3).unwrap(), [0, 0, 0]);
    assert_eq!(r.p.lseek(fd, -1, 0), Err(EINVAL));
    assert_eq!(r.p.lseek(fd, 0, 7), Err(EINVAL));
    assert_eq!(r.p.lseek(fd, 0, 2), Ok(30_001));
}

#[test]
fn a_write_past_the_end_of_a_used_descriptor_is_efbig_and_what_fits_is_taken() {
    let mut r = rig();
    let fd = sized(&mut r, 100);
    let a = map(&mut r, fd, 100, RW, MAP_SHARED, 0).unwrap();
    // In range: written, and seen through the mapping.
    r.p.lseek(fd, 98, 0).unwrap();
    assert_eq!(r.write(fd, b"abcd"), Ok(2));
    assert_eq!(bytes_at(&r, a + 98, 2), b"ab");
    // Nothing fits.
    assert_eq!(r.write(fd, b"x"), Err(EFBIG));
    assert_eq!(r.p.ftruncate(fd, 200), Err(EBUSY));
    assert_eq!(r.p.ftruncate(fd, 50), Err(EBUSY));
    // The same size is not a change.
    assert_eq!(r.p.ftruncate(fd, 100), Ok(()));
}

#[test]
fn a_descriptor_that_arrived_read_only_neither_writes_nor_resizes() {
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let fd = sized(&mut r, PAGE);
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    send(&mut r, a, &[fd]).unwrap();
    let got = receive(&mut r, b);
    let ro = got[0];
    assert_eq!(r.p.fcntl(ro, F_GETFL, 0), Ok(o::RDONLY as i32));
    assert_eq!(r.write(ro, b"x"), Err(EBADF));
    assert_eq!(r.p.ftruncate(ro, 10), Err(EINVAL));
    assert_eq!(
        r.p.fcntl(ro, F_ADD_SEALS, i64::from(seal::GROW)),
        Err(EPERM)
    );
    assert_eq!(map(&mut r, ro, PAGE, RW, MAP_SHARED, 0), Err(EACCES));
}

// ------------------------------------------------------------ mapping

#[test]
fn a_shared_mapping_is_the_object_and_fixes_the_size() {
    let mut r = rig();
    let fd = sized(&mut r, 3 * PAGE);
    assert_eq!(r.write(fd, b"before").unwrap(), 6);
    let a = map(&mut r, fd, 3 * PAGE, RW, MAP_SHARED, 0).unwrap();
    assert_eq!(
        r.net.borrow().mappings().last(),
        Some(&(a, 3 * PAGE as u64, true, true))
    );
    // The bytes written through the descriptor are there, and the other way.
    assert_eq!(bytes_at(&r, a, 6), b"before");
    put_at(&r, a + PAGE as u64, b"through the map");
    r.p.lseek(fd, PAGE as i64, 0).unwrap();
    assert_eq!(r.read(fd, 15).unwrap(), b"through the map");
    // A part, at an offset.
    let b = map(&mut r, fd, PAGE, PROT_READ, MAP_SHARED, PAGE as i64).unwrap();
    assert_eq!(bytes_at(&r, b, 15), b"through the map");
    assert_eq!(
        r.net.borrow().mappings().last(),
        Some(&(b, PAGE as u64, false, true))
    );
    // The object is fixed from the first mapping.
    let h = object(&r, fd);
    assert_eq!(r.p.ftruncate(fd, PAGE as i64), Err(EBUSY));
    assert_eq!(r.p.ftruncate(fd, 4 * PAGE as i64), Err(EBUSY));
    assert_eq!(object(&r, fd), h);
    assert_eq!(size_of_object(&r, fd), 3 * PAGE);
    // The whole mapping goes: a part, or the wrong length, is refused.
    assert_eq!(r.p.memory_munmap(a, PAGE as u64), Some(Err(EINVAL)));
    assert_eq!(r.p.memory_munmap(a, u64::MAX), Some(Err(EINVAL)));
    assert_eq!(r.p.memory_munmap(a + 5, PAGE as u64), None);
    assert_eq!(r.p.memory_munmap(a, 3 * PAGE as u64), Some(Ok(())));
    assert_eq!(r.p.memory_munmap(b, PAGE as u64 - 1), Some(Ok(())));
    assert_eq!(r.p.memory_map_count(), 0);
    // Unmapping does not unpin it.
    assert_eq!(r.p.ftruncate(fd, PAGE as i64), Err(EBUSY));
}

#[test]
fn mmap_checks_what_it_is_asked() {
    let mut r = rig();
    let fd = sized(&mut r, 2 * PAGE);
    let plain = open_file(&mut r);
    // One of the two kinds, no MAP_FIXED, a length, a page offset.
    assert_eq!(map(&mut r, fd, PAGE, RW, 0, 0), Err(EINVAL));
    assert_eq!(
        map(&mut r, fd, PAGE, RW, MAP_SHARED | MAP_PRIVATE, 0),
        Err(EINVAL)
    );
    assert_eq!(
        map(&mut r, fd, PAGE, RW, MAP_SHARED | MAP_FIXED, 0),
        Err(EINVAL)
    );
    assert_eq!(map(&mut r, fd, 0, RW, MAP_SHARED, 0), Err(EINVAL));
    assert_eq!(map(&mut r, fd, PAGE, RW, MAP_SHARED, 1), Err(EINVAL));
    assert_eq!(map(&mut r, fd, PAGE, RW, MAP_SHARED, -4096), Err(EINVAL));
    // Past the object.
    assert_eq!(map(&mut r, fd, 3 * PAGE, RW, MAP_SHARED, 0), Err(EINVAL));
    assert_eq!(
        map(&mut r, fd, PAGE, RW, MAP_SHARED, 2 * PAGE as i64),
        Err(EINVAL)
    );
    // Size 0 has no object; a bad fd; a file is not mapped.
    let empty = memfd(&mut r, 0);
    assert_eq!(map(&mut r, empty, PAGE, RW, MAP_SHARED, 0), Err(EINVAL));
    assert_eq!(map(&mut r, 99, PAGE, RW, MAP_SHARED, 0), Err(EBADF));
    assert_eq!(map(&mut r, plain, PAGE, RW, MAP_SHARED, 0), Err(ENODEV));
    // None of it left a mapping or pinned the descriptor.
    assert_eq!(r.p.memory_map_count(), 0);
    assert_eq!(r.p.ftruncate(fd, 5 * PAGE as i64), Ok(()));
    // A length that is not a page is rounded up, as POSIX has it.
    let a = map(&mut r, fd, 5 * PAGE - 1, PROT_READ, MAP_SHARED, 0).unwrap();
    assert_eq!(r.net.borrow().mappings().last().unwrap().1, 5 * PAGE as u64);
    r.p.memory_munmap(a, 5 * PAGE as u64).unwrap().unwrap();
}

fn open_file(r: &mut Rig) -> i32 {
    r.p.open(
        zoraya_posix::process::AT_FDCWD,
        b"f",
        o::CREAT | o::RDWR,
        0o644,
    )
    .unwrap()
}

#[test]
fn a_private_mapping_is_a_copy_made_once() {
    let mut r = rig();
    let fd = sized(&mut r, 2 * PAGE);
    r.write(fd, b"original").unwrap();
    let held = r.handles();
    let p = map(&mut r, fd, 2 * PAGE, RW, MAP_PRIVATE, 0).unwrap();
    // Its own object, not shared with a child, not the descriptor's.
    assert_eq!(
        r.net.borrow().mappings().last(),
        Some(&(p, 2 * PAGE as u64, true, false))
    );
    assert_eq!(r.handles(), held + 1);
    assert_eq!(bytes_at(&r, p, 8), b"original");
    put_at(&r, p, b"changed!");
    r.p.lseek(fd, 0, 0).unwrap();
    assert_eq!(r.read(fd, 8).unwrap(), b"original");
    // Private mapping does not fix the descriptor's size.
    r.p.ftruncate(fd, 3 * PAGE as i64).unwrap();
    // Unmapping gives the copy back.
    assert_eq!(r.p.memory_munmap(p, 2 * PAGE as u64), Some(Ok(())));
    assert_eq!(r.handles(), held);
}

/// INV-0413-5.
#[test]
fn no_prot_gives_an_executable_mapping() {
    let mut r = rig();
    let fd = sized(&mut r, PAGE);
    for prot in 0..=0xff {
        for flags in [
            MAP_SHARED,
            MAP_PRIVATE,
            0,
            MAP_SHARED | 0x8000,
            MAP_PRIVATE | MAP_FIXED,
        ] {
            let exec = prot & PROT_EXEC != 0;
            let before = r.p.memory_map_count();
            let result = map(&mut r, fd, PAGE, prot, flags, 0);
            assert_eq!(deny_exec(prot).is_err(), exec);
            if exec {
                assert_eq!(result, Err(EACCES), "prot {prot:#x} flags {flags:#x}");
            }
            let valid = matches!(flags & 3, 1 | 2) && flags & MAP_FIXED == 0;
            assert_eq!(
                map_request(PAGE as i64, prot, flags).is_ok(),
                valid && !exec,
                "prot {prot:#x} flags {flags:#x}"
            );
            match result {
                Ok(a) => {
                    assert!(valid && !exec);
                    assert_eq!(r.p.memory_map_count(), before + 1);
                    r.p.memory_munmap(a, PAGE as u64).unwrap().unwrap();
                }
                Err(_) => assert_eq!(r.p.memory_map_count(), before),
            }
        }
        assert_eq!(
            mprotect_errno(prot),
            if prot & PROT_EXEC != 0 {
                EACCES
            } else {
                ENOSYS
            }
        );
    }
    // The kernel call has no flag for executable memory (INV-0051-2), so
    // none of the mappings the fake saw is anything but read or read-write.
    assert!(r.net.borrow().mappings().is_empty());
}

// ------------------------------------------------------------ seals

#[test]
fn seals_are_taken_as_linux_gives_them() {
    let mut r = rig();
    // Not sealable: every seal is `EPERM`.
    let plain = memfd(&mut r, 0);
    assert_eq!(
        r.p.fcntl(plain, F_ADD_SEALS, i64::from(seal::GROW)),
        Err(EPERM)
    );
    let fd = sized(&mut r, 2 * PAGE);
    assert_eq!(r.p.fcntl(fd, F_ADD_SEALS, 0x100), Err(EINVAL));
    assert_eq!(r.p.fcntl(fd, F_ADD_SEALS, -1), Err(EINVAL));
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::SHRINK | seal::GROW))
        .unwrap();
    assert_eq!(
        r.p.fcntl(fd, F_GET_SEALS, 0),
        Ok((seal::SHRINK | seal::GROW) as i32)
    );
    // The size is fixed, by seal before use as by use.
    assert_eq!(r.p.ftruncate(fd, PAGE as i64), Err(EPERM));
    assert_eq!(r.p.ftruncate(fd, 3 * PAGE as i64), Err(EPERM));
    assert_eq!(r.p.ftruncate(fd, 2 * PAGE as i64), Ok(()));
    r.p.lseek(fd, 2 * PAGE as i64, 0).unwrap();
    assert_eq!(r.write(fd, b"x"), Err(EPERM));
    // Sealing against more sealing.
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::SEAL)).unwrap();
    assert_eq!(
        r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::WRITE)),
        Err(EPERM)
    );
    assert_eq!(
        r.p.fcntl(fd, F_GET_SEALS, 0),
        Ok((seal::SHRINK | seal::GROW | seal::SEAL) as i32)
    );
}

/// INV-0413-2: the handle after the seal is read-only, so every mapping,
/// copy, send and fork child is.
#[test]
fn future_write_makes_the_handle_read_only_and_keeps_old_mappings() {
    let mut r = rig();
    let fd = sized(&mut r, 2 * PAGE);
    let old = map(&mut r, fd, 2 * PAGE, RW, MAP_SHARED, 0).unwrap();
    let before = object(&r, fd);
    assert!(!r.net.borrow().is_read_only(before));
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    let after = object(&r, fd);
    assert!(r.net.borrow().is_read_only(after));
    // The writable handle is gone.
    assert!(r.net.borrow().memory_bytes(before).is_none());
    // The old mapping is as it was; a new writable one is refused, a
    // read-only one is made through the read-only handle.
    assert_eq!(
        r.net.borrow().mappings()[0],
        (old, 2 * PAGE as u64, true, true)
    );
    put_at(&r, old, b"still writable");
    assert_eq!(map(&mut r, fd, PAGE, RW, MAP_SHARED, 0), Err(EPERM));
    let ro = map(&mut r, fd, PAGE, PROT_READ, MAP_SHARED, 0).unwrap();
    assert_eq!(bytes_at(&r, ro, 14), b"still writable");
    assert_eq!(
        r.net.borrow().mappings().last(),
        Some(&(ro, PAGE as u64, false, true))
    );
    // `write` is refused; a private copy is a copy and may be written.
    assert_eq!(r.write(fd, b"x"), Err(EPERM));
    let private = map(&mut r, fd, PAGE, RW, MAP_PRIVATE, 0).unwrap();
    r.p.memory_munmap(private, PAGE as u64).unwrap().unwrap();
}

#[test]
fn what_the_receiver_gets_follows_the_seal_at_the_time_of_the_send() {
    let mut r = rig();
    let mut s = second(&r);
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let b = give(&mut r, b, &mut s);
    let fd = sized(&mut r, 2 * PAGE);
    r.write(fd, b"for the reader").unwrap();
    // Before the seal the receiver gets a writable handle.
    send(&mut r, a, &[fd]).unwrap();
    let writable = receive(&mut s, b)[0];
    assert_eq!(s.p.fcntl(writable, F_GETFL, 0), Ok(o::RDWR as i32));
    assert!(!s.net.borrow().is_read_only(object(&s, writable)));
    let w = map(&mut s, writable, PAGE, RW, MAP_SHARED, 0).unwrap();
    assert_eq!(bytes_at(&s, w, 14), b"for the reader");
    // After it, a read-only one. The first receiver's handle is writable
    // still: a seal binds later handles (as Linux defines it), which is why
    // F_SEAL_WRITE is refused once a writable handle has left.
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    assert_eq!(
        r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::WRITE)),
        Err(EBUSY)
    );
    send(&mut r, a, &[fd]).unwrap();
    let later = receive(&mut s, b)[0];
    assert!(s.net.borrow().is_read_only(object(&s, later)));
    assert_eq!(map(&mut s, later, PAGE, RW, MAP_SHARED, 0), Err(EACCES));
}

#[test]
fn a_seal_made_before_the_send_makes_the_receivers_handle_read_only() {
    let mut r = rig();
    let mut s = second(&r);
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let b = give(&mut r, b, &mut s);
    let fd = sized(&mut r, 2 * PAGE);
    let w = map(&mut r, fd, 2 * PAGE, RW, MAP_SHARED, 0).unwrap();
    put_at(&r, w, b"painted");
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    send(&mut r, a, &[fd]).unwrap();
    let got = receive(&mut s, b);
    let received = got[0];
    let h = object(&s, received);
    assert!(s.net.borrow().is_read_only(h), "the receiver's handle");
    assert_eq!(s.p.fcntl(received, F_GETFL, 0), Ok(o::RDONLY as i32));
    assert_eq!(map(&mut s, received, PAGE, RW, MAP_SHARED, 0), Err(EACCES));
    let ro = map(&mut s, received, PAGE, PROT_READ, MAP_SHARED, 0).unwrap();
    assert_eq!(bytes_at(&s, ro, 7), b"painted");
    // Read-only at the kernel too: a record that lies (says it is writable)
    // still holds a handle that cannot be mapped writable.
    let sys_result = s.p.sys.memory_map_at(h, 0, PAGE as u64, true, true);
    assert_eq!(sys_result, Err(zoraya_abi::Status::AccessDenied));
    // Reads of the descriptor work, writes and resizes do not.
    let into = s.arena.user(ARENA / 2, 7);
    assert_eq!(s.p.pread(received, into, 0), Ok(7));
    assert_eq!(s.peek(ARENA / 2, 7), b"painted");
}

#[test]
fn write_seals_refuse_when_a_writable_handle_may_exist() {
    let mut r = rig();
    // Mapped writable here: busy.
    let fd = sized(&mut r, PAGE);
    let w = map(&mut r, fd, PAGE, RW, MAP_SHARED, 0).unwrap();
    assert_eq!(
        r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::WRITE)),
        Err(EBUSY)
    );
    assert_eq!(r.p.fcntl(fd, F_GET_SEALS, 0), Ok(0));
    r.p.memory_munmap(w, PAGE as u64).unwrap().unwrap();
    // A writable handle sent: busy for good.
    let (a, _b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    send(&mut r, a, &[fd]).unwrap();
    assert_eq!(
        r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::WRITE)),
        Err(EBUSY)
    );
    // A read-only mapping, or none at all: allowed, and the handle drops.
    let other = sized(&mut r, PAGE);
    let ro = map(&mut r, other, PAGE, PROT_READ, MAP_SHARED, 0).unwrap();
    r.p.fcntl(other, F_ADD_SEALS, i64::from(seal::WRITE))
        .unwrap();
    assert!(r.net.borrow().is_read_only(object(&r, other)));
    assert_eq!(r.write(other, b"x"), Err(EPERM));
    assert_eq!(map(&mut r, other, PAGE, RW, MAP_SHARED, 0), Err(EPERM));
    r.p.memory_munmap(ro, PAGE as u64).unwrap().unwrap();
    // A writable mapping of another descriptor does not matter.
    let third = sized(&mut r, PAGE);
    let elsewhere = map(&mut r, fd, PAGE, RW, MAP_SHARED, 0).unwrap();
    r.p.fcntl(third, F_ADD_SEALS, i64::from(seal::WRITE))
        .unwrap();
    r.p.memory_munmap(elsewhere, PAGE as u64).unwrap().unwrap();
    // A descriptor that arrived writable cannot promise it: busy.
    let (c, d) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let again = sized(&mut r, PAGE);
    send(&mut r, c, &[again]).unwrap();
    let got = receive(&mut r, d);
    assert_eq!(
        r.p.fcntl(got[0], F_ADD_SEALS, i64::from(seal::WRITE)),
        Err(EBUSY)
    );
}

// ------------------------------------------------------------ sending

#[test]
fn a_descriptor_sent_to_another_process_is_the_same_memory() {
    let mut r = rig();
    let mut s = second(&r);
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let b = give(&mut r, b, &mut s);
    // A 4 MiB buffer (the plan's criterion), written by the sender.
    let size = 4 * 1024 * 1024;
    let fd = sized(&mut r, size);
    let w = map(&mut r, fd, size, RW, MAP_SHARED, 0).unwrap();
    let pattern: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    put_at(&r, w, &pattern);
    send(&mut r, a, &[fd]).unwrap();
    // The sender still has it; its size is fixed.
    assert_eq!(r.p.ftruncate(fd, 5), Err(EBUSY));
    let got = receive(&mut s, b);
    assert_eq!(got.len(), 1);
    let theirs = got[0];
    assert!(
        r.net
            .borrow()
            .same_object(object(&r, fd), object(&s, theirs))
    );
    let m = map(&mut s, theirs, size, PROT_READ, MAP_SHARED, 0).unwrap();
    assert_eq!(bytes_at(&s, m, size), pattern);
    // And what the receiver writes, the sender sees.
    let rw = map(&mut s, theirs, PAGE, RW, MAP_SHARED, 0).unwrap();
    put_at(&s, rw, b"reply");
    assert_eq!(bytes_at(&r, w, 5), b"reply");
    // The size is the sender's, and fixed here too.
    let stat = s.p.fstat(theirs).unwrap();
    assert_eq!(
        u64::from_le_bytes(stat[48..56].try_into().unwrap()),
        size as u64
    );
    assert_eq!(s.p.ftruncate(theirs, 10), Err(EBUSY));
    // The received descriptor reads like the original.
    let into = s.arena.user(ARENA / 2, 8);
    assert_eq!(s.p.pread(theirs, into, 1000), Ok(8));
    assert_eq!(s.peek(ARENA / 2, 8), pattern[1000..1008]);
}

#[test]
fn a_descriptor_with_no_object_cannot_be_sent_and_a_refused_send_changes_nothing() {
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let empty = memfd(&mut r, 0);
    let held = r.handles();
    assert_eq!(send(&mut r, a, &[empty]), Err(EINVAL));
    assert_eq!(r.handles(), held);
    // The send did not fix a size that was never given an object.
    r.p.ftruncate(empty, 100).unwrap();
    r.p.ftruncate(empty, 200).unwrap();
    // A stream socket carries it too, with its byte.
    let (c, d) = r.p.socketpair(AF_UNIX, SOCK_STREAM, 0).unwrap();
    let fd = sized(&mut r, PAGE);
    send(&mut r, c, &[fd]).unwrap();
    let u = r.arena.user(ARENA / 2, 4);
    let got = r.p.recvmsg(d, &[u], 4, 0).unwrap();
    assert_eq!(got.fds.len(), 1);
    assert!(
        r.net
            .borrow()
            .same_object(object(&r, fd), object(&r, got.fds[0]))
    );
    let _ = b;
}

#[test]
fn a_record_that_lies_about_write_access_still_holds_a_read_only_handle() {
    use zoraya_posix::unix::{Kind, Record, encode_packet};
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let fd = sized(&mut r, PAGE);
    let ro = r.p.sys.duplicate_read_only(object(&r, fd)).unwrap();
    // A peer's library would not send this: a record that says read-write
    // with a handle that is not.
    let mut lie = Record::one(Kind::Memory, 2, false);
    lie.read_off = PAGE as u64;
    let message = encode_packet(&[lie], b"x");
    let Some(Desc::Socket(sock)) = r.p.fds.get(a) else {
        panic!("socket");
    };
    let end = sock.ctl.unwrap();
    r.p.sys.channel_write(end, &message, &[ro]).unwrap();
    let got = receive(&mut r, b)[0];
    assert_eq!(r.p.fcntl(got, F_GETFL, 0), Ok(o::RDWR as i32));
    // The kernel's refusal is what holds: `EACCES`, and no mapping.
    assert_eq!(map(&mut r, got, PAGE, RW, MAP_SHARED, 0), Err(EACCES));
    assert_eq!(r.p.memory_map_count(), 0);
    assert!(r.net.borrow().mappings().is_empty());
    // Hostile sizes and seals are refused by the decoder: the message is
    // dropped with its handle closed.
    for (size, flags) in [(u64::MAX, 0u64), (PAGE as u64, 0x400)] {
        let mut odd = Record::one(Kind::Memory, 2, false);
        odd.read_off = size;
        odd.write_off = flags;
        let extra = r.p.sys.duplicate(object(&r, fd)).unwrap();
        let held = r.handles();
        let message = encode_packet(&[odd], b"x");
        r.p.sys.channel_write(end, &message, &[extra]).unwrap();
        let u = r.arena.user(ARENA / 2, 16);
        // Dropped, and nothing else is queued.
        assert_eq!(
            r.p.recvmsg(b, &[u], 8, zoraya_posix::socket::MSG_DONTWAIT)
                .map(|g| g.fds.len()),
            Err(EAGAIN)
        );
        assert_eq!(r.handles(), held - 1, "the hostile message's handle");
    }
}

// ------------------------------------------------------------ fork

#[test]
fn a_fork_fixes_the_size_in_both_processes() {
    let mut r = rig();
    let fd = sized(&mut r, PAGE);
    let writable = object(&r, fd);
    r.p.after_fork_in_parent();
    assert_eq!(r.p.ftruncate(fd, 2 * PAGE as i64), Err(EBUSY));
    assert_eq!(object(&r, fd), writable);
    // A writable handle is out: F_SEAL_WRITE is busy, FUTURE_WRITE is not.
    assert_eq!(
        r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::WRITE)),
        Err(EBUSY)
    );
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    // A child that copied the state before the parent marked it marks it too.
    let mut child = rig();
    let cfd = sized(&mut child, PAGE);
    child.p.after_fork_in_child(7, 7);
    assert_eq!(child.p.ftruncate(cfd, 3 * PAGE as i64), Err(EBUSY));
}

/// Copies go through a mapping of at most 1 MiB at a time: data that crosses
/// those pieces, from an offset that is not a page, arrives whole, in reads,
/// writes and the copy a resize makes.
#[test]
fn data_crossing_the_copy_pieces_arrives_whole() {
    let mut r = rig();
    let fd = memfd(&mut r, 0);
    let piece = 3 * 512 * 1024;
    let one: Vec<u8> = (0..piece).map(|i| (i % 253) as u8).collect();
    let two: Vec<u8> = (0..piece).map(|i| (i % 249) as u8 ^ 0x55).collect();
    // 1.5 MiB at an offset of 1000, then 1.5 MiB after it.
    let u = r.poke(0, &one);
    assert_eq!(r.p.pwrite(fd, u, 1000), Ok(piece));
    let u = r.poke(0, &two);
    assert_eq!(r.p.pwrite(fd, u, 1000 + piece as i64), Ok(piece));
    let check = |r: &mut Rig, fd: i32| {
        let into = r.arena.user(ARENA / 2, piece);
        assert_eq!(r.p.pread(fd, into, 1000), Ok(piece));
        assert!(r.peek(ARENA / 2, piece) == one, "the first piece");
        assert_eq!(r.p.pread(fd, into, 1000 + piece as i64), Ok(piece));
        assert!(r.peek(ARENA / 2, piece) == two, "the second piece");
        // An odd span across the first boundary of 1 MiB.
        let at = 1 << 20;
        let into = r.arena.user(ARENA / 2, 5000);
        assert_eq!(r.p.pread(fd, into, at - 2500), Ok(5000));
        assert!(r.peek(ARENA / 2, 5000)[..] == one[(at - 2500 - 1000) as usize..][..5000]);
    };
    check(&mut r, fd);
    // A resize copies 3 MiB, in pieces, into a bigger object.
    r.p.ftruncate(fd, 3_500_000).unwrap();
    check(&mut r, fd);
    assert_eq!(
        size_of_object(&r, fd),
        3_500_000usize.next_multiple_of(PAGE)
    );
    // A shrink keeps the head.
    r.p.ftruncate(fd, (1000 + piece) as i64).unwrap();
    let into = r.arena.user(ARENA / 2, piece);
    assert_eq!(r.p.pread(fd, into, 1000), Ok(piece));
    assert!(r.peek(ARENA / 2, piece) == one);
    // A private mapping copies as many pages as it asks for, across pieces.
    let p = map(&mut r, fd, (1 << 20) + 2 * PAGE, RW, MAP_PRIVATE, 0).unwrap();
    assert_eq!(bytes_at(&r, p + 1000, 100), one[..100]);
    assert_eq!(
        bytes_at(&r, p + (1 << 20) + 5, 100),
        one[(1 << 20) + 5 - 1000..][..100]
    );
}

#[test]
fn a_descriptor_reports_itself_as_a_file_and_syncs() {
    let mut r = rig();
    let fd = sized(&mut r, 12_345);
    let st = r.p.fstat(fd).unwrap();
    let mode = u32::from_le_bytes(st[16..20].try_into().unwrap());
    assert_eq!(mode & 0o170_000, 0o100_000, "a regular file: {mode:#o}");
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(u64::from_le_bytes(st[48..56].try_into().unwrap()), 12_345);
    assert_eq!(r.p.fsync(fd), Ok(()));
    assert_eq!(r.p.fsync(99), Err(EBADF));
}

#[test]
fn the_state_of_a_descriptor_follows_what_was_done_to_it() {
    let mut r = rig();
    let fd = memfd(&mut r, MFD_ALLOW_SEALING);
    let state = |r: &Rig, fd: i32| {
        let Some(Desc::Memory(m)) = r.p.fds.get(fd) else {
            panic!("no memory descriptor");
        };
        (
            m.size(),
            m.seals(),
            m.is_pinned(),
            m.is_writable(),
            m.access(),
        )
    };
    assert_eq!(state(&r, fd), (0, 0, false, true, o::RDWR));
    r.p.ftruncate(fd, 5000).unwrap();
    assert_eq!(state(&r, fd), (5000, 0, false, true, o::RDWR));
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::GROW)).unwrap();
    assert_eq!(state(&r, fd), (5000, seal::GROW, false, true, o::RDWR));
    // A seal that makes the handle read-only is seen in the state.
    r.p.fcntl(fd, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    assert_eq!(
        state(&r, fd),
        (5000, seal::GROW | seal::FUTURE_WRITE, false, false, o::RDWR)
    );
    // A shared mapping fixes it.
    let a = map(&mut r, fd, PAGE, PROT_READ, MAP_SHARED, 0).unwrap();
    assert!(state(&r, fd).2);
    r.p.memory_munmap(a, PAGE as u64).unwrap().unwrap();
    // The bounds of a resize: the size itself is no change, one more or less is.
    let g = sized(&mut r, 2 * PAGE);
    r.p.fcntl(g, F_ADD_SEALS, i64::from(seal::GROW)).unwrap();
    assert_eq!(r.p.ftruncate(g, 2 * PAGE as i64), Ok(()));
    assert_eq!(r.p.ftruncate(g, 2 * PAGE as i64 + 1), Err(EPERM));
    assert_eq!(r.p.ftruncate(g, 2 * PAGE as i64 - 1), Ok(()));
    let h = sized(&mut r, 2 * PAGE);
    r.p.fcntl(h, F_ADD_SEALS, i64::from(seal::SHRINK)).unwrap();
    assert_eq!(r.p.ftruncate(h, 2 * PAGE as i64 - 1), Err(EPERM));
    assert_eq!(r.p.ftruncate(h, 2 * PAGE as i64 + 1), Ok(()));
}

#[test]
fn seals_and_the_right_to_add_them_travel_with_a_descriptor() {
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let sealable = sized(&mut r, PAGE);
    r.p.fcntl(sealable, F_ADD_SEALS, i64::from(seal::SHRINK))
        .unwrap();
    let plain = r.p.memfd_create(b"p", 0).unwrap();
    r.p.ftruncate(plain, 100).unwrap();
    send(&mut r, a, &[sealable, plain]).unwrap();
    let got = receive(&mut r, b);
    assert_eq!(r.p.fcntl(got[0], F_GET_SEALS, 0), Ok(seal::SHRINK as i32));
    assert_eq!(r.p.fcntl(got[1], F_GET_SEALS, 0), Ok(seal::SEAL as i32));
    // The first takes another seal; the second, made without the right, does not.
    r.p.fcntl(got[0], F_ADD_SEALS, i64::from(seal::GROW))
        .unwrap();
    assert_eq!(
        r.p.fcntl(got[1], F_ADD_SEALS, i64::from(seal::GROW)),
        Err(EPERM)
    );
    // Each is a descriptor of its own: the sender's seals are not changed.
    assert_eq!(r.p.fcntl(sealable, F_GET_SEALS, 0), Ok(seal::SHRINK as i32));
    // The size and the name-less state came across.
    let Some(Desc::Memory(m)) = r.p.fds.get(got[0]) else {
        panic!("no memory descriptor");
    };
    assert_eq!(
        (m.size(), m.is_pinned(), m.is_writable()),
        (PAGE as u64, true, true)
    );
}

/// Review F3 finding 4: a seal the receiver cannot check is not reported.
#[test]
fn a_forged_seal_on_a_received_descriptor_is_not_reported() {
    use zoraya_posix::unix::{Kind, Record, encode_packet};
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let fd = sized(&mut r, PAGE);
    let Some(Desc::Socket(sock)) = r.p.fds.get(a) else {
        panic!("socket");
    };
    let end = sock.ctl.unwrap();
    // A writable handle with every seal claimed: only the ones that hold.
    let all = u64::from(seal::ALL) | zoraya_posix::unix::MEMORY_SEALABLE;
    let mut forged = Record::one(Kind::Memory, 2, false);
    forged.read_off = PAGE as u64;
    forged.write_off = all;
    let h = r.p.sys.duplicate(object(&r, fd)).unwrap();
    r.p.sys
        .channel_write(end, &encode_packet(&[forged], b"x"), &[h])
        .unwrap();
    let got = receive(&mut r, b)[0];
    assert_eq!(
        r.p.fcntl(got, F_GET_SEALS, 0),
        Ok((seal::SEAL | seal::SHRINK | seal::GROW) as i32),
        "no F_SEAL_WRITE, no F_SEAL_FUTURE_WRITE over a writable handle"
    );
    // A read-only handle: FUTURE_WRITE holds, WRITE is still not claimed.
    let ro = r.p.sys.duplicate_read_only(object(&r, fd)).unwrap();
    forged.access = 0;
    r.p.sys
        .channel_write(end, &encode_packet(&[forged], b"x"), &[ro])
        .unwrap();
    let got = receive(&mut r, b)[0];
    assert_eq!(
        r.p.fcntl(got, F_GET_SEALS, 0),
        Ok((seal::SEAL | seal::SHRINK | seal::GROW | seal::FUTURE_WRITE) as i32)
    );
}

/// Review F3 finding 3: the rights a channel entry names come from the
/// library's state (the fake kernel checks the list of each send), never from
/// a probe that spends a handle slot, so a send with no free slot still names
/// the right rights.
#[test]
fn a_send_names_the_rights_the_library_knows_without_a_probe() {
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let writable = sized(&mut r, PAGE);
    let sealed = sized(&mut r, PAGE);
    r.p.fcntl(sealed, F_ADD_SEALS, i64::from(seal::FUTURE_WRITE))
        .unwrap();
    let before = r.handles();
    send(&mut r, a, &[writable, sealed]).unwrap();
    // Two copies made for the message and nothing else: no probe handle.
    assert_eq!(r.handles(), before + 2);
    let got = receive(&mut r, b);
    assert!(!r.net.borrow().is_read_only(object(&r, got[0])));
    assert!(r.net.borrow().is_read_only(object(&r, got[1])));
}

/// ADR-0432 item 5, INV-0432-2: until plan part F4, a spawn cannot be given
/// a memory descriptor.
#[test]
fn a_memory_descriptor_does_not_cross_a_spawn_yet() {
    let mut r = rig();
    let held = r.handles();
    // Close-on-exec: not inherited, and not an error.
    let c = r.p.memfd_create(b"c", MFD_CLOEXEC).unwrap();
    r.p.ftruncate(c, 100).unwrap();
    let ok =
        r.p.inherit(&[], 0)
            .expect("a close-on-exec descriptor is left behind");
    let made: Vec<u32> = ok
        .slots
        .iter()
        .map(|s| s.1)
        .chain(ok.message.iter().copied())
        .collect();
    for h in made {
        r.p.sys.close(h);
    }
    // Without it: `EBADF`, and every handle the attempt made is closed again.
    let fd = sized(&mut r, PAGE);
    let before = r.handles();
    assert_eq!(r.p.inherit(&[], 0).err(), Some(EBADF));
    assert_eq!(r.handles(), before);
    // The same descriptor given to the child as another fd by a file action.
    r.p.fcntl(fd, zoraya_posix::process::F_SETFD, i64::from(FD_CLOEXEC))
        .unwrap();
    assert!(r.p.inherit(&[], 0).is_ok());
    let _ = held;
}

#[test]
fn a_dup_shares_the_description_and_does_not_fix_the_size() {
    let mut r = rig();
    let fd = memfd(&mut r, MFD_ALLOW_SEALING);
    let d = r.p.dup(fd).unwrap();
    r.p.ftruncate(fd, 100).unwrap();
    r.p.ftruncate(d, 300).unwrap();
    r.write(fd, b"abc").unwrap();
    // The position is shared; so are the seals.
    assert_eq!(r.p.lseek(d, 0, 1), Ok(3));
    r.p.fcntl(d, F_ADD_SEALS, i64::from(seal::GROW)).unwrap();
    assert_eq!(r.p.fcntl(fd, F_GET_SEALS, 0), Ok(seal::GROW as i32));
    let before = r.handles();
    r.p.close(fd).unwrap();
    assert_eq!(r.handles(), before);
    r.p.close(d).unwrap();
    assert_eq!(r.handles(), before - 1);
}

// ------------------------------------------------------------ INV-0413-1

/// What one step of the orders does to a descriptor.
#[derive(Clone, Copy, Debug)]
enum Step {
    Truncate(usize),
    MapShared,
    MapPrivate,
    Dup,
    Send,
    Fork,
    Write(usize),
    Seal(u32),
}

/// Does `step`; says whether it fixed the object (a shared mapping, a send
/// or a fork that happened) and what it mapped.
fn apply(r: &mut Rig, fd: i32, a: i32, step: Step, maps: &mut Vec<u64>) -> bool {
    match step {
        Step::Truncate(n) => {
            let _ = r.p.ftruncate(fd, n as i64);
            false
        }
        Step::MapShared => match map(r, fd, PAGE, PROT_READ, MAP_SHARED, 0) {
            Ok(a) => {
                maps.push(a);
                true
            }
            Err(_) => false,
        },
        Step::MapPrivate => {
            if let Ok(a) = map(r, fd, PAGE, RW, MAP_PRIVATE, 0) {
                maps.push(a);
            }
            false
        }
        Step::Dup => {
            let _ = r.p.dup(fd);
            false
        }
        Step::Send => send(r, a, &[fd]).is_ok(),
        Step::Fork => {
            r.p.after_fork_in_parent();
            true
        }
        Step::Write(n) => {
            let bytes = vec![7u8; n];
            let _ = r.write(fd, &bytes);
            false
        }
        Step::Seal(s) => {
            let _ = r.p.fcntl(fd, F_ADD_SEALS, i64::from(s));
            false
        }
    }
}

/// INV-0413-1: every order of up to five of the steps. Once the descriptor
/// was mapped shared, sent or forked, its object is the same one, of the same
/// size, to the end.
#[test]
fn the_object_never_changes_size_once_mapped_or_sent_in_every_order() {
    let steps = [
        Step::Truncate(PAGE),
        Step::Truncate(3 * PAGE),
        Step::MapShared,
        Step::MapPrivate,
        Step::Dup,
        Step::Send,
        Step::Fork,
        Step::Write(5000),
        Step::Seal(seal::FUTURE_WRITE),
    ];
    let mut orders = 0;
    let mut r = rig();
    let (a, b) = r.p.socketpair(AF_UNIX, SOCK_SEQPACKET, 0).unwrap();
    let base: Vec<u32> = r.p.fds.fds().collect();
    let Some(Desc::Socket(far)) = r.p.fds.get(b) else {
        panic!("socket");
    };
    let far = far.ctl.unwrap();
    for i in 0..steps.len().pow(4) {
        let order = [
            steps[i % 9],
            steps[i / 9 % 9],
            steps[i / 81 % 9],
            steps[i / 729 % 9],
        ];
        let fd = sized(&mut r, 2 * PAGE);
        let mut maps = Vec::new();
        // The object's identity and size once it was fixed.
        let mut fixed: Option<(u32, usize)> = None;
        for step in order {
            let pinned = apply(&mut r, fd, a, step, &mut maps);
            let id = r.net.borrow().object_id(object(&r, fd)).unwrap();
            let now = (id, size_of_object(&r, fd));
            if let Some(was) = fixed {
                assert_eq!(was, now, "order {order:?}: the object changed at {step:?}");
            }
            if pinned {
                fixed = Some(now);
            }
        }
        for m in maps {
            r.p.memory_munmap(m, PAGE as u64).unwrap().unwrap();
        }
        // Drain what the sends queued, and the fds the order made.
        while r.net.borrow_mut().take_message(far).is_some() {}
        let open: Vec<u32> = r.p.fds.fds().collect();
        for fd in open {
            if !base.contains(&fd) {
                r.p.close(fd as i32).unwrap();
            }
        }
        orders += 1;
    }
    assert_eq!(orders, 6561);
}
