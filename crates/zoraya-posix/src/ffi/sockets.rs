//! Sockets at the C boundary (ADR-0225; N4): the stream client's calls,
//! `getaddrinfo`, and the address helpers musl's `src/network` would have
//! given. Every call is a thin wrapper over `net.rs`, which is host tested;
//! what is here is pointers, lengths and `struct` layouts. Not offered:
//! `bind`, `listen`, `accept`, datagram sockets, `socketpair`, `poll`,
//! `sendmsg`/`recvmsg`, `getnameinfo`, `gethostbyname` (ADR-0287 lists them).

use super::procs::{after_write, poll, restartable};
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

#[unsafe(no_mangle)]
extern "C" fn connect(fd: i32, addr: *const u8, len: u32) -> i32 {
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

fn recv_flags(fd: i32, buf: *mut u8, n: usize, flags: i32) -> Result<usize, i32> {
    crate::process::Posix::<RtSys>::check_msg_flags(flags)?;
    let u = user(buf, n)?;
    poll();
    let dontwait = flags & sock::MSG_DONTWAIT != 0;
    restartable(|p| p.sock_read(fd, u, dontwait))
}

fn send_flags(fd: i32, buf: *const u8, n: usize, flags: i32) -> Result<usize, i32> {
    crate::process::Posix::<RtSys>::check_msg_flags(flags)?;
    let u = user(buf, n)?;
    poll();
    let dontwait = flags & sock::MSG_DONTWAIT != 0;
    let r = restartable(|p| p.sock_write(fd, u, dontwait));
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
            ptr::copy_nonoverlapping(bytes.as_ptr(), value, room.min(4));
            len.write_unaligned(4);
        }
    });
    unit(r)
}

#[unsafe(no_mangle)]
extern "C" fn getpeername(fd: i32, addr: *mut u8, len: *mut u32) -> i32 {
    // SAFETY: C's contract for `addr` and `len`.
    let r = state(|p| p.getpeername(fd))
        .and_then(|(a, port)| unsafe { put_sockaddr(addr, len, a, port) });
    unit(r)
}

#[unsafe(no_mangle)]
extern "C" fn getsockname(fd: i32, addr: *mut u8, len: *mut u32) -> i32 {
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
/// list by walking it (musl's own results are a block per element, so a
/// program cannot tell).
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
    // SAFETY: C's contract: NULL or NUL-ended strings.
    let node = if node.is_null() {
        None
    } else {
        match unsafe { c_str(node) } {
            Ok(n) => Some(n),
            Err(_) => return sock::EAI_NONAME,
        }
    };
    // SAFETY: as above.
    let service = if service.is_null() {
        None
    } else {
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
    let (addresses, port, canon) = found;
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
