### DM-046: Monolithic kernel vs microkernel

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, BSDs, Windows NT, macOS/XNU (hybrid); alternatives MINIX 3, L4/seL4, QNX, Zircon
- **Classification:** contested
- **Confidence:** medium

**What the design is.** In a monolithic kernel, drivers, filesystems, network stack and memory management all run in one privileged address space and call each other directly. A microkernel keeps only address spaces, threads, IPC and interrupt delivery in privileged mode, and runs everything else as isolated user-mode servers that talk by messages.

**Why it was chosen.** Linux in 1991 was a one-person project on a 386, and it needed to be usable right away. Torvalds' answer to Tanenbaum was that Linux "wins heavily on points of being available now". He also attacked MINIX for its problems with a multithreaded filesystem and with multitasking inside the kernel [S705]. In 2006 he argued that separate address spaces mean "you can't share data structures". Locking and coherency then turn into distributed algorithms, which are harder to write than shared-memory code, and he called "hybrid kernel" a marketing term [S706]. Mach-era microkernels also had a real IPC cost. Biggs et al. point out that the cost was thought to be inherent until L4 [S704].

**Why it is a problem.** All privileged code is in the TCB, and a bug anywhere in it can compromise everything. Biggs et al. reviewed all 115 critical Linux CVEs up to late 2017 and could classify 112 of them. They estimate that a microkernel design alone would have removed 29% of them, and 96% would no longer have been critical [S704]. Tanenbaum et al. argue that MINIX 3 keeps foreign code out of kernel mode completely, for a user-mode-driver overhead below 10% [S707]. Driver faults (DM-047) are the main argument here.

**Why it was not fixed.** Every mainstream kernel would need a rewrite and all its drivers re-hosted. Torvalds, Linux's lead maintainer, rejects the premise that a microkernel is simpler or more reliable [S706].

**Alternatives that exist.** MINIX 3 runs drivers and servers as user processes and has a reincarnation server [S707]. L4 and seL4 are small, fast and, for seL4, formally verified [S704]. QNX and Zircon are production microkernels [unverified — no source fetched for either]. Nooks keeps the monolithic kernel and isolates drivers inside it [S702].

**Cost of the alternatives.** Both sides have evidence. Pro-microkernel: Biggs et al. say IPC cost is now small [S704]. However, their CVE study is an analysis of what would have happened, not a measurement, and it carries about ±9 percentage points of uncertainty by the authors' own estimate. Their 96% figure also includes 38% of CVEs rated only "weakly mitigated", which could still violate confidentiality or integrity [S704]. The authors belong to the seL4 group, so they are not neutral. Anti-microkernel: Torvalds' 2006 points on shared state and distributed coordination [S706] apply to any design that splits the FS, VM and block layers across servers. Restartability is also limited for stateful servers (DM-051) [S707]. The MINIX 3 performance figure comes from the system's own authors [S707].

**Relevance to our OS.** high. This is the top structural decision for a new Rust kernel, and Rust weakens part of the isolation argument (DM-048) without removing the TCB-size argument.

**Sources.** [S704], [S705], [S706], [S707], [S702]

### DM-047: Driver bugs as a dominant cause of OS crashes; fault-isolation approaches

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, OpenBSD, Windows (XP era onward); any OS with in-kernel drivers
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Device drivers are written by many vendors and are the largest and fastest-changing part of the kernel. They run with full kernel privilege, with no isolation from the core.

**Why it was chosen.** Running in the kernel gives direct hardware access, cheap interrupt handling and zero-copy calls into kernel subsystems. When there are few devices and the drivers are written by the kernel team, the risk is small.

**Why it is a problem.** Chou et al. ran static analysis over 21 Linux snapshots and one OpenBSD snapshot. For certain error types, driver error rates were up to 3–7x higher than in the rest of the kernel, and bugs survived about 1.8 years on average [S700]. Swift et al. report that extensions make up over 70% of Linux code, citing Chou et al. [S702]. They also state that drivers caused 85% of recently reported Windows XP failures [S702]. **Provenance caveat:** the only source given for that 85% figure is a 2003 private communication from Rob Short, a Microsoft vice president [S702]. It was never published as a study, and its method and sample are unknown. The SDV paper repeats it by citing Nooks, and changes "failures" to "crashes" [S703]. So the most-quoted number in this field has one unpublished origin, and it should be treated as an anecdote from Microsoft, not an independently established measurement. The bug classes are concrete: misuse of complex driver APIs (SDV encodes more than 60 WDM usage rules) [S703], lock and interrupt misuse, use-after-free and null dereference [S700]. Double fetches from user memory are also concentrated in drivers, with 57 of 90 found in Linux [S746] (see DM-160). The picture changed later. Palix et al. found that over Linux 2.6 the driver fault rate per line fell below that of arch/ and fs/ [S701]. So "drivers are uniquely bad" was strongly true in 2001 and is weaker now. Drivers still have the largest absolute number of faults, and they make up about 57% of the code [S701].

**Why it was not fixed.** Isolating drivers means either a new driver model or overhead (see the alternatives). Vendors write against the existing models.

**Alternatives that exist.** Static verification: SDV, shipped with Windows driver development [S703]. In-kernel isolation: Nooks recovered from 99% of crash-causing injected faults [S702]. User-mode driver frameworks: Windows UMDF, with a host process and a kernel reflector [S720]. Full user-mode drivers: MINIX 3 [S707]. Recovery that hides the failure from applications: shadow drivers [S708].

**Cost of the alternatives.** Nooks cut kHTTPd throughput by 60% and network streams by about 10% [S702]. It explicitly does not defend against malicious drivers [S702]. UMDF excludes filesystem, display and print drivers [S720]. Static checkers find only violations of rules someone has written down [S703]. Recovery is only as good as fault detection: in the shadow-driver tests, 35% of visible failures were not detected automatically [S708]. User-mode drivers isolate nothing if the device can still DMA anywhere (DM-161).

**Relevance to our OS.** high. We will write virtio drivers first, but any driver model we pick fixes where hardware bugs and vendor code end up.

**Sources.** [S700], [S701], [S702], [S703], [S707], [S708], [S720], [S746]

### DM-048: Memory-unsafe implementation language (C) for kernels

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, BSDs, Windows NT, XNU; nearly every production kernel
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Kernels are written in C (and some C++). The language does not enforce bounds, lifetimes or initialization, so spatial and temporal memory safety rests on programmer discipline, review and tooling.

**Why it was chosen.** C was designed to write Unix. It gives predictable code generation, direct control over layout, no runtime, and easy linking with assembly. For decades no memory-safe language had the same control without a garbage collector.

**Why it is a problem.** Large C/C++ codebases show the same pattern again and again. About 70% of the CVEs Microsoft assigns each year are memory-safety issues [S710]. About 70% of 912 high/critical Chromium security bugs since 2015 are too [S709]. Android's share was 76% in 2019 [S711]. These are whole-product figures, not kernel figures. A Linux-kernel-specific breakdown by bug class could not be verified [unverified]. Concrete kernel bug classes include use-after-free and null dereference [S700]. They also include the out-of-bounds read in CrowdStrike's CSagent.sys that crashed about 8.5 million Windows machines [S716], [S717], [S715]. Privileged C code keeps growing; Linux was about 26 MSLOC in 2018 [S704].

**Why it was not fixed.** Rewriting is not practical. The working approach is to write new code in a safe language. Android's memory-safety vulnerabilities fell from 223 to 85 a year (2019–2022) as most new code moved to memory-safe languages. Google calls this a correlation, not proven causation [S711]. Linux now accepts Rust drivers: Binder, Nova, Tyr and null-block are in mainline [S713]. Leaf drivers must go through safe abstractions, not raw C bindings [S712]. Adoption is socially contested. In January 2025 DMA-mapping maintainer Christoph Hellwig refused Rust abstractions and said a multi-language kernel would be "impossible to maintain" [S741].

**Alternatives that exist.** Rust kernels such as Redox [S1115] (also Theseus and Hubris [unverified — not fetched]) and Rust drivers in Linux [S713]. Language-based OSes such as Singularity [S707]. A small verified C core instead of a safe language (seL4) [S704].

**Cost of the alternatives.** Rust still needs `unsafe` for MMIO, DMA, page tables and FFI. Safety then depends on those abstractions being sound [S712]. Mixed-language kernels carry maintenance and review costs that maintainers have rejected publicly [S741]. Memory safety does nothing about logic errors such as a 21-vs-20 field mismatch, except that a checked index would have panicked rather than read wild memory. In kernel mode a panic still takes down the system unless it can be contained (DM-051) [S717].

**Relevance to our OS.** high. We have already chosen Rust. The open questions are how much `unsafe` to allow, where it may live, and what a panic does.

**Sources.** [S709], [S710], [S711], [S712], [S713], [S715], [S716], [S717], [S741], [S700], [S704], [S707], [S1115]

### DM-049: No stable in-kernel driver API/ABI (Linux)

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux (contrast: Windows WDM/KMDF/UMDF, macOS DriverKit)
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Linux deliberately offers no stable binary or source interface to drivers. Kernel internals can change in any release, and whoever changes an interface fixes every in-tree user. Only the syscall interface toward user space is promised to stay stable [S714].

**Why it was chosen.** The kernel documentation gives three reasons [S714]. A binary ABI cannot work because compiler version, config options and architecture change structure layout. A stable source API would freeze mistakes: the USB stack was reworked at least three times, and some security fixes required interface changes. Keeping old interfaces would add unpaid work. The prescribed fix is to merge the driver upstream, where it gets maintained for free.

**Why it is a problem.** Out-of-tree drivers break with every kernel update, so vendors who don't upstream ship kernels that are frozen or patched. The scale of this (Android vendor kernels, proprietary GPU drivers) could not be verified from fetched sources [unverified]. Upstreaming also concentrates all drivers in one TCB (DM-046, DM-047).

**Why it was not fixed.** This is policy, not oversight. Maintainers see the instability as what keeps the kernel improvable [S714].

**Alternatives that exist.** Windows gives vendors stable driver frameworks, including UMDF with a host process model [S720] and the WDM/KMDF APIs that SDV checks [S703]. A WDF driver built against an older framework version keeps loading on later framework versions as long as the major version does not change [S2300]. Microkernels move drivers behind an IPC protocol, so the stable contract is a message protocol, not an in-kernel API.

**Cost of the alternatives.** The two positions:
- *For instability:* a frozen API keeps bad designs alive. The Windows driver API is complex enough that Microsoft built a model checker with more than 60 usage rules to catch misuse [S703]. Linux's in-tree drivers also improved: their fault rate per line fell below other directories over Linux 2.6 [S701]. Linux's fault study does not show that the policy caused this.
- *For a stable interface:* vendors can ship drivers independently of OS releases. WDF binaries keep working across framework minor versions, and new features are reached through explicit runtime availability checks [S2300]. In return, the OS supports old interfaces indefinitely, and a driver that skips those checks can bug-check the machine [S2300].

Neither side's evidence is clean. Note that a microkernel's IPC protocol is also an ABI with exactly this versioning problem (DM-055).

**Relevance to our OS.** medium. With no third-party drivers this costs us nothing now. Whether driver↔kernel or driver↔server protocols are versioned is a decision we make once.

**Sources.** [S714], [S720], [S703], [S701], [S2300]

### DM-050: Third-party kernel code with full privilege (CrowdStrike, July 2024)

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Windows (security products, filter drivers); equally any OS with loadable modules (Linux LKMs, former macOS kexts)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** The OS lets signed third-party modules (security agents, filesystem filters, anti-cheat, drivers) load into the kernel with full privilege. These modules may also interpret data that is updated often.

**Why it was chosen.** Microsoft lists what vendors get from kernel mode [S716]: system-wide visibility, the ability to block file and process operations, resistance to tampering even by attackers with admin-level privileges, and early-boot start via ELAM. Kernel placement can also help high-throughput workloads such as network inspection [S716].

**Why it is a problem.** On 19 July 2024 a CrowdStrike content update (Channel File 291) crashed about 8.5 million Windows devices, under 1% of the installed base [S715]. CrowdStrike's root-cause analysis [S717] describes the mechanism. An IPC template type declared 21 input fields, but the sensor supplied 20. Testing used wildcards for the 21st field, so the mismatch went unnoticed. The first non-wildcard instance made the in-kernel Content Interpreter read past its input array. The interpreter had no runtime bounds check, and content was not deployed in stages [S717]. Microsoft's own conclusion is that kernel drivers give these properties "at the cost of resilience" [S716]. Code signing says who wrote a module, not that it is correct. In-kernel isolation such as Nooks targets exactly this kind of buggy extension, but it explicitly assumes extensions are not malicious, and malicious kernel code can get around it [S702].

**Why it was not fixed.** Microsoft describes user-mode parity for performance and visibility as still a work in progress, so vendors kept kernel components [S716]. EU interoperability commitments also come up in this context [unverified].

**Alternatives that exist.** Microsoft's guidance after the incident: keep kernel sensors minimal, and do content parsing and updates in user mode, where a failure can be recovered from. For tamper resistance it points to user-mode protections such as VBS enclaves [S716]. In November 2024 Microsoft announced platform support for antivirus running in user mode, with a private preview planned for July 2025. It also made deployment rings mandatory for security-vendor updates and announced Quick Machine Recovery for machines that cannot boot [S718]. Other routes, not from Microsoft's guidance, are verified in-kernel bytecode (eBPF-style) [unverified] and microkernel user-mode servers (DM-046).

**Cost of the alternatives.** User-mode security agents give up some visibility and tamper resistance, which are the properties vendors wanted in the first place [S716]. Staged rollout slows the response to fast-moving threats. Recovery tools such as Quick Machine Recovery treat the symptom, not the cause [S718].

**Relevance to our OS.** high. We should decide early whether any code other than the core kernel can ever run privileged, and how a "monitor everything" service would get its visibility without that.

**Sources.** [S715], [S716], [S717], [S718], [S702]

### DM-051: Kernel crash = system crash; no component restart

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, BSDs, Windows NT, XNU (partial exceptions: Windows TDR, UMDF)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** When kernel code hits a fatal error, the whole machine stops (panic or bug check) or limps on in an unknown state (Linux oops). No component can be restarted alone, because all components share one address space and one set of locks.

**Why it was chosen.** If a fault can corrupt any kernel memory, stopping is the only safe response. Restart is also not free. Torvalds argues that because OS components depend on each other and share coherency state, restarting one failed service often does not save the system [S706].

**Why it is a problem.** One driver bug becomes total loss of availability (DM-047, DM-050). Linux shows the strain. Torvalds "absolutely DETESTS" BUG_ON() because it kills the machine with locks held, leaving it undebuggable and unreported [S721]. His rule became "absolutely no new BUG_ON() calls" in favour of WARN_ON_ONCE() and real error handling [S722]. In other words, a monolithic kernel copes by trying not to die, because it cannot die partially. Microsoft says kernel code cannot fail and restart the way a normal user application can [S716].

**Why it was not fixed.** Restart needs isolation (DM-046) plus a way to rebuild the restarted component's state, and both are expensive to retrofit.

**Alternatives that exist.** MINIX 3's reincarnation server restarts drivers and servers that crash or stop answering pings [S707]. Shadow drivers (on Nooks, in Linux) replay logged configuration and resubmit or cancel requests, so the failure is hidden from applications. With injected transient faults that Nooks could detect, all ten test applications kept running [S708]. Windows TDR resets the GPU and reinitializes the display driver without a reboot [S719]. UMDF puts drivers in host processes [S720].

**Cost of the alternatives.** Restart helps with transient, fail-stop faults. Deterministic bugs come back as soon as the component restarts [S708]. Irreversible side effects (writes already sent to disk, printed output) cannot be undone [S708]. Recovery took about 4–5 seconds for disk and network drivers [S708]. MINIX 3 can only restart servers that hold little internal state [S707]. In the L4-based driver-VM approach described in the same paper, a restarted driver comes back in its initial state, for example with the default audio volume [S707]. TDR bug-checks on the sixth GPU hang if five or more occur within a minute, and some legacy applications may render black after a reset [S719]. Recovery is only as good as fault detection: 35% of visible failures were not detected automatically [S708].

**Relevance to our OS.** high. In a Rust kernel the panic strategy (abort vs unwind vs restart a server) sets how much of this we can have.

**Sources.** [S706], [S707], [S708], [S716], [S719], [S720], [S721], [S722]

### DM-052: `errno` and integer error codes

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS (contrast: NT NTSTATUS, Zircon zx_status_t)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** A failing call returns a sentinel (usually -1 or NULL) and puts a small integer in `errno`. The integer comes from a flat, global list of names (EINVAL, ENOENT, …) whose numeric values vary by system.

**Why it was chosen.** Early Unix returned an error indicator plus a code from the kernel, and a single global variable was the cheapest way to expose it to C [unverified — PDP-11 history not checked against a fetched source]. That held until threads arrived. Some architectures still signal failure through a separate flag register, such as a3 on MIPS/Alpha or the carry bit on SPARC [S724].

**Why it is a problem.** errno(3) itself lists the pitfalls [S355]:
- The value means something only after a failure, and success may overwrite it.
- Nothing ever resets it to zero, so for calls where -1 is a valid result the caller must clear it first.
- It had to become thread-local once threads existed [S355], and pthreads functions do not use it at all: they return the error number directly [S118].
- Numeric values differ across Unix systems and even between Linux architectures (EDEADLOCK on MIPS, PowerPC and SPARC).
- Names alias (EAGAIN/EWOULDBLOCK, ENOTSUP/EOPNOTSUPP).

A flat namespace pushes errors toward a few overloaded codes. The kernel docs note that subsystems return ENOSYS or EINVAL for unknown ioctls where ENOTTY is correct [S726]. Once shipped, error values are ABI. Torvalds rejected an ioctl returning ENOENT because userspace depended on the old code [S743]. Codes carry no detail: which argument, which object, which path component.

**Why it was not fixed.** Every C program and libc depends on it; POSIX specifies it.

**Alternatives that exist.** Zircon returns `zx_status_t` directly from every call. Codes are grouped (parameter, state, permission, I/O) and given precise meanings, for example INVALID_ARGUMENT (never valid) vs OUT_OF_RANGE (might become valid), WRONG_TYPE for the wrong handle type, and PEER_CLOSED [S725]. Rust's `Result<T, E>` makes the error part of the return type.

**Cost of the alternatives.** Direct return codes still have no room for detail. Zircon documents conventions, not a way to attach context [S725]. Richer error objects need allocation or variable-size returns across the syscall boundary. A POSIX compatibility layer will have to map native errors back onto errno, and that mapping loses information.

**Relevance to our OS.** high. The error vocabulary of our native ABI becomes ABI the day it ships, and it has to translate cleanly to errno for the C compiler sandbox.

**Sources.** [S355], [S118], [S724], [S725], [S726], [S743]

### DM-053: `ioctl` as an untyped escape hatch

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows DeviceIoControl is analogous
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** `ioctl(fd, request, arg)` passes a device-specific command number and an untyped pointer or integer to whatever driver backs the descriptor. The kernel knows neither the payload's layout nor its direction, and the same is true of the multiplexers fcntl and prctl.

**Why it was chosen.** With one uniform file abstraction, device-specific operations (terminal modes, disk geometry) needed some outlet that did not add a syscall per device. A single escape hatch kept the syscall table small and let drivers grow independently.

**Why it is a problem.** The kernel's own ioctl guide lists the failure modes [S726]:
- Every handler needs a matching `compat_ioctl` path for 32-bit processes.
- Struct layout differs by architecture: 64-bit fields are 4-byte aligned on x86-32 and only 2-byte aligned on m68k. Pointers have to be passed as `__u64`.
- `struct timespec`/`timeval` in payloads broke with the move to 64-bit time_t (DM-056).
- Implicit padding leaks uninitialized kernel memory to user space.
- Versioned commands break existing applications.
- Unknown commands get ENOSYS or EINVAL instead of ENOTTY.

The adding-syscalls guide warns that ioctl can lead to "a somewhat opaque API". It says fcntl "hides a lot of complexity" and calls prctl "a complicated multiplexor" [S727]. Payloads are not described in any machine-readable form, so there is no generic tracing, no way to generate bindings, and no type check at the boundary. The handler reads raw user memory directly, which is where double fetches come from (DM-160). Even error codes become de facto ABI [S743].

**Why it was not fixed.** Thousands of drivers and every user-space tool that talks to devices depend on it. New interfaces keep being added as ioctls because it is the cheapest path, for example PROCMAP_QUERY in 2024 [S732].

**Alternatives that exist.** Dedicated, reviewed syscalls with extensible structs (DM-055) [S727]. Typed IPC protocols with an IDL, such as Fuchsia FIDL [unverified — not fetched]. Plan 9's text control files.

**Cost of the alternatives.** Syscalls need central review and an entry in the syscall tables (shared or per-architecture) [S727]. An IDL needs a compiler, versioning rules and a runtime. Text control files bring back parsing (DM-054). Some escape hatch always survives. The real question is whether it is typed and self-describing.

**Relevance to our OS.** high. A capability/handle-based ABI makes it tempting to have one "invoke(handle, op, bytes)" call, which is ioctl again unless the bytes are typed.

**Sources.** [S726], [S727], [S732], [S743]

### DM-054: Text-based `/proc` and `/sys` interfaces

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux (procfs, sysfs); Plan 9 heritage; other Unixes to lesser degrees
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** The kernel exports state and controls as synthetic files containing human-readable text. Consumers open, read and parse them, and they write text to change settings.

**Why it was chosen.** Text works with `cat`, `grep` and shell scripts, needs no special tooling, can be inspected by people, and is independent of architecture (no struct layout or endianness). sysfs tries to keep this manageable with a rule: ASCII, "preferably with only one value per file". It allows arrays of one type and frowns on mixed types and multi-line formatting [S729].

**Why it is a problem.** procfs predates that rule and breaks it. `/proc/pid/mountinfo` has a variable number of optional fields ended by a lone "-". Parsers must scan for the separator, ignore unknown tags, and cope with reused mount IDs [S730]. Every consumer writes its own parser, and every format change risks breaking one. Text is also slow when the data is large. Profilers that symbolize addresses had to read and parse all of `/proc/pid/maps`. A 2024 binary ioctl (PROCMAP_QUERY) cut an end-to-end address normalization from about 50 µs to about 10 µs, with 35–40x in a microbenchmark [S732]. Reading a file is not atomic across multiple reads, so consumers can see torn snapshots [unverified]. The outcome is a growing set of binary replacements: statmount() returns a struct with a mask of filled fields [S731], and PROCMAP_QUERY is an ioctl on the text file [S732]. Linux now ships two interfaces for the same data.

**Why it was not fixed.** The text formats are ABI. Scripts and tools parse them, so they cannot be changed, only supplemented.

**Alternatives that exist.** Binary syscalls with masks (statmount [S731], statx [S734]). One-value-per-file discipline (sysfs [S729]). Structured self-describing formats such as netlink attributes, or typed IPC to a system service.

**Cost of the alternatives.** Binary interfaces need headers, versioning and tooling to inspect, and they lose shell scriptability. One-value-per-file trades parsing for many open/read/close calls and still gives no atomic multi-value read. Every structured format needs a schema and an evolution story.

**Relevance to our OS.** medium. Observability interfaces get decided early and are hard to remove. Whether they are typed records with a text renderer, or text, is a choice we can still make.

**Sources.** [S729], [S730], [S731], [S732], [S734]


### DM-055: Unchecked flags and fixed structs: syscall extensibility

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux (open, clone, many *2/*3 successors), Unix/POSIX generally
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Many original syscalls take a fixed argument list and a flags word whose unknown bits are silently ignored. Extending them means either a new flag bit, which is unsafe if old kernels ignore it, or a whole new syscall.

**Why it was chosen.** Early syscalls were few, designed by a small team, and changed together with the libc. Checking flags looked like overhead, and nobody expected decades of extensions running on kernels of many ages.

**Why it is a problem.** If unknown bits are ignored, user space cannot tell whether a feature is supported. A program asking for safe behaviour on an old kernel silently gets unsafe behaviour. openat2(2) states it directly: openat ignores unknown flag bits, and openat2 rejects them [S613]. Bits run out and get reused. In clone(), the low byte of flags is the exit signal. CLONE_PID was silently ignored after 2.5.16 and its bit reused for CLONE_PIDFD, and CLONE_STOPPED's bit was reused for CLONE_NEWCGROUP [S733]. A reused bit means an old binary can trigger new semantics. The other route is syscall proliferation: eventfd/eventfd2, dup2/dup3, inotify_init/inotify_init1, pipe/pipe2, renameat/renameat2 [S727].

**Why it was not fixed.** Old calls cannot start rejecting bits that existing binaries pass. The fix applies only to new calls.

**Alternatives that exist.** Linux's current rules [S727]: reject unknown flags with EINVAL, and require zero when no flags exist yet. For complex calls, pass a struct with a size field. Newer fields must default to zero; the kernel zero-extends smaller structs and requires unknown trailing bytes to be zero. openat2 and clone3 follow this. Size acts as a version, and nonzero unsupported fields give E2BIG [S613], [S733]. statx and statmount use request/response masks, so the kernel reports which fields it actually filled [S734], [S731].

**Cost of the alternatives.** Strict rejection makes probing for a feature cost a failed syscall. Size-versioned structs need zero-fill discipline in callers [S613] and a copy-in helper in the kernel. Masks need callers to check what came back. statx accepts unsupported requested bits and just reports what it filled; it rejects only the reserved bit with EINVAL. So masks serve a different purpose than strict flags [S734].

**Relevance to our OS.** high. This is cheap to get right on day one and impossible to fix later: every native syscall and IPC message should reject unknown bits and carry a size or version.

**Sources.** [S727], [S613], [S731], [S733], [S734]

### DM-056: Fixed-width types baked into the ABI (`off_t`, `time_t`, `stat64` proliferation)

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux (esp. 32-bit), glibc, Unix/POSIX; Debian armhf/armel
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** File offsets, sizes, inode numbers, IDs and timestamps were defined as C types whose width matched the machine word (often 32 bits) and were embedded in syscall structs. When the widths turned out too small, each type grew through parallel interfaces instead of in place.

**Why it was chosen.** Memory and register width were scarce. 32 bits covered every disk and timestamp of the era, and fixing the width to `long` matched the hardware.

**Why it is a problem.** On 32-bit Linux, stat exists in three kernel generations: `__old_kernel_stat` with narrow fields, `stat` with a larger st_ino, and `stat64` (Linux 2.4) with larger st_ino, 32-bit uid/gid and later nanosecond timestamps. There are also fstatat64 and newfstatat depending on architecture. glibc hides this by repacking [S735]. statx (Linux 4.11) is a fourth design, built to be extensible from the start [S734]. In user space, glibc kept a parallel `*64` API (`off64_t`, `fseeko64`) under `_LARGEFILE64_SOURCE`, plus `_FILE_OFFSET_BITS=64` to redirect the old names. The default is still 32 bits, and `_TIME_BITS=64` works only together with `_FILE_OFFSET_BITS=64` [S737]. Every library whose public structs contain these types inherits the problem. Debian's 2024 64-bit time_t transition changed the ABI of about 495 library packages and needed 5,000–6,000 rebuilds. Some of the ABI changes (58 in one analysis) came from finally enabling large-file support. i386 was left out to keep legacy binaries running [S736]. Timestamps inside ioctl structs broke too [S726]. Compat layers have to handle explicit 64-bit arguments for 32-bit callers [S727]. The 2038 problem itself is DM-063.

**Why it was not fixed.** Changing a type's width breaks every binary that embeds it. Distributions have to coordinate a flag-day rename [S736].

**Alternatives that exist.** 64-bit-only ABIs: on 64-bit Linux there is a single stat syscall [S735]. Explicit fixed-width fields (`__u64`) in all UAPI structs [S726]. Extensible structs with masks, like statx [S734].

**Cost of the alternatives.** 64-bit fields everywhere cost a little space. That is irrelevant on aarch64, but it is an ABI decision that 32-bit targets would inherit.

**Relevance to our OS.** medium. As a 64-bit-only aarch64 OS we avoid most of this by default. The remaining risk is any "obviously big enough" width we choose now, such as a 32-bit handle, PID or nanosecond count.

**Sources.** [S734], [S735], [S736], [S737], [S726], [S727]

### DM-057: Where the stable ABI lives: syscalls (Linux) vs libc/ntdll (BSD, Windows, macOS)

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux; macOS/XNU; OpenBSD; Windows NT
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Linux promises that the raw syscall interface will not break [S714], [S743]. Programs, including static binaries and runtimes like Go, may trap into the kernel directly. Other systems promise stability only at a user-space library and treat syscall numbers as private.

**Why it was chosen.** Linux's kernel and C library come from different projects, so the kernel boundary is the only contract the kernel developers own. Torvalds' rule is that if a kernel change breaks a user program, that is a kernel bug [S743]. Apple, the BSDs and Microsoft ship kernel and libc together, so the library is the natural contract.

**Why it is a problem.** Each choice has a cost:
- *Syscall-stable (Linux):* every mistake in the syscall layer is permanent. The result is stat generations (DM-056), ignored flags (DM-055) and error codes frozen in place [S743]. Raw syscall conventions also vary by architecture [S724].
- *Library-stable:* programs that bypass libc break. Apple says a static binary "assumes binary compatibility at the kernel system call interface", which Apple does not guarantee, so static binaries are unsupported [S745]. Windows NT syscall numbers shift between releases, for example NtCreateFile is 0x52 through Windows 7, then 0x53, 0x54, and 0x55 from Windows 10 [S744]. Go had to give up most direct syscalls: from Go 1.11 the runtime calls libSystem on macOS so binaries stay compatible with future releases [S738], and Go 1.16 calls libc on OpenBSD because OpenBSD 6.9+ requires it for non-static binaries [S739].

**Why it was not fixed.** Neither model is broken; each fits its system's organisation. OpenBSD went further in the other direction. Since 7.5, pinsyscalls(2) pins each syscall number to its call site in libc, and a mismatch is killed with SIGABRT [S740]. That turns "libc is the ABI" into a security mechanism against injected syscall instructions.

**Alternatives that exist.** Linux model: stable syscalls plus a vDSO. Library model: macOS libSystem, Windows ntdll/kernel32, and OpenBSD libc with pinning [S740]. Capability systems can version the IPC protocol instead.

**Cost of the alternatives.** Library-stable ties every language runtime to the system C library (or a system-provided non-C shim) and rules out fully static binaries [S745], [S739]. Syscall-stable freezes kernel mistakes forever and makes the kernel boundary the attack surface for every program [S714].

**Relevance to our OS.** high. We must choose whether the native ABI is the raw trap interface or a Rust system library (with the POSIX layer above it). That choice decides whether static binaries are possible and how freely the syscall layer can change.

**Sources.** [S714], [S724], [S738], [S739], [S740], [S743], [S744], [S745]

### DM-058: Big Kernel Lock; SMP retrofitted onto uniprocessor kernels

- **Category:** 9. Concurrency and synchronization primitives
- **Systems affected:** Linux (BKL, 1.3.x–2.6.39), FreeBSD (Giant, SMPng), other Unix kernels [unverified for others]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** A kernel written for one CPU gets multiprocessor support through a single global lock: only one CPU may execute (most) kernel code at a time. Concurrency is then added by moving subsystems out from under the lock one by one.

**Why it was chosen.** It was the only feasible first step. In Linux 1.3.x the "lock" was a bit set on kernel entry, so CPUs spun at the door and no driver had to change [S341]. Generic spinlocks did not exist on most architectures, so the BKL was the only SMP mutex available. The 2.1.23 change that spread it (9 to 761 call sites) is also what first allowed two tasks in the kernel at once [S341]. FreeBSD's Giant played the same role [S343].

**Why it is a problem.** The lock was recursive, so there was no risk of deadlocking with oneself, and code took it "just to be safe" at no visible cost [S341]. Its many uses became entangled: unrelated subsystems, such as a webcam driver list and the NFS daemon, excluded each other for no reason [S341]. Nobody could say what a given `lock_kernel()` protected. Each removal required reverse-engineering implicit invariants. When the BKL was pushed out of the VFS, every driver `open` got its own copy, and many driver owners never took the time to check whether it was needed [S341]. The cost: poor scalability for a decade, and latency problems; the realtime tree's low tolerance for latency motivated part of the late removal work [S342].

**Why it was not fixed.** The fix was spread across hundreds of call sites in code with absent or uninterested owners. It took roughly 1995–2011. The end game removed or staged modules nobody would fix [S342] and finished with "BKL: That's all, folks" (January 2011, targeted at 2.6.39) [S340], [S342]. FreeBSD similarly kept code not marked MPSAFE running under Giant [S343].

**Alternatives that exist.** Design for SMP from day one with explicit, documented lock scopes, or avoid shared kernel state (multikernel-style per-core state). Notably, seL4 deliberately keeps a big lock. Because microkernel system calls are short, a big lock performs comparably to fine-grained locking on shared-cache cores, and it keeps the kernel verifiable [S344], [S345].

**Cost of the alternatives.** Fine-grained locking from the start costs complexity, deadlock risk, and verification difficulty [S345]. The big-lock approach only works when the protected code paths are short. It breaks down for a monolithic kernel with long syscalls, which is exactly the BKL lesson [S344].

**Relevance to our OS.** high — the lesson is less "never use a big lock" than "keep what the lock covers small and documented"; a small kernel may legitimately start with one.

**Sources.** [S340], [S341], [S342], [S343], [S344], [S345]

### DM-059: Thread cancellation (`pthread_cancel`)

- **Category:** 9. Concurrency and synchronization primitives
- **Systems affected:** POSIX threads (glibc, musl, BSD libcs); analogous: Windows `TerminateThread`, Java `Thread.stop`
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** One thread can request another's termination. In deferred mode the target dies at the next cancellation point, a long list of functions including `open`, `read`, `close`, `accept` and `pthread_cond_wait`. In asynchronous mode it can die at any instruction. Cleanup handlers run in reverse push order, then TSD destructors [S346], [S118].

**Why it was chosen.** Threads blocked in system calls need some way to be stopped, for example to abort a search or shut down a server, without each blocking call taking a "stop" argument. POSIX tried to make this safe with cancellation points and cleanup handlers instead of raw asynchronous kill.

**Why it is a problem.**
- Asynchronous mode is unusable in practice: only `pthread_cancel`, `pthread_setcancelstate` and `pthread_setcanceltype` are async-cancel-safe [S118].
- Deferred mode is subtle too, although this part was an implementation defect, not a flaw in the POSIX design. glibc implemented each cancellable syscall by briefly enabling *asynchronous* cancellation around it. A cancel arriving after the kernel had acted, but before user space saw the result, could leak a descriptor from `open`/`accept`. For `close` it left the program unable to know whether the fd was released, risking closing a different thread's fd later [S348]. Signal handlers that interrupted such syscalls ran with async cancellation on [S349].
- Reported in 2011 (BZ #12683), the race was fixed only in glibc 2.41, and a follow-up crash fix landed in 2.42 [S350], [S349].
- Every library on a cancellable path must be written cancellation-safe with push/pop handlers, and most are not.
- On Linux the mechanism rides on a hidden real-time signal [S346], inheriting signal complexity (DM-017).
- The forced-kill cousins are worse. `TerminateThread` can leave critical sections and the heap lock held [S351]. Java removed `Thread.stop` because releasing monitors leaves "damaged" objects [S352].

**Why it was not fixed.** It is part of POSIX, and C code depends on it, for example to break out of blocking reads. Fixing the race required arch-specific syscall wrappers [S349].

**Alternatives that exist.** Cooperative cancellation: a flag plus an interrupt that makes blocking calls return, as in Java's recommended pattern [S352]. Cancellation tokens passed explicitly. In capability or handle systems, closing or revoking the handle a thread waits on makes the wait return an error. musl decides precisely whether a syscall completed by checking the saved program counter against assembly labels around it [S348]. That shows deferred cancellation can be implemented without the glibc race.

**Cost of the alternatives.** Cooperative schemes need every blocking primitive to be interruptible, plus disciplined callers. A thread stuck in uncooperative code cannot be stopped, short of killing the whole process.

**Relevance to our OS.** high — the native ABI should define how blocking waits are interrupted (handle close, timeout, cancel token); the POSIX layer must still emulate `pthread_cancel` for ported C code.

**Sources.** [S346], [S118], [S348], [S349], [S350], [S351], [S352]

### DM-060: Non-reentrant libc interfaces with hidden global state (`strtok`, `localtime`, `errno` before TLS)

- **Category:** 9. Concurrency and synchronization primitives
- **Systems affected:** C standard library / POSIX on all Unix-likes; Windows CRT [unverified]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Many C and POSIX functions keep state in static storage. `strtok` stores its position internally [S353]. `localtime`, `gmtime`, `asctime` and `ctime` return pointers to static buffers that any later call overwrites [S354]. `errno` was originally a plain global `int` [S355]. The environment (`getenv`/`setenv`) is an unsynchronised global array.

**Why it was chosen.** These functions predate threads. In a single-threaded process, static buffers save callers from allocating memory and keep signatures short. On 1970s machines both mattered.

**Why it is a problem.** With threads, every such function is a data race. POSIX exempts roughly 90 functions from its thread-safety requirement, including `strtok`, `localtime`, `asctime`, `getenv` and `strerror` [S118]. glibc labels them, for example `MT-Unsafe race:tmbuf env locale` [S354]. The fix doubled the API with `_r` variants (SUSv2), and those differ subtly: `localtime_r` need not call `tzset`, so portable code must call it explicitly [S354]. `errno` had to become a per-thread macro. Old code that declares `extern int errno` is now harmful [S355]. Environment mutation is still unsound with threads. Rust 2024 made `std::env::set_var`/`remove_var` `unsafe` for exactly this reason, after shipping them as safe [S236]. Boehm's broader point: threads bolted onto a language and library designed without them cannot be made fully correct by the library alone [S366].

**Why it was not fixed.** The originals are in ISO C and POSIX and cannot change signature. Removing them breaks source compatibility, so both sets coexist.

**Alternatives that exist.** APIs designed after threads, with caller-supplied buffers or returned owned values (all `_r` functions, Rust `std`). Thread-local error values or error returns instead of `errno`. Immutable process environment snapshots.

**Cost of the alternatives.** More parameters, explicit buffer management, and divergence from code written against the old names.

**Relevance to our OS.** medium — native interfaces can avoid hidden global state entirely, but the POSIX compatibility libc must provide the legacy functions and decide how unsafe `setenv` is allowed to be.

**Sources.** [S118], [S353], [S354], [S355], [S236], [S366]

