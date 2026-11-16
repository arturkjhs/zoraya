//! Sockets at the C boundary (ADR-0225; N4): the stream client's calls,
//! `getaddrinfo`, and the address helpers musl's `src/network` would have
//! given. Every call is a thin wrapper over `net.rs`, which is host tested;
//! what is here is pointers, lengths and `struct` layouts. `bind`, `listen`
//! and `accept` are socket files for Unix sockets (plan part F7, proposed
//! ADR-0439) and a listener of `netd` for TCP (plan part F7b, proposed
//! ADR-0441); not offered: datagram sockets.

use super::procs::{after_write, poll, restartable, waiting};
use super::*;

use crate::socket::{self as sock, Hints};

/// The longest `struct sockaddr` read from a program: `sockaddr_storage`.
const SOCKADDR_MAX: usize = 128;

/// `len` bytes at `p`, copied.
///
/// # Safety
///
/// `p` is NULL or readable for `len` bytes.
unsafe fn copy_bytes(p: *const u8, len: usize) -> Result<Vec<u8>, i32> {
    if len > SOCKADDR_MAX {
        return Err(EINVAL);
    }
    if p.is_null() {
        return if len == 0 {
            Ok(Vec::new())
        } else {
            Err(EFAULT)
        };
    }
    // SAFETY: the caller's contract.
    Ok(unsafe { core::slice::from_raw_parts(p, len) }.to_vec())
}

/// Writes a `sockaddr_in` to a program's `addr` / `addrlen` pair, with
/// `getsockname`'s truncation rule: the length is the structure's size, the
/// copy is as much as fits.
///
/// # Safety
///
/// `addr` is NULL or writable for `*addrlen` bytes; `addrlen` is NULL or a
/// `socklen_t`.
unsafe fn put_sockaddr(addr: *mut u8, addrlen: *mut u32, a: [u8; 4], port: u16) -> Result<(), i32> {
    if addr.is_null() || addrlen.is_null() {
        return if addr.is_null() && addrlen.is_null() {
            Ok(())
        } else {
            Err(EFAULT)
        };
    }
    let bytes = sock::sockaddr_in(a, port);
    // SAFETY: the caller's contract.
    unsafe {
        let room = addrlen.read_unaligned() as usize;
        ptr::copy_nonoverlapping(bytes.as_ptr(), addr, room.min(bytes.len()));
        addrlen.write_unaligned(bytes.len() as u32);
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn socket(domain: i32, ty: i32, protocol: i32) -> i32 {
    int(state(|p| p.socket(domain, ty, protocol)))
}

/// `socketpair` (ADR-0412 item 1): a connected pair of Unix sockets.
#[unsafe(no_mangle)]
extern "C" fn socketpair(domain: i32, ty: i32, protocol: i32, sv: *mut i32) -> i32 {
    if sv.is_null() {
        return int(Err(EFAULT));
    }
    match state(|p| p.socketpair(domain, ty, protocol)) {
        Ok((a, b)) => {
            // SAFETY: C's contract: `sv` is an `int[2]`.
            unsafe {
                sv.write_unaligned(a);
                sv.add(1).write_unaligned(b);
            }
            0
        }
        Err(e) => int(Err(e)),
    }
}

#[unsafe(no_mangle)]
extern "C" fn connect(fd: i32, addr: *const u8, len: u32) -> i32 {
    testcancel();
    poll();
    // SAFETY: C's contract: `addr` is a `struct sockaddr` of `len` bytes.
    let r = unsafe { copy_bytes(addr, len as usize) }.and_then(|a| {
        if a.is_empty() {
            return Err(EINVAL);
        }
        restartable(|p| p.connect(fd, &a))
    });
    unit(r)
}

/// `bind` (ADR-0412 item 5, proposed ADR-0439): a socket file in the line's
/// `/tmp` for a Unix socket; for TCP the address `listen` asks `netd` for.
#[unsafe(no_mangle)]
extern "C" fn bind(fd: i32, addr: *const u8, len: u32) -> i32 {
    // SAFETY: C's contract: `addr` is a `struct sockaddr` of `len` bytes.
    let r = unsafe { copy_bytes(addr, len as usize) }.and_then(|a| {
        if a.is_empty() {
            return Err(EINVAL);
        }
        state(|p| p.bind(fd, &a))
    });
    unit(r)
}

#[unsafe(no_mangle)]
extern "C" fn listen(fd: i32, backlog: i32) -> i32 {
    unit(state(|p| p.listen(fd, backlog)))
}

/// `accept4`: the flags are `SOCK_NONBLOCK` and `SOCK_CLOEXEC`; the peer of
/// an accepted Unix socket has no name, a TCP one is a `sockaddr_in`.
fn accept_with(fd: i32, addr: *mut u8, len: *mut u32, flags: i32) -> i32 {
    testcancel();
    if flags & !(sock::SOCK_NONBLOCK | sock::SOCK_CLOEXEC) != 0 {
        return int(Err(EINVAL));
    }
    if addr.is_null() != len.is_null() {
        return int(Err(EFAULT));
    }
    let nonblock = flags & sock::SOCK_NONBLOCK != 0;
    let cloexec = flags & sock::SOCK_CLOEXEC != 0;
    match waiting(|p| p.accept(fd, nonblock, cloexec)) {
        Ok(new) => {
            let family = (crate::unix::AF_UNIX as u16).to_ne_bytes();
            let put = if with(|p| p.is_unix(new)).unwrap_or(true) {
                // SAFETY: C's contract for `addr` and `len`.
                unsafe { put_unix_addr(addr, len, &family) }
            } else {
                match state(|p| p.getpeername(new)) {
                    // SAFETY: C's contract for `addr` and `len`.
                    Ok((a, port)) => unsafe { put_sockaddr(addr, len, a, port) },
                    Err(e) => Err(e),
                }
            };
            if let Err(e) = put {
                let _ = state(|p| p.close(new));
                return int(Err(e));
            }
            new
        }
        Err(e) => int(Err(e)),
    }
}

#[unsafe(no_mangle)]
extern "C" fn accept(fd: i32, addr: *mut u8, len: *mut u32) -> i32 {
    accept_with(fd, addr, len, 0)
}

#[unsafe(no_mangle)]
extern "C" fn accept4(fd: i32, addr: *mut u8, len: *mut u32, flags: i32) -> i32 {
    accept_with(fd, addr, len, flags)
}

fn recv_flags(fd: i32, buf: *mut u8, n: usize, flags: i32) -> Result<usize, i32> {
    crate::process::Posix::<RtSys>::check_msg_flags(flags)?;
    let u = user(buf, n)?;
    testcancel();
    let dontwait = flags & sock::MSG_DONTWAIT != 0;
    waiting(|p| p.sock_read(fd, u, dontwait))
}

fn send_flags(fd: i32, buf: *const u8, n: usize, flags: i32) -> Result<usize, i32> {
    crate::process::Posix::<RtSys>::check_msg_flags(flags)?;
    let u = user(buf, n)?;
    testcancel();
    let dontwait = flags & sock::MSG_DONTWAIT != 0;
    let r = waiting(|p| p.sock_write(fd, u, dontwait));
    if flags & sock::MSG_NOSIGNAL != 0 {
        r
    } else {
        after_write(r)
    }
}

#[unsafe(no_mangle)]
extern "C" fn recv(fd: i32, buf: *mut u8, n: usize, flags: i32) -> isize {
    size(recv_flags(fd, buf, n, flags))
}

#[unsafe(no_mangle)]
extern "C" fn send(fd: i32, buf: *const u8, n: usize, flags: i32) -> isize {
    size(send_flags(fd, buf, n, flags))
}

#[unsafe(no_mangle)]
extern "C" fn recvfrom(
    fd: i32,
    buf: *mut u8,
    n: usize,
    flags: i32,
    addr: *mut u8,
    addrlen: *mut u32,
) -> isize {
    // Check the pair before any data is taken: an error after `recv_flags`
    // would lose the bytes.
    if addr.is_null() != addrlen.is_null() {
        return size(Err(EFAULT));
    }
    let r = recv_flags(fd, buf, n, flags).and_then(|got| {
        if let Some(Some((a, port))) = with(|p| p.getpeername(fd).ok()) {
            // SAFETY: C's contract for `recvfrom`'s `addr` and `addrlen`.
            unsafe { put_sockaddr(addr, addrlen, a, port) }?;
        }
        Ok(got)
    });
    size(r)
}

/// A stream socket ignores the destination, as Linux does for TCP.
#[unsafe(no_mangle)]
extern "C" fn sendto(
    fd: i32,
    buf: *const u8,
    n: usize,
    flags: i32,
    _addr: *const u8,
    _len: u32,
) -> isize {
    size(send_flags(fd, buf, n, flags))
}

#[unsafe(no_mangle)]
extern "C" fn shutdown(fd: i32, how: i32) -> i32 {
    unit(state(|p| p.shutdown(fd, how)))
}

#[unsafe(no_mangle)]
extern "C" fn setsockopt(fd: i32, level: i32, name: i32, value: *const u8, len: u32) -> i32 {
    // The timeouts are a `struct timeval` and are accepted unread; every
    // other option is an `int`.
    let timeout =
        level == sock::SOL_SOCKET && matches!(name, sock::SO_RCVTIMEO | sock::SO_SNDTIMEO);
    let v = if timeout {
        0
    } else if value.is_null() || len < 4 {
        return int(Err(if value.is_null() { EFAULT } else { EINVAL }));
    } else {
        // SAFETY: C's contract: `value` is readable for `len` >= 4 bytes.
        unsafe { value.cast::<i32>().read_unaligned() }
    };
    unit(state(|p| p.setsockopt(fd, level, name, v)))
}

#[unsafe(no_mangle)]
extern "C" fn getsockopt(fd: i32, level: i32, name: i32, value: *mut u8, len: *mut u32) -> i32 {
    if value.is_null() || len.is_null() {
        return int(Err(EFAULT));
    }
    let r = state(|p| p.getsockopt(fd, level, name)).map(|v| {
        // SAFETY: C's contract: `len` is a `socklen_t` and `value` is
        // writable for that many bytes; at most four are written.
        unsafe {
            let room = len.read_unaligned() as usize;
            let bytes = v.to_ne_bytes();
            let copied = room.min(4);
            ptr::copy_nonoverlapping(bytes.as_ptr(), value, copied);
            // As Linux: the length of what was copied.
            len.write_unaligned(copied as u32);
        }
    });
    unit(r)
}

/// Writes a `struct sockaddr_un` (`bytes`: its family and a path, or the
/// family alone for an unnamed socket) to a program's `addr` / `addrlen`
/// pair, with `getsockname`'s truncation rule.
///
/// # Safety
///
/// As [`put_sockaddr`].
unsafe fn put_unix_addr(addr: *mut u8, addrlen: *mut u32, bytes: &[u8]) -> Result<(), i32> {
    if addr.is_null() || addrlen.is_null() {
        return if addr.is_null() && addrlen.is_null() {
            Ok(())
        } else {
            Err(EFAULT)
        };
    }
    // SAFETY: the caller's contract.
    unsafe {
        let room = addrlen.read_unaligned() as usize;
        ptr::copy_nonoverlapping(bytes.as_ptr(), addr, room.min(bytes.len()));
        addrlen.write_unaligned(bytes.len() as u32);
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn getpeername(fd: i32, addr: *mut u8, len: *mut u32) -> i32 {
    if with(|p| p.is_unix(fd)).unwrap_or(false) {
        // SAFETY: C's contract for `addr` and `len`.
        return unit(
            state(|p| p.unix_sockaddr(fd, true))
                .and_then(|b| unsafe { put_unix_addr(addr, len, &b) }),
        );
    }
    // SAFETY: C's contract for `addr` and `len`.
    let r = state(|p| p.getpeername(fd))
        .and_then(|(a, port)| unsafe { put_sockaddr(addr, len, a, port) });
    unit(r)
}

#[unsafe(no_mangle)]
extern "C" fn getsockname(fd: i32, addr: *mut u8, len: *mut u32) -> i32 {
    if with(|p| p.is_unix(fd)).unwrap_or(false) {
        // SAFETY: C's contract for `addr` and `len`.
        return unit(
            state(|p| p.unix_sockaddr(fd, false))
                .and_then(|b| unsafe { put_unix_addr(addr, len, &b) }),
        );
    }
    // SAFETY: C's contract for `addr` and `len`.
    let r = state(|p| p.getsockname(fd))
        .and_then(|(a, port)| unsafe { put_sockaddr(addr, len, a, port) });
    unit(r)
}

// ------------------------------------------------------------ getaddrinfo

/// `struct addrinfo` on LP64 (musl): four `int`s, `socklen_t`, then three
/// pointers.
const ADDRINFO: usize = 48;

/// One result: an `addrinfo`, its `sockaddr_in` and, in the first, the
/// canonical name, in one `malloc` block, so that `freeaddrinfo` frees a
/// list by walking it; a program that frees `ai_addr` or `ai_canonname`
/// itself would corrupt the heap, as it would with musl's single block.
fn build_addrinfo(
    ty: i32,
    protocol: i32,
    address: [u8; 4],
    port: u16,
    canon: Option<&str>,
) -> *mut u8 {
    let canon_len = canon.map_or(0, |c| c.len() + 1);
    let total = ADDRINFO + sock::SOCKADDR_IN + canon_len;
    let block = malloc(total);
    if block.is_null() {
        return block;
    }
    let sa = sock::sockaddr_in(address, port);
    // SAFETY: `block` is `total` bytes, fresh from `malloc`; every write is
    // inside it, at offsets of `struct addrinfo` as musl lays it out.
    unsafe {
        ptr::write_bytes(block, 0, ADDRINFO);
        block.add(4).cast::<i32>().write_unaligned(sock::AF_INET);
        block.add(8).cast::<i32>().write_unaligned(ty);
        block.add(12).cast::<i32>().write_unaligned(protocol);
        block
            .add(16)
            .cast::<u32>()
            .write_unaligned(sock::SOCKADDR_IN as u32);
        let addr = block.add(ADDRINFO);
        ptr::copy_nonoverlapping(sa.as_ptr(), addr, sa.len());
        block.add(24).cast::<*mut u8>().write_unaligned(addr);
        if let Some(c) = canon {
            let at = addr.add(sock::SOCKADDR_IN);
            ptr::copy_nonoverlapping(c.as_ptr(), at, c.len());
            at.add(c.len()).write(0);
            block.add(32).cast::<*mut u8>().write_unaligned(at);
        }
    }
    block
}

/// `getaddrinfo` (ADR-0225 item 6). `EAI_SYSTEM` leaves `errno` set.
#[unsafe(no_mangle)]
extern "C" fn getaddrinfo(
    node: *const c_char,
    service: *const c_char,
    hints: *const u8,
    result: *mut *mut u8,
) -> i32 {
    if result.is_null() {
        return sock::EAI_FAIL;
    }
    let node = if node.is_null() {
        None
    } else {
        // SAFETY: C's contract: NULL or a NUL-ended string.
        match unsafe { c_str(node) } {
            Ok(n) => Some(n),
            Err(_) => return sock::EAI_NONAME,
        }
    };
    let service = if service.is_null() {
        None
    } else {
        // SAFETY: as above.
        match unsafe { c_str(service) } {
            Ok(n) => Some(n),
            Err(_) => return sock::EAI_SERVICE,
        }
    };
    let h = if hints.is_null() {
        Hints::default()
    } else {
        // SAFETY: C's contract: `hints` is a `struct addrinfo`.
        unsafe {
            Hints {
                flags: hints.cast::<i32>().read_unaligned(),
                family: hints.add(4).cast::<i32>().read_unaligned(),
                socktype: hints.add(8).cast::<i32>().read_unaligned(),
                protocol: hints.add(12).cast::<i32>().read_unaligned(),
            }
        }
    };
    poll();
    let found = match with(|p| p.lookup(node, service, &h)) {
        Some(Ok(found)) => found,
        Some(Err(e)) => {
            if e.eai == sock::EAI_SYSTEM {
                set_errno(e.errno);
            }
            return e.eai;
        }
        None => {
            set_errno(ENOSYS);
            return sock::EAI_SYSTEM;
        }
    };
    let crate::net::Lookup {
        addresses,
        port,
        canonical: canon,
    } = found;
    let mut head: *mut u8 = ptr::null_mut();
    let mut tail: *mut u8 = ptr::null_mut();
    for (i, a) in addresses.iter().enumerate() {
        let one = build_addrinfo(
            sock::SOCK_STREAM,
            sock::IPPROTO_TCP,
            *a,
            port,
            if i == 0 { canon.as_deref() } else { None },
        );
        if one.is_null() {
            // SAFETY: `head` is a list built here, or NULL.
            unsafe { freeaddrinfo(head) };
            return sock::EAI_MEMORY;
        }
        if tail.is_null() {
            head = one;
        } else {
            // SAFETY: `tail` is a block built above, 48 bytes.
            unsafe { tail.add(40).cast::<*mut u8>().write_unaligned(one) };
        }
        tail = one;
    }
    // SAFETY: C's contract: `result` is writable.
    unsafe { result.write_unaligned(head) };
    0
}

/// # Safety
///
/// `list` is NULL or what `getaddrinfo` returned, not freed before.
#[unsafe(no_mangle)]
unsafe extern "C" fn freeaddrinfo(list: *mut u8) {
    let mut at = list;
    while !at.is_null() {
        // SAFETY: each element is a block of `build_addrinfo`, 48 bytes
        // with `ai_next` at 40.
        let next = unsafe { at.add(40).cast::<*mut u8>().read_unaligned() };
        free(at);
        at = next;
    }
}

#[unsafe(no_mangle)]
extern "C" fn gai_strerror(code: i32) -> *const u8 {
    sock::gai_message(code).as_ptr()
}

// ------------------------------------------------------------ addresses

#[unsafe(no_mangle)]
extern "C" fn htons(v: u16) -> u16 {
    v.to_be()
}

#[unsafe(no_mangle)]
extern "C" fn ntohs(v: u16) -> u16 {
    u16::from_be(v)
}

#[unsafe(no_mangle)]
extern "C" fn ntohl(v: u32) -> u32 {
    u32::from_be(v)
}

#[unsafe(no_mangle)]
extern "C" fn inet_pton(family: i32, text: *const c_char, out: *mut u8) -> i32 {
    if family != sock::AF_INET {
        set_errno(EAFNOSUPPORT);
        return -1;
    }
    // SAFETY: C's contract: a NUL-ended string.
    let Ok(text) = (unsafe { c_str(text) }) else {
        return 0;
    };
    match sock::inet_pton4(text) {
        Some(a) => {
            // SAFETY: C's contract: `out` is a `struct in_addr`, 4 bytes.
            match unsafe { put(out, &a) } {
                Ok(()) => 1,
                Err(e) => {
                    set_errno(e);
                    -1
                }
            }
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
extern "C" fn inet_ntop(family: i32, from: *const u8, out: *mut u8, room: u32) -> *const u8 {
    if family != sock::AF_INET {
        set_errno(EAFNOSUPPORT);
        return ptr::null();
    }
    if from.is_null() {
        set_errno(EFAULT);
        return ptr::null();
    }
    // SAFETY: C's contract: `from` is a `struct in_addr`, 4 bytes.
    let a = unsafe {
        [
            from.read(),
            from.add(1).read(),
            from.add(2).read(),
            from.add(3).read(),
        ]
    };
    let mut text = sock::ntop4(a).into_bytes();
    text.push(0);
    if text.len() > room as usize {
        set_errno(ENOSPC);
        return ptr::null();
    }
    // SAFETY: C's contract: `out` is writable for `room` bytes.
    match unsafe { put(out, &text) } {
        Ok(()) => out,
        Err(e) => {
            set_errno(e);
            ptr::null()
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn inet_aton(text: *const c_char, out: *mut u8) -> i32 {
    // SAFETY: C's contract: a NUL-ended string.
    let Ok(text) = (unsafe { c_str(text) }) else {
        return 0;
    };
    match sock::inet_aton(text) {
        Some(a) => {
            if !out.is_null() {
                // SAFETY: `out` is a `struct in_addr` when not NULL.
                let _ = unsafe { put(out, &a) };
            }
            1
        }
        None => 0,
    }
}

/// `INADDR_NONE` (all ones) on failure, as C has it.
#[unsafe(no_mangle)]
extern "C" fn inet_addr(text: *const c_char) -> u32 {
    // SAFETY: C's contract: a NUL-ended string.
    let Ok(text) = (unsafe { c_str(text) }) else {
        return u32::MAX;
    };
    sock::inet_aton(text).map_or(u32::MAX, u32::from_ne_bytes)
}

/// The text goes to a static buffer, as C's `inet_ntoa` does (one thread).
#[unsafe(no_mangle)]
extern "C" fn inet_ntoa(a: u32) -> *const u8 {
    struct Text(UnsafeCell<[u8; 16]>);
    // SAFETY: one thread (ADR-0100 item 5).
    unsafe impl Sync for Text {}
    static BUFFER: Text = Text(UnsafeCell::new([0; 16]));
    let mut text = sock::ntop4(a.to_ne_bytes()).into_bytes();
    text.push(0);
    let at = BUFFER.0.get().cast::<u8>();
    // SAFETY: one thread; a dotted quad and its NUL are at most 16 bytes.
    unsafe { ptr::copy_nonoverlapping(text.as_ptr(), at, text.len()) };
    at
}

/// `getrandom` (ADR-0227 item 4; proposed ADR-0310): the bytes come from the
/// random service over the connection the launcher gave the program, all of
/// `n` or `-1` with `errno` (`ENOSYS` without the connection). Unlike
/// Linux's, a request for more than 256 bytes is not cut short, and the
/// flags that matter there (`GRND_NONBLOCK`, `GRND_RANDOM`) change nothing:
/// the generator never blocks. The lock is taken once per request
/// (`random::in_spans`, INV-0302-2): with threads the wait for the reply is
/// made inside it.
#[unsafe(no_mangle)]
extern "C" fn getrandom(buf: *mut u8, n: usize, flags: u32) -> isize {
    let r = user(buf, n).and_then(|u| {
        poll();
        crate::random::in_spans(u, |part| restartable(|p| p.getrandom(part, flags)))
    });
    size(r)
}

// ------------------------------------------------------------ sendmsg, recvmsg

// `struct msghdr` and `struct cmsghdr` as musl lays them out on AArch64
// (LP64, little endian): the padding after `msg_iovlen`, `msg_controllen` and
// `cmsg_len` is part of the structures.
const MSG_NAME: usize = 0;
const MSG_NAMELEN: usize = 8;
const MSG_IOV: usize = 16;
const MSG_IOVLEN: usize = 24;
const MSG_CONTROL: usize = 32;
const MSG_CONTROLLEN: usize = 40;
const MSG_FLAGS: usize = 48;
const CMSG_HEADER: usize = 16;

/// `CMSG_ALIGN`.
const fn cmsg_align(n: usize) -> usize {
    (n + 7) & !7
}

/// The descriptors of the `SCM_RIGHTS` control messages in `control`. A
/// message of another level or type is refused (`EINVAL`): `SCM_CREDENTIALS`
/// and the rest are not offered (ADR-0412 item 4).
///
/// # Safety
///
/// `control` is NULL or readable for `len` bytes.
unsafe fn scm_rights(control: *const u8, len: usize) -> Result<Vec<i32>, i32> {
    let mut out = Vec::new();
    if len == 0 {
        return Ok(out);
    }
    if control.is_null() {
        return Err(EFAULT);
    }
    let mut at = 0;
    while at + CMSG_HEADER <= len {
        // SAFETY: `at + CMSG_HEADER <= len`, inside what the caller gave.
        let (clen, level, kind) = unsafe {
            let h = control.add(at);
            (
                h.cast::<u32>().read_unaligned() as usize,
                h.add(8).cast::<i32>().read_unaligned(),
                h.add(12).cast::<i32>().read_unaligned(),
            )
        };
        if clen < CMSG_HEADER || at + clen > len {
            return Err(EINVAL);
        }
        if level != sock::SOL_SOCKET || kind != crate::unix::SCM_RIGHTS {
            return Err(EINVAL);
        }
        let data = clen - CMSG_HEADER;
        if !data.is_multiple_of(4) {
            return Err(EINVAL);
        }
        for i in 0..data / 4 {
            // SAFETY: inside this message, checked above.
            out.push(unsafe {
                control
                    .add(at + CMSG_HEADER + 4 * i)
                    .cast::<i32>()
                    .read_unaligned()
            });
            if out.len() > crate::unix::MAX_HANDLES {
                return Err(crate::unix::ETOOMANYREFS);
            }
        }
        at += cmsg_align(clen);
    }
    Ok(out)
}

#[unsafe(no_mangle)]
extern "C" fn sendmsg(fd: i32, msg: *const u8, flags: i32) -> isize {
    if msg.is_null() {
        return size(Err(EFAULT));
    }
    let r = (|| {
        crate::process::Posix::<RtSys>::check_msg_flags(flags)?;
        // SAFETY: C's contract: `msg` is a `struct msghdr`, whose iovecs and
        // control buffer are the program's.
        let (iov, rights) = unsafe {
            let iov = msg
                .add(MSG_IOV)
                .cast::<*const [usize; 2]>()
                .read_unaligned();
            let count = msg.add(MSG_IOVLEN).cast::<i32>().read_unaligned();
            let control = msg.add(MSG_CONTROL).cast::<*const u8>().read_unaligned();
            let controllen = msg.add(MSG_CONTROLLEN).cast::<u32>().read_unaligned();
            (
                vectors(iov, i64::from(count))?,
                scm_rights(control, controllen as usize)?,
            )
        };
        testcancel();
        waiting(|p| p.sendmsg(fd, &iov, &rights, flags))
    })();
    if flags & sock::MSG_NOSIGNAL != 0 {
        size(r)
    } else {
        size(after_write(r))
    }
}

#[unsafe(no_mangle)]
extern "C" fn recvmsg(fd: i32, msg: *mut u8, flags: i32) -> isize {
    if msg.is_null() {
        return size(Err(EFAULT));
    }
    let r = (|| {
        crate::process::Posix::<RtSys>::check_msg_flags(flags)?;
        // SAFETY: C's contract, as `sendmsg`'s.
        let (iov, control, controllen) = unsafe {
            let iov = msg
                .add(MSG_IOV)
                .cast::<*const [usize; 2]>()
                .read_unaligned();
            let count = msg.add(MSG_IOVLEN).cast::<i32>().read_unaligned();
            (
                vectors(iov, i64::from(count))?,
                msg.add(MSG_CONTROL).cast::<*mut u8>().read_unaligned(),
                msg.add(MSG_CONTROLLEN).cast::<u32>().read_unaligned() as usize,
            )
        };
        if controllen > 0 && control.is_null() {
            return Err(EFAULT);
        }
        // As many descriptors as the control buffer holds beside its header.
        let max_fds = controllen.saturating_sub(CMSG_HEADER) / 4;
        testcancel();
        let got = waiting(|p| p.recvmsg(fd, &iov, max_fds, flags))?;
        let mut used = 0;
        if !got.fds.is_empty() {
            let clen = CMSG_HEADER + 4 * got.fds.len();
            // SAFETY: `max_fds` descriptors fit in `controllen` after the
            // header, and `got.fds.len() <= max_fds`.
            unsafe {
                ptr::write_bytes(control, 0, clen);
                control.cast::<u32>().write_unaligned(clen as u32);
                control
                    .add(8)
                    .cast::<i32>()
                    .write_unaligned(sock::SOL_SOCKET);
                control
                    .add(12)
                    .cast::<i32>()
                    .write_unaligned(crate::unix::SCM_RIGHTS);
                for (i, fd) in got.fds.iter().enumerate() {
                    control
                        .add(CMSG_HEADER + 4 * i)
                        .cast::<i32>()
                        .write_unaligned(*fd);
                }
            }
            used = cmsg_align(clen).min(controllen);
        }
        // SAFETY: the fields of the caller's `struct msghdr`.
        unsafe {
            msg.add(MSG_CONTROLLEN)
                .cast::<u32>()
                .write_unaligned(used as u32);
            msg.add(MSG_FLAGS).cast::<i32>().write_unaligned(got.flags);
            // A connected Unix socket has no peer address to give.
            if !msg
                .add(MSG_NAME)
                .cast::<*mut u8>()
                .read_unaligned()
                .is_null()
            {
                msg.add(MSG_NAMELEN).cast::<u32>().write_unaligned(0);
            }
        }
        Ok(got.bytes)
    })();
    size(r)
}
