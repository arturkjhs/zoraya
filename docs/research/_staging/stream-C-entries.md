# Stream C entries

## 3. Inter-process communication

### DM-017: Signals as a notification/IPC mechanism; async-signal safety

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** A signal is a small integer delivered asynchronously to a process (later, to a thread). It either runs a default action (terminate, stop, ignore) or interrupts whatever code is running to call a user handler on the same stack. The same mechanism carries hardware faults (SIGSEGV), job-control events, timers (SIGALRM), child exit (SIGCHLD) and plain IPC notifications (SIGUSR1, SIGHUP for "reload").

**Why it was chosen.** Early Unix ran single-threaded processes on small machines. A software interrupt that reused the hardware-interrupt model was the cheapest way to report faults, let the terminal kill a process, and wake a process out of a blocking call. There was no event loop or descriptor-multiplexing primitive to deliver events to instead.

**Why it is a problem.** A handler can run in the middle of any instruction sequence, including inside `malloc` or stdio, so it may only call *async-signal-safe* functions. That list is short, differs between standards revisions, and implementations deviate (glibc `execl` was unsafe before 2.24; `aio_suspend` is unsafe) [S300]. Any error is a memory-corruption bug. A concrete case: OpenSSH's SIGALRM handler called `syslog()` and could corrupt the heap mid-`malloc`. This was CVE-2006-5051. It was fixed, then re-introduced in 2020 when a guard `#ifdef` was removed, giving unauthenticated remote root on glibc Linux (CVE-2024-6387, "regreSSHion") [S301]. The rest of the history is a series of patches, each exposing the next problem [S303]: one-shot handlers that lost signals; `EINTR` from interrupted system calls; `sigaction` flags; `pselect`/`ppoll` to close wait races; real-time signals whose queues still overflow; and SIGCHLD/SIGCLD semantic differences. Threads made it worse, because delivery is now split between process-directed and thread-directed pending sets [S302].

**Why it was not fixed.** POSIX and every shell, daemon and runtime rely on signal semantics, including SIGKILL, SIGPIPE defaults and SIGHUP-to-reload conventions. Linux added `signalfd` (2.6.22) to read signals as descriptor events [S303]. But the signals must be blocked first. The blocked mask is inherited across `exec`, so children silently miss signals, and synchronous faults still need real handlers [S302], [S303]. The old path cannot be removed.

**Alternatives that exist.** Descriptor-based event delivery: `signalfd`, plus `pidfd` for child exit. Windows has no general signals. Console control events run on a fresh thread the system creates, not by interrupting a running one [S339]. Microkernels (seL4) deliver asynchronous events as notification objects that a thread explicitly waits on [S305]. Faults then become IPC to a fault handler.

**Cost of the alternatives.** Something still has to kill an unresponsive process unconditionally. Every design keeps a non-catchable termination path. Synchronous faults (page fault, divide by zero) need some form of upcall or fault IPC, whose handler runs in a constrained context too. A per-thread fresh handler, as on Windows, moves the problem to ordinary thread synchronisation [S339].

**Relevance to our OS.** high — the POSIX layer must emulate signals for the C toolchain, but the native ABI can choose whether asynchronous interruption of user code exists at all.

**Sources.** [S300], [S301], [S302], [S303], [S305], [S339]

### DM-018: Mach IPC performance and the L4 response

- **Category:** Inter-process communication
- **Systems affected:** Mach 3.0, OSF/1 MK, MkLinux, GNU Hurd, XNU (Mach heritage), L4 family, seL4
- **Classification:** contested
- **Confidence:** high

**What the design is.** Mach 3.0 moved Unix out of the kernel into a user-level server, so every system service became an IPC through ports. Mach ports are kernel-buffered message queues with capability-like port rights, and messages can carry out-of-line memory [S305]. Liedtke's L3/L4 redesigned IPC as minimal synchronous rendezvous between threads, tuned per processor [S304], [S305].

**Why it was chosen.** Mach aimed for multiprocessor and distributed systems, plus Unix compatibility. Asynchronous buffered ports with rich, network-transparent semantics fit that goal, and the generality looked worth the cost at the time.

**Why it is a problem.** By the early 1990s a one-way microkernel message cost about 100 µs. This led vendors to move services back into the kernel [S305]. Chen and Bershad measured Mach 3.0 plus its Unix server executing substantially more system instructions, at more memory cycles per instruction, than monolithic Ultrix [S307]. Some read this as inherent to microkernels. Liedtke reanalysed the same data. A Mach kernel call took 18 µs on a 50 MHz 486, roughly 800 cycles of pure kernel overhead, against 15 cycles in L3. He argued the cache misses came from Mach's large working set, not from the structure itself [S304]. With a minimal, processor-specific design, L4 IPC was 10–20× faster [S305]. L4Linux came within about 5% of native Linux on AIM, against roughly 5–7× larger penalties for MkLinux [S306].

**Why it was not fixed.** Mach's API was baked into its OS servers and into XNU, which kept Mach messaging but runs BSD in the same kernel address space. The L4 community also did not keep Liedtke's full design. Long IPC, IPC timeouts, and clans-and-chiefs were abandoned as complex or unusable. Thread-ID destinations were replaced by endpoint capabilities [S305].

**Alternatives that exist.** seL4/Fiasco.OC: synchronous endpoints with no kernel buffering, badged capabilities, and virtual message registers [S305]. Notification objects for asynchronous signalling. Shared memory for bulk data.

**Cost of the alternatives.** Purely synchronous IPC forces multithreaded servers. Before notifications, a server could not wait for both clients and interrupts. Synchronous rendezvous is "clearly the wrong way" to synchronise across cores [S305]. Processor-specific fast paths reduce portability [S304]. The contest continues: whether 5% overhead and the extra engineering are worth the isolation is still argued, and production OSes mostly chose monolithic or hybrid kernels.

**Relevance to our OS.** high — if services live in user space, IPC cost and semantics set the performance ceiling of everything else.

**Sources.** [S304], [S305], [S306], [S307]. [S308] unverified, not counted.

### DM-019: System V IPC: separate key namespace, objects outside the fd model

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX (XSI), Linux, BSDs, macOS, Solaris
- **Classification:** mistake
- **Confidence:** high

**What the design is.** System V message queues, semaphores and shared memory are named by integer `key_t` values. `*get` calls turn a key into a kernel-wide identifier used by the other calls [S309]. They are not file descriptors. They have their own `ipc_perm` permission bits, and they persist until explicitly removed [S311].

**Why it was chosen.** In the late 1970s and early 1980s the file system offered no shared memory and no counting semaphores. A separate subsystem let unrelated processes rendezvous without a common parent or a filesystem object, and integer keys were cheap.

**Why it is a problem.**
- Keys are a flat global namespace with no collision protection. `ftok` builds keys from inode and device bits and explicitly gives no uniqueness guarantee [S310].
- Objects are not reference-counted by process lifetime. Removal must be explicit, and a crashed program leaks segments whose pages stay in memory or swap [S311].
- Because they are not fds, they do not work with `poll`, descriptor passing, `close-on-exec`, or capability-style delegation.
- Limits are system-wide tunables (`SHMMAX`, `SEMMNI`, …) that differ per OS and must be hand-tuned [S313].
- Containers needed a whole new namespace type just to isolate these identifiers [S312].
- The lifetime model fights other policy. systemd's `RemoveIPC` deletes a user's objects at logout, which can make PostgreSQL "very unreliable" [S313].

**Why it was not fixed.** The interfaces are in the XSI part of POSIX and in long-lived software. They are kept for compatibility, while applications migrate away one by one. PostgreSQL 9.3 moved most of its shared memory to `mmap` specifically to avoid kernel-parameter tuning [S314].

**Alternatives that exist.** POSIX `shm_open`/`mq_open`/named semaphores, which use path-like names and, for shm, fds [S309]. Linux `memfd_create`: an anonymous, fd-backed, reference-counted memory object with seals so it can be shared safely with untrusted peers [S315]. Capability-system VMOs and handles (Zircon, seL4 frames), where there is no global name at all.

**Cost of the alternatives.** Without a global name, two unrelated processes need some broker or inherited channel to find each other. That is a bootstrapping and naming problem the design must solve elsewhere. Seals and fd passing add API surface [S315].

**Relevance to our OS.** medium — the native design should not repeat it, but the POSIX layer may need to emulate SysV shm/sem for ported software.

**Sources.** [S309], [S310], [S311], [S312], [S313], [S314], [S315]

### DM-020: Untyped byte streams as the only composition medium

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX shells and tools, Linux, BSDs, macOS; contrast PowerShell (Windows), Nushell
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Unix pipes, files and terminals carry undifferentiated bytes. Programs compose by agreeing informally on text formats such as lines and whitespace-separated fields. The kernel imposes no schema, and every consumer parses.

**Why it was chosen.** The original authors made this a deliberate style rule. Write programs whose output can become input to "another, as yet unknown, program", and avoid columnar or binary input formats [S316]. A byte stream is the lowest common denominator. It needs no shared type system across languages, it can be inspected by a human, and it keeps the kernel simple.

**Why it is a problem.** Structure is reconstructed by parsing, which is lossy and fragile. Snover's critique: tools bind getting objects, processing them and rendering text together, so downstream tools do "prayer-based parsing" to recover the objects [S317]. Concrete failure modes: filenames containing newlines, spaces, leading dashes or control characters break line-oriented loops, `xargs`, and globs. Some of these are security bugs, such as CVE-2013-7085, arbitrary file deletion through a whitespace filename [S318]. Output formats become de facto ABIs that cannot change without breaking scripts.

**Why it was not fixed.** Every tool reads and writes text, and the conventions are in POSIX. Partial fixes such as `-print0`/`-0` and NUL separators are spread unevenly [S318].

**Alternatives that exist.** PowerShell pipes .NET objects and uses reflection for generic `Where`/`Sort`/`Format` operations [S317]. Nushell passes structured values and tables between built-ins and handles JSON natively [S319].

**Cost of the alternatives.** Object pipelines bind to one runtime and type system (.NET for PowerShell). Once rendered to text, the objects are gone, so legacy commands take part only as text streams [S317]. Nushell gets structure largely by reimplementing tools as built-ins [S319]. The pro-text position [S316] values being language-agnostic, human-inspectable, and debuggable with `cat`. The pro-structure position [S317] values precision and robustness. Both are held by experienced practitioners.

**Relevance to our OS.** medium — this choice shapes the native shell and tool ABI, and a typed IPC layer could carry schemas, but the hosted C toolchain will expect byte streams.

**Sources.** [S316], [S317], [S318], [S319]

### DM-021: Userspace message bus retrofitted (D-Bus; kdbus/bus1 rejected)

- **Category:** Inter-process communication
- **Systems affected:** Linux desktop and systemd stack (D-Bus); contrast Android (binder), macOS (Mach/XPC) [unverified for XPC]
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Linux has no kernel service-bus primitive. D-Bus is a user-space daemon that accepts connections over Unix sockets and routes method calls and signals between them. It adds well-known names, activation, a policy layer, fd passing (`UNIX_FD`), and peer-credential queries [S328].

**Why it was chosen.** Kernel developers prefer mechanism in the kernel and policy in user space. Unix sockets plus `SCM_RIGHTS` and `SO_PEERCRED` already provided the transport. A daemon can ship and evolve without kernel changes.

**Why it is a problem.** Each call crosses the daemon: two hops, extra copies, extra context switches. Proponents said reliable sender metadata cannot be obtained race-free from user space. The kdbus pull request claimed to fix "almost unfixable races" in D-Bus implementations [S322]. The credentials API kept growing (e.g. a ProcessFD field), a sign of retrofitting [S328].

**Why it was not fixed.** kdbus (2015) was never merged. Lutomirski NACKed it. His arguments: memfd over `SCM_RIGHTS` already handles bulk data; metadata could be a small Unix-socket extension; the metadata design broke CRIU; and pool buffering escaped memory-cgroup accounting [S320], [S321], [S324]. Biederman called the metadata transfer code wrong [S322]. Torvalds was inclined to trust his maintainer but was "not AT ALL impressed" by the performance argument, which he thought pointed at user-space fixes [S324]. The merge was delayed and then abandoned [S323]. bus1 (2016) offered capability handles, quotas, causal ordering and multicast [S325]. It stalled, and resurfaced in 2026 as a stripped-down Rust module [S326]. Meanwhile, its authors wrote dbus-broker, a faster user-space broker [S327].

**Alternatives that exist.** Android binder is in-tree, kernel-mediated IPC with object references, and it was accepted [S329], [S325]. Capability-based IPC with kernel-routed handles (seL4 endpoints, Zircon channels).

**Cost of the alternatives.** A kernel bus puts policy, quotas, ordering and accounting in the kernel, which is exactly where the kdbus review found problems [S321], [S324]. bus1's own authors argue binder lacks resource accounting and multicast; binder's supporters answer that it is proven at scale [S326]. The disagreement is about where a bus belongs, not only about implementation quality.

**Relevance to our OS.** high — we must decide early whether service discovery, naming and broadcast are kernel objects, a user-space broker, or capability-passing conventions.

**Sources.** [S320], [S321], [S322], [S323], [S324], [S325], [S326], [S327], [S328], [S329]

### DM-022: Descriptor passing over Unix sockets (`SCM_RIGHTS`): API shape and in-flight GC bugs

- **Category:** Inter-process communication
- **Systems affected:** Linux, BSDs, macOS, Unix/POSIX
- **Classification:** mistake
- **Confidence:** high

**What the design is.** A process sends open file descriptors as ancillary data (`cmsg`) on an `AF_UNIX` socket. The kernel keeps a reference while the message is queued, and the receiver gets new descriptor numbers [S334].

**Why it was chosen.** It retrofitted capability transfer (BSD 4.2-era) onto the existing socket API without a new syscall family. That made privilege separation possible: a privileged process opens a resource and passes it to a sandboxed one.

**Why it is a problem.** The API is awkward and full of edge cases:
- On stream sockets the fds must ride along with at least one data byte.
- There is at most one `SCM_RIGHTS` item per message, capped at 253 fds.
- A truncated control buffer silently closes excess fds, signalled only by `MSG_CTRUNC`.
- Fds over the receiver's `RLIMIT_NOFILE` are dropped.
- Until Linux 4.5 the number of in-flight fds was unlimited [S334].

The deeper flaw is semantic. A socket can be sent over itself or over a peer, which creates reference cycles held only by in-flight messages, so Linux needs a garbage collector [S330]. That collector is a recurring source of races. CVE-2021-0920 is a use-after-free from a `MSG_PEEK` vs GC race. It was reported publicly in 2016 but not fixed, and was then exploited in the wild on Samsung Android phones [S330], [S331], [S333]. CVE-2024-26923 is a GC race against `connect()` [S332]. In 2024 the whole algorithm was replaced with a Tarjan-SCC collector over a graph of inflight sockets [S335], [S336].

**Why it was not fixed.** Cycles are allowed by the API, so the kernel cannot remove the GC without breaking semantics. The ancillary-data format is POSIX and used by D-Bus, systemd, Wayland and others.

**Alternatives that exist.** Zircon channels: a message is bytes plus handles, transferred atomically. Handles are moved (consumed even on failure) and need `ZX_RIGHT_TRANSFER`. A channel cannot be written into itself. Endpoints cannot be duplicated, and queued handles are closed when an endpoint dies [S337], [S338]. seL4 transfers capabilities in IPC.

**Cost of the alternatives.** Move semantics make sharing explicit (duplicate first), and rights checks add API surface [S337]. Whether move-only handles fully remove cycles (e.g. two channels each queued inside the other) is not addressed in the Zircon docs [S338] [unverified]. Any handle-in-message design must still decide this.

**Relevance to our OS.** high — handle transfer in IPC is central to a capability-leaning design, and cycle and lifetime rules must be designed in, not patched later.

**Sources.** [S330], [S331], [S332], [S333], [S334], [S335], [S336], [S337], [S338]

## 9. Concurrency and synchronization primitives

### DM-058: Big Kernel Lock; SMP retrofitted onto uniprocessor kernels

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** Linux (BKL, 1.3.x–2.6.39), FreeBSD (Giant, SMPng), other Unix kernels [unverified for others]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** A kernel written for one CPU gets multiprocessor support through a single global lock: only one CPU may execute (most) kernel code at a time. Concurrency is then added by moving subsystems out from under the lock one by one.

**Why it was chosen.** It was the only feasible first step. In Linux 1.3.x the "lock" was a bit set on kernel entry, so CPUs spun at the door and no driver had to change [S341]. Generic spinlocks did not exist on most architectures, so the BKL was the only SMP mutex available. The 2.1.23 change that spread it (9 to 761 call sites) is also what first allowed two tasks in the kernel at once [S341]. FreeBSD's Giant played the same role [S343].

**Why it is a problem.** The lock was recursive and released on sleep, so code took it "just to be safe" at no visible cost. Its many uses became entangled: unrelated subsystems, such as a webcam driver list and the NFS daemon, excluded each other for no reason [S341]. Nobody could say what a given `lock_kernel()` protected. Each removal required reverse-engineering implicit invariants. When the BKL was pushed out of the VFS, every driver `open` got its own copy, which most maintainers never audited [S341]. The cost: poor scalability for a decade, and latency problems the realtime tree needed fixed [S342].

**Why it was not fixed.** The fix was spread across hundreds of call sites in code with absent or uninterested owners. It took roughly 1995–2011. The end game removed or staged modules nobody would fix [S342] and finished with "BKL: That's all, folks" in 2.6.39 [S340]. FreeBSD similarly kept non-MPSAFE paths under Giant for years [S343].

**Alternatives that exist.** Design for SMP from day one with explicit, documented lock scopes, or avoid shared kernel state (multikernel-style per-core state). Notably, seL4 deliberately keeps a big lock. Because microkernel system calls are short, a big lock performs comparably to fine-grained locking on shared-cache cores, and it keeps the kernel verifiable [S344], [S345].

**Cost of the alternatives.** Fine-grained locking from the start costs complexity, deadlock risk, and verification difficulty [S345]. The big-lock approach only works when the protected code paths are short. It breaks down for a monolithic kernel with long syscalls, which is exactly the BKL lesson [S344].

**Relevance to our OS.** high — the lesson is less "never use a big lock" than "keep what the lock covers small and documented"; a small kernel may legitimately start with one.

**Sources.** [S340], [S341], [S342], [S343], [S344], [S345]

### DM-059: Thread cancellation (`pthread_cancel`)

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** POSIX threads (glibc, musl, BSD libcs); analogous: Windows `TerminateThread`, Java `Thread.stop`
- **Classification:** mistake
- **Confidence:** high

**What the design is.** One thread can request another's termination. In deferred mode the target dies at the next cancellation point, a long list of functions including `open`, `read`, `close`, `accept` and `pthread_cond_wait`. In asynchronous mode it can die at any instruction. Cleanup handlers run in reverse push order, then TSD destructors [S346], [S347].

**Why it was chosen.** Threads blocked in system calls need some way to be stopped, for example to abort a search or shut down a server, without each blocking call taking a "stop" argument. POSIX tried to make this safe with cancellation points and cleanup handlers instead of raw asynchronous kill.

**Why it is a problem.**
- Asynchronous mode is unusable in practice: only `pthread_cancel`, `pthread_setcancelstate` and `pthread_setcanceltype` are async-cancel-safe [S347].
- Deferred mode is subtle too. glibc implemented each cancellable syscall by briefly enabling *asynchronous* cancellation around it. A cancel arriving after the kernel had acted, but before user space saw the result, could leak a descriptor from `open`/`accept`. For `close` it left the program unable to know whether the fd was released, risking closing a different thread's fd later [S348]. Signal handlers that interrupted such syscalls ran with async cancellation on [S349].
- Reported in 2011 (BZ #12683), the race was fixed only in glibc 2.41, and a follow-up crash fix landed in 2.42 [S350], [S349].
- Every library on a cancellable path must be written cancellation-safe with push/pop handlers, and most are not.
- On Linux the mechanism rides on a hidden real-time signal [S346], inheriting signal complexity (DM-017).
- The forced-kill cousins are worse. `TerminateThread` can leave critical sections and the heap lock held [S351]. Java removed `Thread.stop` because releasing monitors leaves "damaged" objects [S352].

**Why it was not fixed.** It is part of POSIX, and C code depends on it, for example to break out of blocking reads. Fixing the race required arch-specific syscall wrappers [S349].

**Alternatives that exist.** Cooperative cancellation: a flag plus an interrupt that makes blocking calls return, as in Java's recommended pattern [S352]. Cancellation tokens passed explicitly. In capability or handle systems, closing or revoking the handle a thread waits on makes the wait return an error. musl decides precisely whether a syscall completed by checking the program counter against labels around it [S348].

**Cost of the alternatives.** Cooperative schemes need every blocking primitive to be interruptible, plus disciplined callers. A thread stuck in uncooperative code cannot be stopped, short of killing the whole process.

**Relevance to our OS.** high — the native ABI should define how blocking waits are interrupted (handle close, timeout, cancel token); the POSIX layer must still emulate `pthread_cancel` for ported C code.

**Sources.** [S346], [S347], [S348], [S349], [S350], [S351], [S352]

### DM-060: Non-reentrant libc interfaces with hidden global state (`strtok`, `localtime`, `errno` before TLS)

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** C standard library / POSIX on all Unix-likes; Windows CRT [unverified]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Many C and POSIX functions keep state in static storage. `strtok` stores its position internally [S353]. `localtime`, `gmtime`, `asctime` and `ctime` return pointers to static buffers that any later call overwrites [S354]. `errno` was originally a plain global `int` [S355]. The environment (`getenv`/`setenv`) is an unsynchronised global array.

**Why it was chosen.** These functions predate threads. In a single-threaded process, static buffers save callers from allocating memory and keep signatures short. On 1970s machines both mattered.

**Why it is a problem.** With threads, every such function is a data race. POSIX exempts roughly 90 functions from its thread-safety requirement, including `strtok`, `localtime`, `asctime`, `getenv` and `strerror` [S347]. glibc labels them, for example `MT-Unsafe race:tmbuf env locale` [S354]. The fix doubled the API with `_r` variants (SUSv2), and those differ subtly: `localtime_r` need not call `tzset`, so portable code must call it explicitly [S354]. `errno` had to become a per-thread macro. Old code that declares `extern int errno` is now harmful [S355]. Environment mutation is still unsound with threads. Rust 2024 made `std::env::set_var`/`remove_var` `unsafe` for exactly this reason, after shipping them as safe [S356]. Boehm's broader point: threads bolted onto a language and library designed without them cannot be made fully correct by the library alone [S366].

**Why it was not fixed.** The originals are in ISO C and POSIX and cannot change signature. Removing them breaks source compatibility, so both sets coexist.

**Alternatives that exist.** APIs designed after threads, with caller-supplied buffers or returned owned values (all `_r` functions, Rust `std`). Thread-local error values or error returns instead of `errno`. Immutable process environment snapshots.

**Cost of the alternatives.** More parameters, explicit buffer management, and divergence from code written against the old names.

**Relevance to our OS.** medium — native interfaces can avoid hidden global state entirely, but the POSIX compatibility libc must provide the legacy functions and decide how unsafe `setenv` is allowed to be.

**Sources.** [S347], [S353], [S354], [S355], [S356], [S366]

### DM-061: Priority inversion; priority inheritance as an afterthought (Mars Pathfinder)

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** VxWorks, POSIX threads (all implementations), Linux (rt-mutex, PI futexes), most general-purpose kernels
- **Classification:** contested
- **Confidence:** high

**What the design is.** Mutexes ignore priority by default. A high-priority thread waiting on a lock held by a low-priority thread can be delayed without bound while medium-priority threads preempt the holder. In POSIX the default mutex protocol is `PTHREAD_PRIO_NONE`. Inheritance (`PRIO_INHERIT`) and ceiling (`PRIO_PROTECT`) are optional features [S363].

**Why it was chosen.** Plain mutexes are cheaper and simpler. Priority inheritance requires tracking the owner, walking PI chains and boosting priorities in the scheduler [S359]. Most workloads are not hard real-time. The issue itself was understood early: Lampson and Redell described it for monitors in 1980 [S361], and Sha, Rajkumar and Lehoczky formalised inheritance and priority ceiling in 1990 [S358].

**Why it is a problem.** Mars Pathfinder (1997) reset repeatedly. A high-priority bus task waited on a mutex held by the low-priority meteorological task, which medium-priority tasks kept preempting. Priority inheritance existed in VxWorks but was off in the default `select()` mutex "for optimum performance". JPL reproduced the fault on a ground replica within 18 hours and fixed it by uploading a change to a global variable; Reeves: "We just missed it" [S357]. Linux added rt-mutexes and PI futexes so user-space pthread mutexes can avoid unbounded inversion [S360]. Even so, chain-depth bounds remain hard to reason about [S359].

**Why it was not fixed.** It is a contested design point, not just inertia. Yodaiken argues PI is unreliable or intrusive. He holds that the original paper's algorithm is wrong, and that PI papers over designs that ignore how priority and mutual exclusion interact. RTLinux omitted it on purpose [S361]. Locke answers that the technical points are old and correct but the conclusion is a strawman, and that PI is a legitimate tool, as Pathfinder showed [S362].

**Alternatives that exist.** Priority ceiling, which bounds blocking to one critical section and prevents deadlock [S358]. Static analysis or lock-free designs for critical paths [S361]. IPC-level priority propagation in synchronous-message systems such as QNX (see GR-018). Scheduling-context donation in seL4 [unverified].

**Cost of the alternatives.** Ceiling requires knowing every lock's users in advance [S361]. PI adds kernel complexity and per-lock overhead [S359], [S360]. Ignoring the problem leaves latent field failures, as with Pathfinder.

**Relevance to our OS.** medium — the compositor and input/audio paths are latency-sensitive, so the choice of default lock protocol and of priority propagation across IPC is a real design decision.

**Sources.** [S357], [S358], [S359], [S360], [S361], [S362], [S363]

### DM-062: Kernel memory model specified after the fact (LKMM)

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** Linux; C/C++ before C11 (pthreads era); any kernel targeting multiple weakly ordered ISAs
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** For most of its life, Linux defined the meaning of concurrent memory accesses only through prose (`memory-barriers.txt`) plus per-architecture barrier implementations. A formal, executable Linux kernel memory model (cat/herd) arrived in 2018 [S364]. The prose document still says it is "not a specification" and is incomplete [S365].

**Why it was chosen.** When SMP Linux appeared, no language-level memory model existed. C had none until C11. Kernel developers needed working barriers across many architectures immediately, and formal tools for weak memory were research prototypes. Torvalds argued that a 500-page spec would not fit a 15M-line project that knowingly works outside specs, and that C11's model cannot simply be adopted by the kernel [S364].

**Why it is a problem.** Linux targets more than 30 architectures with differing weak models (Alpha, ARM, Power, Itanium, MIPS…). That generated long LKML debates about what code may assume [S364]. Prose leaves room for ambiguity. The formal model resolved ambiguities in RCU documentation and `memory-barriers.txt`, and informed fixes to code wrongly relying on fully ordered lock-unlock pairs and to ARM64 lock/fence combinations [S364]. Alpha's dependency-ordering quirk forced a barrier inside `READ_ONCE()` (since v4.15) [S365]. More generally, threads implemented as a library on a compiler with no model cannot guarantee correctness, because ordinary optimisations can introduce races [S366].

**Why it was not fixed.** The kernel's primitives (`READ_ONCE`, `smp_mb`, RCU) predate C11 and do not map cleanly onto it. The kernel chose its own model rather than converge [S364].

**Alternatives that exist.** Start from a language memory model: C11/C++11, and Rust, which follows C++'s [unverified]. Formalise the kernel's own primitives up front with tools such as herd [S364]. Support fewer architectures, or only those with stronger guarantees.

**Cost of the alternatives.** Language models may not express what kernels need, such as dependency ordering and barriers C11 lacks [S364]. Formal models need specialist maintainers [S364].

**Relevance to our OS.** high — a Rust aarch64 kernel inherits Rust's atomics model for free, but device memory, DMA, page-table updates and any custom primitives (RCU-like, seqlocks) still need written ordering rules from day one.

**Sources.** [S364], [S365], [S366]

### DM-120: Futex interface: one multiplexed syscall with accreted operations

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** Linux (futex, PI futexes, futex2); user-space libcs that build on it
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Linux exposes user-space synchronisation through one `futex` syscall whose `op` argument selects among many operations (wait, wake, requeue, cmp-requeue, wake-op, PI lock/unlock, requeue-PI…). The futex word is an aligned `int` in user memory, and the kernel keys wait queues by physical address [S367]. The uncontended path stays in user space (GR-011 covers the core idea, which held up).

**Why it was chosen.** Fast uncontended locking without syscalls was the goal. A single multiplexing entry point let new operations be added quickly as glibc needed condition variables, robust mutexes and PI [S367], [S360].

**Why it is a problem.** The semantics are hard to use correctly. Drepper notes that the inventors' own published user-level code was incorrect [S367]. Operations are added and then found broken: `FUTEX_REQUEUE` "proved to be broken and unusable" and survives only for compatibility [S367]. Kernel-side complexity grew with PI state and requeue-to-PI paths [S360]. One requeue bug, which failed to ensure two distinct futex addresses, became a local privilege escalation, CVE-2014-3153, later listed in CISA's known-exploited catalog [S368]. The interface could not wait on multiple futexes or use sizes other than 32 bits. That led to a separate futex2 interface "designed to overcome limitations of the original" [S369].

**Why it was not fixed.** Every libc and runtime depends on the existing op codes, so they can be added to but not removed [S367], [S369].

**Alternatives that exist.** Separate, typed syscalls per operation (the futex2 direction [S369]). Kernel-provided higher-level objects (Windows events/keyed events, Zircon `zx_futex_*` [unverified]). Notification objects in capability kernels (seL4) [S305].

**Cost of the alternatives.** More syscalls and ABI surface. Higher-level kernel objects cost a syscall even when uncontended unless paired with a user-space fast path. Removing multiplexing loses the ease of extension that made futexes evolve quickly.

**Relevance to our OS.** high — our own wait/wake primitive will underpin the POSIX layer's pthreads and the native runtime, and its extensibility shape (typed calls vs op codes) is an early ABI choice.

**Sources.** [S305], [S360], [S367], [S368], [S369]

## Stream notes

**Searched.**
- Signals: signal-safety(7), signalfd(2), the Qualys regreSSHion advisory, LWN "Ghosts of Unix past", Windows console handlers.
- Mach/L4: Liedtke 1993/1995, Härtig 1997, Chen and Bershad 1993, Heiser and Elphinstone 2016.
- SysV IPC: man pages and PostgreSQL docs and release notes.
- Byte streams: the BSTJ 1978 foreword, the Monad Manifesto, Wheeler, Nushell.
- kdbus/bus1: LKML 2015 threads, LWN 2015/2016/2026, dbus-broker, the D-Bus spec, binderfs.
- SCM_RIGHTS: Project Zero, the NVD API, the Debian tracker, unix(7), 2024 GC rework commits, Fuchsia channel docs.
- BKL: the OLS 2002 paper, the removal commit, LWN, FreeBSD SMPng, seL4 big-lock papers.
- Cancellation: man pages, glibc BZ 12683 (bugzilla blocked by an anti-bot wall, so I used the glibc commit and NEWS via the GitHub mirror instead), EWONTFIX, TerminateThread, Java Thread.stop.
- Non-reentrant libc: man pages and the Rust 2024 edition guide.
- Priority inversion: Reeves, Sha et al., kernel rt-mutex and PI-futex docs, Yodaiken vs Locke, POSIX.
- Memory model: Alglave et al. 2018, memory-barriers.txt, Boehm 2005.
- Futex: Drepper, the futex2 docs, CVE-2014-3153.

**Dropped/merged.** None dropped. DM-018 was reclassified as `contested`; the index listed no classification.

**New entries.** DM-120 (futex interface accretion, tradeoff). It complements GR-011 rather than contradicting it.

**Open questions.**
- Whether Zircon move-only handles can still form unreachable cycles; the docs are silent.
- seL4 scheduling-context donation as a PI alternative, not sourced.
- Whether macOS XPC/launchd is a better bus-design data point for DM-021.
- Rust's atomics model formally deferring to C++20, not sourced.
- Original Linux futex man page and Liedtke 1993 could not be fetched (connection refused); S308 is marked unverified and not counted.
- The glibc fix in 2.41 is inferred from the NEWS fixed-bug list.

**Weak or caution sources.**
- S318 (Wheeler essay) is weak.
- S348 (EWONTFIX, unsigned, attributed to the musl author) is weak; its claims are corroborated by the glibc commit S349.
- S362 (Locke) is an editorial.
- The S330 Project Zero post is a vendor write-up, treated as primary for exploit analysis.
- S357 Reeves is an email reproduced on a course page.
- The Sha et al. S358 copy is a course mirror of the IEEE paper.
- The NVD records were fetched through the NVD JSON API endpoint (the detail page rendered empty).
- Several PDFs were text-extracted locally after fetch.
