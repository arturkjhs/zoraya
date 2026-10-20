# Candidate design principles

Part of [TASK-01](TASK-01-design-mistakes-research.md). **These are candidates for the architect, not decisions.**
Each one is derived only from verified entries in [design-mistakes.md](design-mistakes.md) (DM),
[held-up-well.md](held-up-well.md) (GR) and [alternatives-postmortem.md](alternatives-postmortem.md) (PM). The
evidence and its sources live in those entries; this file does not add new claims.

Each principle has:
- **Statement.**
- **Based on:** the entries it is drawn from. The basis says when an entry is classified `contested` or
  `tradeoff`, because a principle drawn from contested evidence is weaker than one drawn from mistakes.
- **Costs:** what adopting it gives up, from the "Cost of the alternatives" fields of those entries.
- **Open questions:** what the evidence does not settle.

Conflicts between principles are listed explicitly at the end. Several principles are in direct tension with
hosting a C compiler through a POSIX layer. That tension is the central architectural question this research
surfaces, and the research does not resolve it.

IDs `CP-NN` are stable for reference from ADRs.

---

## A. Authority and naming

### CP-01: No ambient authority in the native ABI

**Statement.** Native system calls act on handles the caller holds, never on global names that any process can
try. A new process starts with nothing except what its creator passes it.

**Based on:**
- Mistakes: DM-013 (Linux "capabilities" are privilege bits, not capabilities), DM-019 (System V IPC's global key
  namespace).
- Tradeoffs: DM-009 (ambient authority), DM-014 (retrofitted sandboxing), DM-101
  (containers assembled from retrofits), DM-110 (user namespaces expanding attack surface).
- Right at the time: DM-010 (identity-keyed DAC).
- Related tradeoffs on policy layers: DM-015 (MAC via LSM), DM-016 (Windows ACL complexity).
- Held up: GR-008 (object capabilities, *contested*), GR-003 (NT handles; partial form), GR-004 (Plan 9 namespaces).
- PM: KeyKOS/EROS, Fuchsia/Zircon.

**Costs:**
- Every resource has to be delegated explicitly. Launchers, shells and libraries need a delegation protocol.
- Paths a user types need a powerbox or a trusted chooser.
- Existing software is not structured this way. Capsicum's tcpdump lost DNS because the resolver needed global
  namespaces (GR-008). KeyKOS legacy guests ran, but got none of the capability benefit (PM).
- Hardy reports that the kernel itself got smaller, not larger (DM-009). The cost lies in porting and plumbing.

**Open questions:**
- Is there any global name service at all? If so, what may it hand out? Service discovery and broadcast are a
  related, *contested* question: kernel bus, user-space broker, or capability conventions (DM-021).
- Is a system-wide policy layer needed on top of capability distribution, or derived from it (DM-015)?
- How does a human administrator name things?

### CP-02: Handles are typed and rights only ever decrease

**Statement.** Every handle carries an object type and a rights mask. Duplication and transfer can only reduce
rights. Transfer in IPC moves the handle.

**Based on:**
- Mistakes: DM-022 (SCM_RIGHTS GC bugs), DM-072 (TIOCSTI: one fd grants output and input injection).
- Held up: GR-001 (fds have no rights mask, *with caveats*), GR-003 (NT `DuplicateHandle` can sometimes *increase*
  access), GR-008.
- PM: Amoeba (sparse capabilities can be copied like data).

**Costs:**
- A rights vocabulary has to be defined for every object type, and every API has to check it (GR-001).
- Move semantics make sharing explicit (DM-022).
- Whether move-only handles rule out cycles of in-flight handles is unresolved (DM-022, `[unverified]`).

**Open questions:**
- Revocation: handle-level or object-level?
- What are the cycle and lifetime rules for handles queued inside channels?

### CP-03: No all-powerful principal and no privilege change on exec

**Statement.** No identity bypasses access checks. Executing a file never grants authority. Elevation is a request
to a broker that holds the authority.

**Based on:**
- Mistakes: DM-011 (setuid), DM-013.
- Tradeoffs: DM-012 (root).
- PM: Plan 9 (host owner is "far weaker" than root).

**Costs:**
- A broker daemon is privileged and has an IPC attack surface; polkit is an example (DM-011).
- With no superuser, a misconfigured system can lock out its owner, so a recovery path must be designed (DM-012).

**Open questions:**
- What is the recovery and administration path, and who holds the root of the capability tree at boot?

### CP-04: Names resolve relative to a handle, and the rules for name equality are fixed once

**Statement.**
- Path resolution starts from a directory handle and cannot escape it: no `..` past the handle, and no symlinks, or
  only confined ones.
- Creating a new name for an object requires authority over the object.
- The name encoding and equality rules (bytes, case, normalization) are specified once, together with the Unicode
  version they depend on.

**Based on:**
- Mistakes: DM-039 (path TOCTOU and symlink races).
- Tradeoffs: DM-040 (hard links), DM-041 (global mount namespace), DM-095 (byte-string filenames).
- Right at the time: DM-043 (MAX_PATH, reserved names).
- Contested: DM-094 (case folding and normalization).
- Held up: GR-004, GR-006 (hierarchy, *with caveats*), GR-015.

**Costs:**
- Removing `..` and symlinks breaks POSIX expectations (DM-039).
- Rejecting names breaks round-tripping of archives and of media created elsewhere (DM-095).
- Tying names to Unicode ties the filesystem to a Unicode version (DM-095: APFS).
- Per-process namespaces depend on shared naming conventions (DM-041).

**Open questions:**
- Case sensitivity is genuinely contested (DM-094), and the research does not settle it.
- How does the POSIX layer map names that the native rules reject?

---

## B. Processes and lifetime

### CP-05: Processes are constructed explicitly; a child gets only what it is given

**Statement.**
- The native primitive builds a new process from parts the caller supplies: an image, handles, an address space
  and configuration.
- There is no `fork`. A child inherits no handles, environment or cwd by default.

**Based on:**
- Right at the time: DM-001 (fork), DM-002 (fd inheritance), DM-006 (process-global cwd/umask).
- Tradeoffs: DM-008 (argv).
- Mistakes: DM-085 (environment).
- Held up: GR-005 (explicit creation, *with caveats*).
- PM: Fuchsia, Exokernel.

**Costs:**
- A spawn API needs a parameter for every piece of process state (DM-001, GR-005).
- Copy-on-write snapshot uses (Redis, fuzzers, Zygote) need a separate snapshot API.
- Drepper argues that fork without exec is legitimate, and that spawning loses the shared ASLR layout (DM-001,
  GR-005).
- Cygwin's fork emulation is "neither fast nor reliable" (GR-005).

**Open questions:**
- How much `fork` must the POSIX layer emulate? The C toolchain mostly uses fork+exec, which is shaped like
  `vfork`/`posix_spawn` (GR-005).
- Is a native snapshot or clone-address-space primitive needed?

### CP-06: Processes are objects named by handles; exit status lives on the object

**Statement.** Process identity is a handle, not a reusable integer. Anyone holding the handle can wait for exit.
The object lives until its last handle is closed. Printable IDs are informational only.

**Based on:**
- Right at the time: DM-003 (PID reuse).
- Tradeoffs: DM-005 (zombies).
- Mistakes: DM-130 (waitable events outside the fd model).
- Held up: GR-001.

**Costs:**
- A leaked handle pins kernel state, so a zombie becomes an unclosed handle (DM-005).
- Humans still need short names, which require a lookup path that is explicitly racy (DM-003).

**Open questions:**
- Does a handle-free lookup by name exist at all? If so, for whom?

### CP-07: Every process belongs to an inescapable containment unit

**Statement.** A process is always created inside a job-like object. The job carries resource limits, accounting
and identity, and supports "kill the whole tree". Containers and sandboxes are compositions of these units and
capabilities, not separate kernel features.

**Based on:**
- Right at the time: DM-100 (no reliable tree containment).
- Tradeoffs: DM-101 (no kernel container object).
- Mistakes: DM-111 (cgroups v1).
- PM: Fuchsia (jobs), Genode (budgets).

**Costs:**
- Rules are needed for nesting and for deliberate breakaway. Windows got nesting wrong until Windows 8 (DM-100).
- Accounting adds per-spawn work.
- cgroup v2 shows that a single hierarchy constrains layout (DM-111).

**Open questions:**
- What is the relationship between the job tree and the capability tree? Same tree or separate?

---

## C. Events, I/O and asynchrony

### CP-08: One waitable-object model and one asynchronous I/O model, covering files too

**Statement.** Every kernel object exposes signal state that can be waited on through one port or queue
mechanism. Asynchronous I/O covers all operations, including file I/O, from the start.

**Based on:**
- Mistakes: DM-023 (select/poll/epoll), DM-024 (POSIX/Linux AIO), DM-130.
- Right at the time: DM-029 (files always "ready").
- Tradeoffs: DM-027 (EINTR).
- Contested: DM-026 (readiness vs completion).
- Held up: GR-009 (completion I/O, *with caveats*).

**Costs:**
- Every kernel path has to handle cancellation and continuation (DM-024).
- Completion requires kernel-owned or pinned buffers, which clash with Rust's cancel-by-drop futures (GR-009,
  weak source).
- The POSIX layer has to emulate readiness semantics (DM-026).

**Open questions:**
- Readiness or completion as the native model is contested (DM-026). Whichever is native must emulate the other.

### CP-09: Shared-memory submission interfaces enforce policy per operation and keep the operation set small

**Statement.** If the native ABI uses submission and completion rings, every ring entry is checked against the
caller's capabilities. The ring is not a second, unfiltered syscall table.

**Based on:**
- Tradeoffs: DM-025 (io_uring: 60% of kCTF exploit submissions in one year; disabled by Google).
- Mistakes: DM-160 (double fetch).
- Held up: GR-009.

**Costs:**
- A restricted operation set only partly meets the goal of making everything asynchronous (DM-025).
- Filtering ring entries hurts usability (DM-025).

**Open questions:**
- Can the ring itself be a capability that names exactly the objects and operations it may touch (DM-025)?

### CP-10: No asynchronous interruption of user code in the native ABI

**Statement.**
- Events are delivered as messages or wait results, never by interrupting a running thread.
- Blocking waits are cancelled explicitly, by handle close, timeout or a cancel token.
- An uncatchable kill remains.

**Based on:**
- Mistakes: DM-017 (signals), DM-059 (pthread_cancel).
- Tradeoffs: DM-027, DM-077 (SIGPIPE).
- Right at the time: DM-070 (job control).
- Tradeoffs: DM-028 (uninterruptible sleep), which bears on what happens to in-flight requests when a
  client is killed.

**Costs:**
- Synchronous faults (page fault, divide by zero) still need an upcall or fault IPC (DM-017).
- Cooperative cancellation needs every blocking primitive to be interruptible (DM-059).
- The POSIX layer has to emulate signals. Fuchsia lists signals among the hard parts of Starnix (PM).

**Open questions:**
- What are the semantics of fault delivery?
- How faithful must signal emulation be for the C toolchain?

---

## D. Kernel–user interface

### CP-11: Every interface is extensible by design: strict flags, sized structs, typed messages, copy-in once

**Statement.**
- Unknown flags and nonzero reserved fields are rejected.
- Argument structs carry their size.
- There is no untyped "invoke(handle, op, bytes)" escape hatch unless the bytes are typed by an IDL.
- The kernel copies arguments in once and works only on its private copy.

**Based on:**
- Mistakes: DM-055 (unchecked flags), DM-160.
- Right at the time: DM-053 (ioctl), DM-056 (fixed widths).
- Tradeoffs: DM-054 (text /proc).

**Costs:**
- Probing for a feature costs a failed call (DM-055).
- An IDL needs a compiler, versioning rules and a runtime (DM-053).
- Copying every argument costs bandwidth for large buffers; Zircon caps channel messages at 64 KiB (DM-160).

**Open questions:**
- One IDL for both syscalls and IPC, or only for IPC?
- Do native tools compose through typed messages or byte streams? This is *contested* (DM-020). The POSIX layer
  needs byte streams either way.

### CP-12: Errors are typed values returned at the request site

**Statement.**
- Every operation returns a status from a precise, grouped vocabulary.
- Failures, including allocation and storage errors, reach the caller as values. They do not arrive as signals,
  later kills, or a shared global variable.

**Based on:**
- Right at the time: DM-052 (errno).
- Tradeoffs: DM-030 (overcommit), DM-031 (mmap SIGBUS).
- Mistakes: DM-037 (fsync errors).

**Costs:**
- Direct codes still carry no detail, and richer error objects need variable-size returns (DM-052).
- Mapping native errors back to errno in the POSIX layer loses information (DM-052).

**Open questions:**
- Is there a mechanism for attaching context to an error?

### CP-13: Decide where the stable ABI boundary is before anyone depends on it

**Statement.** Choose once whether the native stable ABI is the raw trap interface or a system library, and
version it from the first release.

**Based on:**
- Tradeoffs: DM-057 (syscalls vs libc/ntdll).
- Contested: DM-049 (no stable driver API).
- Held up: GR-010 ("don't break userspace", *with caveats*).

**Costs:**
- A library-level ABI rules out fully static binaries (DM-057).
- A syscall-level ABI freezes kernel mistakes forever (DM-057). GR-010's a.out history shows mistakes becoming
  permanent.

**Open questions:**
- Which one? The research presents both as genuine tradeoffs.
- What deprecation policy applies before 1.0?

---

## E. Memory and resources

### CP-14: Resource exhaustion surfaces at the request, not as an asynchronous kill

**Statement.** Memory and other resources are committed or charged to a quota when they are requested. Running out
returns an error to the requester. Applications receive memory-pressure signals through the normal event mechanism.

**Based on:**
- Tradeoffs: DM-030 (overcommit and OOM killer), DM-031, DM-034 (pressure signalling).
- PM: Genode (explicit budgets down a parent tree).

**Costs:**
- Strict commit needs backing store for memory that may never be used (DM-030).
- Commit exhaustion is still a failure mode, just a different one (DM-030).
- Every component has to handle running out of quota (PM: Genode).
- `fork` emulation would need commit for a full copy (DM-030).

**Open questions:**
- Quotas through the job tree (CP-07) or through capability-held memory objects?
- Is there purgeable or discardable memory (DM-034)?

### CP-15: Hardware address spaces are the isolation boundary; Rust safety is robustness inside it

**Statement.** Mutually untrusting code runs in separate hardware address spaces. Language safety reduces bugs
within a boundary but is not itself a security boundary.

**Based on:**
- Held up: GR-019 (separate address spaces; Spectre defeats language-enforced confidentiality).
- Right at the time: DM-048 (C kernels).
- Tradeoffs: DM-033 (kernel mapping/Meltdown).
- PM: Singularity/Midori, Theseus (cannot safely host native C output).

**Costs:**
- Address-space switches and IPC copies. Singularity measured up to 25–33% hardware-isolation overhead (GR-019).
- Rust still needs `unsafe` for MMIO, DMA and page tables (DM-048).

**Open questions:**
- Where may `unsafe` live in the kernel?
- What does a kernel panic do (CP-16)?

### CP-16: W^X and DMA isolation are invariants from day one

**Statement.** No mapping is writable and executable at the same time. Executable memory is a capability or goes
through an explicit transition API. Every device gets its own IOMMU domain.

**Based on:**
- Right at the time: DM-032 (executable stacks/heaps), DM-161 (DMA without IOMMU).
- Held up: GR-021 (virtio drivers trusted the device).

**Costs:**
- JITs need explicit flips and entitlements, plus instruction-cache maintenance (DM-032).
- IOMMU use costs throughput and latency and complicates DMA APIs (DM-161).
- Hardening virtio costs performance (GR-021).

**Open questions:**
- How do a hosted C compiler and any JIT get executable pages?
- Does QEMU `virt` run with virtio-iommu or SMMUv3 by default?

---

## F. Kernel structure and drivers

### CP-17: Code outside the core kernel is isolated and can be restarted under supervision

**Statement.** Drivers, filesystems, network stacks and third-party "monitor everything" code run as isolated
components. A supervisor restarts them. Kernel-resident extension code, if any, is restricted and verified.

**Based on:**
- Tradeoffs: DM-047 (driver bugs; the 85% figure is an unpublished anecdote), DM-050 (CrowdStrike), DM-051 (no
  component restart).
- Contested: DM-046 (monolithic vs microkernel), DM-078 (in-kernel network stack).
- Held up: GR-013 (eBPF verifier as attack surface).
- PM: MINIX 3, Hubris, L4.

**Costs:**
- IPC overhead (DM-046, DM-018).
- Restart helps only with transient, fail-stop faults. Stateful servers are hard to restart (DM-051, PM: MINIX 3).
- A user-space stack depends on scheduler co-design (DM-078).
- Microkernel structure is *contested*, not established.

**Open questions:**
- The panic strategy in Rust: abort, unwind, or restart the server (DM-051)?
- Where does VFS state live?
- seL4's AArch64 proofs cover only EL2 on listed boards, and QEMU `virt` is not among them (GR-020).

### CP-18: Concurrency is designed in: documented lock scopes, a written memory model, priority propagation across IPC

**Statement.**
- Every lock has a documented scope. A big lock is acceptable only over short paths.
- The kernel's ordering rules for device memory, DMA, page tables and custom primitives are written down from the
  start.
- The wait/wake primitive is a set of small typed calls.
- IPC defines how priority propagates.

**Based on:**
- Right at the time: DM-058 (BKL), DM-062 (LKMM after the fact).
- Contested: DM-061 (priority inversion).
- Tradeoffs: DM-120 (futex multiplexer).
- Held up: GR-011 (futexes), GR-012 (RCU), GR-018 (QNX synchronous IPC with priority inheritance).

**Costs:**
- Fine-grained locking adds complexity and makes verification harder (DM-058).
- Priority ceiling needs static analysis, and priority inheritance adds per-lock overhead (DM-061).
- Formal models need specialist maintainers (DM-062).

**Open questions:**
- Synchronous call/reply or asynchronous channels as the native IPC (GR-018, DM-018)?
- Which priority protocol is the default (DM-061, *contested*)?

---

## G. Storage

### CP-19: Durability and ordering are written contracts of the storage API

**Statement.**
- The storage API specifies who sees an error, whether failed data is retained, and what a retry means.
- It offers an atomic-replace primitive and states its ordering guarantees.
- Locks are owned by handles.

**Based on:**
- Mistakes: DM-036 (POSIX locks), DM-037 (fsync).
- Tradeoffs: DM-038 (no ordering contract).
- Contested: DM-045 (transactional FS).
- Held up: GR-014 (checksummed CoW, *with caveats*).

**Costs:**
- Strong ordering costs throughput (DM-038).
- Keeping dirty data after errors can pin unbounded memory (DM-037).
- Overwrite-heavy workloads fragment under CoW, and Btrfs `nodatacow` drops checksums (GR-014).

**Open questions:**
- Whether to offer multi-object transactions is *contested* (DM-045).

---

## H. Time

### CP-20: Timeouts are monotonic by type; time is 64-bit; the kernel is tickless

**Statement.**
- Deadline and timeout types in the ABI can only express monotonic time, and their suspend semantics are specified.
- Wall-clock time is a separate type.
- The kernel has no periodic tick.

**Based on:**
- Mistakes: DM-065 (wall vs monotonic).
- Right at the time: DM-063 (2038), DM-067 (HZ tick).
- Tradeoffs: DM-064 (leap seconds).
- Contested: DM-068 (scheduler churn) is related.

**Costs:**
- With several clocks, every timeout API has to say which clock it uses (DM-065).
- Whether suspended time counts has no single right answer (DM-065).
- Tickless kernels interact in complex ways with accounting (DM-067).

**Open questions:**
- Does the system time scale use TAI or UTC (DM-064)?
- Built-in scheduler or pluggable policy (DM-068, *contested*)?

---

## I. Configuration, text and observability

### CP-21: No process-global mutable configuration that libraries read implicitly

**Statement.** Configuration, locale, time zone and working directory are passed explicitly as parameters or
objects. They are not inherited mutable global state.

**Based on:**
- Mistakes: DM-085 (environment), DM-097 (locale).
- Right at the time: DM-006, DM-060 (non-reentrant libc), DM-066 (TZ).

**Costs:**
- Every launcher has to plumb configuration through explicitly.
- Explicit locale parameters clutter text APIs (DM-097).
- The POSIX layer still needs `environ`, `setlocale` and `TZ` (DM-085, DM-097, DM-066).

**Open questions:**
- How is the system zone or locale discovered without ambient state (DM-066)?

### CP-22: UTF-8 is the single native string encoding, validated at the boundary

**Statement.** All native strings, at syscall, IPC and name boundaries, are UTF-8, and the boundary validates them.

**Based on:**
- Right at the time: DM-096 (UTF-16 and A/W).
- Tradeoffs: DM-095.
- Held up: GR-015 (UTF-8, *held up well*).

**Costs:**
- A rejection or lossy-conversion policy is needed (GR-015: Plan 9 replaces invalid bytes with an error rune).
- Validation costs time.
- POSIX byte names that are not valid UTF-8 need a mapping (DM-095).

**Open questions:**
- Validate only, or also normalize (DM-094)?

### CP-23: Configuration and logs are typed, owned and attributable

**Statement.** Each component owns schema-checked configuration. Logs and diagnostics are structured records
carrying a kernel-attested sender identity. Live tracing uses a restricted, safe probe language.

**Based on:**
- Tradeoffs: DM-086 (registry), DM-087 (/etc syntax), DM-089 (syslog), DM-054.
- Contested: DM-088 (init systems).
- Held up: GR-013 (DTrace and eBPF, *with caveats*).

**Costs:**
- Typed stores lose the universality of text and need tools to inspect them (DM-086, DM-087).
- Binary logs need specific readers (DM-089).
- How much belongs in the service manager is *contested* (DM-088).

**Open questions:**
- What is the scope of the service manager?

---

## J. GUI and terminals

### CP-24: The compositor owns input and output; privileged GUI powers are explicit capabilities

**Statement.**
- Clients see only their own input and surfaces.
- Capture, input injection, global shortcuts and accessibility are separately granted capabilities.
- Frame commits are atomic, with explicit fences and a defined opt-out for latency.
- The GUI protocol is not a channel for unauthenticated IPC between clients.

**Based on:**
- Right at the time: DM-079 (X11 security), DM-080 (network transparency), DM-081 (win32k in kernel), DM-083
  (tearing).
- Mistakes: DM-082 (shatter attacks).
- Contested: DM-084 (Wayland fragmentation).
- Held up: GR-016 (Wayland model, *with caveats*).

**Costs:**
- Every capture or automation feature needs a mediated interface (DM-079).
- The compositor becomes a large trusted component (GR-016).
- Atomicity adds latency and new stall modes (DM-083).
- Per-application remoting is no longer built in (DM-080).

**Open questions:**
- Rich core versus minimal core plus extensions (DM-084, *contested*). With a single compositor, fragmentation
  matters less now than it will later.

### CP-25: Terminal rights are separated; control is out of band natively, with a VT translation for POSIX

**Statement.**
- A terminal handle separates the right to write output, read input and inject input.
- The native terminal protocol keeps data and control apart.
- POSIX tools see a VT byte stream through a translator.

**Based on:**
- Right at the time: DM-069 (TTY layer), DM-070.
- Tradeoffs: DM-071 (escape-sequence injection), DM-073 (ConPTY).
- Mistakes: DM-072 (TIOCSTI).

**Costs:**
- A translation layer is imperfect: ConPTY says so itself (DM-073).
- A user-space terminal service sits on the path of every interactive byte (DM-069).

**Open questions:**
- Is the TTY a kernel object or a sandboxed service (DM-069)?

---

## K. Update and boot

### CP-26: The system image is immutable, verified and updated A/B; mutable state is separated

**Statement.**
- The system is an immutable, content-addressed image, verified at boot and updated by switching slots.
- Mutable state lives in a defined separate area.
- Each running service has a known version, and restarting a service is a defined operation.

**Based on:**
- Tradeoffs: DM-090 (in-place updates), DM-091 (in-use files and stale libraries), DM-092 (firmware complexity),
  DM-044 (Windows share modes), DM-007 (shared-library resolution; exact, content-addressed references are the
  Nix-style alternative).
- Contested: DM-093 (verified boot by default).
- Held up: GR-017 (A/B updates and verified boot, *with caveats*).

**Costs:**
- Extra storage (DM-090; Virtual A/B adds complexity, GR-017).
- A reboot is needed to activate an update (DM-090).
- The mutable state partition can still subvert verified code (DM-093, GR-017).

**Open questions:**
- Who holds the keys (DM-093, *contested*: security vs owner control)?
- Direct kernel load on QEMU, or a minimal verified stub (DM-092)?

---

## K2. Networking

### CP-28: Network access and endpoints are capabilities; name resolution is an asynchronous service

**Statement.**
- Binding a port or reaching the network is a granted capability, not a property of a port number or of user
  identity.
- Native endpoints are typed values, not cast `sockaddr` structs.
- Name resolution is an asynchronous, cancellable service, not blocking code linked into every program.

**Based on:**
- Right at the time: DM-074 (BSD sockets API), DM-076 (privileged ports).
- Tradeoffs: DM-075 (name resolution in libc).
- Contested: DM-078 (in-kernel vs user-space network stack).
- PM: Plan 9 (`/net` files and a user-level connection server).

**Costs:**
- The POSIX layer must still provide `sockaddr` and blocking `getaddrinfo` exactly as C code expects (DM-074,
  DM-075).
- A resolver service adds an IPC hop and a daemon that must be up (DM-075).
- Per-port authority needs a policy store and an authority to manage it (DM-076).
- Higher-level connection APIs hide policy that some applications need to control (DM-074).

**Open questions:**
- Does the network stack run in the kernel or as a service (DM-078, *contested*)?

---

## L. Scope

### CP-27: Keep the POSIX compatibility layer's scope fixed and small

**Statement.** The POSIX layer exists to host a C compiler and its toolchain. Its target surface is written down
and changes only by decision.

**Based on:**
- PM patterns: every system that wanted real software built a compatibility layer (Fuchsia/Starnix, L4 with Linux
  VMs, KeyKOS legacy guests, MINIX 3, Redox, Amoeba, ExOS).
- Sprite: a small team chasing moving compatibility targets loses.
- Plan 9: an estimated 90–95% of the effort went to external standards.

**Costs:**
- Some software will not run.
- Pressure to widen scope will recur with every useful program that does not port.

**Open questions:**
- What is the exact POSIX surface the chosen C compiler, linker, make and shell need? This is measurable, and not
  measured here.

---

## Conflicts between principles

Each conflict names both sides and the evidence. None is resolved here.

| # | Conflict | Evidence on each side |
|---|---|---|
| X-1 | **CP-01 / CP-02 (capability-only access) vs CP-27 and ease of POSIX porting** | Capabilities remove whole bug classes (DM-009, DM-039, DM-110). But POSIX programs open global paths after start-up and break under capability mode (GR-008: tcpdump DNS). Legacy guests got no capability benefit (PM: KeyKOS). A POSIX libOS has to protect shared tables, pulling a kernel back in (PM: Exokernel). |
| X-2 | **CP-05 (no fork) vs hosting the C toolchain** | Fork is the root of DM-001, DM-002 and DM-004. But shells, `make` and `cc` drivers use fork+exec, and fork emulation on a spawn-only kernel is "neither fast nor reliable" (GR-005: Cygwin). Drepper's dissent (DM-001) is a further caveat. A vfork/posix_spawn-shaped subset may be enough; this is not measured. |
| X-3 | **CP-10 (no async interruption) vs POSIX signal semantics** | Signals are classed a mistake (DM-017). But the POSIX layer must emulate them, and signals are among the hard parts named for Starnix (PM: Fuchsia). Emulation fidelity determines how much asynchronous machinery comes back. |
| X-4 | **CP-08 (completion-native async) vs readiness for POSIX, and vs CP-09 (small attack surface)** | Which model is native is *contested* (DM-026). The POSIX layer needs readiness (`poll`/`select`). The broadest completion interface to date, io_uring, became the main exploit target (DM-025). |
| X-5 | **CP-11 (copy-in once) vs zero-copy performance (CP-08, CP-09)** | Copy-in removes double-fetch races (DM-160). Shared rings bring back shared mutable memory on purpose (DM-160 cost), and copies cost bandwidth (Zircon's 64 KiB cap). |
| X-6 | **CP-14 (strict commit) vs fork emulation and memory efficiency** | Strict commit makes failure a return value (DM-030). Fork without overcommit needs commit for a full copy of the parent (DM-030), and strict commit reserves backing store for memory that is never touched. |
| X-7 | **CP-17 (isolated, restartable components) vs performance and simplicity** | Microkernel vs monolithic is *contested* (DM-046). IPC cost is real but implementation-dependent (DM-018). Address-space isolation costs up to 25–33% in Singularity's measurements (GR-019). Stateful servers resist restart (DM-051). |
| X-8 | **CP-04 / CP-22 (strict, validated names) vs POSIX byte names and interoperability** | Byte-string names allow control characters and option injection (DM-095). But rejecting names breaks archive round-tripping and media created elsewhere (DM-095). Case semantics are *contested* (DM-094). |
| X-9 | **CP-26 (verified boot by default) vs owner control and developer iteration** | Verified boot is *contested*: ChromeOS's security position vs the FSF's owner-control position (DM-093). Also, every developer build must be signable. |
| X-10 | **CP-13 (stable versioned ABI early) vs freedom to fix early mistakes** | A strict no-regressions rule makes mistakes permanent (GR-010: a.out). Not stabilizing leaves dependents with nothing to target (DM-057). |
| X-11 | **CP-24 (rich, compositor-owned GUI policy) vs a minimal protocol core** | A rich mandated core froze policy in X and win32k (DM-080, DM-081). A minimal core fragmented through extensions (DM-084, *contested*). The compositor becomes a large trusted component (GR-016). |
| X-12 | **CP-07 (inescapable containment) vs legitimate breakaway** | Mandatory jobs give reliable supervision (DM-100). But user sessions launching independent apps need defined breakaway, and Windows' early nesting restriction broke tools (DM-100). |
| X-13 | **CP-03 (no superuser) vs recoverability** | A missing superuser removes the all-powerful target (DM-012). But a misconfigured capability tree can lock out its owner. Recovery needs an out-of-band path, as with SIP and Recovery mode (DM-012). |
| X-14 | **CP-19 (strong ordering and durability) vs throughput** | Delayed allocation exists because ordering costs throughput (DM-038). Retaining dirty data after errors can pin memory against a dead device (DM-037). |
| X-15 | **CP-21 (explicit configuration) vs ergonomics and the POSIX layer** | Implicit environment and locale cause thread-safety and parsing bugs (DM-085, DM-097). Every launcher must plumb configuration, and the POSIX layer reintroduces `environ` for ported code (DM-085). |
