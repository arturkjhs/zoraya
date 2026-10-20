# Stream E entries — categories 5 (Memory management) and 10 (Time, clocks and scheduling)

## 5. Memory management

### DM-030: Memory overcommit and the OOM killer

- **Category:** Memory management
- **Systems affected:** Linux (default heuristic overcommit), other Unix-likes to varying degrees; Android (userspace killers); contrast Windows NT (strict commit accounting)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Linux by default lets processes reserve more virtual memory than RAM plus swap can back. Only "obviously" excessive requests are refused (mode 0). Mode 1 always overcommits, and mode 2 enforces a commit limit of swap plus a share of RAM [S500]. When memory actually runs out at page-fault time, there is no allocation call left to fail, so the kernel's OOM killer picks a victim process by a heuristic "badness" score and kills it [S501].

**Why it was chosen.** Much allocated memory is never touched: sparse arrays, large reserved-but-unused heaps, and copy-on-write pages after `fork()`. Overcommitting uses RAM more efficiently and needs less swap [S500][S501]. Fork-heavy Unix workloads make this pressure worse: without overcommit, a large process calling `fork()` must find commit for a full copy or fail with ENOMEM (an anecdotal point from reader discussion [S503], weak).

**Why it is a problem.** It moves allocation failure from a call site, where the program can handle it, to an arbitrary later memory access, where it cannot. Victim selection is a heuristic. It kills the wrong process often enough that the kernel grew years of knobs (oom_adj, oom_score_adj, cgroup proposals, name-based policies), each criticised in turn [S501]. Forking servers are especially likely victims because child memory is added to the parent's score [S501]. Even `MAP_NORESERVE` and similar promises carry no guarantee: the mmap man page's BUGS section says any process can be killed when memory runs out [S509]. In practice the kernel OOM killer often acts too late. Android and server fleets moved the kill decision to userspace daemons driven by pressure signals [S506][S504].

**Why it was not fixed.** Strict mode 2 exists [S500], but it is not the default. Software written against overcommit (huge virtual reservations, fork of large processes) would fail under strict mode. The fix that actually happened was incremental: better heuristics, then userspace killers using PSI [S504][S506].

**Alternatives that exist.** Windows NT charges every committed page against a system commit limit equal to RAM plus page files. When the charge reaches the limit, allocations fail, and system-managed page files can grow [S502]. Linux mode 2 does the same [S500].

**Cost of the alternatives.** Strict commit needs backing store for memory that may never be used. Microsoft warns that hitting the commit limit causes freezes, crashes and malfunctions, so commit exhaustion is still a failure mode, just a different one [S502]. Programs must handle allocation failure everywhere, and many do not.

**Relevance to our OS.** high — the commit policy determines whether "allocation failed" is a value returned to the caller or a kill delivered asynchronously, and that shapes every API above it, including the POSIX layer's `fork`.

**Sources.** [S500], [S501], [S502], [S503], [S504], [S506], [S509]

### DM-031: `mmap`'d files: errors surface as `SIGBUS`; truncation hazards

- **Category:** Memory management
- **Systems affected:** POSIX/Unix generally, Linux; Wayland compositors (shared-memory buffers); databases using mmap
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** `mmap` makes file contents appear as ordinary memory. I/O happens through page faults, so there is no call that can return an error. Touching a page past end-of-file raises SIGBUS. POSIX leaves the effect of the file changing size under a mapping unspecified [S509].

**Why it was chosen.** It is a uniform, zero-copy way to read files and share memory. The page cache is reused directly, with no system call per access and no duplicate buffer in user space [S510].

**Why it is a problem.** Errors that `read` would return become signals that can arrive at any load or store:
- **I/O errors.** Any code that touches mapped memory can receive SIGBUS, and a DBMS has to handle it with cumbersome signal handlers. Corrupted pages are persisted silently [S510].
- **Hidden stalls.** Any access may block on I/O, and there is no asynchronous read path [S510].
- **Scalability.** Page eviction and TLB shootdowns limit scaling on fast storage [S510].
- **Abandonment.** MongoDB (MMAPv1) and InfluxDB moved off mmap after significant engineering cost [S510].
- **Hostile peers.** A process sharing a file can truncate it and crash the other side with SIGBUS, or change the data after it has been validated [S511][S512]. The 2014 sealing work was aimed at exactly the compositor-receives-client-buffer case [S512].

**Why it was not fixed.** Sealing (`memfd_create` plus `F_SEAL_SHRINK`, Linux 3.17) gives a fix that both sides must opt into [S511]. Seven years later no Wayland compositor enforced it, because old clients and portable clients use `shm_open`. Compositors kept SIGBUS-handler hacks instead [S513]. Simon Ser's 2021 proposal for an OpenBSD-style `__MAP_NOFAULT` (out-of-bounds reads return zeros) shows the problem was still open [S513].

**Alternatives that exist.**
- File sealing, so the receiver can verify that the buffer cannot shrink [S511][S512].
- No-fault mappings, as in OpenBSD [S513].
- Explicit buffer-pool I/O (`pread`, O_DIRECT, io_uring), where errors come back as return values [S510].

**Cost of the alternatives.** Explicit I/O needs a user-space cache and copies, which is more code. Sealing needs cooperation from every client [S513]. Zero-fill-on-fault hides truncation instead of reporting it.

**Relevance to our OS.** high — the compositor will receive client buffers from untrusted processes, so the shared-memory object type must make "cannot shrink or change under you" checkable, and must not require signal handlers.

**Sources.** [S509], [S510], [S511], [S512], [S513]

### DM-032: Executable stacks/heaps by default; W^X retrofitted

- **Category:** Memory management
- **Systems affected:** Unix/Linux, Windows (pre-DEP), OpenBSD, macOS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Historically, any readable page of a process could be executed, including the stack, the heap and data. W^X ("write xor execute") is the policy that a page is writable or executable but not both. It was added to existing systems in the early 2000s.

**Why it was chosen.** Much hardware had no per-page execute permission. OpenBSD 3.3 (2003) could enforce W^X only on sparc, sparc64, alpha and hppa, because those MMUs had a real execute bit. i386 and powerpc came later [S514]. Trampolines, thunks and JITs placed code in data pages, and nothing in the threat model of the time forbade it.

**Why it is a problem.** Executable data pages are what makes classic buffer-overflow code injection work. DEP and W^X exist to stop it [S516][S518]. Retrofitting shows how much of the cost is compatibility:
- **Windows DEP.** It broke JITs that did not mark their code executable, and ATL 7.1-era thunks [S516].
- **Linux.** In 2020, x86 binaries without a `PT_GNU_STACK` marking still got every readable mapping made executable [S519].
- **glibc 2.41 (2025).** `dlopen` stopped making the stack executable on behalf of libraries [S520]. That broke Steam games, Discord, Julia and MATLAB, and forced a tunable that turns executable stacks back on [S521].
- **OpenBSD.** Strict enforcement came only in 6.0 (2016), and still needs a `wxallowed` escape hatch because ports violate it [S515].

W^X alone does not stop return-to-existing-code attacks [S518]. Plain W^X also allows a mapping to switch from W to X later, unlike PaX MPROTECT or iOS [S517] (weak source).

**Why it was not fixed.** Earlier, hardware support was missing [S514]. Later, binaries and toolchain defaults made the stack executable through missing markings [S519][S520][S521].

**Alternatives that exist.**
- OpenBSD mandatory W^X with a per-binary opt-out [S515].
- Windows DEP with explicit `PAGE_EXECUTE*` allocation and a write-then-protect pattern [S516].
- Apple silicon enforces W^X for all apps. JITs must use a single `MAP_JIT` region with per-thread write/execute toggling, behind an entitlement [S522].

**Cost of the alternatives.** JITs need explicit permission flips, entitlements and instruction-cache maintenance [S522]. Some languages' nested-function trampolines need executable stacks [S519] (inferred). Every enforcement step has broken shipped software [S516][S521].

**Relevance to our OS.** high — on aarch64 with no legacy binaries, W^X can be an invariant from day one. The open question is how the POSIX layer and a hosted C compiler or JIT get executable memory: as a capability, or through a W-to-X transition API.

**Sources.** [S514], [S515], [S516], [S517], [S518], [S519], [S520], [S521], [S522]

### DM-033: Kernel mapped into every user address space; Meltdown/KPTI

- **Category:** Memory management
- **Systems affected:** Linux, Windows, macOS, BSDs on affected x86 and some Arm cores
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Mainstream kernels mapped their own memory into the top of every process's address space and protected it only with supervisor-only page permissions. A system call or interrupt then needs no page-table switch.

**Why it was chosen.** Performance. Keeping the kernel mapped avoids flushing the TLB on every entry to and exit from the kernel [S526]. Page-permission checks were assumed to be enforced architecturally, including during speculation.

**Why it is a problem.** Meltdown showed that out-of-order execution on affected CPUs reads kernel memory from user space before the permission fault takes effect. It needs no software bug and breaks address-space isolation, including in cloud settings [S523]. The fix was KAISER, later KPTI: two page-table sets, with only an entry stub mapped while user code runs [S525]. KAISER had been proposed months earlier against KASLR side channels [S524], and the Meltdown authors said it must be deployed immediately [S523].
- **Per-entry cost.** A CR3 write of about a hundred cycles on every kernel entry and exit, plus TLB flushes without PCID [S525].
- **Workload cost.** Typical regressions were single-digit percentages, about 30% in the worst syscall-heavy case [S526].
- **Memory.** Extra memory per process [S525].
- **arm64.** KPTI was also expensive there and incompatible with the Statistical Profiling Extension (SPE) [S527].

**Why it was not fixed.** It was fixed, but as an emergency retrofit, and the overhead is paid only on affected hardware [S525]. The underlying hazard (shared mappings plus speculation) is a property of hardware, so the right choice depends on the CPU.

**Alternatives that exist.**
- Separate kernel address space (KPTI-style) [S525].
- ARMv8.5 E0PD, which makes EL0 accesses to the kernel half fault in constant time without unmapping, so Linux can skip KPTI on CPUs that support it [S527].
- PCID or ASIDs to reduce flush cost [S526].

**Cost of the alternatives.** Full separation costs time on every syscall and interrupt [S525][S526]. E0PD needs ARMv8.5 or later on every CPU in the system [S527]. Whether E0PD fully covers speculative access is something I could not settle from primary sources [unverified].

**Relevance to our OS.** medium — aarch64's split TTBR0/TTBR1 design and E0PD make the shared mapping cheap to protect on new cores, but QEMU `virt` on Apple Silicon and future CPUs need a policy for when hardware isolation is not trustworthy.

**Sources.** [S523], [S524], [S525], [S526], [S527]

### DM-034: No first-class memory-pressure signalling to applications

- **Category:** Memory management
- **Systems affected:** Linux (until PSI, 2018), traditional Unix; contrast Windows, iOS/macOS, Android
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Classic Unix gives a process no standard way to learn that the system is short of memory. Pressure shows up as swapping, slowdowns, and finally an OOM kill. Linux added Pressure Stall Information (PSI) in 2018: per-system and per-cgroup stall-time metrics with pollable thresholds [S504].

**Why it was chosen.** On swap-backed time-sharing systems, memory pressure was meant to be invisible. The kernel paged things out, and applications were not expected to cooperate. Load average looked like enough of a signal (inferred from [S505]).

**Why it is a problem.** Without a signal, caches cannot shrink before the system is hurting, and killers act too late.
- **Linux metrics.** Load average does not measure time lost to waiting [S505]. Before PSI there was no accurate measure of lost productivity, so operators either under-used hardware or accepted disruptions [S504].
- **Android.** The in-kernel low-memory killer used fixed free-memory thresholds, thrashed on low-RAM devices without killing anything, slowed reclaim, and was forked by vendors. It was removed in kernel 4.12. Its replacement, userspace lmkd, first ran on vmpressure events full of false positives, and moved to PSI in Android 10 [S506].
- **iOS.** Even with signals, delivery is best-effort. When demand rises faster than apps respond, the system "jettisons" apps [S508].

**Why it was not fixed.** For decades there was no agreed metric. PSI came from Facebook's container fleet [S505]. It now has to be adopted by userspace managers, one at a time [S506].

**Alternatives that exist.**
- Windows has `CreateMemoryResourceNotification` since XP: waitable low and high memory objects [S507].
- iOS has app-delegate callbacks, notifications and a dispatch memory-pressure source, plus kernel-discardable `NSPurgeableData` [S508].
- Linux has PSI triggers that can be polled through file descriptors, including by unprivileged users with coarse windows [S504].

**Cost of the alternatives.** Notifications are advisory and system-wide [S507]. Responding wrongly, for example by walking the whole object graph, can make pressure worse by pulling pages out of compression [S508]. PSI still needs a policy daemon to act on it [S506].

**Relevance to our OS.** high — a compositor-based desktop with caching clients benefits from a pressure event channel designed with the memory model (for example, purgeable or discardable memory objects) instead of retrofitted.

**Sources.** [S504], [S505], [S506], [S507], [S508]

### DM-035: `brk`/`sbrk` legacy heap interface

- **Category:** Memory management
- **Systems affected:** Unix/POSIX historically, Linux/glibc (main arena), BSDs
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** `brk`/`sbrk` move a single "program break", the end of one contiguous data segment, to grow or shrink the heap. They come from 4.3BSD, were marked LEGACY in SUSv2, and were removed in POSIX.1-2001 [S528].

**Why it was chosen.** With a simple linear address-space layout and no general `mmap`, one growable segment was the obvious and cheap heap primitive.

**Why it is a problem.**
- **One shared break.** The break is a single per-process cursor, and memory can only be returned to the kernel from the top. The old BSD malloc could return memory "only in rare circumstances" [S529]. glibc trims only when the free space at the top of the heap exceeds a threshold, and sends large allocations to `mmap` instead [S530].
- **Predictability.** A contiguous heap is predictable, which helps exploitation. OpenBSD's mmap-based malloc gets randomization, out-of-band metadata and guard behaviour, and was reported as about 30% faster [S529].
- **Inconsistent API.** The argument type of `sbrk` varies across systems, and the man page advises avoiding both calls [S528].

**Why it was not fixed.** It does not need fixing. Modern allocators already avoid or minimise it [S529][S530]. It survives for binary compatibility, and OpenBSD notes it still reduces the address space available to applications on 32-bit systems [S529].

**Alternatives that exist.** Allocators built only on `mmap` (OpenBSD malloc [S529]), and hybrid allocators (glibc [S530]).

**Cost of the alternatives.** More system calls and page-granular waste for small heaps, and the kernel must zero fresh pages [S530].

**Relevance to our OS.** low — a new OS should simply not provide a break. The POSIX layer can emulate `sbrk` over an address-space-reservation primitive if a ported program needs it.

**Sources.** [S528], [S529], [S530]

## 10. Time, clocks and scheduling

### DM-063: 32-bit `time_t` and the year 2038

- **Category:** Time, clocks and scheduling
- **Systems affected:** Unix/POSIX ABIs on 32-bit platforms, Linux kernel and C libraries, filesystems with 32-bit on-disk timestamps
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Unix time is a count of seconds since 1970. On 32-bit ABIs it was stored as a signed 32-bit `time_t`, which overflows in January 2038. The width is baked into structs (`timeval`, `timespec`, `input_event`), ioctls, syscalls and on-disk formats [S531].

**Why it was chosen.** On 1970s hardware, a machine word was cheap and the range (to 2038) looked far beyond the system's expected lifetime. Signed values allowed dates before 1970.

**Why it is a problem.** The fix is a whole-ABI transition, not a type change:
- **Kernel.** Linux 5.6 (2020) was the first kernel usable as a base for 32-bit systems that must run past 2038 [S531].
- **Filesystems.** ext4 with small inodes, ext2, XFS (at the time) and UFS keep signed 32-bit timestamps on disk [S531].
- **User space.** User space must be rebuilt [S531]. Runtimes that make syscalls directly (futex, seccomp) must move to new time64 syscalls [S531]. Some interfaces, most importantly `struct input_event`, cannot be fixed compatibly and fall back to CLOCK_MONOTONIC or unsigned 32-bit times, which only pushes the limit to 2106 [S531].
- **glibc.** glibc 2.34 (2021) made 64-bit `time_t` opt-in through `_TIME_BITS=64`, which also requires `_FILE_OFFSET_BITS=64`, and left the default at 32 bits [S532].

Any library whose ABI includes `time_t` changes ABI when this is switched, which is why distributions treat it as a mass transition [S532].

**Why it was not fixed.** ABI stability. Every struct and syscall containing time needed a duplicate, and distributions had to coordinate library rebuilds [S531][S532].

**Alternatives that exist.** 64-bit `time_t` on all 64-bit ABIs, and the time64 syscall set on 32-bit Linux [S531]. Windows FILETIME is a 64-bit count of 100 ns units since 1601 [S541].

**Cost of the alternatives.** Larger structs. On 32-bit Linux, a decade of duplicated interfaces and coordinated rebuilds [S531][S532].

**Relevance to our OS.** medium — the OS is 64-bit-only, so `time_t` width is easy. The lesson is to keep time types (width, epoch, resolution) out of fixed-layout ABIs and on-disk formats unless they are versioned.

**Sources.** [S531], [S532], [S541]

### DM-064: Leap seconds in POSIX time

- **Category:** Time, clocks and scheduling
- **Systems affected:** POSIX (specification), Linux, all NTP-synchronised systems
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** POSIX defines "seconds since the Epoch" so that every day has exactly 86400 seconds. It leaves the relationship to actual UTC unspecified [S533]. UTC, however, inserts leap seconds to track Earth's rotation, so a POSIX clock must repeat or skip a second, or be smeared [S534][S535].

**Why it was chosen.** It makes calendar arithmetic simple and table-free: converting a timestamp to a date needs no leap-second table and gives the same result on every machine.

**Why it is a problem.** Every leap second exercises a code path that is used almost never:
- **2012.** The Linux timekeeping code did not notify hrtimers. CLOCK_REALTIME timers fired a second early and caused futex load spikes in MySQL, Java and Firefox [S536].
- **2017.** A clock stepping backwards produced a negative duration that panicked Cloudflare's DNS server [S537].

Workarounds diverge. Google has smeared since 2008, using different smear shapes over time. Other organisations smear over 1000 s, 2000 s or configurable cosines, so clocks disagree by up to about 0.5 s around each event [S534]. The CGPM cited exactly these disruptions and uncoordinated practices, plus an untested negative leap second [S535].

**Why it was not fixed.** POSIX cannot represent 23:59:60 without breaking the 86400-second invariant that existing code relies on [S533]. Linux offers CLOCK_TAI, but its offset must be supplied by NTP [S538].

**Alternatives that exist.**
- Leap smearing [S534].
- Separate TAI and monotonic clocks [S538].
- The policy fix: the CGPM resolved in 2022 to raise the UT1-UTC tolerance by 2035, effectively ending leap seconds [S535].

**Cost of the alternatives.** A smear makes clocks wrong by up to half a second relative to UTC for a day, and mixed smear policies disagree [S534]. TAI-based system time needs a leap table to display civil time.

**Relevance to our OS.** medium — leap seconds are being phased out, but the general lesson stands. Define which time scale the kernel exposes, and never let civil-time discontinuities reach interval timers.

**Sources.** [S533], [S534], [S535], [S536], [S537], [S538]

### DM-065: Wall clock vs monotonic clock misuse; monotonic clocks added late

- **Category:** Time, clocks and scheduling
- **Systems affected:** POSIX/Unix (`gettimeofday`, `time`), Linux, language runtimes (Go before 1.9); contrast Windows QPC
- **Classification:** mistake
- **Confidence:** high

**What the design is.** For decades the default Unix time APIs returned only wall-clock time, which can be stepped by an administrator, NTP or leap seconds. CLOCK_MONOTONIC appeared as an option in POSIX.1-1996 and became mandatory only in POSIX.1-2024. Linux added MONOTONIC_RAW in 2.6.28 and BOOTTIME (which includes suspend) only in 2.6.39 [S538].

**Why it was chosen.** Early hardware had a single time-of-day clock and no stable high-rate counter. On Windows, early QPC was unreliable on multi-core systems with non-invariant TSCs, for example [S541]. One clock looked simpler.

**Why it is a problem.** Programs measure intervals by subtracting wall-clock readings, and those intervals go negative or jump.
- **Prevalence.** A survey of Go code found about 29% of `time.Now` calls were used for elapsed time and were exposed to clock resets [S539].
- **Incident.** Cloudflare's 2017 DNS outage was a negative duration from `time.Now()` [S537].
- **Too many clocks.** Linux now has several monotonic clocks with different suspend semantics. In 2018 an attempt to make CLOCK_MONOTONIC include suspend time was reverted: systemd watchdogs killed daemons after resume, journald declared journals corrupt, and NetworkManager and MATE misbehaved [S540]. Once programs depend on a clock's documented semantics, the clock cannot be fixed.

**Why it was not fixed.** Existing APIs could not change meaning [S540]. The fixes were new clocks [S538] and runtime-level changes. Go embedded a monotonic reading inside `time.Time` precisely so that existing code would be fixed without edits, arguing that two near-identical APIs invite rare, simultaneous failures [S539].

**Alternatives that exist.**
- Windows QPC: monotonic, unaffected by DST, leap seconds or time setting, kept separate from the UTC API [S541].
- Go's dual-reading `Time` [S539].
- Linux's explicit clock IDs [S538].

**Cost of the alternatives.** Dual readings cost extra clock reads and a larger struct [S539]. Multiple clocks force every API with a timeout to say which clock it means. The suspend question (count it or not) has no single right answer [S540].

**Relevance to our OS.** high — the timeout and deadline types in the native syscall ABI can be monotonic-only by construction, with suspend semantics specified up front. The Arm Generic Timer gives a fixed-frequency counter for this [S541].

**Sources.** [S537], [S538], [S539], [S540], [S541]

### DM-066: Time zones via `TZ` and process-global state

- **Category:** Time, clocks and scheduling
- **Systems affected:** POSIX/C library (`tzset`, `localtime`, `setenv`), glibc, Rust ecosystem on Unix
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** The C library's local time depends on the `TZ` environment variable (or `/etc/localtime`), read by `tzset`. `tzset` is called implicitly by conversion functions and stores its results in the globals `tzname`, `timezone` and `daylight` [S542]. There is one time zone per process, held in global mutable state.

**Why it was chosen.** It suited single-threaded processes with one user at one location, and passing configuration through the environment was idiomatic Unix.

**Why it is a problem.**
- **Globals are unsafe.** Reading the globals can be undefined behaviour in multithreaded programs [S542].
- **Hidden environment reads.** Time conversion reads the environment, while `setenv`/`unsetenv` are MT-Unsafe [S543]. If any thread changes any environment variable while another formats a local time, the result can be a crash. Rust's chrono had a segfault advisory (RUSTSEC-2020-0159, related CVE-2020-26235) triggered when a third-party library set an environment variable in another thread [S544].
- **Language workarounds.** Rust eventually made `std::env::set_var` `unsafe` in the 2024 edition [S545].
- **Single zone.** A server formatting times for many users cannot do so with one process-global zone [S546].

**Why it was not fixed.** The interface is in ISO C and POSIX, and the globals are part of the ABI [S542]. Thread-safe variants only appeared as extensions [S546].

**Alternatives that exist.** NetBSD `tzalloc`/`localtime_rz` pass an explicit `timezone_t` object, so many zones can be used without touching `TZ` [S546]. Languages with explicit zone objects do the same [unverified for specific libraries].

**Cost of the alternatives.** Every call site has to pass a zone, and the system zone needs some discovery mechanism that is not ambient environment.

**Relevance to our OS.** medium — time zones are user-space policy. The lesson for the OS is to avoid process-global mutable configuration (environment) that library code reads implicitly. The POSIX layer must still emulate `TZ`.

**Sources.** [S542], [S543], [S544], [S545], [S546]

### DM-067: Fixed periodic timer tick (HZ)

- **Category:** Time, clocks and scheduling
- **Systems affected:** Linux (pre-NO_HZ and still in some configurations), Windows (global timer resolution), classic Unix
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** The kernel takes a periodic interrupt at a fixed rate (HZ) on every CPU, whether or not there is work. It uses the interrupt for timekeeping, timers and time-slice preemption [S547]. Time is often counted in ticks (jiffies) [S549].

**Why it was chosen.** It is simple. One periodic hardware timer drives everything. It is still the right choice for heavily loaded systems whose idle gaps are short [S547].

**Why it is a problem.**
- **Energy.** A periodic tick wastes energy on idle systems. The kernel documentation cites battery drain two to three times faster [S547].
- **Virtualization.** In a mainframe running 1,500 guests, about half the CPU went on unneeded ticks [S547].
- **Jitter.** The tick adds jitter to real-time and HPC work [S547].
- **Wraparound bugs.** Tick counters wrap, so Linux deliberately starts jiffies 5 minutes before the 32-bit wrap to make wrap bugs show up early [S549].
- **Windows.** A related mistake: before Windows 10 2004, `timeBeginPeriod` set a global timer resolution. Any one process asking for 1 ms raised interrupt frequency for every process and prevented power-saving states [S548].

**Why it was not fixed.** It has been retrofitted, at a cost. Tickless idle adds work on idle entry and exit. NO_HZ_FULL needs a housekeeping CPU and RCU callback offloading, works only with exactly one runnable task, keeps a residual tick of about 1 Hz, and needs a reboot to reconfigure [S547]. Windows changed `timeBeginPeriod` semantics only in 2020, and again for hidden windows in Windows 11 [S548].

**Alternatives that exist.** One-shot, deadline-programmed timers (dynticks) [S547], timer coalescing, and per-process timer resolution [S548].

**Cost of the alternatives.** More clock reprogramming. Higher latency for real-time on some hardware. Complex interactions with RCU, load balancing and accounting [S547].

**Relevance to our OS.** high — a new aarch64 kernel can be tickless from the start using the Generic Timer's compare registers, and can account time in nanoseconds rather than ticks. Retrofitting this later is costly, as Linux shows.

**Sources.** [S547], [S548], [S549]

### DM-068: General-purpose scheduler design churn (O(1) → CFS → EEVDF; sched_ext)

- **Category:** Time, clocks and scheduling
- **Systems affected:** Linux; implicitly any OS with a single general-purpose scheduler
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Linux has one in-kernel general-purpose fair scheduler for all workloads, from phones to servers. CFS, merged in 2.6.23 (2007) [S553], was replaced by EEVDF starting in 6.6 [S551]. sched_ext adds a scheduler class whose policy is supplied at runtime as BPF programs [S550]. (The earlier O(1) scheduler is not covered by sources fetched here [unverified].)

**Why it was chosen.** One scheduler means one place to fix bugs, and no fragmentation of tuning across vendors. CFS was conceptually simple, a weighted fair queue on a single CPU [S552].

**Why it is a problem.**
- **Multicore bugs.** Multicore and NUMA workarounds made CFS very complex. Lozi et al. found four bugs that left cores idle while threads waited: 13–24% losses on typical workloads, 138x in a corner case. The bugs evaded conventional tools [S552].
- **No way to express latency.** CFS could not express latency needs. It relied on fragile heuristics, and latency-sensitive tasks were pushed toward privileged real-time classes [S553].
- **Churn.** EEVDF replaces heuristics with deadlines and per-task slice requests via `sched_setattr` [S551][S553]. That is another full scheduler replacement 16 years after CFS.

**Why it was not fixed.** This is a governance question as much as a technical one.
- **Out-of-tree pressure.** Out-of-tree desktop schedulers such as Kolivas's BFS claimed better interactivity and openly gave up NUMA and large-SMP scaling. They never targeted mainline [S555] (weak).
- **Contested merge.** sched_ext's merge was disputed. Torvalds planned to merge over the scheduler maintainers' objections, and Gleixner asked for time to integrate it collaboratively [S554].

**Alternatives that exist.**
- **Pluggable policy.** Pluggable scheduling policy with a safe fallback: sched_ext reverts to the fair class on error, stall or SysRq-S [S550].
- **Clearer policies.** Better-defined algorithms (EEVDF) [S551].
- **Per-workload schedulers.** Per-workload or user-level schedulers, which is the argument for sched_ext [S550].

**Cost of the alternatives.** The two positions:
- **Pro-pluggable.** Experimentation and workload-specific policies without kernel forks [S550].
- **Anti-pluggable.** Fragmentation, and improvements that never reach the default scheduler, as reflected in the maintainers' objections [S554]. Exactly which arguments Zijlstra made is not captured verbatim in the fetched sources [unverified].
- **Shared risk.** A single scheduler concentrates risk, as the wasted-cores bugs show [S552].

**Relevance to our OS.** medium — the choice between one built-in policy and a minimal kernel mechanism with policy elsewhere (in user space or loadable) should be explicit early, including how a desktop compositor expresses latency needs.

**Sources.** [S550], [S551], [S552], [S553], [S554], [S555]

## Stream notes

**Searched.**
- **Memory management.** Linux overcommit docs and LWN OOM-killer history. Windows commit-charge, DEP and memory-notification docs. PSI docs and LWN PSI coverage. Android lmkd. Apple memory warnings and JIT docs. mmap, memfd, brk and mallopt man pages. The Crotty/Leis/Pavlo CIDR paper (text extracted locally from the PDF). The LWN sealing article and Simon Ser's 2021 LKML post. OpenBSD 3.3 and 6.0 notes and Moerbeek's malloc slides. Kees Cook's READ_IMPLIES_EXEC patch and glibc 2.41 notes. Meltdown (USENIX page), the KAISER RFC, kernel PTI docs, LWN KAISER, and the arm64 E0PD commit.
- **Time, clocks and scheduling.** The Bergmann y2038 pull and glibc 2.34 announcement. POSIX.1-2024 §4.19. Google leap smear, CGPM 2022 Resolution 4, Stultz's 2012 leap-second patches, and the Cloudflare 2017 post. clock_gettime(2), the Go monotonic design doc, the 2018 MONOTONIC/BOOTTIME revert, and Windows QPC docs. tzset/setenv man pages, RUSTSEC-2020-0159, the Rust 2024 edition guide, and NetBSD tzalloc. NO_HZ docs, timeBeginPeriod, and jiffies.h. sched_ext and EEVDF kernel docs, the Lozi et al. EuroSys 2016 paper (text extracted locally), LWN on EEVDF, the Gleixner LKML reply on sched_ext, and Linux Magazine on BFS.

**Dropped/merged.** None. DM-035 was kept but at low relevance; it could be merged into a general "legacy address-space interfaces" note if the coordinator prefers.

**Classification changes vs index.** DM-033 is classed as tradeoff, not right-at-the-time: the cost-benefit depends on the hardware, and E0PD and fixed CPUs remove the need for KPTI. DM-030 is classed as tradeoff, not contested. Practitioner disagreement exists, but the only anti-overcommit voices I found were LWN reader comments (weak), so I did not present it as a sourced two-sided dispute.

**New entries.** None added. Candidate for DM-140 that I did not research: Transparent Huge Pages default behaviour causing latency spikes (vendors such as databases recommend disabling it). Unverified, not written.

**Open questions.**
- Whether ARMv8.5 E0PD fully blocks speculative access to the kernel half. The LWN comment thread disagrees, and the commit message claims only constant-time faults.
- The O(1) scheduler era and the precise substance of Zijlstra's sched_ext objections were not captured from primary sources.
- The final merge version of sched_ext (6.12) is not confirmed by fetched sources.
- The Debian/Gentoo time_t transition details: LWN was rate-limited and the Debian slides could not be text-extracted, so they are not cited.

**Weak or suspect sources.**
- S503 (LWN reader comments) and S517 (isopenbsdsecu.re, an OpenBSD-critical site) are labelled weak.
- S555 (Linux Magazine news brief) is labelled weak.
- S521 (Phoronix) is secondary.
- S518 is a Bugtraq reply that quotes de Raadt rather than his own post.
- S527 and S540 were read via the GitHub mirror commit pages; the commit dates were not displayed.
- S510 and S552 were read by local text extraction of the PDFs, not by WebFetch summarisation.
- S550–S555 were checked on 2026-10-20, after a tool rate-limit interruption.
