# Stream I: Held up well (GR-001 to GR-020, reserve GR-021 to GR-029)

### GR-001: File descriptors as a uniform handle abstraction

- **Category:** Kernel–user interfaces and ABI design
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; descendants include Plan 9 and Zircon handles
- **Classification:** held-up-with-caveats
- **Confidence:** high

**What the design is.** `open`/`creat`/`pipe` return a small integer that indexes a per-process table of open objects. The same `read`/`write`/`close` calls work on ordinary files, devices ("special files" under `/dev`) and pipes. Descriptors are inherited across `fork` and `exec`, which is how a shell sets up the standard streams 0 and 1 [S900].

**Why it was chosen.** Ritchie and Thompson list "compatible file, device, and inter-process I/O" among the main features of the 1974 system. The goal was a small system: under two person-years of effort, on a $40,000 PDP-11. With one handle type and one small call set, a program like `pr` doesn't need to know whether its output goes to a file, a terminal or a pipe [S900].

**Why it held up.** The abstraction has lasted 50 years, and Linux keeps extending it rather than replacing it. Processes were the main object not yet represented by a descriptor, and `pidfd` (Linux 5.3, 2019) closed that gap. It fixed PID-reuse races, and pidfds can be polled together with other descriptors [S901]. Plan 9 pushed the same idea further. Windows (GR-003) and Zircon (GR-008) arrived at the same pattern independently: an unforgeable per-process index into a kernel table of objects [S903][S911].

**Where it falls short.** (1) The uniform call set is too narrow. Anything that doesn't fit read/write goes through `ioctl`, so uniformity at the API level turns into untyped per-device protocols (see DM-053). (2) A Unix fd has no rights attached beyond its open mode. It can't be weakened to fewer operations, and Capsicum had to add this later (see GR-008). (3) Inheritance by default leaks descriptors into children (DM-002). (4) The fd and the open file description it points to are different things, and APIs that key on one or the other (POSIX locks, epoll) cause bugs (DM-023, DM-036). (5) Integer slots are reused right after `close`, which makes double-close and use-after-close races possible. Zircon also documents that handle values are reused [S911]. (6) Plan 9's own paper admits that process creation and shared memory did not fit the file model [S903].

**Alternatives that exist.** NT handles carry an access mask fixed when the handle is opened (GR-003). Zircon handles carry rights that can be reduced on duplication [S911]. Capsicum adds rights to Unix fds after the fact.

**Cost of the alternatives.** Typed handles with rights need a rights vocabulary for each object type, and every API has to check it. Plan 9 shows that forcing everything into one generic interface also has limits [S903].

**Relevance to our OS.** high. The native handle type is the base of the whole syscall ABI. The main open question is whether handles carry rights and a type tag from the start, not whether to have handles at all.

**Sources.** [S900], [S901], [S903], [S911]

### GR-002: Pipes and composition of small programs

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS, Windows (cmd/PowerShell), Plan 9
- **Classification:** held-up-with-caveats
- **Confidence:** high

**What the design is.** `pipe()` returns descriptors for a kernel buffer. A reader blocks until a writer writes, and "neither process need know that a pipe ... is involved" [S900]. The shell's `|` notation runs commands at the same time, with each command's standard output connected to the next command's standard input [S900].

**Why it was chosen.** The 1974 paper argues the case directly. Without redirection and pipes, `ls` would need its own pagination and multi-column printing options. The paper calls it "unwise for efficiency reasons" to expect every command author to provide every output option [S900]. Pipes let small single-purpose tools combine, and they replaced temporary files.

**Why it held up.** Every mainstream OS shell has adopted the pipeline. The pattern also extends beyond shells: build systems, compiler drivers (preprocessor to compiler to assembler) and `ssh`-based remote composition all rely on the fact that a program doesn't know whether it is talking to a file, a terminal or another process [S900]. The mechanism is cheap, needs no naming or registry, and its lifetime is tied to its descriptors.

**Where it falls short.** (1) The original authors noted that a pipe "must be set up by a common ancestor" of both processes, so it is not a general IPC mechanism [S900]. Named pipes and sockets were added later to cover this. (2) The medium is untyped bytes. Every consumer re-parses text, and in-band framing such as newlines in filenames breaks pipelines. This is DM-020, a contested entry, and PowerShell's object pipelines are the counter-design. (3) Pipes carry only one direction and one stream. There is no structured error channel besides stderr and exit status, and no backpressure signal other than blocking. (4) A writer whose reader has gone away gets `SIGPIPE` by default (DM-077). (5) Pipes carry data, not authority. Passing descriptors needs a different mechanism (`SCM_RIGHTS`, DM-022).

**Alternatives that exist.** Plan 9 builds richer composition from per-process namespaces and file servers (GR-004) [S903]. Typed or object pipelines exist: in PowerShell, cmdlets pass .NET objects down the pipeline, not text [S953]. Zircon channels carry both bytes and handles [S911].

**Cost of the alternatives.** Typed pipelines require every tool to agree on a schema or object model. Universal composability with text-based tools is lost, or has to be recovered through serializers.

**Relevance to our OS.** high. The sandboxed POSIX layer has to provide byte pipes for the C toolchain in any case. The native IPC primitive could carry handles and framing as well, so a byte pipe would be one use of it.

**Sources.** [S900], [S903], [S911], [S953]

### GR-003: NT handles and the object manager

- **Category:** Security, authority and identity
- **Systems affected:** Windows NT and successors
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** All kernel resources are objects managed by one component, the object manager: files, devices, processes, threads, events, mutexes, sections, registry keys, jobs, tokens and symbolic links. Each object has a common header (name, type) and a type-specific body. The object manager provides creation and destruction, a namespace, per-process resource tracking, access-rights tracking and lifetime management [S905]. Programs never touch objects directly. They hold handles stored in a per-process table [S904].

**Why it was chosen.** Microsoft's own documentation gives two reasons. Objects let system functionality change behind a stable interface, and they let Windows security apply uniformly: each object has an ACL, which the system checks "each time an application creates a handle" [S904]. The caller requests an access mask when it opens the object, and later operations are allowed only if that right was granted [S906].

**Why it held up.** One mechanism covers more than 25 object types [S905], so waiting, duplication, inheritance, security descriptors and auditing work the same way for each. Checking access at open time and storing the granted mask in the handle works like a capability: once a handle is held, it can be passed to a less-privileged process, which can use exactly the granted rights. Process creation accepts an explicit list of handles to inherit (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, Vista and later). It also accepts mitigation, job and AppContainer attributes, so a sandboxed child can be built without inheriting ambient state [S908]. This model has run in NT unchanged in its basic shape since 1993 [unverified exact date].

**Where it falls short.** (1) Handles coexist with ambient authority. Any process can open a named object if its token passes the ACL check, so this is not a pure capability system (see DM-009, DM-016). (2) The object namespace has symbolic links that low-privilege users can create. These enabled a class of "resource planting" attacks in which a sandboxed process redirects a privileged service. Microsoft's 2015 fix marks links created from a sandbox and refuses to follow them from outside, at a small per-lookup cost [S907]. (3) User32/GDI objects are a separate handle category outside the kernel object manager (see DM-081). (4) The ACL and token model on top of handles is complex (DM-016).

**Alternatives that exist.** Zircon handles also carry rights but have no global ambient namespace. Rights can be reduced with `zx_handle_duplicate` or `zx_handle_replace`, and transfer over a channel moves the handle out of the sender [S911]. Unix fds have no rights mask (GR-001).

**Cost of the alternatives.** Removing the ambient namespace means every resource must be passed explicitly. That makes POSIX-style "open by path" programs harder to port (see GR-008).

**Relevance to our OS.** high. NT shows that a single object/handle layer with rights checked at open time works at scale in a commercial OS. Its symlink history shows the risk of keeping a writable global namespace beside it.

**Sources.** [S904], [S905], [S906], [S907], [S908], [S911]

### GR-004: Plan 9 per-process namespaces

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Plan 9, Inferno; partial descendants: Linux mount namespaces, FUSE, 9P in WSL/QEMU
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** Every resource is presented as a file tree. Each process builds its own view with `mount` (attach a server's tree) and `bind` (make an existing file or directory appear somewhere else). Directories can be unions searched in a set order. `rfork` flags decide whether a child shares or copies its parent's namespace [S903].

**Why it was chosen.** Plan 9 was built as a distributed system for networked terminals and CPU servers. Private namespaces let each window under 8½ have its own `/dev/cons` and `/dev/bitblt`, with no special kernel support for terminals. `/bin` becomes a union of per-architecture and private binaries, replacing `$PATH` [S903].

**Why it held up.** The paper documents real uses. A window system ran with no special kernel support (X run as a client of 8½ remotely was measured at about a 10% graphics slowdown). Networking used `/net` clone/ctl/data files. `import` mounted a remote machine's `/proc` for debugging or used its TCP stack as a gateway. `cpu` rebuilt the terminal's namespace on a remote server [S903]. As a model, the idea outlasted the system: per-process mount namespaces are the base of Linux containers, and 9P is still used as a host/guest file protocol [unverified for specific products].

**Where it falls short.** (1) Plan 9 itself never achieved wide adoption, so evidence at scale comes from descendants that took only part of the design. (2) The paper lists limits: process creation, network addressing and authentication, and shared memory did not map cleanly onto files [S903]. (3) With no global namespace, programs depend on shared naming conventions [S903]. (4) Unprivileged namespace construction is safe only if there are no setuid-style programs that trust paths. Linux's retrofit of user and mount namespaces has produced privilege-escalation bugs (see DM-041). (5) Everything-as-a-file pushes typed operations into ad hoc text written to ctl files (compare DM-054).

**Alternatives that exist.** A single global tree (classic Unix, DM-041). Explicit capability-only resource passing without paths (Zircon component namespaces, built from handles passed at process creation [S910]).

**Cost of the alternatives.** A global tree gives ambient authority by path. Capability-only passing needs every program to be launched with the right set of handles.

**Relevance to our OS.** high. A per-process namespace built from handles is a natural way to present POSIX paths to the C compiler while confining what it can reach.

**Sources.** [S903], [S910]

### GR-005: Explicit process creation (`CreateProcess`, `posix_spawn`, Zircon launcher)

- **Category:** Process model and program execution
- **Systems affected:** Windows NT, POSIX (`posix_spawn`), Fuchsia/Zircon; contrast: Unix `fork`+`exec`
- **Classification:** held-up-well
- **Confidence:** high

**What the design is.** A new process is created from an executable together with an explicit description of its initial state: arguments, environment, handles or descriptors, and security attributes. The parent's address space is not cloned. Windows `CreateProcess` takes an extensible attribute list [S908]. POSIX `posix_spawn` takes file actions and attributes [S909]. Fuchsia's `fuchsia.process.Launcher` (used through `fdio_spawn`) requires the caller to supply the job, the executable and the startup handles [S910].

**Why it was chosen.** The `posix_spawn` rationale says `fork` is hard or impossible to implement efficiently without an MMU or swapping, and is too slow for realtime use [S909]. Baumann et al. argue more broadly that `fork` was "a clever hack" suited to the 1970s. They say it is not thread-safe, does not compose, inherits everything by default (insecure), and encourages overcommit [S902].

**Why it held up.** Measured: posix_spawn takes about 0.5 ms regardless of the parent's size, while fork+exec cost grows with the parent's memory [S902]. Major forking applications (Apache, Chrome, PostgreSQL) have Windows ports that don't fork, so fork "is clearly not essential" [S902]. Windows has used CreateProcess since NT. Its attribute list has gained parent override, handle allow-lists, mitigation policies, job lists and AppContainer capabilities without breaking existing callers [S908]. That is evidence the explicit design extends well. Fuchsia shipped with no fork at all [S910].

**Where it falls short.** (1) posix_spawn is not a complete replacement. Some operations can't be expressed, such as setting terminal attributes or entering a namespace [S902][S909]. (2) Errors in the child before exec are reported poorly, for example as exit status 127 [S909][S902]. (3) Anything the API does not support must be set up in the parent, which "is costly and complicated" in multithreaded programs [S909]. (4) An all-in-one call needs an open-ended parameter set. Windows needed STARTUPINFOEX attribute lists to stay extensible [S908]. (5) Use cases of fork without exec, such as snapshots (Redis) and fuzzing, need a separate copy-on-write API [S902].

**Alternatives that exist.** Baumann et al. propose a high-level spawn plus low-level cross-process operations: create an empty process, then map memory and install handles into it from outside [S902]. Zircon follows this pattern under its launcher service [S910].

**Cost of the alternatives.** Porting fork-heavy POSIX code (shells, `make`, `cc` driver invoking `cpp`/`as`/`ld` via `fork`+`exec`) needs emulation or refactoring. Cygwin's fork-on-CreateProcess is "neither fast nor reliable" [S902].

**Relevance to our OS.** high. The native primitive can be spawn-only. The POSIX layer then has to decide how far it emulates `fork` for the C toolchain. Mostly that means `vfork`/`posix_spawn`-shaped use, which is cheap to support.

**Sources.** [S902], [S908], [S909], [S910]

### GR-006: Hierarchical filesystem namespace

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Multics, Unix/POSIX, Linux, BSDs, Windows NT, macOS, Plan 9 — essentially all general-purpose OSes
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** Files are organized in a rooted tree of directories and named by a path from the root or from a working directory. Multics (1965) introduced the tree, path names relative to a working directory, links layered on the tree, and per-entry access control lists [S912]. Unix adopted the tree, with mountable subtrees on removable volumes [S900].

**Why it was chosen.** Daley and Neumann needed a way to organize secondary storage shared by many users. A tree gives each user a private subtree, while links allow sharing without copies [S912]. In Unix, mounting let the single tree span several devices without the programmer needing to know which device a file lives on [S900].

**Why it held up.** It has lasted about 60 years and is used by every mainstream OS. It also serves as the organizing idea for non-disk resources: Plan 9 file servers, `/proc`, NT's object namespace and URLs [S903][S905]. The most serious attempt to replace it in a mainstream OS failed. Microsoft's WinFS was cut from Vista and then, in 2006, dropped as a standalone product, with its pieces folded into SQL Server and ADO.NET [S914].

**Where it falls short.** (1) Seltzer and Murphy argue that the hierarchy is "a rudimentary attempt at simple organization". With libraries of hundreds of gigabytes, users find data by attributes and search, not by path, and applications have each built their own tagging systems on top of the filesystem [S913]. (2) Daley and Neumann already observed that links turn the tree into a graph that can contain loops [S912]. Hard links and symlinks remain sources of security and semantic problems (DM-039, DM-040). (3) A single global tree is a source of ambient authority: anything nameable by path can be attempted (DM-009, DM-041). (4) Mobile OSes keep a tree internally but hide it from users behind per-app containers and media libraries [unverified as a general claim].

**Alternatives that exist.** Tag- and search-based namespaces (Seltzer and Murphy's hFAD proposal [S913]; BeFS indexed attributes [unverified]). Relational stores (WinFS [S914]). Per-process trees (GR-004).

**Cost of the alternatives.** WinFS is the main data point, and it did not ship [S914]. Search-based naming still needs stable identifiers for programs, so tree paths, or object IDs that programs treat like paths, keep coming back.

**Relevance to our OS.** medium. A tree is the obvious choice for the POSIX layer and the C compiler. The open questions are whether the native layer exposes one global tree at all, and whether links are allowed.

**Sources.** [S900], [S903], [S905], [S912], [S913], [S914]

### GR-007: Demand-paged virtual memory and `mmap` as a unified mechanism

- **Category:** Memory management
- **Systems affected:** Multics, Unix (SunOS/BSD `mmap`), Linux, Windows NT (sections), macOS/Mach (memory objects), Zircon (VMOs)
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** Each process has its own virtual address space backed by fixed-size pages, which are loaded only when touched and evicted under memory pressure. The same mechanism maps files, program images, shared libraries and anonymous memory. Multics went furthest. All online information was directly addressable as segments, and "the Multics user no longer uses files": data was referenced in place, without copying through I/O buffers [S915].

**Why it was chosen.** Multics (1969/1972) used fixed 1,024-word pages to simplify allocation and avoid compaction. It replaced pages using hardware "used" bits, and shared segments between processes by pointing their descriptors at the same page table, so no copying was needed [S915]. One mechanism provides isolation, sharing and lazy loading.

**Why it held up.** Every mainstream OS today uses demand-paged virtual memory, and it underpins nearly everything else: sharing code pages between processes, copy-on-write, memory-mapped executables and per-process isolation (GR-019). Each successor kept the unification of file mapping and memory: NT sections, Mach memory objects, Zircon VMOs [unverified for the last three, by design familiarity].

**Where it falls short.** The places where it falls short are mostly where "a file is just memory" is pushed too far. Crotty, Leis and Pavlo show that using mmap in place of a DBMS buffer pool causes four problems [S916]:
- Transactional safety is lost, because the OS can write dirty pages back at any time.
- Any memory access can turn into a blocking I/O stall that the program cannot schedule.
- I/O errors arrive as `SIGBUS` instead of return codes.
- TLB shootdowns during eviction limit scalability. Their measurements show mmap reaching far below the bandwidth of modern flash devices [S916].

Overcommit and the OOM killer (DM-030) and mmap truncation hazards (DM-031) are also consequences of treating memory as an abstraction the kernel can over-promise.

**Alternatives that exist.** Explicit buffer management in user space (DBMS buffer pools; `O_DIRECT`/io_uring) [S916]. Single-address-space systems that rely on language safety instead of page-table isolation (Singularity). Spectre undermines them, because speculation defeats language-enforced confidentiality within one address space [S963] (see GR-019).

**Cost of the alternatives.** Explicit I/O makes each application implement its own caching and eviction. Dropping page-table isolation gives up the strongest isolation boundary hardware provides.

**Relevance to our OS.** high. Paged VM is a given on aarch64. The design choices are whether file mapping is a first-class handle (a VMO-like object), how mapping errors are reported, and whether memory can be overcommitted.

**Sources.** [S915], [S916], [S963]

### GR-008: Object capabilities (KeyKOS/EROS, seL4, Zircon handles, Capsicum)

- **Category:** Security, authority and identity
- **Systems affected:** KeyKOS/EROS/CapROS, seL4, Fuchsia/Zircon, FreeBSD Capsicum; partial: NT handles
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Authority is carried by unforgeable references (capabilities) to objects, and an operation is allowed only if the caller holds a capability with sufficient rights. In seL4 every operation needs a capability, capabilities live in kernel memory (CNodes), and kernel memory itself is handed out as Untyped capabilities that user code retypes and can revoke [S919]. Zircon handles carry rights that can be reduced on duplicate or replace, and handles move between processes over channels [S911]. Capsicum adds this to Unix: fds gain fine-grained rights, and `cap_enter` puts a process into capability mode, where global namespaces are no longer reachable [S917].

**Why it was chosen.** Ambient authority (DM-009) lets any compromised component use everything its user can do. Capabilities make least privilege and delegation explicit. Capsicum's authors argue that DAC and MAC were not designed to compartmentalize applications such as browsers [S917].

**Why it held up.** In practice:
- **Retrofit cost.** Capsicum showed that a retrofit can be cheap. tcpdump was sandboxed with about two lines plus rights narrowing, and Chromium got OS-backed sandboxing in about 100 lines, compared with much larger code in Chromium's other platform sandboxes [S917].
- **Verification.** seL4 shows the model can be verified formally and also performs well. Integrity and confidentiality proofs rest on the capability model [S919][S920].
- **Deployment.** Zircon builds its whole userspace on handles and ships in Google products [unverified for the deployment claim]. NT's open-time access masks (GR-003) are a widely deployed partial form.

**Where it falls short / why contested.** (1) Mainstream adoption is thin. Capsicum is in FreeBSD, but the Linux port (Google, 2013–2014) was never merged [unverified: based on the absence of upstream code and on secondary reports]. (2) Porting existing software is uneven. In capability mode tcpdump's DNS lookups failed, because the resolver needed global namespaces [S917]. Programs that open paths late, or everywhere, need to be restructured or given a proxy. (3) Revocation, auditing ("who can reach X?") and naming are harder than with ACLs. seL4 needed explicit revoke and derivation-tree machinery [S919]. (4) Handle-value reuse races remain [S911]. Supporters (seL4, Zircon, Capsicum authors) argue it is the only model that composes; mainstream OS vendors have kept ACLs plus sandboxes (seccomp, AppContainer). Hence `contested`.

**Alternatives that exist.** Identity-based DAC/MAC with syscall-filter sandboxes (DM-010, DM-014, DM-015). The NT hybrid of ACL-checked open plus a rights-carrying handle (GR-003).

**Cost of the alternatives.** Retrofitted sandboxes are policy-heavy and fragile. ACLs keep confused-deputy problems.

**Relevance to our OS.** high. This is the central decision for a "capability-leaning" OS. Capsicum's DNS example predicts exactly where the POSIX layer for the C compiler will run into trouble: anything that opens global paths after start-up.

**Sources.** [S911], [S917], [S919], [S920]

### GR-009: Completion-based I/O (IOCP, io_uring ring model)

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Windows NT (IOCP), Linux (io_uring since 5.1), also Solaris event ports [unverified]
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** The application submits operations and later collects completion records from a queue, instead of asking which descriptors are ready and then doing the I/O itself. Windows IOCP associates handles with a port. Completed overlapped operations queue packets that worker threads dequeue, and the port limits how many threads run at once [S922]. Linux io_uring places a submission ring and a completion ring in memory shared between the application and the kernel. Many requests can be batched into one `io_uring_enter`, or, with SQPOLL, submitted with no system call at all [S924][S923].

**Why it was chosen.** IOCP gave servers a way to handle many connections with about one thread per CPU. The port wakes the most recently blocked thread first (LIFO) to keep caches warm, while completions are delivered in FIFO order [S922]. Linux's older AIO was hard to use, inefficient, and weak for buffered I/O [S923]. System calls became more expensive after the Spectre/Meltdown mitigations, so batching through shared rings directly cuts per-request overhead [S924].

**Why it held up.** IOCP has been the standard high-performance I/O model on Windows for decades. Microsoft still recommends it for servers with thousands of connections, and its thread-pool API is built on it [S922]. It covers files, sockets, pipes and device control, which removes the "regular files are always ready" gap of readiness APIs (DM-029) [S922]. io_uring gives Linux the same completion model and extends it to most system calls [S923][S924].

**Where it falls short.** (1) **Security.** Google reported that 60% of kCTF exploit submissions in one year targeted io_uring, at a cost of about USD 1M in rewards. It was used in every submission that bypassed Google's mitigations. Google then disabled io_uring on ChromeOS, made it unreachable for Android apps, and was considering disabling it on GKE [S925] (see DM-025). The cause is a large asynchronous kernel surface reached through shared memory, not the completion model as such. (2) Buffer ownership is harder: buffers belong to the kernel until the operation completes, which complicates cancellation and memory-safe language bindings [unverified as a general claim]. (3) The completion and readiness models are hard to mix in one event loop (DM-026, contested).

**Alternatives that exist.** Readiness APIs (epoll/kqueue) (DM-023). Synchronous message passing to I/O servers (QNX, GR-018). Asynchronous channels plus ports (Zircon).

**Cost of the alternatives.** Readiness models need non-blocking variants for every operation and cannot cover disk I/O cleanly. Threads per request cost memory and context switches.

**Relevance to our OS.** high. A completion queue fits a Rust async runtime and a capability IPC kernel well. io_uring's record is a warning to keep the shared-memory interface small and verifiable.

**Sources.** [S922], [S923], [S924], [S925]

### GR-010: Linux "don't break userspace" ABI stability rule

- **Category:** Kernel–user interfaces and ABI design
- **Systems affected:** Linux; contrast: OpenBSD/macOS (libc as the stable boundary), Windows (stable Win32 above ntdll)
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** The kernel treats any change that makes working userspace software behave worse on a newer kernel as a bug, regardless of whether the software was "correct". The documentation calls "We don't cause regressions" the first rule of kernel development. It covers only interfaces to userland (including significant performance regressions) and excludes kernel-internal APIs and out-of-tree modules [S926].

**Why it was chosen.** Linux ships its syscall ABI as the public boundary, with many independent libcs and static binaries above it (DM-057). Without a firm rule, users could not upgrade kernels independently of their userspace. Torvalds' 2012 statement: "It's not about time. It's about not breaking user space." Compatibility shims stay until nobody uses the old versions [S927].

**Why it held up.** Users can upgrade the kernel without coordinating with distributions or applications, which underpins container images that run old userspaces on new kernels [unverified as a measured claim]. The rule is written down, has explicit exceptions (security fixes, rare single-user cases, staging drivers), and is enforced by the top maintainer [S926].

**Where it falls short.** (1) **Mistakes become permanent.** a.out support, unused "in any serious way for decades", resisted removal until 2022. Torvalds objected because of Alpha ECOFF, and a single Atari Jaguar toolchain user blocked removal until Kees Cook wrote an ELF wrapper [S928]. (2) **The scope is disputed.** Practitioners disagree about whether sysfs files, tracepoints and uevents are covered, and whether the rule holds up in practice [S929] (weak source: a comment thread). (3) **Security fixes override it** [S926]. Hardening such as disabling `TIOCSTI` (DM-072) or unprivileged BPF (GR-013) therefore arrives as config options and sysctls, not as clean removals. (4) It protects only the syscall boundary. The in-kernel driver API is deliberately unstable (DM-049).

**Alternatives that exist.** Keeping the stable ABI in a library (libc, ntdll/Win32) so syscalls can change (OpenBSD, macOS, Windows; see DM-057). Versioned interfaces with explicit deprecation windows [unverified as a specific system practice].

**Cost of the alternatives.** A library-level ABI breaks static binaries and alternative libcs. Deprecation windows break old software on schedule, which users experience as breakage.

**Relevance to our OS.** medium. We have no compatibility obligations yet. The lesson is to decide early where the stable boundary is, and to give interfaces version or extension points before anyone depends on them, because a strict no-regressions rule turns every early mistake into a permanent one.

**Sources.** [S926], [S927], [S928], [S929]

### GR-011: Futexes

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** Linux (since 2.5/2.6); analogues: Windows `WaitOnAddress` (Windows 8+), others
- **Classification:** held-up-with-caveats
- **Confidence:** high

**What the design is.** A futex is a 32-bit word in ordinary (possibly shared) memory. Lock state lives entirely in that word and is changed with atomic instructions in user space. The kernel is entered only when contention is detected, to put a waiter to sleep ("wait if the value is still X") or to wake waiters on that address [S930][S931].

**Why it was chosen.** Franke, Russell and Kirkwood (OLS 2002) wanted to replace heavyweight kernel locking such as System V semaphores and `fcntl` locks for enterprise workloads like databases and JVMs. In those workloads the syscall cost dominates when contention is low. They set as an explicit requirement that the uncontended case avoid system calls entirely [S930].

**Why it held up.** Futexes are the base of glibc's pthread mutexes, condition variables and semaphores, so in practice every threaded Linux program uses them [S931]. Windows adopted the same address-based wait/wake idea in Windows 8 with `WaitOnAddress`/`WakeByAddress*`, which supports 1-, 2-, 4- and 8-byte values [S933]. Because the kernel keeps no per-lock object, there is nothing to allocate or leak, and a lock can be placed anywhere in shared memory.

**Where it falls short.** (1) **Hard to use correctly.** Drepper's "Futexes Are Tricky" shows that the obvious mutex built on futexes has one performance bug and two correctness bugs. He notes that futexes caused problems "even to their inventors" [S931]. (2) **The multiplexed syscall became an attack surface.** The single `futex()` call grew requeue, priority-inheritance and robust-list operations. CVE-2014-3153, a `futex_requeue` bug that let local users gain privileges, was widely exploited [S932] (exploitation scale [unverified]). (3) **Missing features.** The API couldn't wait on several futexes at once or use other sizes. That led to the futex2/`futex_waitv` work (2021), motivated by game engines [S934]. (4) Spurious wakeups are part of the contract, so every caller must loop and re-check [S933].

**Alternatives that exist.** Kernel-object locks (System V semaphores, NT mutex objects): simpler, but they cost a syscall on every operation [S930]. Address-based waiting with typed sizes (`WaitOnAddress`) [S933]. Pure spinning in user space.

**Cost of the alternatives.** A syscall on every lock and unlock is the cost futexes were designed to remove [S930]. Kernel-object locks need handles and lifetime management.

**Relevance to our OS.** high. Rust's `std` synchronization primitives assume a futex-like wait/wake primitive, so the kernel will need one. The lessons are to keep it as separate small operations rather than one multiplexer, and to include multi-wait from the start.

**Sources.** [S930], [S931], [S932], [S933], [S934]

### GR-012: Read-copy-update (RCU)

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** Linux (since 2002), DYNIX/ptx (origin) [unverified]; user-space liburcu [unverified]
- **Classification:** held-up-well
- **Confidence:** high

**What the design is.** An update has two phases. First, *removal*: a pointer is swapped so that new readers see the new version. Second, *reclamation*: the old version is freed only after every reader that might hold it has finished, which is called a grace period. Readers take no locks and use no atomic read-modify-write operations. They only mark their critical sections (`rcu_read_lock`/`unlock`) and load pointers with `rcu_dereference`. Updaters wait with `synchronize_rcu()` or defer the free with `call_rcu()`/`kfree_rcu()` [S935].

**Why it was chosen.** Read-mostly kernel data structures (routing tables, dentry cache, module lists) were bottlenecked by reader-writer locks. Those locks write to shared cache lines even for readers, so they scale badly on many CPUs. RCU puts the whole cost on the update side [S935][S936].

**Why it held up.** By 2012 there were 6,258 RCU API uses in Linux, spread across networking, filesystems, security, virtualization and the core kernel [S936]. The same survey measures read-side overhead that is orders of magnitude below reader-writer locks. Converting code to RCU also removed whole classes of deadlock, because readers cannot block updaters [S936]. RCU has stayed in active use and development for over 20 years, with real-time and many-thousand-CPU requirements [S936].

**Where it falls short.** (1) It fits **read-mostly** data only. The kernel documentation says it is unsuitable for update-heavy workloads, and locking was still more than ten times as common in 2012 [S935][S936]. (2) **Memory is reclaimed late.** Freed memory waits for a grace period, and `call_rcu` floods need rate limiting to resist denial of service [S935]. (3) **Readers may see stale data** and cannot block inside a critical section without special variants (SRCU, RCU-Tasks) [S935]. (4) The rules are subtle: pointers are valid only inside the read section, and `SLAB_TYPESAFE_BY_RCU` guarantees only the type of an object, not its identity [S935]. (5) Production RCU is complex, far beyond the simple textbook implementations [S936].

**Alternatives that exist.** Reader-writer locks; hazard pointers; epoch-based reclamation (as in Rust's `crossbeam-epoch` [unverified]); per-CPU data.

**Cost of the alternatives.** Reader-writer locks cost scalability. Hazard pointers add a store and a fence for each pointer a reader takes.

**Relevance to our OS.** medium. RCU-like deferred reclamation is the established answer for scalable read-mostly kernel tables. In Rust, its lifetime rules can be partly encoded in types, which would address its main weakness, subtle misuse.

**Sources.** [S935], [S936]

### GR-013: Dynamic tracing (DTrace, eBPF)

- **Category:** Configuration, observability and system management
- **Systems affected:** Solaris/illumos, FreeBSD, macOS (DTrace); Linux (eBPF); Windows (DTrace port, eBPF-for-Windows) [unverified for Windows]
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** Instrumentation is inserted into a running production kernel and its applications on demand, then removed. User-supplied programs run at probe points under a safety check and aggregate data in the kernel. DTrace, from the 2004 paper, uses dynamic instrumentation, offers about 30,000 probes even on small systems, has "zero probe effect" when disabled, and guarantees safety when enabled [S937]. eBPF runs verified bytecode in kernel context. A verifier checks termination and memory bounds before the program is attached, and a JIT compiles it to native code [S938].

**Why it was chosen.** Cantrill et al. argued that performance problems show up in production, not on developers' machines. A tool that slows the system when idle, or that can crash it through misuse, cannot be used in production. Those two requirements shaped the whole design [S937].

**Why it held up.** DTrace spread from Solaris to FreeBSD and macOS [unverified for the specific ports], and its probe/aggregation model became the template that later tools followed. eBPF extended the model beyond tracing into networking and security enforcement [S938]. Being able to ask arbitrary questions of a live system without rebooting or deploying a debug build has become an expected feature of a modern OS [unverified as an adoption measure].

**Where it falls short.** (1) **The verifier is an attack surface.** In 2017 Jann Horn found eight eBPF verifier bugs, some giving arbitrary kernel read and write. The recommended mitigation was to disable unprivileged BPF [S939]. Upstream later made unprivileged BPF off by default, citing speculative-execution risks and noting that distros already did this [S940]. A safe-by-verification design therefore ended up root-only in practice. (2) The verifier checks safety, not intent [S938]. A privileged eBPF program can still observe secrets. (3) Probes on unstable internal functions break across kernel versions (see GR-010 scope disputes [S929]). (4) DTrace's safety came from a deliberately limited language (no loops). eBPF relaxed that limit and paid for it with verifier complexity [S937][S938].

**Alternatives that exist.** Static tracepoints only. Kernel debuggers. Loadable kernel modules (unsafe). Out-of-kernel tracing via hardware trace units.

**Cost of the alternatives.** Static-only probes miss unforeseen questions. Kernel modules give up the safety guarantee.

**Relevance to our OS.** medium. Observability is cheaper to design in than to retrofit. A small, non-Turing-complete probe language in the DTrace style may be safer than a general verifier, given eBPF's history.

**Sources.** [S937], [S938], [S939], [S940], [S929]

### GR-014: Checksummed copy-on-write storage (ZFS, Btrfs, APFS)

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** ZFS (Solaris/illumos/FreeBSD/Linux), Btrfs (Linux), APFS (Apple) [APFS details unverified]
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** Blocks are never overwritten in place. New data is written elsewhere, and the tree of block pointers is updated up to a new root. In ZFS each block's checksum and algorithm are stored in the parent's block pointer, so every read can be verified. If redundancy exists, corrupt blocks are repaired automatically, and periodic scrubs find latent corruption [S941]. Snapshots and clones become cheap because old roots remain valid.

**Why it was chosen.** Disks and their firmware corrupt data silently. A study of 1.53 million drives over 41 months found more than 400,000 checksum mismatches that the drives had not reported [S942]. A filesystem that trusts the device returns that corruption to applications. Copy-on-write also keeps the on-disk state consistent after a crash without `fsck`.

**Why it held up.** ZFS is "robust to a wide range of disk faults" under fault injection [S943]. Its checksum and self-healing design has stayed essentially the same in OpenZFS, and checksums also protect `zfs send/receive` streams [S941]. Checksummed or copy-on-write designs are now the default in several mainstream systems (Btrfs in some Linux distributions, APFS on Apple platforms [unverified]).

**Where it falls short.** (1) **Memory is not covered.** The same fault-injection study found ZFS "less resilient to memory corruption", which can return corrupt data or crash the system [S943]. End-to-end protection ends at the RAM boundary unless ECC is used. (2) **Overwrite-heavy workloads.** Databases and VM images fragment under CoW. Btrfs's escape hatch, `nodatacow`, also turns off data checksums and compression [S945], so the guarantee is lost exactly where those workloads need it. (3) **Implementation maturity.** Btrfs RAID5/6 is still labelled "unstable" and "do not use" other than for testing [S944]. (4) Write amplification and space accounting with snapshots are complex [unverified as a quantitative claim].

**Alternatives that exist.** Journaling filesystems with metadata-only checksums (ext4, XFS). Block-layer integrity (dm-integrity, T10 PI). Application-level checksums (databases).

**Cost of the alternatives.** Journaling doesn't detect silent data corruption [S942]. Block-layer integrity can't tell which copy is good without filesystem knowledge.

**Relevance to our OS.** high. A new filesystem can make checksums and CoW the default from the start. It must decide in advance how overwrite-heavy files keep their integrity, and whether atomic snapshot/rollback is exposed to the update system (GR-017).

**Sources.** [S941], [S942], [S943], [S944], [S945]

### GR-015: UTF-8

- **Category:** Text, encoding and locale
- **Systems affected:** Plan 9 (first), Unix/Linux, macOS, the web; Windows (UTF-8 code page since 1903)
- **Classification:** held-up-well
- **Confidence:** high

**What the design is.** UTF-8 is a variable-length (1–4 byte) encoding of Unicode. ASCII bytes represent only themselves. Bytes of a multibyte sequence always have the high bit set, so `/`, NUL and newline never appear inside another character. The first byte gives the sequence length, character boundaries can be found from any position, and byte order matches code-point order [S947].

**Why it was chosen.** In 1992 Plan 9 needed Unicode text in a byte-stream system built on pipes. Fixed 16-bit Unicode would have required converting all text going into and out of the system and carrying byte-order state in streams. Pike and Thompson noted that the Unicode Consortium was "thinking of files, not pipes" [S946]. The draft ISO UTF allowed ASCII bytes such as `/` inside multibyte sequences, which would break filenames. Their modification, informally called FSS-UTF, became UTF-8 [S946].

**Why it held up.** As of October 2026, 99.1% of websites with a known encoding use UTF-8 [S951]. Microsoft now recommends UTF-8 code pages for Windows apps and has supported forcing a process's code page to UTF-8 since Windows 10 1903 [S952]. This is the platform that had built most heavily on UTF-16 (compare DM-096). Plan 9 converted its whole system, including the C compiler, with null-terminated byte-string interfaces unchanged [S946]. RFC 3629 standardized it in 2003 [S947].

**Where it falls short.** (1) **Security of decoders.** Overlong forms (e.g. `C0 80` for NUL), encoded surrogates and invalid sequences have been used to bypass filters such as `/../` checks. RFC 3629 requires rejecting them, but lenient decoders keep appearing [S947]. (2) **Normalization.** Canonically equivalent strings have different bytes, which affects filenames and access-control comparisons (DM-094) [S947]. (3) **Size and indexing.** Text stored as runes grows for ASCII and shrinks for Japanese. Character indexing and `%10s`-style width are no longer O(1) or byte-based [S946]. (4) **Not enforced.** Unix filenames remain arbitrary bytes (DM-095). (5) Plan 9 chose to replace invalid input with an error rune rather than fail, a policy decision that converts invalid bytes lossily [S946].

**Alternatives that exist.** UTF-16 (Windows, Java, JavaScript), UTF-32, legacy code pages.

**Cost of the alternatives.** UTF-16 is still variable-length (surrogates), byte-order dependent and not ASCII-compatible (DM-096).

**Relevance to our OS.** high. UTF-8 everywhere is an easy decision. The real choices are whether the kernel or filesystem *validates* UTF-8 in names and whether it normalizes them.

**Sources.** [S946], [S947], [S951], [S952]

### GR-016: Compositor-owned input and client isolation (Wayland model)

- **Category:** Graphics, input and window systems
- **Systems affected:** Wayland compositors (GNOME/Mutter, KDE/KWin, wlroots, Weston); comparable: Android SurfaceFlinger, macOS WindowServer [unverified]
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** The compositor is the display server. It owns KMS output and evdev input directly, finds which surface is the target of each input event, and forwards the event only to that client, in surface-local coordinates [S948]. Clients render into their own buffers and give them to the compositor [S948]. Each client sees only its own objects. Seats have a single keyboard and pointer focus, clients cannot take explicit grabs, and clipboard selections are offered only to the client that has keyboard focus [S949].

**Why it was chosen.** X11 puts every client in one global namespace. Any client can move or resize other windows, read their contents, and send input to them, and screenshot tools work as ordinary clients reading the framebuffer [S955]. In X the server couldn't route input correctly to windows the compositor had transformed, because only the compositor knew the transforms [S948].

**Why it held up.** Wayland "starts from a much stronger security position than X". Clients can't snoop on other programs' input, inject synthetic events, or grab all input [S954]. Buffer sharing moved from guessable 32-bit GEM names to DMA-buf fd passing [S954]. Screen capture now goes through the compositor or a user-mediated portal: the user picks sources in a dialog, and only those PipeWire streams become visible to the requester [S950][S955]. Major Linux desktops have moved to it as their default [unverified as a dated claim].

**Where it falls short.** (1) **Privileged clients were left unsolved.** Virtual keyboards, screen readers, screenshot tools and lockers need powers the core protocol denies. In 2014 the proposed policy framework was still early planning [S954]. Today this is handled by per-compositor protocols and portals, which fragments the ecosystem (DM-084). (2) **The security model is not specified.** The official protocol documentation's "Security and Authentication" section is still a list of notes, not a design [S949]. (3) **Isolation stops at the protocol.** LD_PRELOAD-style injection into clients still works, and the protocol cannot prevent it [S954]. (4) XWayland clients still share an X11 namespace with each other [unverified].

**Alternatives that exist.** X11 with the SECURITY extension or a nested X server per trust domain [unverified]. Windows UIPI integrity levels for window messages (DM-082). In-kernel window systems (DM-081).

**Cost of the alternatives.** Retrofitted X11 restrictions break existing clients. A compositor-centric model moves window management, capture policy and accessibility into the compositor, so it becomes a large trusted component.

**Relevance to our OS.** high. This is the planned model. Its open problem is a capability-style design for privileged clients (capture, input injection, accessibility), which Wayland never specified centrally.

**Sources.** [S948], [S949], [S950], [S954], [S955]

### GR-017: Atomic A/B updates and verified boot (ChromeOS, Android)

- **Category:** Boot, update and recovery
- **Systems affected:** ChromeOS, Android; also image-based Linux distributions [unverified]
- **Classification:** held-up-well
- **Confidence:** high

**What the design is.** Verified boot starts from read-only firmware. That firmware checks an RSA signature over a hash of the read-write firmware, which in turn verifies the kernel. The kernel then checks root-filesystem blocks against a hash tree as they are read, via a transparent verifying block device (dm-verity) [S956]. A/B updates install the new image into the inactive slot in the background. The device reboots into the new slot and automatically falls back to the old slot if the new one fails to boot [S957].

**Why it was chosen.** ChromeOS's threat model is an opportunistic remote or local attacker. The goal is that a compromise cannot persist through permanent local changes, and that a safe recovery is always possible outside the attacker's control [S956]. Verifying the whole root partition before booting would have added at least 5 seconds to boot on netbooks of the time, so blocks are verified on demand and cached [S956]. Android adopted A/B to cut the number of devices bricked by bad OTAs and taken to repair centres [S957].

**Why it held up.** Both are deployed on very large fleets (ChromeOS, Android). Android's documentation lists the benefits: updates install while the device is in use, reboot time is unchanged, a failed install or boot falls back to the old slot, and updates can be streamed without free space for the package [S957]. Android later made Virtual A/B a requirement for GMS devices launching with Android 11 or later. That kept the guarantees while cutting the storage cost [S958]. ChromeOS kept user ownership: verification can be bypassed in developer mode with user consent, with no remote attestation required [S956].

**Where it falls short.** (1) **Mutable state is outside the guarantee.** User data, and system state such as network configuration, time zone and cached data, live on the writable stateful partition. If the boot process consumes that data, it can still subvert the verified software [S956]. (2) **Verification can't prevent re-exploitation.** It ensures a clean image, but the same exploit works again until the system is updated [S956]. (3) **Storage cost.** Full A/B duplicates partitions. Virtual A/B exists to reduce that overhead, using compressed copy-on-write snapshots that are merged after a successful boot, which adds complexity [S958]. (4) It requires a read-only system image, which conflicts with in-place package management (DM-090).

**Alternatives that exist.** In-place package updates with transactional package managers (DM-090). Snapshot-based rollback on a CoW filesystem (GR-014). Measured boot with remote attestation instead of local enforcement.

**Cost of the alternatives.** In-place updates can be interrupted halfway. Attestation-centred designs take control away from the device owner.

**Relevance to our OS.** high. With no legacy package model, the system image can be read-only, verified and A/B from the first release. The open question is how much state lives outside the image and how it is validated.

**Sources.** [S956], [S957], [S958]

### GR-018: Synchronous message passing with priority inheritance (QNX)

- **Category:** Inter-process communication
- **Systems affected:** QNX (since at least 1992), L4/seL4 (synchronous IPC); contrast: Zircon (asynchronous channels)
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** A client calls `MsgSend` and blocks until a server does `MsgReceive` and then `MsgReply`. The kernel copies the message directly from one address space to the other, with no intermediate queue [S959]. When a higher-priority client sends, the server thread is raised to the client's priority at send time, before it receives the message, so a medium-priority thread cannot delay the request [S960]. POSIX calls are implemented as messages. For example, `lseek` is a `MsgSend` to the filesystem manager associated with the fd [S968].

**Why it was chosen.** Hildebrand's 1992 paper describes message-driven priority as the way to avoid the priority inversion that fixed-priority message-passing systems suffer. Servers run at the priority of the client they are serving [S962]. Synchronous send/reply needs no kernel buffering and gives natural flow control [S959].

**Why it held up.** QNX has shipped this design for more than 30 years in embedded and automotive systems [unverified as market data], while still providing a POSIX environment on top of a microkernel [S962]. Its documentation shows that system services can be structured as a set of servers called by message. If sends always go "up" a hierarchy, the system is free of deadlock by construction [S961]. seL4 also builds on fast synchronous IPC and reports the fastest cross-address-space IPC among microkernels [S919].

**Where it falls short.** (1) **Deadlock freedom depends on discipline.** Two threads must never send to each other, and sends must go up a hierarchy. Notifications from higher to lower levels need a separate non-blocking mechanism (pulses, `MsgDeliverEvent`) [S961]. (2) **Priority inheritance is a policy with side effects.** The boosted priority stays until the next message is received rather than reverting on reply. It can be turned off per channel. Privileged priorities are capped [S960]. (3) **Copy costs.** QNX double-copies short messages through a buffer because that beats remapping pages, which shows that "direct copy" is a tuning question [S968]. (4) Asynchronous patterns (I/O completion, GR-009) have to be built from pulses and events [S959][S961]. Zircon chose asynchronous channels instead [S911].

**Alternatives that exist.** Asynchronous message queues (Mach ports, Zircon channels). Shared-memory rings (io_uring-style). Priority-ceiling protocols.

**Cost of the alternatives.** Asynchronous queues need kernel buffering, quotas and backpressure, and they lose implicit priority propagation.

**Relevance to our OS.** high. The native IPC choice (sync call/reply vs async channels) is central. QNX shows that synchronous IPC with priority inheritance can carry a full POSIX personality, which is useful evidence for hosting the C compiler.

**Sources.** [S959], [S960], [S961], [S962], [S968], [S919], [S911]

### GR-019: Separate address spaces as the default isolation unit

- **Category:** Kernel structure, drivers and fault isolation
- **Systems affected:** All mainstream OSes; contrast: Singularity (software-isolated processes), unikernels, in-process sandboxes
- **Classification:** held-up-well
- **Confidence:** high

**What the design is.** Each protection domain (a process) gets its own page tables, and hardware enforces that it cannot read or write another domain's memory. Isolation is a property of the MMU, not of the code that runs inside the domain.

**Why it was chosen.** Hardware isolation doesn't depend on the correctness of compilers, verifiers or runtimes, and it applies to code in any language.

**Why it held up.** The best-documented challenge, Singularity, measured hardware isolation costs of up to 25–33% on its benchmarks, compared with under 5% for language-based software-isolated processes. Even so, it kept hardware isolation as defence in depth [S969]. Spectre then removed the main premise of the alternative. V8's engineers concluded that speculative side channels "defeat all language-enforced confidentiality" within one address space, with no known complete software mitigation, and Chrome moved its security model to process isolation [S963]. Site Isolation, deployed to all desktop Chrome users, costs 9–13% memory overhead and under 2.25% extra page-load latency [S964]. A large vendor accepted that cost in production, which is strong evidence of value.

**Where it falls short.** (1) **Cost.** Address-space switches, TLB pressure and IPC copying are the reason microkernel designs had to fight for performance (DM-018), and they were Singularity's whole motivation [S969]. (2) **Sharing the kernel.** The kernel's mapping in every address space turned into Meltdown (DM-033). Address-space separation also doesn't by itself stop cross-process microarchitectural leaks, which need further mitigations [S963][S964]. (3) **The wrong granularity on its own.** A process isolates memory, not authority. Without capabilities, an isolated process still has ambient access (DM-009).

**Alternatives that exist.** Language-based isolation (Singularity/Midori, WebAssembly, eBPF). Hardware compartments within one address space (CHERI, MPK) [unverified for specifics]. Separate VMs per domain.

**Cost of the alternatives.** Language isolation can't keep secrets under speculation [S963] and ties the system to one compiler toolchain. VMs cost more than processes.

**Relevance to our OS.** high. A Rust kernel might be tempted to rely on language safety for user isolation. The Spectre evidence argues for keeping hardware address spaces as the security boundary, and using language safety for robustness inside a boundary.

**Sources.** [S963], [S964], [S969]

### GR-020: Formal verification of a kernel (seL4)

- **Category:** Kernel structure, drivers and fault isolation
- **Systems affected:** seL4; partial: CertiKOS, Hyperkernel and others [unverified]
- **Classification:** held-up-with-caveats
- **Confidence:** high

**What the design is.** A machine-checked proof (Isabelle/HOL) shows that the kernel's C implementation refines an abstract specification. Further proofs cover integrity and confidentiality, and on some architectures translation validation of the compiled binary [S918][S919].

**Why it was chosen.** "Complete formal verification is the only known way" to guarantee freedom from programming errors [S918]. A microkernel is small enough to make this feasible: 8,700 lines of C and 600 lines of assembly in 2009 [S918].

**Why it held up.** (1) **The proof is maintained.** It is machine-checked and updated continuously as changes are merged, not done once and frozen [S919]. (2) **It now covers our target architecture.** On AArch64, functional correctness (including the fast path) and integrity are proved. A confidentiality proof was announced complete in August 2026 [S920][S921]. Functional correctness rules out buffer overflows, null dereferences and use-after-free in the verified code [S919]. (3) **Verification didn't cost performance.** seL4 claims the fastest IPC among microkernels it knows of, through a verified fast path [S919]. (4) **The cost was manageable.** seL4-specific proof effort was about 11 person-years, about 20 including research and tools. Kernel development itself took about 2.2 person-years, against about 6 for the unverified L4 Pistachio [S918]. Proof effort is therefore several times development effort.

**Where it falls short.** (1) **Assumptions.** Compiler (on configurations without binary proofs), assembly, hardware, cache/TLB management and boot code are all trusted [S918][S919]. DMA must be off or trusted, and SMMU/IOMMU support, debug interfaces and kernel startup are outside the verified configurations [S919][S920]. (2) **AArch64 coverage.** There is no binary-correctness proof for AArch64, and MCS proofs are "in progress" [S920]. (3) **Timing channels** are explicitly not covered [S919]. (4) **Multicore.** The SMP kernel uses a big lock and is not verified for SMP [S919]. (5) The proof covers the kernel only. A system built on seL4 is as trustworthy as its unverified user-level components and configuration [S919]. (6) **Change is expensive.** Kernel changes require proof updates, which pushes toward a minimal, rarely changing kernel.

**Alternatives that exist.** Testing and fuzzing, memory-safe implementation languages (Rust), partial verification of selected components, and push-button verification of restricted kernel designs [unverified].

**Cost of the alternatives.** Testing and Rust remove whole bug classes (Rust removes memory-safety errors) but not logic or specification errors, and they can't make the isolation guarantees seL4 proves.

**Relevance to our OS.** high. seL4 runs on our target ISA. It is both a possible reference design for capability semantics and a benchmark for what a "trustworthy kernel" claim would need to back up.

**Sources.** [S918], [S919], [S920], [S921]

### GR-021: virtio as a standard paravirtual device interface

- **Category:** Kernel structure, drivers and fault isolation
- **Systems affected:** Linux/KVM, QEMU, lguest (origin), many guest OSes; QEMU `virt` machine
- **Classification:** held-up-with-caveats
- **Confidence:** medium

**What the design is.** Virtual devices (block, network, console, and others) share one transport: virtqueues (vring) in guest memory, feature bits negotiated between driver and device, and discovery through PCI or MMIO, so they look like ordinary devices [S965][S966]. OASIS standardized the interface (VIRTIO 1.0, 2016) [S966].

**Why it was chosen.** In 2007 Linux supported at least eight virtualization systems (Xen, KVM, VMware VMI, IBM System p/z, UML, lguest, iSeries). Each had its own block, network and console drivers with varying features and quality [S965]. One transport and driver set removed that duplication. Its stated goal is a "straightforward, efficient, standard and extensible mechanism" instead of "boutique per-environment" ones [S966].

**Why it held up.** It went from Russell's de-facto proposal to an OASIS standard with feature-bit extensibility, so new device types and features are added without breaking old drivers [S966]. On QEMU it is the standard way to get disks, networking and consoles into a guest [unverified as a general claim, but consistent with the `virt` board]. The interface has spread beyond hypervisors to SmartNICs, vDPA and VDUSE, and is the device model for confidential VMs (TDX, SEV) [S967].

**Where it falls short.** (1) **Drivers trust the device.** Linux virtio drivers were written to trust the host. They lacked checks on descriptors, ring indices and lengths, so a malicious host could corrupt guest memory. This matters for confidential VMs and for hardware that implements virtio [S967]. (2) **Hardening costs performance and is still disputed.** Disabling indirect descriptors was objected to on performance grounds, and maintainers disagreed about hardening only TDX mode [S967]. (3) It is a virtualization-only interface. Real hardware needs other drivers.

**Alternatives that exist.** Emulated real hardware (e1000, AHCI); per-hypervisor paravirtual devices (Xen netfront, VMware vmxnet).

**Cost of the alternatives.** Emulating real hardware is slow. Per-hypervisor interfaces multiply drivers [S965].

**Relevance to our OS.** high. On QEMU `virt` on aarch64, virtio-mmio/PCI devices are likely to be the first and possibly only drivers. Writing them in Rust as untrusted-device parsers from the start avoids the retrofit Linux is going through.

**Sources.** [S965], [S966], [S967]

## Stream notes

**What was searched.** Primary pointers from the brief:
- Ritchie & Thompson 1974 (MIT recreation; the Bell Labs/Nokia copies returned 403/410)
- Plan 9 namespaces and UTF papers
- Microsoft object manager, handle, IOCP, CreateProcess attribute and WaitOnAddress docs
- seL4 SOSP 2009, FAQ and verified-configurations pages, and the Aug 2026 AArch64 confidentiality announcement
- McKenney's RCU survey and the kernel RCU docs
- DTrace USENIX 2004
- OpenZFS docs plus the FAST 2008/2010 corruption studies
- Franke/Russell/Kirkwood OLS 2002 and Drepper
- Torvalds' LKML posts (via the lkml.iu.edu mirror; lore.kernel.org returned 403) and kernel regression docs
- Chromium OS verified boot and Android A/B/Virtual A/B docs
- Zircon handles and process creation

Also used:
- Baumann et al. (fork), Capsicum, Crotty et al. (mmap)
- Seltzer/Murphy and WinFS (hierarchy critique and failed alternative)
- Google's kCTF io_uring post, LWN on eBPF verifier bugs
- QNX 7.1 docs and Hildebrand 1992
- "Spectre is here to stay", Site Isolation, Deconstructing Process Isolation
- virtio paper/spec and LWN on virtio hardening

PDFs were fetched with curl and converted with Ghostscript where WebFetch could not read them. A mid-run rate limit forced curl for a few HTML pages; each URL cited was retrieved.

**Downgrades and classification.** Only five entries are `held-up-well`: GR-005, GR-012, GR-015, GR-017, GR-019. Fifteen are `held-up-with-caveats`. GR-008 (object capabilities) is `contested`: the evidence for the model is strong, but mainstream adoption is thin and vendors have chosen ACL plus sandbox instead. Unix-origin items (GR-001 fds, GR-002 pipes, GR-006 hierarchy) were deliberately kept at `with-caveats`, with their known failure modes cross-referenced to DM entries. Nothing was dropped or merged.

**New entries.** GR-021 (virtio as a standard paravirtual device interface). It was added because QEMU `virt` makes it directly relevant to our first drivers.

**Open questions / unverified.**
- (a) Whether the Capsicum Linux port was ever merged. I believe not, but found only secondary pointers (linuxfr.org, a mailing-list forward), which I did not fetch. Marked [unverified] in GR-008.
- (b) APFS checksum scope (Apple's page returned no body text). APFS claims are marked [unverified].
- (c) Exact date and NT version for the object manager's origin.
- (d) The scale of CVE-2014-3153 exploitation.
- (e) Whether seL4's 2026 AArch64 confidentiality proof covers MCS or SMP configurations. The announcement doesn't say, and the verified-configurations page lists MCS as in progress.
- (f) Several adoption claims (Wayland default on desktops, DTrace ports, QNX market) are marked [unverified] rather than sourced.

**Possibly weak sources.**
- [S929] is an LWN comment thread, labelled weak and used only to show that practitioners dispute the scope of the rule.
- [S951] (W3Techs) is a commercial survey, labelled secondary.
- [S921] is a forum announcement by a seL4/Proofcraft principal. It is primary, but it is a press-style claim and is backed by [S920].
- [S938] (ebpf.io) is an advocacy site, labelled secondary.

**For the coordinator.** GR-001/GR-002/GR-003 cross-reference DM-002, DM-009, DM-016, DM-020, DM-022, DM-023, DM-036, DM-053, DM-077 and DM-081. GR-009 relates to DM-025/026/029, GR-013 to DM-049/072, GR-016 to DM-079/082/084, and GR-017 to DM-090. Please check that those DM entries do not contradict the GR claims.
