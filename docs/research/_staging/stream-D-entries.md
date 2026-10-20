# Stream D entries

## 4. I/O, asynchrony and event handling

### DM-023: `select` → `poll` → `epoll` evolution; epoll's fd-vs-file-description semantics

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX (`select`, `poll`), Linux (`epoll`); contrast FreeBSD/macOS (`kqueue`), Solaris (`/dev/poll`, event ports)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `select` and `poll` are stateless: each call passes the whole interest set, the kernel scans it, and the result says which descriptors are ready. Linux `epoll` makes the interest set a kernel object. You add entries once with `epoll_ctl` and get ready events with `epoll_wait`. The key of an epoll registration is the pair of fd number and *open file description*, not the fd alone [S403].

**Why it was chosen.** `select` dates from early-1980s BSD, when a process had a few dozen descriptors and scanning a bitmap cost almost nothing. The stateless design adds no kernel state and is easy to reason about. When servers began holding thousands of idle connections, Banga, Mogul and Druschel measured `select` at nearly 25% of CPU in a proxy, even after optimising it. They proposed an explicit stateful interest-registration API [S400]. epoll (2002) took that direction. Tying registrations to the file object is cheap to implement, because the kernel hooks the wait queue of the underlying file.

**Why it is a problem.** The `select`/`poll` design was reasonable for its time: its cost grows with the number of watched descriptors, not with the number of events [S400][S401]. epoll's semantics have specific failure modes:
- Closing an fd does not remove the registration while another descriptor shares the description, whether from `dup`, `fork` or `SCM_RIGHTS`. The program then gets events for an fd number it has already closed, or that has been reused. After the close, `EPOLL_CTL_DEL` cannot remove the stale entry, and the only fix is to rebuild the epoll set [S403][S405].
- Libraries cannot defend against this, because their callers may close fds directly [S405].
- Epoll was not designed for multithreaded use. Level-triggered mode wakes every thread (thundering herd). Edge-triggered mode needs a drain-to-`EAGAIN` discipline, which can starve other descriptors or lose wakeups. Later fixes were bolted on: `EPOLLONESHOT`, and `EPOLLEXCLUSIVE` in Linux 4.5 [S403][S404].

**Why it was not fixed.** Linux's no-regressions rule freezes the semantics, so every fix arrived as an opt-in flag. The man page now just documents the hazards [S403].

**Alternatives that exist.** kqueue (FreeBSD 2000) applies registration changes and waits in a single call. It deletes knotes when the descriptor is closed, and it covers many event types besides fd readiness [S401][S402]. Lemon criticised the alternatives of the time. RT-signal queues overflow and deliver stale events for closed or reused fds. `/dev/poll` keeps returning `POLLNVAL` for closed fds and cannot itself be polled [S401]. Completion-based designs avoid readiness entirely; see DM-026.

**Cost of the alternatives.** Keying registrations to the descriptor means the close path must find and tear down registrations, which adds a cost to every `close` [S401]. Stateful interest sets of any kind bring kernel memory, accounting and lifetime questions that `poll` never had.

**Relevance to our OS.** high. If our handles are capabilities that can be duplicated or transferred, we must decide explicitly whether a wait registration belongs to the handle or to the object. That is the exact choice epoll got wrong.

**Sources.** [S400], [S401], [S402], [S403], [S404], [S405]

### DM-024: POSIX AIO and Linux native AIO limitations

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** POSIX (`aio_*`), glibc, Linux native AIO (`io_setup`/`io_submit`/`io_getevents`); contrast Windows overlapped I/O
- **Classification:** mistake
- **Confidence:** high

**What the design is.** POSIX defines `aio_read`/`aio_write` with signal or thread notification. glibc implements them in user space with a pool of threads [S408]. Linux added a separate kernel AIO interface for asynchronous submission. glibc never wrapped it, and programs reach it through `libaio`, whose wrappers have different prototypes [S409].

**Why it was chosen.** Kernel AIO was built for databases doing `O_DIRECT` I/O to raw block devices. For that narrow case, an asynchronous path straight to the block layer was what users needed. A user-space POSIX AIO built on threads is portable, and it needs no kernel changes.

**Why it is a problem.**
- Native AIO is asynchronous only for `O_DIRECT`. Buffered I/O runs synchronously, so most applications cannot use it [S410][S406].
- Even `O_DIRECT` submissions block when metadata must be read or the device's request slots are full. Applications therefore offload submission to threads anyway [S410].
- It supports few operations beyond read and write, and its `fsync` support is incomplete [S406].
- Each I/O copies about 104 bytes of descriptors and needs at least two syscalls [S410].
- glibc's threaded POSIX AIO "scales poorly" [S408].

The result was a niche interface with "long term undiscovered bugs" [S410]. Every event loop kept a private thread pool for file I/O instead (see DM-029). Torvalds called AIO "a horrible ad-hoc design" and refused to extend it one syscall at a time [S407].

**Why it was not fixed.** Extending AIO was tried several times: Axboe in 2010, then LaHaise's 2016 thread-based patches. Generic async-syscall proposals (fibrils, syslets, threadlets, acall) were never merged [S406][S410]. Axboe found that retrofitting AIO made the API "even more complicated" and fixed only one deficiency, so he started over with io_uring [S410]. The old ABI remains for compatibility.

**Alternatives that exist.** Windows NT designed overlapped I/O in from the start. File, socket and pipe handles all complete through the same mechanism, IOCP [S417]. Linux io_uring (5.1, 2019) supports buffered I/O and arbitrary operations through shared rings (DM-025).

**Cost of the alternatives.** Making every operation asynchronous means every kernel path must handle cancellation and continuation. Linux's history suggests this is either slow (punting to kernel threads) or invasive. Torvalds warned that async code has "subtle and nasty bugs" and races with fd close [S411]. io_uring's security record (DM-025) bears that out.

**Relevance to our OS.** high. Native async I/O has to be part of the design from day one and cover all operations. Bolting it onto a synchronous VFS produces exactly this outcome.

**Sources.** [S406], [S407], [S408], [S409], [S410], [S411], [S417]

### DM-025: `io_uring`: performance win vs security attack surface

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Linux (5.1+); downstream: ChromeOS, Android, Docker/containerd, GKE
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** io_uring is a pair of submission and completion rings shared through `mmap` between the application and the kernel. It supports pre-registered files and buffers, kernel-side polling of the submission queue, and a growing set of opcodes that has become a second syscall table [S410]. It was posted in January 2019 and merged in March 2019 for 5.1 [S411].

**Why it was chosen.** Native AIO could not be fixed (DM-024). Fast NVMe devices made syscall overhead and per-I/O copies dominant, and Spectre/Meltdown mitigations made syscalls more expensive. Shared rings batch submissions and can avoid syscalls entirely [S410].

**Why it is a problem.** io_uring runs operations on behalf of the process in kernel worker contexts. That path bypasses syscall-level policy and touches large amounts of kernel code. In Google's kCTF VRP, 60% of exploit submissions targeted io_uring. Google disabled it on ChromeOS and its production servers, blocked it for Android apps with seccomp, and considered disabling it by default on GKE Autopilot [S412][S413]. seccomp sees only `io_uring_enter`, not the operations inside the ring, so allowing io_uring under a seccomp sandbox likely gives a way around the sandbox [S416]. Before 2023 the only system-wide off switch was compiling io_uring out of the kernel. The patch adding a sysctl cited many unprivileged-exploitable bugs [S414]. The resulting `kernel.io_uring_disabled`/`io_uring_group` knobs [S415] are coarse.

**Why it was not fixed.** The fix is in progress, not refused. Per-ring restrictions (`IORING_REGISTER_RESTRICTIONS`) exist, but they cannot inspect arguments. Task-level restrictions and cBPF filters were still being reworked in 2026 [S416]. The deeper issue is structural: a second, asynchronous path to most of the kernel must replicate every check that the synchronous path enforces.

**Alternatives that exist.** Windows IOCP limits completion delivery to a fixed set of I/O operations, not arbitrary syscalls [S417]. A capability system could make the ring itself a capability that names exactly which objects and operations it may touch. Per-ring restrictions are a weak form of this [S416].

**Cost of the alternatives.** Disabling io_uring gives up large gains in IOPS and syscall reduction [S410]. A restricted op set means the "everything async" goal (DM-024) is only partly met. Filtering every ring entry costs performance and is hard to use: the cBPF filters were described as "a bit of pain" [S416].

**Relevance to our OS.** high. If our native ABI is ring-based, policy enforcement (capabilities or a seccomp equivalent) has to apply to each ring entry, not to the ring-entry syscall. That needs to be designed in, not added later.

**Sources.** [S410], [S411], [S412], [S413], [S414], [S415], [S416], [S417]

### DM-026: Readiness-based vs completion-based I/O models

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix family (`select`/`poll`/`epoll`/`kqueue`: readiness), Windows NT (overlapped I/O and IOCP: completion), Linux io_uring (completion); runtimes such as libuv, Rust async
- **Classification:** contested
- **Confidence:** medium

**What the design is.** A readiness API tells you that an operation would not block now, and you then perform it with an ordinary call. A completion API takes the operation and buffer up front and later reports that it has finished [S417][S410].

**Why it was chosen.** Readiness grew out of Unix's synchronous-call model. It needs no change to `read`/`write`, and the kernel never holds user buffers. NT built asynchronous I/O into its I/O manager from the start, with IOCP adding a scheduler-aware thread pool. IOCP releases waiting threads in LIFO order and caps the number running at a set concurrency value [S417].

**Why it is a problem.** The two camps disagree about which model is the problem.
- *Pro-completion position.* Readiness cannot express disk I/O (DM-029). It needs at least two syscalls per operation, and portable runtimes end up with one design per OS: libuv runs epoll, kqueue, event ports and IOCP behind one interface [S418]. Completion covers files and sockets uniformly [S417][S410].
- *Pro-readiness position.* Completion commits a buffer to every pending read, which costs memory with many idle connections. It also transfers buffer ownership to the kernel. That makes cancellation hard: dropping a request does not stop the kernel writing into the buffer, and in Rust this clashes with borrowed-buffer APIs like `AsyncRead` [S419]. Software written for readiness finds IOCP impractical to port. wepoll exists to emulate epoll on Windows for that reason [S421].

**Why it was not fixed.** Each ecosystem's ABI froze its model, so portable runtimes absorb the mismatch [S418][S421].

**Alternatives that exist.** io_uring narrows the gap from the completion side. Provided buffer rings let the kernel pick a buffer only when data arrives, so more reads can be in flight than there are buffers [S420]. io_uring also offers poll-style readiness operations [S410]. kqueue is readiness-based but more general [S402].

**Cost of the alternatives.** Completion models need explicit cancellation semantics and kernel-owned or pinned buffers [S419], and their per-operation kernel paths add attack surface (DM-025). Readiness models leave file I/O to thread pools [S418].

**Relevance to our OS.** high. This is a foundational ABI choice. A capability-style kernel with message passing leans toward completion, while the POSIX compatibility layer needs readiness semantics, so whichever model is native must be able to emulate the other.

**Sources.** [S402], [S410], [S417], [S418], [S419], [S420], [S421]

### DM-027: `EINTR` and restartable system calls ("PC loser-ing")

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; language runtimes on top (Go, Python, Rust std)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** When a signal arrives during a blocking "slow" syscall, Unix runs the handler and then either restarts the call (`SA_RESTART`) or fails it with `EINTR`, leaving the caller to retry. Which calls restart depends on the interface. `poll`, `epoll_wait`, `select`, `nanosleep`, System V IPC and sockets with timeouts never restart, whatever `SA_RESTART` says [S423].

**Why it was chosen.** Gabriel's essay records this as the archetype of "worse is better." Unix did not back the PC out so the interrupted call could be transparently re-entered, as MIT's ITS did. It returned an error and let the user program retry, because "implementation simplicity was more important than interface simplicity" [S422]. On a small PDP-11 kernel, unwinding to the syscall boundary was the simplest way to deliver a signal promptly.

**Why it is a problem.** The cost moves to every call site, forever.
- The restart rules form a per-call table that few programmers know [S423].
- Linux adds a non-POSIX case: some calls return `EINTR` after `SIGSTOP`/`SIGCONT` even with no handler installed [S423].
- Retrying requires accounting for partial transfers.
- A runtime change can expose latent bugs everywhere. Go 1.14's signal-based preemption made programs "see more slow system calls fail with `EINTR`" [S424]. Writes to cgroupfs began failing even though Go installs handlers with `SA_RESTART` [S425], and the Go team judged the fix too risky to backport [S426].

**Why it was not fixed.** The semantics are part of POSIX and of every application's error handling. Changing them would break programs that rely on `EINTR` to interrupt a blocking wait, the main legitimate use.

**Alternatives that exist.** With `SA_RESTART`, the kernel already restarts many calls transparently, which is close to the MIT approach, but only for some calls [S423]. Linux's internal restart-block mechanism, which resumes sleeps with the remaining time, goes further [unverified]. In Windows NT, user-mode APCs interrupt only waits that the thread has explicitly made alertable [unverified]. Cancelling I/O is an explicit operation on the request: IRPs are cancelled through `IoCancelIrp` or when the issuing thread exits [S430]. Signal-free designs remove the problem entirely by delivering events as messages (DM-130).

**Cost of the alternatives.** Transparent restart needs every blocking kernel path to be safely re-entrant or to unwind cleanly. Linux kernel developers have noted that interruptible sleeps require cleanup code on every early-return path [S427]. Explicit cancellation needs cancel routines in every driver that may queue requests indefinitely [S430].

**Relevance to our OS.** high. If we have no asynchronous signal upcalls, `EINTR` disappears natively and lives only in the POSIX layer, but we still need a uniform cancellation mechanism for blocking operations.

**Sources.** [S422], [S423], [S424], [S425], [S426], [S427], [S430]

### DM-028: Uninterruptible sleep (D state); hung NFS

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Linux (`TASK_UNINTERRUPTIBLE`), other Unix kernels with the same sleep model; NFS clients
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A kernel thread waiting in `TASK_UNINTERRUPTIBLE` ignores all signals, including `SIGKILL`, until an explicit wakeup [S427]. NFS "hard" mounts, the default, retry indefinitely, so an unreachable server leaves processes unkillable in "D" state [S428].

**Why it was chosen.** Many kernel paths, such as page-cache I/O, metadata locks and disk writes, cannot easily unwind halfway. Applications also assume file writes cannot be interrupted, as Alan Cox argued against making those sleeps interruptible [S427]. For local disks, the wait was assumed to be short and bounded.

**Why it is a problem.** The assumption fails for network filesystems, failing devices and buggy drivers. The process cannot be killed, the hung-task detector fires, and often only a reboot recovers. NFS's `intr` option was a partial escape. It has been ignored since 2.6.25, and the `soft` alternative risks silent data corruption [S428]. TASK_KILLABLE (2.6.25) lets fatal signals break the sleep, since a dying process never returns to user space to mishandle the error. Adoption was slow: at first only NFS used it, and no drivers did [S427]. Killable waits also spread hangs: a VFS path can take a mutex in uninterruptible sleep behind a killable NFS waiter, so innocent processes enter D state too [S429].

**Why it was not fixed.** Converting each sleep site requires a correct cleanup path. This is done one site at a time across thousands of call sites, and a sleep holding a lock can block others no matter how the lock holder sleeps [S427][S429].

**Alternatives that exist.** NT requires drivers that may queue IRPs indefinitely to support cancellation. The I/O manager cancels IRPs when the issuing thread exits, and after five minutes it disassociates IRPs whose cancellation has not completed [S430]. In microkernels with user-space file servers, a client blocked on IPC can in principle be torn down independently of the server [unverified for specific systems].

**Cost of the alternatives.** Every driver and filesystem path needs cancellation logic, which is a large source of bugs [S430]. Abandoning an in-flight request may leave a device or remote server in an unknown state. That is why Linux NFS prefers "hard" and warns about "soft" [S428].

**Relevance to our OS.** medium. With drivers and filesystems in user space, a process killer can reclaim a client without asking the server, but we must define what happens to its in-flight requests.

**Sources.** [S427], [S428], [S429], [S430]

### DM-029: Regular files always "ready"; no non-blocking disk I/O in the readiness model

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** POSIX, Linux, BSDs, macOS; event-loop runtimes (libuv/Node.js, Samba)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** POSIX requires regular files to "always select true" for reading, writing and error [S431]. `O_NONBLOCK` has no effect on buffered file reads. A read of uncached data blocks the thread until the page cache is filled [S432].

**Why it was chosen.** Disk access was treated as "fast": bounded and not dependent on a remote party, unlike terminals, pipes and sockets. "Ready" for a file has no clear meaning, because any byte may or may not be cached and readiness cannot be tracked per byte range. Making file reads always ready kept `select` simple.

**Why it is a problem.** A single-threaded event loop stalls whenever it touches an uncached file, a slow disk or a network filesystem. Every major runtime therefore keeps a thread pool for file I/O. libuv states plainly that no platform file I/O primitives were usable, so it uses a limited-size pool [S418]. Samba tries a read first and falls back to threads [S432]. Even fully cached reads pay the cost of passing work between threads [S432]. Retrofits are hard:
- `O_NONBLOCK` could not be given meaning for files, because programs like Samba and squid set it and rely on it being ignored [S432].
- Checking cache residency first with `mincore`/`fincore` is racy [S432].
- The eventual `RWF_NOWAIT` (Linux 4.14) works for `preadv2` reads only, and it had a bug in 5.9/5.10 that returned 0 before EOF [S433].

**Why it was not fixed.** The POSIX text and decades of applications fix the semantics [S431][S432]. Linux took the route of new syscalls and flags, and then of io_uring.

**Alternatives that exist.** Windows overlapped I/O treats files and sockets alike [S417]. io_uring does buffered file I/O asynchronously [S410]. FreeBSD kqueue's vnode read filter reports the distance to EOF instead of firing unconditionally as `poll` does [S402], although that does not make the reads themselves non-blocking.

**Cost of the alternatives.** Completion-based file I/O needs the whole storage stack to be asynchronous or to punt to kernel workers (DM-024, DM-025). A readiness signal for files is ill-defined, because readiness depends on the offset and on cache state.

**Relevance to our OS.** high. Our filesystem will be a service from the start, so file and network I/O can share one asynchronous model. The POSIX layer then only has to emulate "always ready".

**Sources.** [S402], [S410], [S417], [S418], [S431], [S432], [S433]

### DM-130: Waitable events outside the descriptor model (signals, timers, process exit) and their fd retrofits

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX, Linux (`signalfd`, `timerfd`, `eventfd`, `pidfd`), BSDs/macOS (kqueue filters); contrast Windows NT (waitable objects), Fuchsia/Zircon (ports)
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** In classic Unix, only descriptors can be passed to `select`/`poll`. Process exit is reported by `SIGCHLD` and `wait`, timers by `SIGALRM`, and signals by asynchronous handlers. An event loop that needs all of these must convert them into fd events, either with the self-pipe trick or with later Linux fds: `signalfd`, `timerfd`, `eventfd`, and `pidfd` in 5.3 [S464][S465].

**Why it was chosen.** Signals predate multiplexed I/O, and `select` was added in BSD for terminals and sockets. Each kind of event got its own mechanism when it was needed, and no single "waitable object" concept existed to unify them.

**Why it is a problem.**
- Lemon notes that signal-based notification allows only one handler per process, which makes it "unsuitable for use in library code" [S401].
- Each retrofit carries over the old mechanism's process-global semantics. For `signalfd` to work, the signal must be blocked with `sigprocmask`. The blocked mask and pending signals then survive `execve`, so child processes inherit an unexpected mask [S465].
- `pidfd_open` waits on process exit only if `SIGCHLD` is not ignored and nothing else reaps the child. Otherwise the caller must create the child with `CLONE_PIDFD` [S464].
- Until pidfds, waiting on a process by PID was racy (see DM-017 and DM-003).

**Why it was not fixed.** Each retrofit was added in its own Linux release while the original mechanisms stayed for POSIX compatibility. kqueue unified these events in 2000 [S401][S402], but Linux took a different direction.

**Alternatives that exist.**
- NT made processes, threads, events, mutexes, semaphores, timers and change notifications waitable handles with one wait call [S463].
- kqueue has filters for processes, signals, timers, vnodes and user events in one queue [S402].
- Zircon's `zx_object_wait_async` delivers signal changes on any waitable object as packets to a port. User packets and IPC arrive on the same port [S466][S467].

**Cost of the alternatives.**
- `WaitForMultipleObjects` is limited to `MAXIMUM_WAIT_OBJECTS` handles. Above that, the docs recommend extra waiting threads or the thread pool [S463].
- Zircon's async waits fire once per registration, and edge mode can permanently miss a transition that happens before the call completes [S467]. Both systems still need a second API (IOCP, or Zircon channel reads) for data I/O.
- A uniform waitable model still has to define what "signalled" means for each object type.

**Relevance to our OS.** high. If every kernel object exposes signal state that can be waited on through one port or queue, this whole class of retrofits disappears. The POSIX layer can then emulate signals on top of it rather than the reverse.

**Sources.** [S401], [S402], [S463], [S464], [S465], [S466], [S467]

## 12. Networking APIs

### DM-074: BSD sockets API design (`sockaddr` casts, protocol-specific structs)

- **Category:** Networking APIs
- **Systems affected:** BSD, Linux, macOS, POSIX, Windows (Winsock); contrast Plan 9, IETF TAPS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** A socket is a descriptor created with a (family, type, protocol) triple. Addresses are passed as a generic `struct sockaddr *` plus a length, and the caller casts protocol-specific structs such as `sockaddr_in` and `sockaddr_in6` to it. The application chooses the address family, resolves names, picks an address and connects. Protocol options go through `setsockopt` with numeric levels [S436][S437].

**Why it was chosen.** 4.2BSD (1983) had to support several protocol families at once, in C with no generics. A tagged struct with a length was the cheapest form of polymorphism. Making sockets descriptors let `read`/`write` work on connections, and the design has lasted more than forty years.

**Why it is a problem.**
- The casts are unchecked. A wrong size or family compiles fine, and returned addresses are silently truncated when the buffer is too small [S437].
- IPv6 did not fit `sockaddr_in`. That forced `sockaddr_in6`, `sockaddr_storage` (sized and aligned so that casts work) and replacements for `gethostbyname`, and every application had to be ported [S436].
- The length type itself changed from `int` to a proposed `size_t` and then to `socklen_t` [S437].
- The IETF's TAPS architecture lists the structural problems. TCP and TLS-over-TCP need different calls. Sockets bind to one address and one interface. TCP is an unstructured byte stream, so every application parses its own message boundaries. Asynchrony is "try and fail". Mechanisms like Happy Eyeballs racing must be implemented separately in each application [S434].

**Why it was not fixed.** Every networked program depends on it, and RFC 3493 explicitly kept IPv6 changes "as small as possible" for that reason [S436]. TAPS is designed to coexist with sockets, not to replace them [S434].

**Alternatives that exist.**
- Plan 9 exposes networks as `/net/<proto>/clone|ctl|data` files. `dial("tcp!host!service")` takes a textual address, and a user-level connection server does resolution. The paper says only name resolution differs between networks [S435].
- TAPS defines a protocol-agnostic connection/message API with path and protocol racing built in [S434].
- `getaddrinfo` already lets applications avoid writing IPv4/IPv6-specific code [S438].

**Cost of the alternatives.** Textual addresses and file-based control move parsing and error reporting into strings, without type checking either. Higher-level APIs like TAPS hide policy, such as which address or path is used, and that hurts applications needing precise control [S434]. An API that resolves names for you inherits the resolver problems in DM-075.

**Relevance to our OS.** medium. Our native API can take typed endpoint values or names and include resolution and racing. The POSIX layer must still provide `sockaddr` exactly as C code expects for the compiler toolchain.

**Sources.** [S434], [S435], [S436], [S437], [S438]

### DM-075: Name resolution in libc: blocking, no async, NSS

- **Category:** Networking APIs
- **Systems affected:** glibc/Linux (NSS), POSIX `getaddrinfo`; musl; language runtimes (Go); contrast Windows `GetAddrInfoEx`, Plan 9 `cs`
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Name lookup is a synchronous libc call, `getaddrinfo`. It may consult `/etc/hosts`, DNS, LDAP and so on, as configured by `nsswitch.conf`. glibc implements each source as a shared library, `libnss_SERVICE.so`, loaded into the calling process [S438][S441]. The only async variant on Linux is the GNU extension `getaddrinfo_a` [S439].

**Why it was chosen.** Host lookup started as reading `/etc/hosts`, which was local and fast. NSS made the set of sources configurable per site without relinking programs.

**Why it is a problem.**
- A lookup can take a long time because of the network, but the call blocks a thread [S438]. Event-loop programs need a thread per lookup or a separate library. Go's docs explain that a blocked C lookup costs an OS thread. Go therefore ships its own DNS resolver, falls back to libc when `nsswitch.conf` uses features it does not implement, and caps cgo lookups at 500 [S440].
- `getaddrinfo_a` is GNU-only and notifies by signal or by a new thread. A request already being processed cannot be cancelled [S439].
- Loading NSS modules into every process means that resolver code, and its dependencies, run in each caller's address space. Static binaries that call `getaddrinfo` still `dlopen` NSS modules. For static setuid binaries this enabled CVE-2025-4802, a local attack that loaded a library from `LD_LIBRARY_PATH` (glibc 2.27 to 2.39) [S443].
- Resolver behaviour differs between libcs. musl, for example, queries nameservers in parallel and lacked DNS-over-TCP until 1.2.4 [S442]. The same configuration can give different results.

**Why it was not fixed.** The API is in POSIX and NSS is glibc's ABI. Async lookup has stayed an extension, and runtimes work around it individually [S439][S440].

**Alternatives that exist.**
- Plan 9 puts resolution in a user-level connection server reached through files, so it is not linked into each program [S435].
- Windows `GetAddrInfoExW` supports overlapped/async completion and cancellation, but only since Windows 8 [S444].
- macOS requires going through the system resolver, which is why Go uses cgo there [S440].

**Cost of the alternatives.** A resolver service adds an IPC hop and a daemon that must be up, and it becomes a privileged single point of failure. Bundled resolvers such as Go's diverge from system policy and must detect when to defer to it [S440].

**Relevance to our OS.** medium. Making name resolution an async service behind a capability fits our model. The POSIX layer must still offer blocking `getaddrinfo` for C programs.

**Sources.** [S435], [S438], [S439], [S440], [S441], [S442], [S443], [S444]

### DM-076: Privileged ports below 1024

- **Category:** Networking APIs
- **Systems affected:** Unix/BSD, Linux, (macOS until 10.14); contrast iOS, Windows
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Binding a TCP/UDP port below 1024 requires root, or on Linux `CAP_NET_BIND_SERVICE`. Linux makes the threshold configurable per network namespace with `net.ipv4.ip_unprivileged_port_start` (default 1024; 0 disables it) [S445].

**Why it was chosen.** On early multi-user hosts run by trusted administrators, a connection from a source port below 1024 proved that root on the remote host had sent it. BSD `rcmd`/`rlogin` built authentication on this: `rresvport` obtains a privileged port, and servers then trust `.rhosts`/`hosts.equiv` [S446]. The low-port rule also stopped ordinary users from impersonating well-known services on shared machines.

**Why it is a problem.** The proof only holds if every host on the network is administered by someone trustworthy, which stopped being true once anyone could run their own machine. Meanwhile the rule forces daemons that serve port 80 or 443 to start as root or carry a capability. That is ambient privilege for a trivial need, and it encouraged setuid daemons and root-run servers. It is also global per namespace and tied to the port number, not to any notion of which program should own the service.

**Why it was not fixed.** The default stays for compatibility, but it is now widely disabled. Linux added the per-namespace sysctl in 2017 so container managers could lift the restriction without giving up control of the network namespace [S447]. Docker sets it to 0 in containers by default since 2020, calling privileged-port binding safe [S449]. Apple removed the restriction in macOS Mojave without documenting it, and iOS never had it [S448].

**Alternatives that exist.** Grant a specific port to a specific program as a capability or by policy, for example through socket activation, where the service manager binds and passes the fd [unverified as a sourced claim here]. Or drop the restriction entirely, as iOS, macOS and Docker containers do [S448][S449].

**Cost of the alternatives.** With no restriction on a shared host, any local user can grab a well-known port before the real daemon starts. Review of the Docker change found that it must not be applied in the host network namespace, and runc later rejected that case [S449]. Per-port authority needs a policy store and an authority to manage it.

**Relevance to our OS.** low. With no compatibility obligations, we can make "bind port N" a capability like any other and not encode trust in port numbers.

**Sources.** [S445], [S446], [S447], [S448], [S449]

### DM-077: `SIGPIPE` as default on writing to a closed socket/pipe

- **Category:** Networking APIs
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; language runtimes (Python, Rust)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Writing to a pipe with no readers, or to a stream socket whose peer has gone away, sends `SIGPIPE`. The default action kills the process. Only if the signal is ignored does `write` return `EPIPE` [S450][S452].

**Why it was chosen.** For shell pipelines this is useful: in `producer | head`, the producer dies as soon as `head` exits, even if it never checks write errors. Most 1970s filters did not check them.

**Why it is a problem.** The same rule applies to sockets, where a peer closing a connection is a routine event, not a reason to kill a server.
- Every network program must disable the signal: process-wide with `SIG_IGN`, per call with `MSG_NOSIGNAL` (Linux 2.2; POSIX only from 2008) [S451], or per socket with BSD's `SO_NOSIGPIPE` [S453].
- Ignoring the signal process-wide is global state that libraries cannot safely change [S451].
- Runtimes override it, which creates inconsistency. Python ignores `SIGPIPE` at startup and turns the error into `BrokenPipeError`, so a plain `script | head` produces a traceback [S454]. Rust's startup code has ignored `SIGPIPE` since 2014. That is undocumented process-wide state, Rust libraries behave differently from Rust applications, and the change is still debated [S455].

**Why it was not fixed.** POSIX mandates the behaviour for both pipes and sockets [S452], and shell pipelines depend on it.

**Alternatives that exist.** Per-call and per-socket opt-outs [S451][S453]. Runtimes that make `EPIPE` an ordinary error [S454]. A design could treat a closed reader as an error return everywhere and let pipeline tools exit on it.

**Cost of the alternatives.** Without the signal, every producer in a pipeline must check write errors, or it keeps running after its reader has gone. Python's `BrokenPipeError` noise and Rust's debate show that both defaults hurt someone [S454][S455].

**Relevance to our OS.** low. A native API without asynchronous signals returns an error. Only the POSIX layer needs to emulate `SIGPIPE`, for pipelines of ported tools.

**Sources.** [S450], [S451], [S452], [S453], [S454], [S455]

### DM-078: Network stack in the kernel vs user-space stacks

- **Category:** Networking APIs
- **Systems affected:** Linux, BSDs, Windows (in-kernel stacks); DPDK, Arrakis, IX, Google Snap, Fuchsia (netstack as a component); Linux XDP/AF_XDP as a middle ground
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Mainstream OSes run TCP/IP inside the kernel, behind the sockets API. The alternatives are:
- Kernel bypass: the application drives the NIC directly with poll-mode drivers, as in DPDK [S459].
- Control-plane/data-plane splits: the kernel sets up protection and hardware or virtualization enforces it, as in Arrakis and IX [S456][S457].
- A user-space networking service: Google's Snap [S458], or Fuchsia's netstack component [S462].

**Why it was chosen.** An in-kernel stack shares the NIC safely among many processes. It enforces port ownership and firewalling, and it works with central tools such as tcpdump and netstat [S460][S461]. On 1980s hardware, the kernel was the only place to put a shared, protected stack.

**Why it is a problem.** The two positions disagree.
- *Pro-bypass / user space.* Kernel crossings, copies and generic code paths limit performance. Arrakis reports 2 to 5x lower latency and 9x higher throughput than tuned Linux [S456]. IX reports up to 3.6x the throughput of Linux with more than 2x lower tail latency, while keeping protection [S457]. Snap runs on over half of Google's fleet, gets more than 3x the Gbps per core of the kernel stack, and can be upgraded without draining machines [S458]. Fuchsia treats the stack as a replaceable component [S462].
- *Pro-kernel.* Bypass gives up isolation and security, breaks tooling, dedicates cores, and makes each application reimplement routing and protocols [S460]. Cloudflare found a gain of only about 5 to 10% CPU. Against that, bypass loses tcpdump, iptables-based DDoS defences, and the ability to share the NIC with SSH [S461].

**Why it was not fixed.** Not a defect to fix but an ongoing split. Linux's answer is XDP and AF_XDP: programmable fast paths inside the kernel, with zero-copy delivery to user space [S460].

**Alternatives that exist.** All of the above. XDP keeps the kernel in charge but does not quite match DPDK's peak performance [S460].

**Cost of the alternatives.** Bypass and dataplane OSes need NIC virtualization (SR-IOV, multiple queues), dedicated cores and per-application stacks [S459][S460]. A user-space service costs an extra IPC hop and depends on a good scheduler co-design; Snap needed a kernel/user-space scheduler co-design [S458]. A rewrite is a big job: Fuchsia's netstack replacement was justified by language and ownership concerns, and reaching performance parity was listed as a risk [S462].

**Relevance to our OS.** high. As a capability-leaning, possibly microkernel-style OS, we will probably host the stack as a service. Snap, IX and Arrakis show this can perform well, but only with careful IPC and scheduling design.

**Sources.** [S456], [S457], [S458], [S459], [S460], [S461], [S462]

## Stream notes

**What was searched.** USENIX proceedings (Banga/Mogul/Druschel 1999, Lemon 2001, Arrakis and IX at OSDI 2014), man7.org and FreeBSD man pages, POSIX Issue 8, LWN (AIO, TASK_KILLABLE, non-blocking buffered reads, io_uring restrictions), LKML (Torvalds on AIO, Rizzo's io_uring sysctl, Kirby's hung-task report), Google's kCTF post and its oss-security repost, Axboe's io_uring paper and Kernel Recipes slides, Microsoft Learn (IOCP, IRP cancellation, WaitForMultipleObjects, GetAddrInfoExW), Go and Rust issue trackers, RFC 3493 and RFC 9621, the Plan 9 networking paper, Fuchsia docs, moby PR #41030, the Linux commit adding ip_unprivileged_port_start, the XDP paper (CoNEXT 2018), Snap (SOSP 2019) and DPDK docs. The full text of PDFs (kqueue, io_uring, XDP, Kernel Recipes) was extracted locally after downloading. A WebFetch rate limit hit partway through, so some later pages (pipe(7), send(2), POSIX write, FreeBSD setsockopt, Python signal docs, Rust issue #62569 via the GitHub API, signalfd(2)) were fetched with curl instead.

**Dropped/merged.** Nothing was dropped. DM-023 covers the multithreaded epoll problems (thundering herd, EPOLLEXCLUSIVE) rather than a separate entry.

**New entries.** DM-130: waitable events outside the descriptor model (signals, timers, process exit) and the signalfd/timerfd/pidfd retrofits, contrasted with NT waitable objects, kqueue filters and Zircon ports. It may overlap with DM-017 (signals, stream C), DM-019 (SysV IPC outside the fd model) and stream A's PID-reuse/pidfd entry. The coordinator may want to merge it, or keep it as the "unified wait" angle.

**Open questions.**
- I could not verify a source for Linux's internal `restart_block` mechanism or for NT alertable-wait/APC semantics. Both are marked [unverified] in DM-027.
- Whether microkernel file-server clients really avoid D-state hangs in practice is [unverified] in DM-028; I found no specific system's documentation.
- Socket activation as a per-port grant is [unverified] in DM-076.
- I could not confirm whether Plan 9 kills writers to a closed pipe with a note, so it is omitted from DM-077.
- Search results mentioned a 2026 epoll use-after-free CVE ("Bad Epoll", CVE-2026-46242), but only through news sites. I did not verify it and did not use it.
- The Google kCTF blog page body did not render. The 60% figure and the ChromeOS/Android/servers/GKE statements are verified through the blog author's own oss-security repost [S413].

**Weak or caveated sources.** S404 and S405 (Majkowski's personal blog), S419 (without.boats blog), S421 (wepoll README) and S461 (Cloudflare blog) are labelled weak. Each is used only for practitioner viewpoints or anecdotal failure modes, and each entry also has primary sources. S448 (Apple Developer Forums) is an Apple engineer's statement and is labelled secondary. Apple never documented the Mojave change. S410 was fetched through an Internet Archive copy because kernel.dk's TLS certificate failed. S407 is Torvalds's LKML email as mirrored on LWN. S411's Torvalds quotes are as reproduced on Axboe's slides.
