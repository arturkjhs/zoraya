//! Processes, signals and the terminal at the C boundary (stage 6d, ADR-0102
//! to ADR-0106, ADR-0108, ADR-0142): the handlers run here, between calls
//! into the state, never inside one; `spawn`, `snapshot_process` and the
//! kernel's waits are made here.

use super::*;

use zoraya_abi::posix::signal::{
    SIGABRT, SIGCHLD, SIGCONT, SIGKILL, SIGPIPE, SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU,
};
use zoraya_abi::posix::{EntryKind, Facts};
use zoraya_abi::spawn::{Encoder as SpawnEncoder, HandleEntry, Transfer};
use zoraya_abi::{Disposition, RequestKind, Rights, process_signals};
use zoraya_proto_posix as bp;
use zoraya_proto_tty as tp;
use zoraya_protocol::{Below, Data, decode};

use crate::jobs::{FileAction, Next, Prepared, WNOHANG, WUNTRACED};
use crate::signals::{
    Action, SIG_BLOCK, SIG_DFL, SIG_IGN, SIG_SETMASK, Taken, bit,
    flags::{SA_RESETHAND, SA_RESTART, SA_SIGINFO},
};

// ------------------------------------------------------------ delivery

/// What a round of delivery did.
struct Delivered {
    ran: bool,
    /// Every handler that ran had `SA_RESTART`.
    restart: bool,
}

/// `si_code` values.
const SI_USER: i32 = 0;
const CLD_EXITED: i32 = 1;

/// Runs signal `n`'s handler on this thread (ADR-0106 item 6), with the
/// signal and the action's mask blocked for its duration.
fn run_handler(n: u32, a: Action) {
    let old = with(|p| {
        let old = p.signals.enter(n, &a);
        if a.flags & SA_RESETHAND != 0 {
            sync_disposition(p, n);
        }
        old
    })
    .unwrap_or(0);
    let mut info = [0u8; 128];
    info[..4].copy_from_slice(&(n as i32).to_le_bytes());
    let code = if n == SIGCHLD { CLD_EXITED } else { SI_USER };
    info[8..12].copy_from_slice(&code.to_le_bytes());
    if a.flags & SA_SIGINFO != 0 {
        // SAFETY: the program installed this address with `SA_SIGINFO` as a
        // `void (*)(int, siginfo_t *, void *)`; `info` is a `siginfo_t`'s
        // 128 bytes, alive for the call.
        let f: extern "C" fn(i32, *mut u8, *mut u8) = unsafe { core::mem::transmute(a.handler) };
        f(n as i32, info.as_mut_ptr(), ptr::null_mut());
    } else {
        // SAFETY: the program installed this address as a `void (*)(int)`.
        let f: extern "C" fn(i32) = unsafe { core::mem::transmute(a.handler) };
        f(n as i32);
    }
    // A handler that does not return (dash's `onint`, make's re-raise)
    // leaves the wait it interrupted abandoned, as the timelines show; the
    // mask is then the program's to put back, as on Linux.
    let _ = with(|p| p.signals.leave(old));
}

/// Takes every request and acts on each (ADR-0106 items 4 and 7).
fn deliver() -> Delivered {
    let mut d = Delivered {
        ran: false,
        restart: true,
    };
    loop {
        match with(|p| p.next_delivery()).unwrap_or(Next::None) {
            Next::None => return d,
            Next::Terminate(n) => zoraya_rt::process_exit_terminated(n),
            Next::Stop(n) => stop_self(n),
            Next::Run(n, a) => {
                run_handler(n, a);
                d.ran = true;
                if a.flags & SA_RESTART == 0 {
                    d.restart = false;
                }
            }
        }
    }
}

/// Asks the broker to stop this process; returns when it is continued.
fn stop_self(n: u32) {
    let _ = state(|p| p.broker_simple(&bp::StopSelf { signal: n }, &[]));
}

/// The poll at a library entry (ADR-0106 item 5): only with a handler
/// installed (INV-0106-2), or with a pending one now unblocked.
pub(super) fn poll() {
    let wanted =
        with(|p| p.signals.handlers_installed() || p.signals.pending & !p.signals.blocked != 0)
            .unwrap_or(false);
    if wanted {
        deliver();
    }
}

/// Runs `f` on the state, delivering what interrupts it: again when every
/// handler that ran had `SA_RESTART`, else `EINTR` (INV-0006-7).
pub(super) fn restartable<R>(
    mut f: impl FnMut(&mut Posix<RtSys>) -> Result<R, i32>,
) -> Result<R, i32> {
    loop {
        match state(&mut f) {
            Err(EINTR) => {
                let d = deliver();
                if d.ran && !d.restart {
                    return Err(EINTR);
                }
            }
            r => return r,
        }
    }
}

/// A signal of the process's own (ADR-0106 item 8): `raise`, `kill` of
/// oneself, `SIGPIPE` on a write.
fn raise_signal(n: u32) -> Result<(), i32> {
    raise_signal_for(n, false)
}

/// As [`raise_signal`]; `process` is set for a signal directed at the process
/// (`kill` of oneself): one this thread blocks goes to a thread that does not
/// (ADR-0304 item 3), instead of staying pending here.
fn raise_signal_for(n: u32, process: bool) -> Result<(), i32> {
    if n == 0 {
        return Ok(());
    }
    if n == SIGKILL {
        zoraya_rt::process_exit_terminated(SIGKILL);
    }
    if n == SIGSTOP {
        stop_self(n);
        return Ok(());
    }
    let taken = with(|p| {
        let t = p.signals.take(n);
        if process && t == Taken::Pending {
            p.route(n);
        }
        t
    })
    .unwrap_or(Taken::Drop);
    match taken {
        Taken::Run(a) => run_handler(n, a),
        Taken::Default(crate::signals::DefaultAction::Terminate) => {
            zoraya_rt::process_exit_terminated(n)
        }
        Taken::Default(crate::signals::DefaultAction::Stop) => stop_self(n),
        Taken::Default(_) | Taken::Drop | Taken::Pending => {}
    }
    Ok(())
}

/// Delivers what is deliverable to this thread; true if a handler ran
/// (ADR-0304 item 11).
pub(super) fn deliver_ran() -> bool {
    deliver().ran
}

/// After a write: `EPIPE` is `SIGPIPE` first (ADR-0106 item 8).
pub(super) fn after_write(r: Result<usize, i32>) -> Result<usize, i32> {
    if r == Err(EPIPE) {
        let _ = raise_signal(SIGPIPE);
    }
    r
}

pub(super) fn read_fd(fd: i32, u: User) -> Result<usize, i32> {
    testcancel();
    poll();
    restartable(|p| p.read(fd, u))
}

pub(super) fn write_fd(fd: i32, u: User) -> Result<usize, i32> {
    testcancel();
    poll();
    after_write(restartable(|p| p.write(fd, u)))
}

// ------------------------------------------------------------ sigaction

/// The kernel's disposition for `n` from its POSIX action (ADR-0106 item 1),
/// and the broker's record for the stop and continue signals (item 9).
fn sync_disposition(p: &mut Posix<RtSys>, n: u32) {
    if let Some(kind) = zoraya_abi::posix::signal::kind(n) {
        let _ = p.sys.set_disposition(kind, p.signals.kernel_disposition(n));
    }
}

fn stop_record(a: &Action) -> u32 {
    if a.is_handler() {
        bp::stop::HANDLER
    } else if a.handler == SIG_IGN {
        bp::stop::IGNORE
    } else {
        bp::stop::DEFAULT
    }
}

/// musl's `struct sigaction`: the handler, a 128-byte `sigset_t`, the flags,
/// the restorer.
const SIGACTION_SIZE: usize = 152;

/// # Safety
///
/// `p` is a `struct sigaction`.
unsafe fn read_sigaction(p: *const u8) -> Action {
    // SAFETY: the caller's contract; the fields at musl's offsets.
    unsafe {
        Action {
            handler: (p as *const usize).read_unaligned(),
            mask: (p.add(8) as *const u64).read_unaligned(),
            flags: (p.add(136) as *const u32).read_unaligned(),
        }
    }
}

/// # Safety
///
/// `p` is a writable `struct sigaction`.
unsafe fn write_sigaction(p: *mut u8, a: &Action) {
    // SAFETY: the caller's contract.
    unsafe {
        ptr::write_bytes(p, 0, SIGACTION_SIZE);
        (p as *mut usize).write_unaligned(a.handler);
        (p.add(8) as *mut u64).write_unaligned(a.mask);
        (p.add(136) as *mut u32).write_unaligned(a.flags);
    }
}

fn set_action(n: i32, new: Option<Action>) -> Result<Action, i32> {
    let n = u32::try_from(n).map_err(|_| EINVAL)?;
    let old = state(|p| {
        let old = p.signals.set(n, new)?;
        if new.is_some() {
            sync_disposition(p, n);
        }
        Ok(old)
    })?;
    if let Some(a) = new
        && matches!(n, SIGTSTP | SIGTTIN | SIGTTOU | SIGCONT)
        && stop_record(&a) != stop_record(&old)
    {
        // The one sigaction that costs a round trip (ADR-0106 item 9).
        let m = bp::SetStopDisposition {
            signal: n,
            disposition: Below(stop_record(&a)),
        };
        let _ = state(|p| p.broker_simple(&m, &[]));
    }
    poll();
    Ok(old)
}

#[unsafe(no_mangle)]
extern "C" fn sigaction(n: i32, act: *const u8, old: *mut u8) -> i32 {
    // SAFETY: C's contract: `act` is NULL or a `struct sigaction`.
    let new = (!act.is_null()).then(|| unsafe { read_sigaction(act) });
    unit(set_action(n, new).map(|o| {
        if !old.is_null() {
            // SAFETY: C's contract: `old` is NULL or writable.
            unsafe { write_sigaction(old, &o) };
        }
    }))
}

/// `signal` and `bsd_signal`: BSD semantics, `SA_RESTART`.
#[unsafe(no_mangle)]
extern "C" fn signal(n: i32, handler: usize) -> usize {
    let a = Action {
        handler,
        mask: 0,
        flags: SA_RESTART,
    };
    match set_action(n, Some(a)) {
        Ok(old) => old.handler,
        Err(e) => {
            set_errno(e);
            usize::MAX
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn bsd_signal(n: i32, handler: usize) -> usize {
    signal(n, handler)
}

// ------------------------------------------------------------ masks

/// # Safety
///
/// `set` is NULL or a `sigset_t`.
unsafe fn read_set(set: *const u64) -> Option<u64> {
    // SAFETY: the caller's contract; the first word holds signals 1 to 64.
    (!set.is_null()).then(|| unsafe { set.read_unaligned() })
}

/// # Safety
///
/// `set` is NULL or a writable `sigset_t` (128 bytes).
unsafe fn write_set(set: *mut u64, value: u64) {
    if !set.is_null() {
        // SAFETY: the caller's contract.
        unsafe {
            ptr::write_bytes(set as *mut u8, 0, 128);
            set.write_unaligned(value);
        }
    }
}

fn set_mask(how: i32, set: Option<u64>) -> Result<u64, i32> {
    let old = state(|p| p.signals.mask(how, set))?;
    // Unblocked: what was pending is delivered now (item 2).
    poll();
    Ok(old)
}

#[unsafe(no_mangle)]
extern "C" fn sigprocmask(how: i32, set: *const u64, old: *mut u64) -> i32 {
    // SAFETY: C's contract for sigprocmask.
    let new = unsafe { read_set(set) };
    unit(set_mask(how, new).map(|o| {
        // SAFETY: as above.
        unsafe { write_set(old, o) }
    }))
}

#[unsafe(no_mangle)]
extern "C" fn pthread_sigmask(how: i32, set: *const u64, old: *mut u64) -> i32 {
    match sigprocmask(how, set, old) {
        0 => 0,
        // SAFETY: musl's errno.
        _ => unsafe { *__errno_location() },
    }
}

/// `sigset_t` operations: signals 1 to 64 in the first word.
fn set_op(set: *mut u64, n: i32, f: impl FnOnce(u64, u64) -> u64) -> i32 {
    if set.is_null() {
        return int(Err(EFAULT));
    }
    if !(1..=64).contains(&n) {
        return int(Err(EINVAL));
    }
    // SAFETY: C's contract: `set` is a `sigset_t`.
    unsafe { set.write_unaligned(f(set.read_unaligned(), bit(n as u32))) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn sigemptyset(set: *mut u64) -> i32 {
    if set.is_null() {
        return int(Err(EFAULT));
    }
    // SAFETY: C's contract.
    unsafe { write_set(set, 0) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn sigfillset(set: *mut u64) -> i32 {
    if set.is_null() {
        return int(Err(EFAULT));
    }
    // SAFETY: C's contract. musl leaves out its two internal signals; here
    // the standard ones and the real-time range are all set.
    unsafe { write_set(set, u64::MAX) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn sigaddset(set: *mut u64, n: i32) -> i32 {
    set_op(set, n, |s, b| s | b)
}

#[unsafe(no_mangle)]
extern "C" fn sigdelset(set: *mut u64, n: i32) -> i32 {
    set_op(set, n, |s, b| s & !b)
}

#[unsafe(no_mangle)]
extern "C" fn sigismember(set: *const u64, n: i32) -> i32 {
    if set.is_null() || !(1..=64).contains(&n) {
        return int(Err(EINVAL));
    }
    // SAFETY: C's contract.
    i32::from(unsafe { set.read_unaligned() } & bit(n as u32) != 0)
}

// ------------------------------------------------------------ waiting

/// `sigsuspend` (ADR-0106 items 2 and 3): the mask for the wait, a wait on
/// the process's requests and the children, until a handler runs.
fn suspend_with(mask: Option<u64>) -> i32 {
    testcancel();
    let old = match mask {
        Some(m) => match state(|p| p.signals.mask(SIG_SETMASK, Some(m))) {
            Ok(o) => Some(o),
            Err(e) => return int(Err(e)),
        },
        None => None,
    };
    loop {
        let d = deliver();
        if d.ran {
            break;
        }
        let r = state(|p| p.wait_for(&[], None));
        match r {
            Err(EINTR) | Ok(_) => {}
            // Nothing can wake it: no handler, no child.
            Err(EINVAL) => {
                zoraya_rt::sleep_until(u64::MAX);
            }
            Err(_) => break,
        }
    }
    if let Some(o) = old {
        let _ = state(|p| p.signals.mask(SIG_SETMASK, Some(o)));
        poll();
    }
    int(Err(EINTR))
}

#[unsafe(no_mangle)]
extern "C" fn sigsuspend(mask: *const u64) -> i32 {
    // SAFETY: C's contract: `mask` is a `sigset_t`.
    let m = unsafe { read_set(mask) };
    if m.is_none() {
        return int(Err(EFAULT));
    }
    suspend_with(m)
}

#[unsafe(no_mangle)]
extern "C" fn pause() -> i32 {
    suspend_with(None)
}

/// `nanosleep`'s core (ADR-0106 item 10): the port's deadline, `EINTR`
/// with the time left when a handler ran.
fn sleep_until(deadline: u64) -> Result<(), u64> {
    loop {
        let r = state(|p| p.wait_for(&[], Some(deadline)));
        match r {
            Ok((None, None)) => return Ok(()),
            Err(EINTR) => {
                let d = deliver();
                if d.ran {
                    let now = zoraya_rt::clock_monotonic();
                    return Err(deadline.saturating_sub(now));
                }
            }
            Ok(_) => {}
            Err(_) => {
                zoraya_rt::sleep_until(deadline);
                return Ok(());
            }
        }
        if zoraya_rt::clock_monotonic() >= deadline {
            return Ok(());
        }
    }
}

pub(super) fn nanosleep_until(deadline: u64, rem: *mut [i64; 2]) -> i32 {
    testcancel();
    poll();
    match sleep_until(deadline) {
        Ok(()) => 0,
        Err(left) => {
            if !rem.is_null() {
                // SAFETY: C's contract: `rem` is NULL or a `struct timespec`.
                unsafe {
                    rem.write([(left / 1_000_000_000) as i64, (left % 1_000_000_000) as i64])
                };
            }
            EINTR
        }
    }
}

/// `waitpid` and its kin (ADR-0104 item 11).
fn wait_child(pid: i32, status: *mut i32, options: i32) -> i32 {
    testcancel();
    poll();
    match restartable(|p| p.waitpid(pid, options)) {
        Ok(Some((child, s))) => {
            if !status.is_null() {
                // SAFETY: C's contract: `status` is NULL or an int.
                unsafe { status.write(s) };
            }
            child as i32
        }
        Ok(None) => 0,
        Err(e) => int(Err(e)),
    }
}

#[unsafe(no_mangle)]
extern "C" fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32 {
    wait_child(pid, status, options)
}

#[unsafe(no_mangle)]
extern "C" fn wait(status: *mut i32) -> i32 {
    wait_child(-1, status, 0)
}

/// The shim's target for musl's `__sys_wait4` (`pclose.c`, proposed
/// ADR-0210): `waitpid` with the result or `-errno`, and a zero `struct
/// rusage` as `wait3` gives.
#[unsafe(no_mangle)]
extern "C" fn zoraya_posix_wait4(pid: i64, status: i64, options: i64, usage: i64) -> i64 {
    if usage != 0 {
        // SAFETY: C's contract: `usage` is NULL or a `struct rusage`.
        unsafe { ptr::write_bytes(usage as *mut u8, 0, 144) };
    }
    poll();
    let status = status as *mut i32;
    match restartable(|p| p.waitpid(pid as i32, options as i32)) {
        Ok(Some((child, s))) => {
            if !status.is_null() {
                // SAFETY: C's contract: `status` is NULL or an int.
                unsafe { status.write(s) };
            }
            i64::from(child)
        }
        Ok(None) => 0,
        Err(e) => -i64::from(e),
    }
}

#[unsafe(no_mangle)]
extern "C" fn wait3(status: *mut i32, options: i32, usage: *mut u8) -> i32 {
    if !usage.is_null() {
        // No accounting: a zero `struct rusage` (144 bytes).
        // SAFETY: C's contract: `usage` is NULL or a `struct rusage`.
        unsafe { ptr::write_bytes(usage, 0, 144) };
    }
    wait_child(-1, status, options)
}

/// `pselect` (ADR-0106 items 3 and 10): stream ends wait for their signals;
/// files, directories, `/dev/null` and terminals are ready at once (a
/// terminal's readiness, `poll_readable`, is deferred by ADR-0105).
#[unsafe(no_mangle)]
extern "C" fn pselect(
    nfds: i32,
    readfds: *mut u64,
    writefds: *mut u64,
    exceptfds: *mut u64,
    timeout: *const [i64; 2],
    sigmask: *const u64,
) -> i32 {
    // A cancellation point, acted on before any buffer is built.
    // A cancellation point, acted on before any buffer is built.
    testcancel();
    if !(0..=1024).contains(&nfds) {
        return int(Err(EINVAL));
    }
    let words = (nfds as usize).div_ceil(64);
    let get = |set: *mut u64| -> Vec<u64> {
        if set.is_null() {
            vec![0; words]
        } else {
            (0..words)
                // SAFETY: C's contract: each set is NULL or an `fd_set` of
                // 1024 bits, and `words` covers `nfds` of them.
                .map(|i| unsafe { set.add(i).read_unaligned() })
                .collect()
        }
    };
    let (rin, win) = (get(readfds), get(writefds));
    let deadline = if timeout.is_null() {
        None
    } else {
        // SAFETY: C's contract: a `struct timespec`.
        let [s, ns] = unsafe { timeout.read() };
        if s < 0 || !(0..1_000_000_000).contains(&ns) {
            return int(Err(EINVAL));
        }
        let length = (s as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(ns as u64);
        Some(zoraya_rt::clock_monotonic().saturating_add(length))
    };
    // SAFETY: C's contract for the mask.
    let mask = unsafe { read_set(sigmask) };
    let old = match mask {
        Some(m) => state(|p| p.signals.mask(SIG_SETMASK, Some(m))).ok(),
        None => None,
    };
    let restore = |old: Option<u64>| {
        if let Some(o) = old {
            let _ = state(|p| p.signals.mask(SIG_SETMASK, Some(o)));
        }
    };
    loop {
        if deliver().ran {
            restore(old);
            poll();
            return int(Err(EINTR));
        }
        let r = state(|p| select_once(p, nfds, &rin, &win, deadline));
        match r {
            Ok(Some((rout, wout, n))) => {
                let put = |set: *mut u64, v: &[u64]| {
                    if !set.is_null() {
                        for (i, w) in v.iter().enumerate() {
                            // SAFETY: C's contract, as for the reads above.
                            unsafe { set.add(i).write_unaligned(*w) };
                        }
                    }
                };
                put(readfds, &rout);
                put(writefds, &wout);
                if !exceptfds.is_null() {
                    put(exceptfds, &vec![0; words]);
                }
                restore(old);
                poll();
                return n;
            }
            Ok(None) => {}
            Err(EINTR) => {}
            Err(e) => {
                restore(old);
                return int(Err(e));
            }
        }
    }
}

/// One look at the fds and, if none is ready, one wait: `Some` with the
/// ready sets (and a timeout's empty ones), `None` to look again.
#[allow(clippy::type_complexity)] // the two sets and the count pselect gives
fn select_once(
    p: &mut Posix<RtSys>,
    nfds: i32,
    rin: &[u64],
    win: &[u64],
    deadline: Option<u64>,
) -> Result<Option<(Vec<u64>, Vec<u64>, i32)>, i32> {
    use crate::sys::signals::{PEER_CLOSED, READABLE, WRITABLE};
    let mut rout = vec![0u64; rin.len()];
    let mut wout = vec![0u64; win.len()];
    let mut count = 0;
    let mut sources: Vec<(Handle, u32)> = Vec::new();
    let mut owners: Vec<(usize, bool)> = Vec::new();
    for fd in 0..nfds as usize {
        let (w, b) = (fd / 64, 1u64 << (fd % 64));
        for (want, ready, writing) in [(rin, &mut rout, false), (win, &mut wout, true)] {
            if want[w] & b == 0 {
                continue;
            }
            let end = match p.fds.get(fd as i32) {
                None => return Err(EBADF),
                Some(Desc::Stream {
                    read, write, tty, ..
                }) if !tty => {
                    if writing {
                        write.map(|e| (e, WRITABLE | PEER_CLOSED))
                    } else {
                        read.map(|e| (e, READABLE | PEER_CLOSED))
                    }
                }
                Some(Desc::Socket(sk)) => sk.wait_source(writing),
                Some(_) => None,
            };
            match end {
                Some((e, signals)) => {
                    let now = p.sys.signals_of(e).unwrap_or(signals);
                    if now & signals != 0 {
                        ready[w] |= b;
                        count += 1;
                    } else {
                        sources.push((e, signals));
                        owners.push((fd, writing));
                    }
                }
                None => {
                    ready[w] |= b;
                    count += 1;
                }
            }
        }
    }
    if count > 0 {
        return Ok(Some((rout, wout, count)));
    }
    if deadline.is_some_and(|d| zoraya_rt::clock_monotonic() >= d) {
        return Ok(Some((rout, wout, 0)));
    }
    match p.wait_for(&sources, deadline)? {
        (None, None) => Ok(Some((rout, wout, 0))),
        _ => Ok(None),
    }
}

// ------------------------------------------------------------ kill

#[unsafe(no_mangle)]
extern "C" fn kill(pid: i32, sig: i32) -> i32 {
    let Ok(n) = u32::try_from(sig) else {
        return int(Err(EINVAL));
    };
    if n > zoraya_abi::posix::signal::MAX {
        return int(Err(EINVAL));
    }
    let me = with(|p| p.facts.pid as i32).unwrap_or(0);
    let r = if pid == me && me > 0 {
        raise_signal_for(n, true)
    } else {
        state(|p| p.kill_other(pid, n))
    };
    // kill polls always (item 5): a signal to one's own group is here now.
    deliver();
    unit(r)
}

#[unsafe(no_mangle)]
extern "C" fn killpg(pgrp: i32, sig: i32) -> i32 {
    if pgrp <= 1 {
        return int(Err(EINVAL));
    }
    kill(-pgrp, sig)
}

#[unsafe(no_mangle)]
extern "C" fn raise(sig: i32) -> i32 {
    match u32::try_from(sig) {
        Ok(n) if n <= zoraya_abi::posix::signal::MAX => {
            poll();
            unit(raise_signal(n))
        }
        _ => int(Err(EINVAL)),
    }
}

/// `SIGABRT` (ADR-0106 item 8): a handler runs; if it returns, the default.
#[unsafe(no_mangle)]
extern "C" fn abort() -> ! {
    let _ = with(|p| {
        p.signals.blocked &= !bit(SIGABRT);
    });
    let _ = raise_signal(SIGABRT);
    zoraya_rt::process_exit_terminated(SIGABRT)
}

// ------------------------------------------------------------ groups

#[unsafe(no_mangle)]
extern "C" fn setpgid(pid: i32, pgid: i32) -> i32 {
    if pid < 0 || pgid < 0 {
        return int(Err(EINVAL));
    }
    let m = bp::SetPgid {
        pid: pid as u32,
        pgid: pgid as u32,
    };
    unit(state(|p| {
        p.broker_simple(&m, &[])?;
        if pid == 0 || pid as u32 == p.facts.pid {
            // The broker refuses any change: the group is the one it had.
            let own = p.facts.pid;
            p.facts.pgid = if pgid == 0 { own } else { pgid as u32 };
        }
        Ok(())
    }))
}

fn lookup(what: u32, pid: i32) -> Result<i32, i32> {
    let m = bp::Lookup {
        what: Below(what),
        pid: u32::try_from(pid).map_err(|_| ESRCH)?,
    };
    state(|p| {
        let n = p.broker_call(&m, &[])?;
        let (_, r) = decode::<bp::LookupReply>(&p.input[..n], 0).map_err(|_| EINVAL)?;
        match crate::jobs::broker_errno(r.status.0) {
            0 => Ok(r.value as i32),
            e => Err(e),
        }
    })
}

#[unsafe(no_mangle)]
extern "C" fn getpgid(pid: i32) -> i32 {
    if pid == 0 {
        return with(|p| p.facts.pgid as i32).unwrap_or(0);
    }
    int(lookup(bp::lookup::PGID, pid))
}

#[unsafe(no_mangle)]
extern "C" fn getsid(pid: i32) -> i32 {
    if pid == 0 {
        return with(|p| p.facts.sid as i32).unwrap_or(0);
    }
    int(lookup(bp::lookup::SID, pid))
}

#[unsafe(no_mangle)]
extern "C" fn zoraya_fork_group(pgid: i32) -> i32 {
    if pgid < -1 {
        return int(Err(EINVAL));
    }
    with(|p| p.fork_group = pgid);
    0
}

// ------------------------------------------------------------ the terminal

#[unsafe(no_mangle)]
extern "C" fn tcsetpgrp(fd: i32, pgid: i32) -> i32 {
    if pgid < 0 {
        return int(Err(EINVAL));
    }
    unit(state(|p| {
        p.tty_of(fd)?;
        p.broker_simple(&bp::Tcsetpgrp { pgid: pgid as u32 }, &[])
    }))
}

#[unsafe(no_mangle)]
extern "C" fn tcgetpgrp(fd: i32) -> i32 {
    int(state(|p| {
        p.tty_of(fd)?;
        let n = p.broker_call(&bp::Tcgetpgrp {}, &[])?;
        let (_, r) = decode::<bp::TcgetpgrpReply>(&p.input[..n], 0).map_err(|_| EINVAL)?;
        match crate::jobs::broker_errno(r.status.0) {
            0 => Ok(r.pgid as i32),
            e => Err(e),
        }
    }))
}

/// musl's `struct termios`: four flag words, `c_line`, `c_cc[32]`, the two
/// speeds.
const TERMIOS_SIZE: usize = 60;

fn get_attr(fd: i32) -> Result<[u8; TERMIOS_SIZE], i32> {
    state(|p| {
        let conn = p.tty_of(fd)?;
        let n = p.tty_call(conn, &tp::GetAttr {})?;
        let (_, a) = decode::<tp::Attr<'_>>(&p.input[..n], 0).map_err(|_| EIO)?;
        let mut t = [0u8; TERMIOS_SIZE];
        for (i, w) in [a.iflag, a.oflag, a.cflag, a.lflag].iter().enumerate() {
            t[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        let cc = a.cc.0.len().min(32);
        t[17..17 + cc].copy_from_slice(&a.cc.0[..cc]);
        // B38400 in and out.
        t[52..56].copy_from_slice(&15u32.to_le_bytes());
        t[56..60].copy_from_slice(&15u32.to_le_bytes());
        Ok(t)
    })
}

fn set_attr(fd: i32, t: &[u8; TERMIOS_SIZE]) -> Result<(), i32> {
    let word = |i: usize| u32::from_le_bytes([t[i * 4], t[i * 4 + 1], t[i * 4 + 2], t[i * 4 + 3]]);
    state(|p| {
        let conn = p.tty_of(fd)?;
        let m = tp::SetAttr {
            iflag: word(0),
            oflag: word(1),
            cflag: word(2),
            lflag: word(3),
            cc: Data(&t[17..49]),
        };
        let n = p.tty_call(conn, &m)?;
        if crate::process::reply_ok(&p.input[..n]) {
            Ok(())
        } else {
            Err(EINVAL)
        }
    })
}

#[unsafe(no_mangle)]
extern "C" fn tcgetattr(fd: i32, t: *mut u8) -> i32 {
    if t.is_null() {
        return int(Err(EFAULT));
    }
    // SAFETY: C's contract: `t` is a `struct termios`.
    unit(get_attr(fd).and_then(|a| unsafe { put(t, &a) }))
}

#[unsafe(no_mangle)]
extern "C" fn tcsetattr(fd: i32, _when: i32, t: *const u8) -> i32 {
    if t.is_null() {
        return int(Err(EFAULT));
    }
    let mut a = [0u8; TERMIOS_SIZE];
    // SAFETY: C's contract: `t` is a `struct termios`.
    unsafe { ptr::copy_nonoverlapping(t, a.as_mut_ptr(), TERMIOS_SIZE) };
    unit(set_attr(fd, &a))
}

const TCGETS: i64 = 0x5401;
const TCSETS: i64 = 0x5402;
const TCSETSW: i64 = 0x5403;
const TCSETSF: i64 = 0x5404;
const TIOCGPGRP: i64 = 0x540f;
const TIOCSPGRP: i64 = 0x5410;
const TIOCGWINSZ: i64 = 0x5413;

/// `ioctl` on a terminal (ADR-0105 item 7); `ENOTTY` on anything else.
pub(super) fn ioctl_tty(fd: i32, request: i64, argument: i64) -> Result<i32, i32> {
    let arg = argument as *mut u8;
    match request {
        TIOCGWINSZ => {
            let (rows, columns) = state(|p| {
                let conn = p.tty_of(fd)?;
                let n = p.tty_call(conn, &tp::WindowSize {})?;
                let (_, r) = decode::<tp::WindowSizeReply>(&p.input[..n], 0).map_err(|_| EIO)?;
                Ok((r.rows, r.columns))
            })?;
            let mut w = [0u8; 8];
            w[..2].copy_from_slice(&(rows as u16).to_le_bytes());
            w[2..4].copy_from_slice(&(columns as u16).to_le_bytes());
            // SAFETY: C's contract: a `struct winsize`.
            unsafe { put(arg, &w) }?;
            Ok(0)
        }
        TCGETS => {
            let a = get_attr(fd)?;
            // SAFETY: C's contract: a `struct termios`.
            unsafe { put(arg, &a) }?;
            Ok(0)
        }
        TCSETS | TCSETSW | TCSETSF => {
            if arg.is_null() {
                return Err(EFAULT);
            }
            let mut a = [0u8; TERMIOS_SIZE];
            // SAFETY: C's contract: a `struct termios`.
            unsafe { ptr::copy_nonoverlapping(arg, a.as_mut_ptr(), TERMIOS_SIZE) };
            set_attr(fd, &a).map(|()| 0)
        }
        TIOCGPGRP => {
            let g = tcgetpgrp(fd);
            if g < 0 {
                // SAFETY: musl's errno.
                return Err(unsafe { *__errno_location() });
            }
            // SAFETY: C's contract: a `pid_t`.
            unsafe { put(arg, &g.to_le_bytes()) }?;
            Ok(0)
        }
        TIOCSPGRP => {
            if arg.is_null() {
                return Err(EFAULT);
            }
            // SAFETY: C's contract: a `pid_t`.
            let g = unsafe { (arg as *const i32).read_unaligned() };
            match tcsetpgrp(fd, g) {
                // SAFETY: musl's errno.
                -1 => Err(unsafe { *__errno_location() }),
                _ => Ok(0),
            }
        }
        _ => state(|p| p.tty_of(fd).and(Err(ENOTTY))),
    }
}

// ------------------------------------------------------------ images

/// The longest `#!` line read (Linux's `BINPRM_BUF_SIZE`).
const SHEBANG: usize = 256;

/// What an exec'd path is.
enum Program {
    Elf(Handle),
    /// `#!`: the interpreter and its one optional argument.
    Script(Vec<u8>, Option<Vec<u8>>),
}

/// Loads `path` into an image (ADR-0108): read through the library into a
/// memory object, then `image_create`. A `#!` line is a script; anything
/// else that is not ELF is `ENOEXEC`.
fn load(p: &mut Posix<RtSys>, path: &[u8]) -> Result<Program, i32> {
    p.access_at(process::AT_FDCWD, path, process::X_OK, 0)?;
    let fd = p.open(process::AT_FDCWD, path, o::RDONLY | o::CLOEXEC, 0)?;
    let r = load_fd(p, fd);
    let _ = p.close(fd);
    r
}

fn load_fd(p: &mut Posix<RtSys>, fd: i32) -> Result<Program, i32> {
    let size = p.lseek(fd, 0, 2)? as u64;
    p.lseek(fd, 0, 0)?;
    let mut head = [0u8; SHEBANG];
    let got = p.read(fd, User::new(head.as_mut_ptr() as u64, SHEBANG))?;
    let head = &head[..got];
    if head.starts_with(b"#!") {
        let line = head[2..].split(|&b| b == b'\n').next().unwrap_or(&[]);
        if got == SHEBANG && !head[2..].contains(&b'\n') {
            return Err(ENOEXEC);
        }
        let mut words = line
            .split(|&b| b == b' ' || b == b'\t')
            .filter(|w| !w.is_empty());
        let interpreter = words.next().ok_or(ENOEXEC)?.to_vec();
        // Linux gives the rest of the line as one argument.
        let rest_at = line
            .windows(interpreter.len())
            .position(|w| w == &interpreter[..])
            .map_or(line.len(), |i| i + interpreter.len());
        let rest: &[u8] = line[rest_at..].trim_ascii();
        let arg = (!rest.is_empty()).then(|| rest.to_vec());
        return Ok(Program::Script(interpreter, arg));
    }
    if !head.starts_with(b"\x7fELF") || size == 0 {
        return Err(ENOEXEC);
    }
    // Whole pages for the object; the image is the file's length.
    let pages = size.next_multiple_of(PAGE_SIZE as u64);
    let object = zoraya_rt::memory_create(pages, MemoryFlags::NONE).map_err(|s| {
        debug_later(p, "load: memory_create", (s, pages));
        ENOMEM
    })?;
    let address = match zoraya_rt::memory_map(object, 0, pages, MapFlags::WRITE) {
        Ok(a) => a,
        Err(s) => {
            debug_later(p, "load: memory_map", (s, pages));
            let _ = zoraya_rt::handle_close(object);
            return Err(ENOMEM);
        }
    };
    let filled = (|| {
        p.lseek(fd, 0, 0)?;
        let mut done = 0u64;
        while done < size {
            let n = p.read(fd, User::new(address + done, (size - done) as usize))?;
            if n == 0 {
                return Err(EIO);
            }
            done += n as u64;
        }
        Ok(())
    })();
    let _ = zoraya_rt::memory_unmap(address, pages);
    let image = filled.and_then(|()| {
        zoraya_rt::image_create(object, size).map_err(|s| {
            debug_later(p, "load: image_create", s);
            ENOEXEC
        })
    });
    let _ = zoraya_rt::handle_close(object);
    image.map(Program::Elf)
}

/// The image for `path` and the vector it runs with: a script's interpreter
/// first, then its argument, the script's path and the rest (ADR-0108,
/// Linux's rule; one level).
/// An image, the vector it runs with, and the path it was found at.
type Resolved = (Handle, Vec<Vec<u8>>, Vec<u8>);

fn resolve_program(p: &mut Posix<RtSys>, path: &[u8], argv: &[Vec<u8>]) -> Result<Resolved, i32> {
    match load(p, path)? {
        Program::Elf(image) => Ok((image, argv.to_vec(), path.to_vec())),
        Program::Script(interpreter, arg) => match load(p, &interpreter)? {
            Program::Elf(image) => {
                let mut v = vec![interpreter.clone()];
                v.extend(arg);
                v.push(path.to_vec());
                v.extend(argv.iter().skip(1).cloned());
                Ok((image, v, interpreter))
            }
            Program::Script(..) => Err(ENOEXEC),
        },
    }
}

// ------------------------------------------------------------ spawn

/// The C strings of a NULL-terminated vector.
///
/// # Safety
///
/// `v` is NULL or a NULL-terminated array of C strings.
unsafe fn strings(v: *const *const c_char) -> Result<Vec<Vec<u8>>, i32> {
    let mut out = Vec::new();
    if v.is_null() {
        return Ok(out);
    }
    for i in 0.. {
        // SAFETY: the caller's contract.
        let s = unsafe { v.add(i).read() };
        if s.is_null() {
            break;
        }
        let mut n = 0;
        // SAFETY: a C string.
        while unsafe { *s.add(n) } != 0 {
            n += 1;
        }
        // SAFETY: the `n` bytes before the NUL.
        out.push(unsafe { core::slice::from_raw_parts(s.cast::<u8>(), n) }.to_vec());
        if out.len() > 65536 {
            return Err(E2BIG);
        }
    }
    Ok(out)
}

/// How a child starts (from `posix_spawn`'s attributes or `exec`).
struct Start<'a> {
    /// `zoraya_proto_posix::group::*` and the pgid for `JOIN`.
    group: (u32, u32),
    /// Signals whose action becomes the default (`SETSIGDEF`).
    defaults: u64,
    /// The child's mask, if `SETSIGMASK`.
    mask: Option<u64>,
    /// `POSIX_SPAWN_TCSETPGRP` (ADR-0142).
    foreground: bool,
    actions: &'a [FileAction],
}

/// The child's ignored set (ADR-0104 item 6): what is `SIG_IGN` now and not
/// reset by `defaults`.
fn ignored_for(p: &Posix<RtSys>, defaults: u64) -> u32 {
    let mut ignored = 0u32;
    for n in 1..=zoraya_abi::posix::signal::MAX {
        if p.signals.action(n).handler == SIG_IGN && defaults & bit(n) == 0 {
            ignored |= 1 << n;
        }
    }
    ignored
}

/// The dispositions the new image starts with (ADR-0106 item 1, ADR-0104
/// item 6): ignored stays `Ignore`; every other signal as its default.
fn start_dispositions(e: &mut SpawnEncoder<'_>, ignored: u32) {
    for n in 1..=zoraya_abi::posix::signal::MAX {
        let Some(kind) = zoraya_abi::posix::signal::kind(n) else {
            continue;
        };
        let d = if ignored & (1 << n) != 0 || !zoraya_abi::posix::signal::default_terminates(n) {
            Disposition::Ignore
        } else {
            Disposition::Terminate
        };
        e.disposition(kind, d);
    }
}

/// The record and the handles of its message for a child (ADR-0107).
struct Record {
    bytes: Vec<u8>,
    handles: Vec<Handle>,
}

fn child_record(
    p: &mut Posix<RtSys>,
    facts: &Facts,
    exe: &[u8],
    inherited: &crate::jobs::Inherit<Handle>,
    image: Handle,
) -> Result<Record, i32> {
    let mut handles = inherited.message.clone();
    let mut entries = Vec::new();
    for &(i, kind, index) in &inherited.entries {
        let mut index = index;
        if kind == EntryKind::Image
            && let Ok(dup) = zoraya_rt::handle_duplicate(image, rights_of(image))
        {
            handles.push(dup);
            index = (handles.len() - 1) as u8;
        }
        entries.push(abi::Entry {
            prefix: &p.entries[i].prefix,
            kind,
            handle: index,
        });
    }
    let exe = core::str::from_utf8(exe).unwrap_or("");
    let flags = if p.form == crate::path::Form::Absolute {
        abi::ABSOLUTE
    } else {
        0
    };
    let mut bytes = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
    let n = abi::encode(
        facts,
        flags,
        exe,
        p.cwd.entry as u32,
        &p.cwd.path,
        &inherited.fds,
        &entries,
        handles.len(),
        &mut bytes,
    );
    match n {
        Some(n) => {
            bytes.truncate(n);
            Ok(Record { bytes, handles })
        }
        None => {
            for h in handles.drain(inherited.message.len()..) {
                let _ = zoraya_rt::handle_close(h);
            }
            Err(E2BIG)
        }
    }
}

/// Writes the record, then `SELF`, on the bootstrap channel (ADR-0107 item
/// 5), moving every handle.
fn send_record(boot: Handle, record: Record, process: Handle) -> Result<(), i32> {
    let entries: Vec<ChannelEntry> = record
        .handles
        .iter()
        .map(|&h| ChannelEntry {
            handle: h,
            rights: rights_of(h),
            transfer: Transfer::Move,
        })
        .collect();
    let r = zoraya_rt::channel_write(boot, &record.bytes, &entries);
    if r.is_err() {
        for h in record.handles {
            let _ = zoraya_rt::handle_close(h);
        }
        return Err(E2BIG);
    }
    // TRANSFER to move it; the child receives WAIT alone.
    let me = zoraya_rt::handle_duplicate(process, Rights::WAIT.union(Rights::TRANSFER))
        .map_err(|_| EAGAIN)?;
    let entry = [ChannelEntry {
        handle: me,
        rights: Rights::WAIT,
        transfer: Transfer::Move,
    }];
    zoraya_rt::channel_write(boot, SELF_MESSAGE, &entry).map_err(|_| EAGAIN)
}

/// Spawns `path` with `argv` and `envp` into `job` (a new pid's, or this
/// process's own for `exec`): the image, the inheritance, the record. The
/// child's process handle, or the error with everything made undone.
#[allow(clippy::too_many_arguments)] // Reason: one per part of a spawn.
fn spawn_into(
    p: &mut Posix<RtSys>,
    job: Handle,
    broker_end: Handle,
    facts: &Facts,
    image: Handle,
    argv: &[Vec<u8>],
    envp: &[Vec<u8>],
    exe: &[u8],
    start: &Start<'_>,
    suspended: bool,
) -> Result<Handle, i32> {
    let inherited = p.inherit(start.actions, facts.pgid).inspect_err(|e| {
        let at = p.stats.inherit_failed_at;
        debug_later(p, "spawn: inherit", (e, at));
    })?;
    let made: Vec<Handle> = inherited
        .slots
        .iter()
        .filter(|s| s.2)
        .map(|s| s.1)
        .collect();
    let undo_made = |made: &[Handle], message: &[Handle]| {
        for &h in made.iter().chain(message) {
            let _ = zoraya_rt::handle_close(h);
        }
    };
    let record = match child_record(p, facts, exe, &inherited, image) {
        Ok(r) => r,
        Err(e) => {
            undo_made(&made, &inherited.message);
            return Err(e);
        }
    };
    let (boot, boot_child) = match zoraya_rt::channel_create() {
        Ok(c) => c,
        Err(_) => {
            undo_made(&made, &record.handles);
            return Err(EAGAIN);
        }
    };
    let mut buffer = vec![0u8; zoraya_abi::spawn::MAX_SPAWN_SIZE];
    let mut e = SpawnEncoder::new(&mut buffer, job, image);
    if suspended {
        e.suspended();
    }
    start_dispositions(&mut e, facts.ignored);
    for &(fd, h, moved) in &inherited.slots {
        e.handle(HandleEntry {
            slot: slot::of_fd(fd),
            handle: h,
            rights: rights_of(h),
            transfer: if moved {
                Transfer::Move
            } else {
                Transfer::Duplicate
            },
        });
    }
    for (slot_no, h, rights) in [
        (slot::BOOTSTRAP.slot(), boot_child, rights_of(boot_child)),
        (slot::BROKER.slot(), broker_end, rights_of(broker_end)),
        // A process job handle holds what the broker gave it, no more.
        (slot::PROCESS_JOB.slot(), job, abi::job_rights::OWN),
    ] {
        e.handle(HandleEntry {
            slot: slot_no,
            handle: h,
            rights,
            transfer: Transfer::Move,
        });
    }
    for a in argv {
        e.arg(a);
    }
    for v in envp {
        e.env(v);
    }
    let Some(length) = e.finish() else {
        undo_made(&made, &record.handles);
        let _ = zoraya_rt::handle_close(boot);
        let _ = zoraya_rt::handle_close(boot_child);
        return Err(E2BIG);
    };
    p.stats.max_spawn_buffer = p.stats.max_spawn_buffer.max(length as u64);
    let process = match zoraya_rt::spawn(&buffer[..length]) {
        Ok(h) => h,
        Err(s) => {
            // The field first, before the calls below can be refused too.
            let seen = zoraya_rt::last_refusal();
            debug_later(p, "spawn", s);
            // A refused spawn moved nothing (INV-0042-3).
            undo_made(&made, &record.handles);
            let _ = zoraya_rt::handle_close(boot);
            let _ = zoraya_rt::handle_close(boot_child);
            return Err(match s {
                Status::NoMemory | Status::OverQuota => {
                    super::rt_errno_seen(s, Call::Process, seen)
                }
                Status::LimitReached => EAGAIN,
                _ => E2BIG,
            });
        }
    };
    let sent = send_record(boot, record, process);
    let _ = zoraya_rt::handle_close(boot);
    if let Err(e) = sent {
        // The child finds its bootstrap channel closed with no record, and
        // ends (`bootstrap`).
        let _ = zoraya_rt::handle_close(process);
        return Err(e);
    }
    Ok(process)
}

/// `posix_spawn` (ADR-0104 items 4 and 5, ADR-0142).
fn spawn_child(
    p: &mut Posix<RtSys>,
    path: &[u8],
    argv: &[Vec<u8>],
    envp: &[Vec<u8>],
    start: &Start<'_>,
) -> Result<u32, i32> {
    let (image, argv, exe) =
        resolve_program(p, path, argv).inspect_err(|e| debug_later(p, "posix_spawn: load", e))?;
    let ignored = ignored_for(p, start.defaults);
    let stops = ignored & ((1 << SIGTSTP) | (1 << SIGTTIN) | (1 << SIGTTOU) | (1 << SIGCONT));
    let prepared = match p.prepare(start.group.0, start.group.1, stops, false) {
        Ok(x) => x,
        Err(e) => {
            let _ = zoraya_rt::handle_close(image);
            return Err(e);
        }
    };
    let facts = Facts {
        pid: prepared.pid,
        ppid: p.facts.pid,
        pgid: prepared.pgid,
        sid: prepared.sid,
        ignored,
        // `SETSIGMASK`'s, else the parent's (ADR-0190 item 18).
        blocked: crate::signals::to_record(start.mask.unwrap_or(p.signals.blocked)),
        ..p.facts
    };
    let child_job = prepared.child_job;
    let r = spawn_into(
        p,
        child_job,
        prepared.broker_end,
        &facts,
        image,
        &argv,
        envp,
        &exe,
        start,
        start.foreground,
    );
    let _ = zoraya_rt::handle_close(image);
    let process = match r {
        Ok(h) => h,
        Err(e) => {
            // Nothing moved: the job and the end are still ours to give back.
            let _ = zoraya_rt::handle_close(child_job);
            let _ = zoraya_rt::handle_close(prepared.broker_end);
            let _ = zoraya_rt::handle_close(prepared.parent_job);
            let _ = p.broker_simple(&bp::SpawnAbort { pid: prepared.pid }, &[]);
            return Err(e);
        }
    };
    if let Err(e) = commit(p, &prepared, process) {
        // The child runs with no record of the broker's: let it run rather
        // than stay stopped, and give its pid back (review R6).
        debug_later(p, "posix_spawn: commit", e);
        if start.foreground {
            let _ = zoraya_rt::process_resume(process);
        }
        let _ = zoraya_rt::handle_close(process);
        let _ = zoraya_rt::handle_close(prepared.parent_job);
        let _ = p.broker_simple(&bp::SpawnAbort { pid: prepared.pid }, &[]);
        return Err(e);
    }
    if start.foreground {
        // The child cannot read the terminal before its group is the
        // foreground (ADR-0104 item 5).
        let _ = p.broker_simple(
            &bp::Tcsetpgrp {
                pgid: prepared.pgid,
            },
            &[],
        );
        let _ = zoraya_rt::process_resume(process);
    }
    let _ = zoraya_rt::handle_close(process);
    let _ = start.mask;
    Ok(prepared.pid)
}

/// `spawn_commit`, and the child in the list.
fn commit(p: &mut Posix<RtSys>, prepared: &Prepared<Handle>, process: Handle) -> Result<(), i32> {
    // At the type's rights: a moved handle lists what it holds, and the
    // library's channel writes list a type's (no call reads a handle's).
    let dup = zoraya_rt::handle_duplicate(process, rights_of(process)).map_err(|_| EAGAIN)?;
    let start = zoraya_rt::clock_monotonic();
    p.broker_simple(&bp::SpawnCommit { pid: prepared.pid }, &[dup])?;
    p.stats.commits += 1;
    p.stats.commit_ns += zoraya_rt::clock_monotonic().saturating_sub(start);
    p.children.push(crate::jobs::Child {
        pid: prepared.pid,
        pgid: prepared.pgid,
        job: prepared.parent_job,
        stop_reported: false,
        notified: false,
    });
    Ok(())
}

// musl's `posix_spawnattr_t`: flags, pgroup, the default set, the mask.
const ATTR_FLAGS: usize = 0;
const ATTR_PGROUP: usize = 4;
const ATTR_DEF: usize = 8;
const ATTR_MASK: usize = 136;
const ATTR_SIZE: usize = 336;

const POSIX_SPAWN_SETPGROUP: i32 = 2;
const POSIX_SPAWN_SETSIGDEF: i32 = 4;
const POSIX_SPAWN_SETSIGMASK: i32 = 8;
/// ADR-0142: below the sign bit of the `short` `setflags` takes.
const POSIX_SPAWN_TCSETPGRP: i32 = 0x4000;
const FLAGS_KNOWN: i32 = 0xff | POSIX_SPAWN_TCSETPGRP;

#[unsafe(no_mangle)]
extern "C" fn posix_spawnattr_init(a: *mut u8) -> i32 {
    if a.is_null() {
        return EINVAL;
    }
    // SAFETY: C's contract: `a` is a `posix_spawnattr_t`.
    unsafe { ptr::write_bytes(a, 0, ATTR_SIZE) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawnattr_destroy(_a: *mut u8) -> i32 {
    0
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawnattr_setflags(a: *mut u8, flags: i16) -> i32 {
    let flags = i32::from(flags);
    if a.is_null() || flags & !FLAGS_KNOWN != 0 {
        return EINVAL;
    }
    // SAFETY: C's contract.
    unsafe { (a.add(ATTR_FLAGS) as *mut i32).write_unaligned(flags) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawnattr_setpgroup(a: *mut u8, pgroup: i32) -> i32 {
    if a.is_null() {
        return EINVAL;
    }
    // SAFETY: C's contract.
    unsafe { (a.add(ATTR_PGROUP) as *mut i32).write_unaligned(pgroup) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawnattr_setsigdefault(a: *mut u8, set: *const u64) -> i32 {
    if a.is_null() || set.is_null() {
        return EINVAL;
    }
    // SAFETY: C's contract.
    unsafe { (a.add(ATTR_DEF) as *mut u64).write_unaligned(set.read_unaligned()) };
    0
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawnattr_setsigmask(a: *mut u8, set: *const u64) -> i32 {
    if a.is_null() || set.is_null() {
        return EINVAL;
    }
    // SAFETY: C's contract.
    unsafe { (a.add(ATTR_MASK) as *mut u64).write_unaligned(set.read_unaligned()) };
    0
}

/// musl's `posix_spawn_file_actions_t` keeps a pointer at offset 8; here it
/// is the library's own list of actions.
const ACTIONS_AT: usize = 8;
const ACTIONS_SIZE: usize = 80;

/// # Safety
///
/// `fa` is a `posix_spawn_file_actions_t` that `init` set up.
unsafe fn actions_of<'a>(fa: *mut u8) -> Option<&'a mut Vec<FileAction>> {
    // SAFETY: the caller's contract; the pointer is a leaked `Box` or NULL.
    let list = unsafe { (fa.add(ACTIONS_AT) as *mut *mut Vec<FileAction>).read_unaligned() };
    // SAFETY: as above.
    (!list.is_null()).then(|| unsafe { &mut *list })
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawn_file_actions_init(fa: *mut u8) -> i32 {
    if fa.is_null() {
        return EINVAL;
    }
    let list: *mut Vec<FileAction> = Box::into_raw(Box::new(Vec::new()));
    // SAFETY: C's contract: `fa` is a `posix_spawn_file_actions_t`.
    unsafe {
        ptr::write_bytes(fa, 0, ACTIONS_SIZE);
        (fa.add(ACTIONS_AT) as *mut *mut Vec<FileAction>).write_unaligned(list);
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawn_file_actions_destroy(fa: *mut u8) -> i32 {
    if fa.is_null() {
        return EINVAL;
    }
    // SAFETY: C's contract: set up by `init`.
    let list = unsafe { (fa.add(ACTIONS_AT) as *mut *mut Vec<FileAction>).read_unaligned() };
    if !list.is_null() {
        // SAFETY: the `Box` `init` leaked, given back once.
        drop(unsafe { Box::from_raw(list) });
        // SAFETY: as above.
        unsafe {
            (fa.add(ACTIONS_AT) as *mut *mut Vec<FileAction>).write_unaligned(ptr::null_mut())
        };
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawn_file_actions_adddup2(fa: *mut u8, from: i32, to: i32) -> i32 {
    if fa.is_null() || from < 0 || to < 0 {
        return EBADF;
    }
    // SAFETY: C's contract.
    match unsafe { actions_of(fa) } {
        Some(l) => {
            l.push(FileAction::Dup2(from, to));
            0
        }
        None => EINVAL,
    }
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawn_file_actions_addclose(fa: *mut u8, fd: i32) -> i32 {
    if fa.is_null() || fd < 0 {
        return EBADF;
    }
    // SAFETY: C's contract.
    match unsafe { actions_of(fa) } {
        Some(l) => {
            l.push(FileAction::Close(fd));
            0
        }
        None => EINVAL,
    }
}

#[unsafe(no_mangle)]
extern "C" fn posix_spawn(
    pid: *mut i32,
    path: *const c_char,
    fa: *mut u8,
    attr: *const u8,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> i32 {
    // SAFETY: C's contract for posix_spawn.
    let path = match unsafe { c_str(path) } {
        Ok(p) => p.to_vec(),
        Err(e) => return e,
    };
    // SAFETY: as above.
    let (argv, envp) = match unsafe { (strings(argv), strings(envp)) } {
        (Ok(a), Ok(e)) => (a, e),
        _ => return E2BIG,
    };
    let actions = if fa.is_null() {
        Vec::new()
    } else {
        // SAFETY: C's contract.
        unsafe { actions_of(fa) }.cloned().unwrap_or_default()
    };
    let (flags, pgroup, defaults, mask) = if attr.is_null() {
        (0, 0, 0, 0)
    } else {
        // SAFETY: C's contract: a `posix_spawnattr_t`.
        unsafe {
            (
                (attr.add(ATTR_FLAGS) as *const i32).read_unaligned(),
                (attr.add(ATTR_PGROUP) as *const i32).read_unaligned(),
                (attr.add(ATTR_DEF) as *const u64).read_unaligned(),
                (attr.add(ATTR_MASK) as *const u64).read_unaligned(),
            )
        }
    };
    let group = if flags & POSIX_SPAWN_SETPGROUP == 0 {
        (bp::group::INHERIT, 0)
    } else if pgroup == 0 {
        (bp::group::NEW, 0)
    } else {
        (bp::group::JOIN, pgroup as u32)
    };
    let start = Start {
        group,
        defaults: if flags & POSIX_SPAWN_SETSIGDEF != 0 {
            defaults
        } else {
            0
        },
        mask: (flags & POSIX_SPAWN_SETSIGMASK != 0).then_some(mask),
        foreground: flags & POSIX_SPAWN_TCSETPGRP != 0 && flags & POSIX_SPAWN_SETPGROUP != 0,
        actions: &actions,
    };
    poll();
    let started = zoraya_rt::clock_monotonic();
    match state(|p| {
        let r = spawn_child(p, &path, &argv, &envp, &start);
        if r.is_ok() {
            p.stats.spawns += 1;
            p.stats.spawn_ns += zoraya_rt::clock_monotonic().saturating_sub(started);
        }
        r
    }) {
        Ok(child) => {
            if !pid.is_null() {
                // SAFETY: C's contract: `pid` is NULL or a `pid_t`.
                unsafe { pid.write(child as i32) };
            }
            0
        }
        Err(e) => e,
    }
}

// ------------------------------------------------------------ exec

/// `execve` (ADR-0104 item 6, ADR-0108): the new image into this process's
/// own job, the broker told, and this image ends. Returns only on failure.
fn exec(path: &[u8], argv: &[Vec<u8>], envp: &[Vec<u8>]) -> i32 {
    poll();
    let started = zoraya_rt::clock_monotonic();
    let r = state(|p| {
        let job = p.process_job.ok_or(ENOSYS)?;
        let (image, argv, exe) = resolve_program(p, path, argv).inspect_err(|e| {
            debug_later(p, "exec: load", e);
        })?;
        let (server, client) = match zoraya_rt::channel_create() {
            Ok(c) => c,
            Err(_) => {
                let _ = zoraya_rt::handle_close(image);
                return Err(EAGAIN);
            }
        };
        // POSIX keeps the mask and every `SIG_IGN` across `execve`
        // (ADR-0190 item 18).
        let facts = Facts {
            ignored: ignored_for(p, 0),
            blocked: crate::signals::to_record(p.signals.blocked),
            ..p.facts
        };
        let start = Start {
            group: (bp::group::INHERIT, 0),
            defaults: 0,
            mask: None,
            foreground: false,
            actions: &[],
        };
        // The job handle moves to the new image: this one is ending.
        let r = spawn_into(
            p, job, client, &facts, image, &argv, envp, &exe, &start, false,
        );
        let _ = zoraya_rt::handle_close(image);
        let process = match r {
            Ok(h) => h,
            Err(e) => {
                let _ = zoraya_rt::handle_close(server);
                let _ = zoraya_rt::handle_close(client);
                return Err(e);
            }
        };
        // The new image runs in this pid's job: from here this image ends
        // whatever happens (review R5); a failed commit leaves the broker's
        // record on the old image, which then reads as its exit.
        p.process_job = None;
        // This image's report goes before the hand-over, while the broker
        // still answers its connection (ADR-0110 item 7).
        p.stats.execs += 1;
        p.stats.exec_ns += zoraya_rt::clock_monotonic().saturating_sub(started);
        super::report_stats(p);
        let committed = match zoraya_rt::handle_duplicate(process, rights_of(process)) {
            Ok(dup) => p.broker_simple(&bp::ExecCommit {}, &[dup, server]),
            Err(_) => {
                let _ = zoraya_rt::handle_close(server);
                Err(EAGAIN)
            }
        };
        let _ = zoraya_rt::handle_close(process);
        if let Err(e) = committed {
            debug_later(p, "exec: commit", e);
            zoraya_rt::process_exit(127);
        }
        Ok(())
    });
    match r {
        // The new image runs: this one ends without atexit or stdio.
        Ok(()) => zoraya_rt::process_exit(0),
        Err(e) => e,
    }
}

#[unsafe(no_mangle)]
extern "C" fn execve(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> i32 {
    // SAFETY: C's contract for execve.
    let path = match unsafe { c_str(path) } {
        Ok(p) => p.to_vec(),
        Err(e) => return int(Err(e)),
    };
    // SAFETY: as above.
    let (argv, envp) = match unsafe { (strings(argv), strings(envp)) } {
        (Ok(a), Ok(e)) => (a, e),
        _ => return int(Err(E2BIG)),
    };
    let e = exec(&path, &argv, &envp);
    int(Err(e))
}

unsafe extern "C" {
    static mut __environ: *mut *mut c_char;
}

/// `execvp`: `$PATH` walked as POSIX says, `ENOEXEC` run by `/bin/sh`.
#[unsafe(no_mangle)]
extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> i32 {
    // SAFETY: C's contract for execvp.
    let file = match unsafe { c_str(file) } {
        Ok(f) => f.to_vec(),
        Err(e) => return int(Err(e)),
    };
    // SAFETY: as above, and musl's `__environ`.
    let (argv, envp) = match unsafe { (strings(argv), strings(__environ as *const *const c_char)) }
    {
        (Ok(a), Ok(e)) => (a, e),
        _ => return int(Err(E2BIG)),
    };
    if file.is_empty() {
        return int(Err(ENOENT));
    }
    if file.contains(&b'/') {
        let e = exec(&file, &argv, &envp);
        return int(Err(e));
    }
    let path = envp
        .iter()
        .find_map(|v| v.strip_prefix(b"PATH="))
        .map_or(b"/bin:/usr/bin".to_vec(), <[u8]>::to_vec);
    let mut last = ENOENT;
    for dir in path.split(|&b| b == b':') {
        let mut full = if dir.is_empty() {
            b".".to_vec()
        } else {
            dir.to_vec()
        };
        full.push(b'/');
        full.extend_from_slice(&file);
        match exec(&full, &argv, &envp) {
            ENOEXEC => {
                let mut v = vec![b"sh".to_vec(), full.clone()];
                v.extend(argv.iter().skip(1).cloned());
                return int(Err(exec(b"/bin/sh", &v, &envp)));
            }
            ENOENT | ENOTDIR => {}
            e => last = e,
        }
    }
    int(Err(last))
}

/// `execl` without C variadics (a stable toolchain has none): on AAPCS64 a
/// variadic call passes its first eight arguments in `x0` to `x7`, as a
/// fixed one does, so the path, `arg0` and six more are read as parameters.
/// A longer list is `E2BIG`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // Reason: the eight argument registers.
extern "C" fn execl(
    path: *const c_char,
    a0: *const c_char,
    a1: *const c_char,
    a2: *const c_char,
    a3: *const c_char,
    a4: *const c_char,
    a5: *const c_char,
    a6: *const c_char,
) -> i32 {
    let all = [a0, a1, a2, a3, a4, a5, a6];
    let Some(end) = all.iter().position(|a| a.is_null()) else {
        return int(Err(E2BIG));
    };
    let mut v: Vec<*const c_char> = all[..end].to_vec();
    v.push(ptr::null());
    // SAFETY: musl's `__environ`, set before main.
    execve(path, v.as_ptr(), unsafe { __environ }
        as *const *const c_char)
}

// ------------------------------------------------------------ fork

/// With `ZORAYA_POSIX_DEBUG` in the environment, why a process call failed
/// goes to fd 2: C sees only an `errno`.
fn debug_later(p: &mut Posix<RtSys>, what: &str, detail: impl core::fmt::Debug) {
    let wanted = zoraya_rt::startup()
        .envp
        .any(|e| e.starts_with(b"ZORAYA_POSIX_DEBUG="));
    if wanted {
        use core::fmt::Write;
        let mut line = String::new();
        let _ = writeln!(line, "libposix: {what}: {detail:?}");
        let _ = p.write(2, User::new(line.as_ptr() as u64, line.len()));
    }
}

/// `fork` (ADR-0102's wrapper): the pid first, the move list, the snapshot;
/// the parent commits, the child sets its facts right.
#[unsafe(no_mangle)]
extern "C" fn fork() -> i32 {
    poll();
    let started = zoraya_rt::clock_monotonic();
    let r = state(|p| {
        let hint = core::mem::replace(&mut p.fork_group, -1);
        let (form, pgid) = match hint {
            -1 => (bp::group::INHERIT, 0),
            0 => (bp::group::NEW, 0),
            g => (bp::group::JOIN, g as u32),
        };
        let prepared = p
            .prepare(form, pgid, 0, true)
            .inspect_err(|e| debug_later(p, "fork: prepare", e))?;
        let moves = match p.fork_moves(prepared.pgid, &prepared) {
            Ok(m) => m,
            Err(e) => {
                debug_later(p, "fork: moves", e);
                let _ = zoraya_rt::handle_close(prepared.child_job);
                let _ = zoraya_rt::handle_close(prepared.broker_end);
                let _ = zoraya_rt::handle_close(prepared.parent_job);
                let _ = p.broker_simple(&bp::SpawnAbort { pid: prepared.pid }, &[]);
                return Err(e);
            }
        };
        let mut buffer = vec![0u8; zoraya_abi::snapshot::MAX_SNAPSHOT_SIZE];
        let mut e = zoraya_abi::snapshot::Encoder::new(&mut buffer, prepared.child_job);
        if let Some(me) = p.self_handle {
            e.self_slot(me.slot());
        }
        for &(slot_no, h) in &moves {
            e.moved(zoraya_abi::snapshot::Move {
                slot: slot_no,
                handle: h,
            });
        }
        let Some(length) = e.finish() else {
            for (_, h) in moves {
                let _ = zoraya_rt::handle_close(h);
            }
            let _ = zoraya_rt::handle_close(prepared.parent_job);
            let _ = p.broker_simple(&bp::SpawnAbort { pid: prepared.pid }, &[]);
            return Err(EAGAIN);
        };
        let before = zoraya_rt::clock_monotonic();
        let snap = zoraya_rt::snapshot_process(&buffer[..length]);
        let snapshot_ns = zoraya_rt::clock_monotonic().saturating_sub(before);
        match snap {
            Ok(Some(process)) => {
                // The parent: the moved handles are the child's now.
                if let Err(e) = commit(p, &prepared, process) {
                    debug_later(p, "fork: commit", e);
                    let _ = zoraya_rt::handle_close(process);
                    let _ = zoraya_rt::handle_close(prepared.parent_job);
                    let _ = p.broker_simple(&bp::SpawnAbort { pid: prepared.pid }, &[]);
                    return Err(e);
                }
                let _ = zoraya_rt::handle_close(process);
                p.stats.forks += 1;
                p.stats.snapshot_ns += snapshot_ns;
                p.stats.fork_ns += zoraya_rt::clock_monotonic().saturating_sub(started);
                Ok(prepared.pid as i32)
            }
            Ok(None) => {
                // The child, in a copy of the parent's memory (ADR-0102
                // item 5): its own facts, a port of its own.
                p.sys.port = None;
                p.after_fork_in_child(prepared.pid, prepared.pgid);
                super::restart_stats(p);
                Ok(0)
            }
            Err(s) => {
                // The field first: the calls below can be refused too, and
                // would overwrite it (ADR-0263 item 4).
                let seen = zoraya_rt::last_refusal();
                debug_later(p, "fork: snapshot_process", s);
                for (_, h) in moves {
                    let _ = zoraya_rt::handle_close(h);
                }
                let _ = zoraya_rt::handle_close(prepared.parent_job);
                let _ = p.broker_simple(&bp::SpawnAbort { pid: prepared.pid }, &[]);
                // ADR-0263 item 4: processes -> EAGAIN, memory -> ENOMEM.
                Err(match s {
                    Status::NoMemory | Status::OverQuota => {
                        super::rt_errno_seen(s, Call::Process, seen)
                    }
                    _ => EAGAIN,
                })
            }
        }
    });
    int(r)
}

#[unsafe(no_mangle)]
extern "C" fn vfork() -> i32 {
    fork()
}

// ------------------------------------------------------------ start-up

/// The fixed slots a spawned process holds (ADR-0107 item 5), and the
/// kernel dispositions of what the record ignores.
pub(super) fn start_up(p: &mut Posix<RtSys>) {
    let valid = |h: Handle| zoraya_rt::handle_type(h).ok().map(|_| h);
    p.broker = valid(slot::BROKER);
    p.process_job = valid(slot::PROCESS_JOB);
    if p.self_handle.is_none() {
        p.self_handle = valid(slot::SELF);
    }
    let _ = process_signals::REQUEST_PENDING;
    let _ = RequestKind::Interrupt;
    let _ = SIG_DFL;
    let _ = SIG_BLOCK;
    let _ = WNOHANG | WUNTRACED;
}
