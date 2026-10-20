### DM-016: Windows security descriptors/ACL complexity and token model

- **Category:** Security, authority and identity
- **Systems affected:** Windows NT and successors
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Every securable object has a security descriptor with an owner, a DACL of prioritized allow/deny entries, and a SACL [S226], [S227]; every process has a primary access token listing user, group and logon SIDs, privileges, optional restricting SIDs, and whether it is a primary or impersonation token, and a thread can additionally hold an impersonation token [S225]. Vista added Mandatory Integrity Control: integrity labels, stored in the SACL, are checked before the DACL, and objects get no-write-up by default [S226]; AppContainer adds a further isolation layer that MIC also respects [S226].

**Why it was chosen.** NT was designed for enterprise domains and formal security evaluation [unverified], so it made access control uniform across all kernel object types and expressive enough for any policy. Tokens give a clean per-thread security context and support impersonation for servers.

**Why it is a problem.** Expressiveness exceeds what developers can reason about. Govindavajhala and Appel count up to about 30 rights on about 15 object kinds, and their scanner found several user-to-administrator escalations caused by commercial vendors' ACL misconfigurations on registry keys, files and services (CERT VU#953860) [S227]. Impersonation tokens make servers act with the client's authority, a confused-deputy surface [unverified as a measured trend]. Before Vista, applications typically ran as administrators, so Windows needed registry (and file) virtualization, which Microsoft calls an "interim" compatibility technology [S228], and integrity levels as a second, mandatory layer [S226].

**Why it was not fixed.** The model is consistent and is the basis of Active Directory; complexity lives in policy rather than in the mechanism, so it is retrofitted with layers (MIC, AppContainer) rather than replaced.

**Alternatives that exist.** Unix's simpler mode bits: three privileges for user, group and other on a few kinds of object [S227] (less expressive; that this yields fewer misconfigurations is plausible but not measured in the cited source); capability handles (NT handles themselves carry granted access masks after open — see GR-003).

**Cost of the alternatives.** Simpler models cannot express delegation to specific groups or deny entries, and push complexity into setuid helpers (DM-011).

**Relevance to our OS.** medium — positive lesson: one uniform object/security model across all kernel objects; negative lesson: expressive ACL languages produce misconfiguration CVEs.

**Sources.** [S225], [S226], [S227], [S228]

### DM-017: Signals as a notification/IPC mechanism; async-signal safety

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** mistake
- **Confidence:** high

**What the design is.** A signal is a small integer delivered asynchronously to a process (later, to a thread). It either runs a default action (terminate, stop, ignore) or interrupts whatever code is running to call a user handler on the same stack. The same mechanism carries hardware faults (SIGSEGV), job-control events, timers (SIGALRM), child exit (SIGCHLD) and plain IPC notifications (SIGUSR1, SIGHUP for "reload").

**Why it was chosen.** Early Unix ran single-threaded processes on small machines. A software interrupt that reused the hardware-interrupt model was the cheapest way to report faults, let the terminal kill a process, and wake a process out of a blocking call. There was no event loop or descriptor-multiplexing primitive to deliver events to instead. Brown, looking back, calls the original design simple and elegant but "fatally flawed": the one-shot handlers raced from the start, so the IPC/notification use was problematic even before threads arrived [S303]. Synchronous fault delivery (SIGSEGV, SIGFPE), by contrast, he describes as handled well by the original design [S303].

**Why it is a problem.** A handler can run in the middle of any instruction sequence, including inside `malloc` or stdio, so it may only call *async-signal-safe* functions. That list is short, differs between standards revisions, and implementations deviate (glibc `execl` was unsafe before 2.24; `aio_suspend` is unsafe) [S300]. Calling an unsafe function from a handler can corrupt memory or deadlock. A concrete case: OpenSSH's SIGALRM handler called `syslog()` and could corrupt the heap mid-`malloc`. This was CVE-2006-5051. It was fixed, then re-introduced in 2020 when a guard `#ifdef` was removed, giving unauthenticated remote root on glibc Linux (CVE-2024-6387, "regreSSHion") [S301]. The rest of the history is a series of patches, each exposing the next problem [S303]: one-shot handlers that had to re-arm themselves and could lose signals; `EINTR` from interrupted system calls; `sigaction` flags; `pselect`/`ppoll` to close wait races; real-time signals whose limited queues can still lose signals; and SIGCHLD/SIGCLD semantic differences. Threads made it worse, because delivery is now split between process-directed and thread-directed pending sets [S302].

**Why it was not fixed.** POSIX and every shell, daemon and runtime rely on signal semantics, including SIGKILL, SIGPIPE defaults and SIGHUP-to-reload conventions. Linux added `signalfd` (2.6.22, 2007) to read signals as descriptor events [S302], [S303]. But the signals must be blocked first. The blocked mask stays in force across `exec`, so helper programs (including ones spawned behind the scenes by libraries) do not see the signals they expect unless the parent unblocks them first, and synchronously generated signals such as SIGSEGV still need real handlers [S302], [S303]. The old path cannot be removed.

**Alternatives that exist.** Descriptor-based event delivery: `signalfd`, plus `pidfd` for child exit. Windows has no general asynchronous-signal mechanism for IPC [unverified as stated; see S339 for the console case]. Console control events run on a fresh thread the system creates, not by interrupting a running one [S339]. Microkernels (seL4) deliver asynchronous events as notification objects that a thread explicitly waits on [S305]. Faults then become IPC to a fault handler.

**Cost of the alternatives.** Something still has to kill an unresponsive process unconditionally. Every design keeps a non-catchable termination path. Synchronous faults (page fault, divide by zero) need some form of upcall or fault IPC, whose handler runs in a constrained context too. A per-thread fresh handler, as on Windows, moves the problem to ordinary thread synchronisation [S339].

**Relevance to our OS.** high — the POSIX layer must emulate signals for the C toolchain, but the native ABI can choose whether asynchronous interruption of user code exists at all.

**Sources.** [S300], [S301], [S302], [S303], [S305], [S339]

### DM-018: Mach IPC performance and the L4 response

- **Category:** Inter-process communication
- **Systems affected:** Mach 3.0, OSF/1 MK, MkLinux, GNU Hurd, XNU (Mach heritage), L4 family, seL4
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Mach 3.0 moved Unix out of the kernel into a user-level server, so every system service became an IPC through ports. Mach ports are kernel-buffered message queues, unlike later L4 endpoints [S305]; port rights act like capabilities and messages can carry out-of-line memory [unverified in cited sources]. Liedtke's L3/L4 redesigned IPC as minimal synchronous rendezvous between threads, tuned per processor [S304], [S305].

**Why it was chosen.** Mach aimed for multiprocessor and distributed systems, plus Unix compatibility. Asynchronous buffered ports with rich, network-transparent semantics fit that goal, and the generality looked worth the cost at the time.

**Why it is a problem.** By the early 1990s a one-way microkernel message cost about 100 µs. This led vendors to move services back into the kernel [S305]. Chen and Bershad measured Mach 3.0 plus its Unix server executing substantially more system instructions, at more memory cycles per instruction, than monolithic Ultrix [S307]. Some read this as inherent to microkernels. Liedtke reanalysed the same data. A best-case Mach kernel call took 18 µs on a 50 MHz 486 (about 900 cycles), of which roughly 800 cycles were pure kernel overhead, against a minimal 15 cycles in L3. He argued the cache misses came from Mach's large working set, not from the structure itself [S304]. With a minimal, processor-specific design, L4 IPC was 10–20× faster [S305]. L4Linux came within about 5% of native Linux on AIM, against roughly 5–7× larger penalties for MkLinux [S306].

**Why it was not fixed.** Mach's API was baked into its OS servers and into XNU, which kept Mach messaging but runs BSD in the same kernel address space [unverified in cited sources]; vendors generally responded to slow IPC by moving services back into the kernel [S305]. The L4 community also did not keep Liedtke's full design. Long IPC, IPC timeouts, and clans-and-chiefs were abandoned as complex or unusable. Thread-ID destinations were replaced by endpoint capabilities [S305].

**Alternatives that exist.** seL4/Fiasco.OC: synchronous endpoints with no kernel buffering, badged capabilities, and virtual message registers [S305]. Notification objects for asynchronous signalling. Shared memory for bulk data.

**Cost of the alternatives.** Purely synchronous IPC forces multithreaded servers. Before notifications, a server could not wait for both clients and interrupts. Synchronous rendezvous is "clearly the wrong way" to synchronise across cores [S305]. Processor-specific fast paths reduce portability [S304]. Classified as a tradeoff rather than contested: the cited primary sources agree that Mach's slowness came from its implementation and generality, not from user-level services as such [S304], [S305], and L4 paid for its speed with lost features that later had to be re-added (notifications) [S305]. Whether the remaining overhead of about 5% and the extra engineering are worth the isolation is the separate, genuinely contested monolithic-vs-microkernel question (DM-046).

**Relevance to our OS.** high — if services live in user space, IPC cost and semantics set the performance ceiling of everything else.

**Sources.** [S304], [S305], [S306], [S307]. [S308] unverified, not counted.

### DM-019: System V IPC: separate key namespace, objects outside the fd model

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX (XSI), Linux, BSDs, macOS, Solaris
- **Classification:** mistake
- **Confidence:** high

**What the design is.** System V message queues, semaphores and shared memory are named by integer `key_t` values [S310]. `*get` calls turn a key into an identifier used by the other calls [S309]. They are not file descriptors. They have their own `ipc_perm` permission bits, and they persist until explicitly removed [S311].

**Why it was chosen.** In the late 1970s and early 1980s the file system offered no shared memory and no counting semaphores. A separate subsystem let unrelated processes rendezvous without a common parent or a filesystem object, and integer keys were cheap.

**Why it is a problem.**
- Keys are a flat global namespace with no collision protection. `ftok` builds keys from inode and device bits and explicitly gives no uniqueness guarantee [S310].
- Objects are not reference-counted by process lifetime. Removal must be explicit, and a crashed program leaks segments whose pages stay in memory or swap [S311].
- Because they are not fds, they do not work with `poll`, descriptor passing, `close-on-exec`, or capability-style delegation.
- Limits are system-wide tunables (`SHMMAX`, `SEMMNI`, …) whose defaults and setting methods vary per OS; some BSD defaults are "unworkably small" and must be raised by hand [S313].
- Containers needed a whole new namespace type just to isolate these identifiers [S312].
- The lifetime model fights other policy. systemd's `RemoveIPC` deletes a user's objects at logout, which can make PostgreSQL "very unreliable" [S313].

**Why it was not fixed.** The interfaces are in the XSI part of POSIX and in long-lived software. They are kept for compatibility, while applications migrate away one by one. PostgreSQL 9.3 moved most of its shared memory to `mmap` specifically to avoid kernel-parameter tuning [S314].

**Alternatives that exist.** POSIX shared memory, message queues and semaphores, offered as alternative APIs for the same jobs [S309] (`shm_open` uses path-like names and returns an fd [unverified in cited sources]). Linux `memfd_create`: an anonymous, fd-backed, reference-counted memory object with seals so it can be shared safely with untrusted peers [S315]. Capability-system VMOs and handles (Zircon, seL4 frames), where there is no global name at all.

**Cost of the alternatives.** Without a global name, two unrelated processes need some broker or inherited channel to find each other. That is a bootstrapping and naming problem the design must solve elsewhere. Seals and fd passing add API surface [S315].

**Relevance to our OS.** medium — the native design should not repeat it, but the POSIX layer may need to emulate SysV shm/sem for ported software.

**Sources.** [S309], [S310], [S311], [S312], [S313], [S314], [S315]

### DM-020: Untyped byte streams as the only composition medium

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX shells and tools, Linux, BSDs, macOS; contrast PowerShell (Windows), Nushell
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Unix pipes, files and terminals carry undifferentiated bytes. Programs compose by agreeing informally on text formats such as lines and whitespace-separated fields. The kernel imposes no schema, and every consumer parses.

**Why it was chosen.** The original authors made this a deliberate style rule. Write programs whose output can become input to "another, as yet unknown, program", and avoid columnar or binary input formats [S316]. A byte stream is the lowest common denominator. It needs no shared type system across languages, it can be inspected by a human, and it keeps the kernel simple. Even Snover, the main critic, grants that the traditional model reflected the technology available when it emerged [S317].

**Why it is a problem.** Structure is reconstructed by parsing, which is lossy and fragile. Snover's critique: tools bind getting objects, processing them and rendering text together, so downstream tools do "prayer-based parsing" to recover the objects [S317]. Concrete failure modes: filenames containing newlines, spaces, leading dashes or control characters break line-oriented loops, `xargs`, and globs. Some of these are security bugs, such as CVE-2013-7085 in Debian's `uscan`, where a whitespace character in a filename let remote attackers delete arbitrary files (as listed by Wheeler [S318], a weak source). Output formats become de facto ABIs that cannot change without breaking scripts.

**Why it was not fixed.** Every tool reads and writes text, and the conventions are in POSIX. Partial fixes such as `-print0`/`-0` and NUL separators are spread unevenly [S318].

**Alternatives that exist.** PowerShell pipes .NET objects and uses reflection for generic `Where`/`Sort`/`Format` operations [S317]. Nushell passes structured values and tables between built-ins and handles JSON natively [S319].

**Cost of the alternatives.** Object pipelines bind to one runtime and type system (.NET for PowerShell). Once rendered to text, the objects are gone, so legacy commands take part only as text streams [S317]. Nushell gets structure largely by reimplementing tools as built-ins [S319]. The pro-text position, rooted in the original style rules [S316], values being language-agnostic, human-inspectable, and debuggable with ordinary tools (this characterisation is ours, not a quotation). The pro-structure position [S317] values precision and robustness. Both are held by experienced practitioners.

**Relevance to our OS.** medium — this choice shapes the native shell and tool ABI, and a typed IPC layer could carry schemas, but the hosted C toolchain will expect byte streams.

**Sources.** [S316], [S317], [S318], [S319]

### DM-021: Userspace message bus retrofitted (D-Bus; kdbus/bus1 rejected)

- **Category:** Inter-process communication
- **Systems affected:** Linux desktop and systemd stack (D-Bus); contrast Android (binder), macOS (Mach/XPC) [unverified for XPC]
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Linux has no kernel service-bus primitive. D-Bus is a user-space daemon that accepts connections over Unix sockets and routes method calls and signals between them. It adds well-known names, activation, a policy layer, fd passing (`UNIX_FD`), and peer-credential queries [S328].

**Why it was chosen.** Kernel developers prefer mechanism in the kernel and policy in user space. Unix sockets plus `SCM_RIGHTS` and `SO_PEERCRED` already provided the transport. A daemon can ship and evolve without kernel changes.

**Why it is a problem.** Each call crosses the daemon: two hops, extra copies, extra context switches. Proponents said reliable sender metadata cannot be obtained race-free from user space. The kdbus pull request (quoted in [S322]) claimed to fix "almost unfixable races" in D-Bus implementations. The credentials API kept growing (e.g. a ProcessFD field), a sign of retrofitting [S328].

**Why it was not fixed.** kdbus (2015) was never merged. Lutomirski NACKed it. His arguments: memfd over `SCM_RIGHTS` already handles bulk data; metadata could be a small Unix-socket extension; the metadata design broke CRIU; and pool buffering escaped memory-cgroup accounting [S320], [S321], [S324]. Biederman said the metadata-transfer code was wrong (quoted in [S322]). Torvalds was inclined to trust his maintainer but was "not AT ALL impressed" by the performance argument, which he thought pointed at user-space fixes [S324]. The merge was delayed for another cycle [S323] and never happened; LWN later called it "the sad story of kdbus" [S326]. bus1 (2016) offered capability-like handles, per-user quotas, global (causal) message ordering and multicast [S325]. It stalled, and resurfaced in 2026 stripped down to the basics and reimplemented in Rust [S326]. Meanwhile, the bus1 project produced dbus-broker, a user-space D-Bus broker aimed at high performance and reliability [S327].

**Alternatives that exist.** Android binder is in-tree, kernel-mediated IPC with object references, and it was accepted [S329], [S325]. Capability-based IPC with kernel-routed handles (seL4 endpoints, Zircon channels).

**Cost of the alternatives.** A kernel bus puts policy, quotas, ordering and accounting in the kernel, which is exactly where the kdbus review found problems [S321], [S324]. In LWN reader comments, a bus1 developer argues binder does not account resource usage and has no ordered multicast, and another commenter answers that binder is proven on billions of devices [S326] (comments, weak evidence; S325 confirms binder lacks multicast). The disagreement is about where a bus belongs, not only about implementation quality.

**Relevance to our OS.** high — we must decide early whether service discovery, naming and broadcast are kernel objects, a user-space broker, or capability-passing conventions.

**Sources.** [S320], [S321], [S322], [S323], [S324], [S325], [S326], [S327], [S328], [S329]

### DM-022: Descriptor passing over Unix sockets (`SCM_RIGHTS`): API shape and in-flight GC bugs

- **Category:** Inter-process communication
- **Systems affected:** Linux, BSDs, macOS, Unix/POSIX
- **Classification:** mistake
- **Confidence:** high

**What the design is.** A process sends open file descriptors as ancillary data (`cmsg`) on an `AF_UNIX` socket. The kernel keeps a reference while the message is queued, and the receiver gets new descriptor numbers [S334].

**Why it was chosen.** It retrofitted capability transfer (BSD-era [unverified for exact release]) onto the existing socket API without a new syscall family. That made privilege separation possible: a privileged process opens a resource and passes it to a sandboxed one.

**Why it is a problem.** The API is awkward and full of edge cases:
- On Linux stream sockets the fds must ride along with at least one byte of real data.
- There is at most one `SCM_RIGHTS` item per message, capped at 253 fds (`SCM_MAX_FD`; 255 before 2.6.38).
- A too-small control buffer silently truncates the fd list, signalled only by `MSG_CTRUNC`.
- Fds over the receiver's `RLIMIT_NOFILE` are automatically closed.
- Until Linux 4.5 the number of in-flight fds was unlimited [S334].

The deeper flaw is semantic. A socket can be sent over itself or over a peer, which creates reference cycles held only by in-flight messages, so Linux needs a garbage collector [S330]. That collector is a recurring source of races. CVE-2021-0920 is a use-after-free from a `MSG_PEEK` vs GC race. It was found and discussed on a public thread in 2016, but the suggested patch was not applied until it was re-reported in 2021; it was exploited in the wild from at least November 2020 to root Samsung devices, and CISA lists it as known-exploited [S330], [S331], [S333]. CVE-2024-26923 is a GC race against `connect()` [S332]. In 2024 the whole algorithm was replaced: each inflight AF_UNIX socket became a vertex in a directed graph, and the collector now finds dead strongly connected components [S335], [S336].

**Why it was not fixed.** Cycles are allowed by the API, so the kernel cannot remove the GC without breaking semantics. The ancillary-data format is POSIX and used by D-Bus, systemd, Wayland and others.

**Alternatives that exist.** Zircon channels: a message is bytes plus handles, transferred atomically. Handles are moved (discarded from the sender on success or failure) and need `ZX_RIGHT_TRANSFER`. A channel handle cannot be written into its own channel. Channels cannot be duplicated, and when the last handle to an endpoint closes, queued messages and the handles inside them are destroyed, possibly recursively [S337], [S338]. seL4 transfers capabilities in IPC.

**Cost of the alternatives.** Move semantics make sharing explicit (duplicate first), and rights checks add API surface [S337]. Whether move-only handles fully remove cycles (e.g. two channels each queued inside the other) is not addressed in the Zircon docs [S338] [unverified]. Any handle-in-message design must still decide this.

**Relevance to our OS.** high — handle transfer in IPC is central to a capability-leaning design, and cycle and lifetime rules must be designed in, not patched later.

**Sources.** [S330], [S331], [S332], [S333], [S334], [S335], [S336], [S337], [S338]

### DM-023: `select` → `poll` → `epoll` evolution; epoll's fd-vs-file-description semantics

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX (`select`, `poll`), Linux (`epoll`); contrast FreeBSD/macOS (`kqueue`), Solaris (`/dev/poll`, event ports)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `select` and `poll` are stateless: each call passes the whole interest set, the kernel scans it, and the result says which descriptors are ready. Linux `epoll` makes the interest set a kernel object. You add entries once with `epoll_ctl` and get ready events with `epoll_wait`. The key of an epoll registration is the pair of fd number and *open file description*, not the fd alone [S403].

**Why it was chosen.** `select` dates from early-1980s BSD, when a process had a few dozen descriptors and scanning a bitmap cost almost nothing. The stateless design adds no kernel state and is easy to reason about. When servers began holding thousands of idle connections, Banga, Mogul and Druschel measured `select` at nearly 25% of CPU in a proxy, even after optimising it. They proposed an explicit stateful interest-registration API [S400]. epoll (Linux 2.5.44, 2002) took that direction [S403]. Tying registrations to the file object is cheap to implement, because the kernel hooks the wait queue of the underlying file.

**Why it is a problem.** The `select`/`poll` design was reasonable for its time, but its cost grows with the number of watched descriptors, not with the number of events [S400][S401]. epoll's semantics have specific failure modes:
- Closing an fd does not remove the registration while another descriptor shares the description, whether from `dup`, `fork` or `SCM_RIGHTS`. The program then gets events for an fd number it has already closed, or that has been reused. After the close, `EPOLL_CTL_DEL` cannot remove the stale entry, and the only fix is to rebuild the epoll set [S403][S405].
- Libraries cannot fully defend against this, because their callers may close fds directly (Majkowski's blog [S405], weak; S403 makes the same point about fds duplicated behind the scenes by library functions).
- Epoll was not originally designed for balancing load across threads, as Majkowski argues [S404] (weak). Level-triggered mode wakes every waiting thread (thundering herd). Edge-triggered mode needs a drain-to-`EAGAIN` discipline, which can starve other descriptors [S403]. Later fixes were bolted on: `EPOLLONESHOT`, and `EPOLLEXCLUSIVE` in Linux 4.5 [S403][S404].

**Why it was not fixed.** Linux's no-regressions rule freezes the semantics, so every fix arrived as an opt-in flag. The man page now just documents the hazards [S403].

**Alternatives that exist.** kqueue (FreeBSD 4.1, 2000) applies registration changes and waits in a single call. It deletes knotes when the descriptor is closed, and it covers many event types besides fd readiness [S401][S402]. Lemon criticised the alternatives of the time. RT-signal queues overflow and deliver stale events for closed or reused fds. `/dev/poll` keeps returning `POLLNVAL` for closed fds and cannot itself be polled [S401]. Completion-based designs avoid readiness entirely; see DM-026.

**Cost of the alternatives.** Keying registrations to the descriptor means the close path must find and tear down registrations, which kqueue does through a per-descriptor knote array [S401]; that is extra bookkeeping on `close`. Stateful interest sets of any kind bring kernel memory, accounting and lifetime questions that `poll` never had.

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
- It supports few operations beyond read and write, and asynchronous `fsync` did not work at all in 2016 kernels (no filesystem implemented the method) [S406].
- Each I/O copies about 104 bytes of descriptors and needs at least two syscalls [S410].
- glibc's threaded POSIX AIO "scales poorly" [S408].

The result was a niche interface with "long term undiscovered bugs" [S410]. Applications and libraries kept private I/O offload thread pools instead [S410] (see DM-029). Torvalds called AIO "a horrible ad-hoc design" and refused to extend it one syscall at a time [S407].

**Why it was not fixed.** Extending AIO was tried several times: Axboe in 2010, then LaHaise's 2016 thread-based patches. Generic async-syscall proposals (fibrils, syslets, threadlets, acall) were never merged [S406][S410]. Axboe found that retrofitting AIO made the API "even more complicated" and fixed only one deficiency, so he started over with io_uring [S410]. The old ABI remains for compatibility.

**Alternatives that exist.** Windows overlapped I/O treats files, sockets, named pipes and mail slots alike: any handle that supports overlapped I/O can complete through the same mechanism, IOCP [S417] (that NT had it from its first release is not stated in the cited source). Linux io_uring (5.1, 2019) supports buffered I/O and arbitrary operations through shared rings (DM-025).

**Cost of the alternatives.** Making every operation asynchronous means every kernel path must handle cancellation and continuation. Linux's history suggests this is either slow (punting to kernel threads) or invasive. Torvalds, quoted in Axboe's slides, predicted io_uring would have "subtle and nasty bugs", including security bugs from racing with fd close [S411]. io_uring's security record (DM-025) bears that out.

**Relevance to our OS.** high. Native async I/O has to be part of the design from day one and cover all operations. Bolting it onto a synchronous VFS produces exactly this outcome.

**Sources.** [S406], [S407], [S408], [S409], [S410], [S411], [S417]

### DM-025: `io_uring`: performance win vs security attack surface

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Linux (5.1+); downstream: ChromeOS, Android, Docker/containerd, GKE
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** io_uring is a pair of submission and completion rings shared through `mmap` between the application and the kernel. It supports pre-registered files and buffers, kernel-side polling of the submission queue, and an extensible opcode field designed to carry many request types [S410]; the opcode set has since grown into something like a second syscall table (our characterisation). It was first posted on 8 January 2019 and merged on 8 March 2019 for 5.1-rc1 [S411].

**Why it was chosen.** Native AIO could not be fixed (DM-024). Fast NVMe devices made syscall overhead and per-I/O copies dominant, and Spectre/Meltdown mitigations made syscalls more expensive. Shared rings batch submissions and can avoid syscalls entirely [S410].

**Why it is a problem.** io_uring runs operations on behalf of the process in kernel worker contexts. That path bypasses syscall-level policy and touches large amounts of kernel code. In Google's kCTF VRP, 60% of exploit submissions targeted io_uring. Google disabled it on ChromeOS and its production servers, blocked it for Android apps with seccomp, and considered disabling it by default on GKE Autopilot [S220][S413]. seccomp has no visibility into operations requested through io_uring, so letting a seccomp-sandboxed program use io_uring "almost certainly" lets it bypass the sandbox [S416]. Before 2023 the only system-wide off switch was compiling io_uring out of the kernel. The patch adding a sysctl cited many unprivileged-exploitable bugs [S414]. The resulting `kernel.io_uring_disabled`/`io_uring_group` knobs [S415] are coarse.

**Why it was not fixed.** The fix is in progress, not refused. Per-ring restrictions (`IORING_REGISTER_RESTRICTIONS`) exist, but they cannot inspect arguments. Task-level restrictions and cBPF filters were still being reworked in 2026 [S416]. The deeper issue is structural: a second, asynchronous path to most of the kernel must replicate every check that the synchronous path enforces.

**Alternatives that exist.** Windows IOCP limits completion delivery to a fixed set of I/O operations, not arbitrary syscalls [S417]. A capability system could make the ring itself a capability that names exactly which objects and operations it may touch. Per-ring restrictions are a weak form of this [S416].

**Cost of the alternatives.** Disabling io_uring gives up large gains in IOPS and syscall reduction [S410]. A restricted op set means the "everything async" goal (DM-024) is only partly met. Filtering ring entries is hard to use: Axboe said the switch to classic BPF filters comes with "a bit of pain on the usability front" [S416].

**Relevance to our OS.** high. If our native ABI is ring-based, policy enforcement (capabilities or a seccomp equivalent) has to apply to each ring entry, not to the ring-entry syscall. That needs to be designed in, not added later.

**Sources.** [S410], [S411], [S220], [S413], [S414], [S415], [S416], [S417]

### DM-026: Readiness-based vs completion-based I/O models

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix family (`select`/`poll`/`epoll`/`kqueue`: readiness), Windows NT (overlapped I/O and IOCP: completion), Linux io_uring (completion); runtimes such as libuv, Rust async
- **Classification:** contested
- **Confidence:** medium

**What the design is.** A readiness API tells you that an operation would not block now, and you then perform it with an ordinary call. A completion API takes the operation and buffer up front and later reports that it has finished [S417][S410].

**Why it was chosen.** Readiness grew out of Unix's synchronous-call model. It needs no change to `read`/`write`, and the kernel never holds user buffers. NT built asynchronous I/O into its I/O manager from the start, with IOCP adding a scheduler-aware thread pool. IOCP releases waiting threads in LIFO order and caps the number running at a set concurrency value [S417].

**Why it is a problem.** The two camps disagree about which model is the problem.
- *Pro-completion position.* Readiness cannot express disk I/O (DM-029). It needs at least two syscalls per operation, and portable runtimes end up with one design per OS: libuv runs epoll, kqueue, event ports and IOCP behind one interface [S418]. Completion covers files and sockets uniformly [S417][S410].
- *Pro-readiness position.* Classic completion commits a buffer to every pending read, which costs memory with many idle connections (the problem io_uring's provided buffers were built to address [S420]). It also transfers buffer ownership to the kernel. That makes cancellation hard: dropping a request does not stop the kernel writing into the buffer, and in Rust this clashes with borrowed-buffer APIs like `AsyncRead` [S419]. Software written for readiness can find IOCP impractical when it is designed to be cross-platform; wepoll exists to emulate epoll on Windows for that reason [S421] (README, weak).

**Why it was not fixed.** Each ecosystem's ABI froze its model, so portable runtimes absorb the mismatch [S418][S421].

**Alternatives that exist.** io_uring narrows the gap from the completion side. Provided buffer rings let the kernel pick a buffer only when data arrives, so more reads can be in flight than there are buffers [S420]. io_uring also offers poll-style readiness operations [S410]. kqueue is readiness-based but more general [S402].

**Cost of the alternatives.** Completion models need explicit cancellation semantics and kernel-owned or pinned buffers [S419], and their per-operation kernel paths add attack surface (DM-025). Readiness models leave file I/O to thread pools [S418].

**Relevance to our OS.** high. This is a foundational ABI choice. A capability-style kernel with message passing leans toward completion, while the POSIX compatibility layer needs readiness semantics, so whichever model is native must be able to emulate the other.

**Sources.** [S402], [S410], [S417], [S418], [S419], [S420], [S421]

### DM-027: `EINTR` and restartable system calls ("PC loser-ing")

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; language runtimes on top (Go, Python, Rust std)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** When a signal arrives during a blocking "slow" syscall, Unix runs the handler and then either restarts the call (`SA_RESTART`) or fails it with `EINTR`, leaving the caller to retry. Which calls restart depends on the interface. `poll`, `epoll_wait`, `select`, `nanosleep`, System V IPC and sockets with timeouts never restart, whatever `SA_RESTART` says [S116].

**Why it was chosen.** Gabriel's essay records this as the archetype of "worse is better." Unix did not back the PC out so the interrupted call could be transparently re-entered, as MIT's ITS did. It returned an error and let the user program retry, because "implementation simplicity was more important than interface simplicity" [S422]. On a small PDP-11 kernel, unwinding to the syscall boundary was the simplest way to deliver a signal promptly. It was a conscious tradeoff that the MIT side disputed at the time [S422], and the cost of the alternative (clean unwinding or re-entry on every blocking path [S427]) still applies today, so this is a tradeoff rather than a decision whose constraints have since disappeared.

**Why it is a problem.** The cost moves to every call site, forever.
- The restart rules form a per-call table that few programmers know [S116].
- Linux adds a non-POSIX case: some calls return `EINTR` after `SIGSTOP`/`SIGCONT` even with no handler installed [S116].
- Retrying requires accounting for partial transfers.
- A runtime change can expose latent bugs everywhere. Go 1.14's signal-based preemption meant that programs using the low-level `syscall` packages would "see more slow system calls fail with `EINTR`" [S424]. Writes to cgroupfs began failing even though Go installs handlers with `SA_RESTART` [S425], and the Go team judged the fix "much too invasive and risky" to backport [S426].

**Why it was not fixed.** The semantics are part of POSIX and of every application's error handling. Changing them would break programs that rely on `EINTR` to interrupt a blocking wait, the main legitimate use.

**Alternatives that exist.** With `SA_RESTART`, the kernel already restarts many calls transparently, which is close to the MIT approach, but only for some calls [S116]. Linux's internal restart-block mechanism, which resumes sleeps with the remaining time, goes further [unverified]. In Windows NT, a user-mode APC runs only when its thread enters an alertable wait (`SleepEx`, `WaitForSingleObjectEx` and similar), and the wait then returns with a status saying APCs ran [S2100]. Cancelling I/O is an explicit operation on the request: IRPs are cancelled through `IoCancelIrp` or when the issuing thread exits [S430]. Signal-free designs remove the problem entirely by delivering events as messages (DM-130).

**Cost of the alternatives.** Transparent restart needs every blocking kernel path to be safely re-entrant or to unwind cleanly. Linux kernel developers have noted that interruptible sleeps require cleanup code on every early-return path [S427]. Explicit cancellation needs cancel routines in every driver that may queue requests indefinitely [S430].

**Relevance to our OS.** high. If we have no asynchronous signal upcalls, `EINTR` disappears natively and lives only in the POSIX layer, but we still need a uniform cancellation mechanism for blocking operations.

**Sources.** [S422], [S116], [S424], [S425], [S426], [S427], [S430], [S2100]

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

**Cost of the alternatives.** Every driver that may hold requests indefinitely needs cancellation logic [S430]; Microsoft now steers drivers to cancel-safe queues or KMDF to abstract it away, which suggests hand-written cancel routines were hard to get right [S430] (inference). Abandoning an in-flight request may leave a device or remote server in an unknown state. That is why Linux NFS prefers "hard" and warns about "soft" [S428].

**Relevance to our OS.** medium. With drivers and filesystems in user space, a process killer can reclaim a client without asking the server, but we must define what happens to its in-flight requests.

**Sources.** [S427], [S428], [S429], [S430]

### DM-029: Regular files always "ready"; no non-blocking disk I/O in the readiness model

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** POSIX, Linux, BSDs, macOS; event-loop runtimes (libuv/Node.js, Samba)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** POSIX requires regular files to "always select true" for reading, writing and error [S431]. `O_NONBLOCK` has no effect on buffered file reads. A read of uncached data blocks the thread until the page cache is filled [S432].

**Why it was chosen.** Disk access was treated as "fast": bounded and not dependent on a remote party, unlike terminals, pipes and sockets (the same assumption behind uninterruptible disk waits [S427]; the original rationale for `select` is not documented in the cited sources). "Ready" for a file has no clear meaning, because any byte may or may not be cached and readiness cannot be tracked per byte range. Making file reads always ready kept `select` simple.

**Why it is a problem.** A single-threaded event loop stalls whenever it touches an uncached file, a slow disk or a network filesystem. Every major runtime therefore keeps a thread pool for file I/O. libuv states plainly that no platform file I/O primitives were usable, so it uses a limited-size pool [S418]. Samba's developers wanted to try a non-blocking read first and fall back to threads only on a cache miss [S432], because thread offload adds communication overhead even when the data is already cached [S432]. Retrofits are hard:
- `O_NONBLOCK` could not be given meaning for files, because programs like Samba and squid set it and rely on it being ignored [S432].
- Checking cache residency first with a proposed `fincore()` is indirect and racy, since the cache can change between the check and the read [S432].
- The eventual `RWF_NOWAIT` (Linux 4.14) is meaningful only for `preadv2` reads, and Linux 5.9 and 5.10 had a bug where it could return 0 before EOF [S433].

**Why it was not fixed.** The POSIX text and decades of applications fix the semantics [S431][S432]. Linux took the route of new syscalls and flags, and then of io_uring.

**Alternatives that exist.** Windows overlapped I/O treats files and sockets alike [S417]. io_uring does buffered file I/O asynchronously [S410]. FreeBSD kqueue's vnode read filter reports the distance to EOF instead of firing unconditionally as `poll` does [S402], although that does not make the reads themselves non-blocking.

**Cost of the alternatives.** Completion-based file I/O needs the whole storage stack to be asynchronous or to punt to kernel workers (DM-024, DM-025). A readiness signal for files is ill-defined, because readiness depends on the offset and on cache state.

**Relevance to our OS.** high. Our filesystem will be a service from the start, so file and network I/O can share one asynchronous model. The POSIX layer then only has to emulate "always ready".

**Sources.** [S402], [S410], [S417], [S418], [S427], [S431], [S432], [S433]

### DM-030: Memory overcommit and the OOM killer

- **Category:** Memory management
- **Systems affected:** Linux (default heuristic overcommit), other Unix-likes to varying degrees; Android (userspace killers); contrast Windows NT (strict commit accounting)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Linux by default lets processes reserve more virtual memory than RAM plus swap can back. Only "obviously" excessive requests are refused (mode 0). Mode 1 always overcommits, and mode 2 enforces a commit limit of swap plus a share of RAM [S500]. When memory actually runs out at page-fault time, there is no allocation call left to fail, so the kernel's OOM killer picks a victim process by a heuristic "badness" score and kills it [S501].

**Why it was chosen.** Much allocated memory is never touched: sparse arrays, large reserved-but-unused heaps, and copy-on-write pages after `fork()`. Overcommitting uses RAM more efficiently and needs less swap [S500][S501]. Fork-heavy Unix workloads make this pressure worse. Baumann et al. argue that fork encourages overcommit: a conservative implementation must account for a worst case of double the process's virtual size, and failing fork for that reason is undesirable, so Linux overcommits by default [S100].

**Why it is a problem.** It moves allocation failure from a call site, where the program can handle it, to an arbitrary later memory access, where it cannot. Victim selection is a heuristic based mainly on memory size, adjusted for run time, niceness and privilege, and the kernel grew years of knobs and proposals to steer it (oom_adj, a cgroup OOM controller, name-based policies), several of which drew objections [S501]. LWN's 2009 review concluded that killing by kernel heuristics is not optimal [S501]. Forking servers are prime victims because half of each non-sharing child's memory is added to the parent's score [S501]. Even `MAP_NORESERVE` and similar promises carry no guarantee: the mmap man page's BUGS section says any process can be killed when memory runs out [S509]. Android moved the kill decision from an in-kernel low-memory killer to the userspace lmkd daemon driven by PSI pressure monitors [S506], and PSI was designed so userspace can see pressure before latency spikes and OOM kills [S504].

**Why it was not fixed.** Strict mode 2 exists [S500], but it is not the default. Software written against overcommit (huge virtual reservations, fork of large processes) would fail under strict mode. The fix that actually happened was incremental: better heuristics, then userspace killers using PSI [S504][S506].

**Alternatives that exist.** Windows NT charges every committed page against a system commit limit equal to RAM plus page files. When the charge reaches the limit, allocations fail, and system-managed page files can grow [S502]. Linux mode 2 does the same [S500].

**Cost of the alternatives.** Strict commit needs backing store for memory that may never be used. Microsoft warns that hitting the commit limit causes freezes, crashes and malfunctions, so commit exhaustion is still a failure mode, just a different one [S502]. Programs must handle allocation failure everywhere, and many do not.

**Relevance to our OS.** high — the commit policy determines whether "allocation failed" is a value returned to the caller or a kill delivered asynchronously, and that shapes every API above it, including the POSIX layer's `fork`.

**Sources.** [S100], [S500], [S501], [S502], [S504], [S506], [S509]

