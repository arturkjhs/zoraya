### DM-031: `mmap`'d files: errors surface as `SIGBUS`; truncation hazards

- **Category:** Memory management
- **Systems affected:** POSIX/Unix generally, Linux; Wayland compositors (shared-memory buffers); databases using mmap
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** `mmap` makes file contents appear as ordinary memory. I/O happens through page faults, so there is no call that can return an error. Touching a page past end-of-file raises SIGBUS. The effect of the file changing size under a mapping is unspecified [S509].

**Why it was chosen.** It is a uniform, zero-copy way to read files and share memory. The page cache is reused directly, with no system call per access and no duplicate buffer in user space [S510].

**Why it is a problem.** Errors that `read` would return become signals that can arrive at any load or store:
- **I/O errors.** Any code that touches mapped memory can receive SIGBUS, and a DBMS has to handle it with cumbersome signal handlers. Pages corrupted in memory (for example by pointer bugs) are written back silently, with no chance to check them first [S510].
- **Hidden stalls.** Any access may block on I/O, and there is no asynchronous read path [S510].
- **Scalability.** Page eviction and TLB shootdowns limit scaling on fast storage [S510].
- **Abandonment.** MongoDB (MMAPv1) and InfluxDB moved off mmap after significant engineering cost [S510]. (MongoDB later reintroduced mmap in a limited role [S510]; the paper is a position paper by buffer-pool advocates.)
- **Hostile peers.** A process sharing a file can truncate it and crash the other side with SIGBUS, or change the data after it has been validated [S315][S512]. The 2014 sealing patchset listed the compositor-receives-client-buffer case as its first use case [S513].

**Why it was not fixed.** Sealing (`memfd_create` plus `F_SEAL_SHRINK`, Linux 3.17) gives a fix that both sides must opt into [S315]. Seven years later no Wayland compositor enforced it, because old clients and portable clients use `shm_open`. Compositors kept relying on SIGBUS handlers instead [S513]. Simon Ser's 2021 proposal for an OpenBSD-style `__MAP_NOFAULT` (out-of-bounds reads return zeros) shows the problem was still open [S513].

**Alternatives that exist.**
- File sealing, so the receiver can verify that the buffer cannot shrink [S315][S512].
- No-fault mappings, as in OpenBSD [S513].
- Explicit buffer-pool I/O (`pread`, O_DIRECT, io_uring), where errors come back as return values [S510].

**Cost of the alternatives.** Explicit I/O needs a user-space cache and copies, which is more code. Sealing needs cooperation from every client [S513]. Zero-fill-on-fault hides truncation instead of reporting it.

**Relevance to our OS.** high — the compositor will receive client buffers from untrusted processes, so the shared-memory object type must make "cannot shrink or change under you" checkable, and must not require signal handlers.

**Sources.** [S509], [S510], [S315], [S512], [S513]

### DM-032: Executable stacks/heaps by default; W^X retrofitted

- **Category:** Memory management
- **Systems affected:** Unix/Linux, Windows (pre-DEP), OpenBSD, macOS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Historically, any readable page of a process could be executed, including the stack, the heap and data. W^X ("write xor execute") is the policy that a page is writable or executable but not both. It was added to existing systems in the early 2000s.

**Why it was chosen.** Much hardware had no per-page execute permission. OpenBSD 3.3 (2003) could enforce W^X only on sparc, sparc64, alpha and hppa, because those MMUs had a real execute bit. i386 and powerpc came later [S514]. Trampolines, thunks and JITs placed code in data pages, and nothing in the threat model of the time forbade it. The constraint was soft rather than absolute: PaX emulated non-executable pages on x86 from 2000, before OpenBSD shipped W^X [S517] (weak source).

**Why it is a problem.** Executable data pages are what makes classic buffer-overflow code injection work. DEP and W^X exist to stop it [S516][S518]. Retrofitting shows how much of the cost is compatibility:
- **Windows DEP.** It broke JITs that did not mark their code executable, and ATL 7.1-era thunks [S516].
- **Linux.** In 2020, x86 binaries without a `PT_GNU_STACK` marking still got every readable mapping made executable [S519].
- **glibc 2.41 (2025).** `dlopen` stopped making the stack executable on behalf of libraries [S520]. That broke Steam games, Discord, Julia and MATLAB, and forced a tunable that turns executable stacks back on [S521].
- **OpenBSD.** Strict enforcement came only in 6.0 (2016), and still needs a `wxallowed` escape hatch because ports violate it [S515].

W^X alone does not stop return-to-existing-code attacks [S518]. Plain W^X also allows a mapping to switch from W to X later, unlike PaX MPROTECT or iOS [S517] (weak source).

**Why it was not fixed.** Earlier, hardware support was missing [S514]. Later, binaries and toolchain defaults made the stack executable through missing markings [S519][S520][S521].

**Alternatives that exist.**
- OpenBSD mandatory W^X; violating binaries must be marked `PT_OPENBSD_WXNEEDED` and live on a `wxallowed` file system [S515].
- Windows DEP with explicit `PAGE_EXECUTE*` allocation and a write-then-protect pattern [S516].
- Apple silicon enforces W^X for all apps. JITs use a `MAP_JIT` region with per-thread write/execute toggling; under the Hardened Runtime this needs an entitlement and is limited to one region [S522].

**Cost of the alternatives.** JITs need explicit permission flips, entitlements and instruction-cache maintenance [S522]. Stack trampolines and unmarked assembly still need an executable stack [S519]. Every enforcement step has broken shipped software [S516][S521].

**Relevance to our OS.** high — on aarch64 with no legacy binaries, W^X can be an invariant from day one. The open question is how the POSIX layer and a hosted C compiler or JIT get executable memory: as a capability, or through a W-to-X transition API.

**Sources.** [S514], [S515], [S516], [S517], [S518], [S519], [S520], [S521], [S522]

### DM-033: Kernel mapped into every user address space; Meltdown/KPTI

- **Category:** Memory management
- **Systems affected:** Linux, Windows, macOS, BSDs on affected x86 and some Arm cores
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Mainstream kernels mapped their own memory into the top of every process's address space and protected it only with supervisor-only page permissions. A system call or interrupt then needs no page-table switch.

**Why it was chosen.** Performance. Keeping the kernel mapped avoids flushing the TLB on every entry to and exit from the kernel [S526]. Page-permission checks were assumed to be enforced architecturally, including during speculation (inferred; the Meltdown paper describes memory isolation as the foundation that this broke [S523]).

**Why it is a problem.** Meltdown showed that out-of-order execution on affected CPUs reads kernel memory from user space before the permission fault takes effect. It needs no software bug and breaks address-space isolation, including in cloud settings [S523]. The fix was KAISER, later KPTI: two page-table sets, with only an entry stub mapped while user code runs [S525]. KAISER had been proposed months earlier against KASLR side channels [S524], and the Meltdown authors said it must be deployed immediately [S523].
- **Per-entry cost.** A CR3 write of about a hundred cycles on every kernel entry and exit, plus TLB flushes without PCID [S525].
- **Workload cost.** Typical regressions were single-digit percentages (about 5%), with about 30% on a loopback networking test as the worst case reported [S526].
- **Memory.** Extra memory per process [S525].
- **arm64.** KPTI was also expensive there and incompatible with the Statistical Profiling Extension (SPE) [S527].

**Why it was not fixed.** It was fixed, but as an emergency retrofit [S523][S525]. PTI can be disabled at boot (`nopti`) [S525]; that it is enabled by default only on affected CPUs is [unverified] from the cited sources. The underlying hazard (shared mappings plus speculation) is a property of hardware, so the right choice depends on the CPU.

**Alternatives that exist.**
- Separate kernel address space (KPTI-style) [S525].
- ARMv8.5 E0PD, which makes EL0 accesses to the kernel half fault in constant time without unmapping. It is enabled only if every CPU supports it, and the initial patch states the aim of not using KPTI on such CPUs, leaving that integration to follow-up patches [S527].
- PCID or ASIDs to reduce flush cost [S526].

**Cost of the alternatives.** Full separation costs time on every syscall and interrupt [S525][S526]. E0PD needs ARMv8.5 or later on every CPU in the system [S527]. Whether E0PD fully covers speculative access is something I could not settle from primary sources [unverified].

**Relevance to our OS.** medium — aarch64's split TTBR0/TTBR1 design and E0PD make the shared mapping cheap to protect on new cores, but QEMU `virt` on Apple Silicon and future CPUs need a policy for when hardware isolation is not trustworthy.

**Sources.** [S523], [S524], [S525], [S526], [S527]

### DM-034: No first-class memory-pressure signalling to applications

- **Category:** Memory management
- **Systems affected:** Linux (until PSI, 2018), traditional Unix; contrast Windows, iOS/macOS, Android
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Classic Unix gives a process no standard way to learn that the system is short of memory. Pressure shows up as swapping, slowdowns, and finally an OOM kill. Linux added Pressure Stall Information (PSI) in 2018: per-system and per-cgroup stall-time metrics with pollable thresholds [S504].

**Why it was chosen.** On swap-backed time-sharing systems, memory pressure was meant to be invisible. The kernel paged things out, and applications were not expected to cooperate. Load average looked like enough of a signal (inferred from [S505]). There is no direct evidence that this was a deliberate, constraint-driven choice, and Windows has offered waitable memory notifications since XP [S507], so the entry treats it as a tradeoff (transparent paging versus application cooperation) rather than as right-at-the-time.

**Why it is a problem.** Without a signal, caches cannot shrink before the system is hurting, and killers act too late.
- **Linux metrics.** Load average does not measure time lost to waiting [S505]. Before PSI there was no accurate measure of lost productivity, so operators either under-used hardware or accepted disruptions [S504].
- **Android.** The in-kernel low-memory killer used fixed free-memory thresholds, thrashed on low-RAM devices without killing anything, slowed reclaim, and was customized by device partners. It was removed in kernel 4.12. Its replacement, userspace lmkd, first ran on vmpressure events full of false positives, and moved to PSI in Android 10 [S506].
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

**What the design is.** `brk`/`sbrk` move a single "program break", the end of one contiguous data segment, to grow or shrink the heap. They date back at least to Seventh Edition Unix [S2200]; the Linux man page lists them under 4.3BSD and SUSv1, marked LEGACY in SUSv2 and removed in POSIX.1-2001 [S528].

**Why it was chosen.** With a simple linear address-space layout and no general `mmap` (inferred), one growable segment was the obvious and cheap heap primitive. The V7 manual shows the hardware model: the break was rounded to 64 bytes on the PDP-11 and could fail if too many segmentation registers would be needed [S2200].

**Why it is a problem.**
- **One shared break.** The break is a single per-process cursor, and memory can only be returned to the kernel from the top. The old BSD malloc could return memory "only in rare circumstances" [S529]. glibc trims only when the free space at the top of the heap exceeds a threshold, and sends large allocations to `mmap` instead [S530].
- **Predictability.** A contiguous heap is predictable, which helps exploitation. OpenBSD's mmap-based malloc gets randomization, out-of-band metadata and guard behaviour, and early speed tests showed about a 30% speedup over the old malloc, though less in the final version [S529].
- **Inconsistent API.** The argument type of `sbrk` varies across systems, and the man page advises avoiding both calls [S528].

**Why it was not fixed.** It does not need fixing. Modern allocators already avoid or minimise it [S529][S530]. It survives for binary compatibility, and OpenBSD notes it still reduces the address space available to applications on 32-bit systems [S529].

**Alternatives that exist.** Allocators built only on `mmap` (OpenBSD malloc [S529]), and hybrid allocators (glibc [S530]).

**Cost of the alternatives.** More system calls and page-granular waste for small heaps, and the kernel must zero fresh pages [S530].

**Relevance to our OS.** low — a new OS should simply not provide a break. The POSIX layer can emulate `sbrk` over an address-space-reservation primitive if a ported program needs it.

**Sources.** [S528], [S529], [S530], [S2200]


### DM-036: POSIX advisory record locks are owned by the process and dropped on any close()

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** POSIX (specified behaviour), Linux, BSDs, macOS, any POSIX-conformant system
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `fcntl(F_SETLK/F_SETLKW)` byte-range locks belong to a (process, inode) pair, not to the file descriptor or open file description. POSIX says that when a process closes *any* descriptor for a file, all of that process's locks on the file go away, even locks taken through a different open file description [S603]. The locks are not inherited across `fork()`, and threads in one process never conflict with each other [S600].

**Why it was chosen.** When the interface was designed, processes were single-threaded and a process was the natural unit of ownership. Tying locks to the process made cleanup on exit simple (inferred; locks are released when the process terminates [S600]), and the interface gave byte-range granularity, which BSD `flock()` (whole-file only) does not provide [S601]. Nobody was thinking about libraries that open files behind the caller's back, or about threads (inferred). Layton points to Jeremy Allison's account that the close semantics were codified from an early implementation rather than designed [S601], which weakens the steelman.

**Why it is a problem.** Lock state is effectively global to the process, so code that does not know about the lock can destroy it. A library routine that opens, reads and closes the same file, without the caller knowing, silently drops the caller's lock; opening two hard links to one file and closing one triggers it too. Layton says this can lead to silent data corruption [S601][S600]. Because threads share ownership, the locks cannot coordinate threads; Layton's demo shows threads overwriting each other's appends [S601]. SQLite's source calls POSIX locks "broken by design". It has to keep a reference-counted `unixInodeInfo` per inode, do its own lock bookkeeping, and defer `close()` of descriptors (the `pUnused` list) until the last lock is released [S602]. Developers of Samba, NFS Ganesha, SQLite and OpenJDK expressed interest in OFD locks to simplify their code and avoid corruption when files are closed [S601].

**Why it was not fixed.** The semantics are in the standard and existing programs depend on them, so changing `F_SETLK` was not an option. Linux added a parallel lock type instead.

**Alternatives that exist.** Open file description (OFD) locks (Linux 3.15, originally "file-private" locks) are byte-range locks owned by the open file description. They are inherited across `fork()`/`dup()`, released only on the last close, and conflict between separate opens even within one process [S600][S601]. They are now standardized as `F_OFD_*` in POSIX.1-2024 [S603]. BSD `flock()` has the right ownership but only whole-file granularity [S601].

**Cost of the alternatives.** OFD locks do no deadlock detection [S600]. Conflicting locks from the two families conflict with each other even within one process and on the same descriptor, so mixed use in one program is subtle [S600][S601]. Retrofitting took a decade-old API plus a new set of commands, and it left two lock models in the standard.

**Relevance to our OS.** medium. Any lock primitive should be owned by a handle or capability with explicit lifetime. The POSIX layer still has to emulate the broken semantics if hosted C programs rely on them.

**Sources.** [S600], [S601], [S602], [S603]

### DM-037: fsync() error semantics: errors reported once, dirty data dropped ("fsyncgate")

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Linux (ext4, XFS, Btrfs), NetBSD, macOS, OpenBSD (behaviour since changed so later calls keep failing while the file stays open [S608]); POSIX (underspecified); PostgreSQL, SQLite, LevelDB, LMDB, Redis as victims
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Buffered writes land in the page cache and are written back asynchronously. If writeback fails, Linux file systems mark the affected pages clean (discarding or keeping stale contents) and report the error once to a later `fsync()` [S604][S606]. POSIX says nothing about the file's state after `fsync()` fails [S605].

**Why it was chosen.** Kernel developers' rationale: the most common write error is a removed USB stick. Keeping pages dirty indefinitely for a device that is gone would exhaust memory, so the data is dropped [S604]. Error state lived in the page cache's mapping and was historically cleared when reported. Before `errseq_t` (4.13/4.16) only the first caller after an error could see it [S607].

**Why it is a problem.** PostgreSQL retried `fsync()` after EIO on the assumption that the dirty data was still pending. On Linux the retry *succeeds*, because the error flag was cleared, and the checkpoint advances past writes that never reached disk. Ringer reported real corruption on XFS [S605]. PostgreSQL's checkpointer often opened files just before `fsync()`, so it never saw errors from writeback that happened before its `open()` [S604]. Rebello et al. found that ext4, XFS and Btrfs all mark pages clean after failure, while page contents and failure reporting vary between them, and that none of five widely used applications (PostgreSQL, LMDB, LevelDB, SQLite, Redis) handle fsync failure sufficiently, with data loss and corruption as outcomes [S606]. The meaning of "successful fsync" also differs by platform. On Darwin, `fsync()` does not flush the drive's cache, and a separate `F_FULLFSYNC` is needed [S609].

**Why it was not fixed.** Keeping pages dirty was rejected over memory exhaustion. A sticky per-inode error was considered unlikely because it would diverge further from POSIX [S604]. `errseq_t` improved reporting to "once per open file description" but is explicitly "not perfect" [S607]. Kernel developers told PostgreSQL to use direct I/O, which Andres Freund called "a metric ton of work" [S604]. PostgreSQL's response was to PANIC and recover from WAL; this landed in v12 and was backported to 9.4 [S608]. The classification as a mistake applies mainly to the reporting side (errors lost, retries falsely succeeding); dropping the data itself is a tradeoff against memory exhaustion that FreeBSD resolved the other way [S604][S608].

**Alternatives that exist.** FreeBSD keeps buffers dirty after a write error, so later `fsync()` calls keep failing (with a policy exception when the device disappears) [S608]. Applications can use direct I/O and manage durability themselves [S604]. Google had an out-of-tree netlink error-notification channel [S604].

**Cost of the alternatives.** Keeping dirty data can pin unbounded memory against a dead device [S604]. Direct I/O moves caching, readahead and alignment into every application [S604]. Crash-and-replay needs a WAL design.

**Relevance to our OS.** high. Durability and error-reporting semantics of the storage API are a core contract that has to be specified (who sees an error, whether data is retained, what a retry means) rather than inherited.

**Sources.** [S604], [S605], [S606], [S607], [S608], [S609]

### DM-038: No ordering or atomicity contract for file updates; crash consistency left to folklore (ext4 delalloc, 2009)

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** POSIX (unspecified crash semantics), Linux ext3/ext4/Btrfs/XFS/ReiserFS, applications (Git, Mercurial, LevelDB, SQLite, PostgreSQL, HDFS, ZooKeeper, desktop config stores)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** POSIX defines `write`, `rename`, `fsync`, etc. but leaves what survives an unclean shutdown undefined [S611]. Applications build "atomic" updates from idioms such as write-temp-then-rename, and correctness depends on unstated file-system behaviour ("persistence properties") [S612].

**Why it was chosen.** Leaving ordering unspecified lets file systems delay allocation, batch and reorder writes, which is a big performance win. ext4's delayed allocation could hold data for up to a minute versus about five seconds on ext3 [S610]. `fsync()` on every update was expensive; Ts'o cites the fsync performance problems of Firefox 3.0 [S611]. Ts'o's position was that apps needing durability must call `fsync()`/`fdatasync()` [S611].

**Why it is a problem.** After ext4 shipped, users found zero-length configuration files after crashes. Desktop apps used write-then-rename without `fsync`, and ext4 committed the rename before the data [S610]. The pattern had "worked" only because ext3's `data=ordered` mode, designed as a security measure against exposing stale blocks, flushed data before metadata as a side effect [S610]. Pillai et al. tested six Linux file systems and found persistence properties vary widely. For example, appends to one file are persisted before a later rename in ext3's ordered mode, but not in ext4's same mode unless a special option is enabled. They found 60 crash vulnerabilities in 11 mature applications, including a Git case where a crash after rename breaks all later commits [S612]. Correct code needs knowledge the API never states, such as fsyncing the parent directory after create or rename.

**Why it was not fixed.** The kernel changed behaviour rather than the contract. 2.6.30 added heuristics that force allocation on rename-over-existing and on truncate, with a mount option to turn them off [S610][S611]. The standard still leaves crash semantics open.

**Alternatives that exist.** Explicit atomic-replace primitives (Windows `ReplaceFile` is Microsoft's recommended replacement for TxF single-file use [S639]). Transactional system calls (TxOS) [S617]. Embedded transactional stores (Ts'o recommended SQLite-like stores) [S611]. Pillai et al. propose ext3-fast to keep ordering cheaply [S612].

**Cost of the alternatives.** Strong ordering costs throughput; that is why delayed allocation exists [S610]. Transactional APIs proved hard to adopt (see DM-045). A database for every config file adds a dependency and its own failure modes.

**Relevance to our OS.** high. A new FS API can state ordering and atomic-replace guarantees explicitly. Whether to pay their cost is a central filesystem design decision.

**Sources.** [S610], [S611], [S612], [S617], [S639]

### DM-039: Path-based TOCTOU and symlink races

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** POSIX/Unix (all), Linux (container runtimes: runc, Docker), BSDs, macOS; Windows (reparse points) [unverified for Windows specifics]
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Most file system calls take a pathname that the kernel resolves anew on every call, following symlinks and `..` through a mutable shared tree. A program that checks a path (`stat`, `access`) and then uses it (`open`) is operating on two independent lookups.

**Why it was chosen.** String paths are simple, human-readable and composable with the shell. Symlinks were added for flexibility across file systems. In a single-user or trusted multi-user setting with no adversarial co-tenants, races were a minor concern.

**Why it is a problem.** An attacker who can modify any directory along the path can swap a component between check and use. The kernel's own docs call symlink TOCTOU "a long-standing class of security issues" [S619]. In 2009 a search of the NVD for "symlink attack" returned over 600 hits [S617]. In CVE-2018-15664, `docker cp` was vulnerable to a symlink-exchange race giving root read-write access to the host [S615]. runc has had 14 path-safety CVEs since 2017, including 2025 bugs that combine mounts and symlinks to reach `/proc/sys/kernel/core_pattern` [S614]. Symlinks also make `..` ambiguous: `getwd` and `cd ..` disagree [S618]. Porter et al., who propose system transactions as that change, argue that the counter-measure arms race suggests TOCTOU can be eliminated "only by changing the API". They note Linux had to add close-on-exec flags to 15 syscalls to close one race [S617].

**Why it was not fixed.** Every path-taking syscall and every program depends on the current semantics. Linux kept adding narrow fixes: `O_NOFOLLOW`, `*at()` calls, `protected_symlinks` [S619], and finally `openat2()` with `RESOLVE_BENEATH`, `RESOLVE_IN_ROOT`, `RESOLVE_NO_SYMLINKS`, `RESOLVE_NO_MAGICLINKS` and `RESOLVE_NO_XDEV` (5.6) [S613]. Adoption is limited: seccomp policies often block `openat2` because it takes a pointer argument, and procfs magic links need extra handling [S614].

**Alternatives that exist.** Plan 9 has no symlinks and defines `..` lexically, with kernel-tracked names and `fd2path` [S618]. Fuchsia does not implement `..` on file servers, so a directory handle confines its holder to that subtree [S616]. Handle-relative resolution with explicit restrictions (`openat2`, libpathrs) [S613][S614]. Transactional system calls serialize the attacker's change [S617].

**Cost of the alternatives.** Removing `..` and symlinks breaks POSIX expectations. Fuchsia's client-side canonicalization works only because it has no symlinks [S616]. `RESOLVE_*` flags break existing layouts if applied indiscriminately, and resolution can fail with `EAGAIN` under races [S613]. Manual component-by-component traversal is "quite finicky" [S614].

**Relevance to our OS.** high. Whether names are resolved relative to handles (capability-style) or against a global mutable tree largely decides whether this bug class exists at all.

**Sources.** [S613], [S614], [S615], [S616], [S617], [S618], [S619]

### DM-040: Hard links to files the user cannot access

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Unix/POSIX, Linux (before protected_hardlinks), BSDs, macOS
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A hard link is a second directory entry for the same inode. Traditionally any user could create one to any file on the same file system, including files they cannot read or write and setuid binaries, because link creation checks only write permission on the target directory [S620].

**Why it was chosen.** In the inode/directory-entry model, names are just references, and a link count lets files have several names cheaply (used by tools and early `mv`/`rename` implementations [unverified]). Checking only the directory was consistent with treating directories as the authority over names.

**Why it is a problem.** A root process following a hard link planted in a shared directory such as `/tmp` crosses a privilege boundary, which is the hardlink TOCTOU race [S620][S619]. A user can "pin" a vulnerable setuid/setgid binary by linking to it, so an administrator's upgrade leaves the old exploitable copy reachable [S620][S621]. Users are surprised they can link files they cannot access [S620]. Link-restriction fixes (chiefly for symlinks) were discussed from 1996 onward, and the patch history lists repeated objections [S621].

**Why it was not fixed.** Objections recorded against the symlink half of the patch included POSIX conformance, breaking applications, "just use `mkstemp()`/`O_EXCL`", and whether the fix belonged in the VFS or an LSM [S621]. Linux adopted the Openwall/grsecurity restriction as `protected_hardlinks` (and `protected_symlinks`); the patches were posted in 2012 [S620][S621]. Linking then requires owning the file or having read and write access [S621][S619]. Cook argued POSIX already permits this check [S620]. Known breakage was small: some `at` daemons [S620].

**Alternatives that exist.** Restricting link creation to owners or holders of read and write access (Linux, Ubuntu for about 1.5 years before mainline) [S620]. Plan 9 builds aliasing through per-process bind/mount rather than persistent links [S161]; whether Plan 9 file servers support hard links at all could not be verified [unverified].

**Cost of the alternatives.** Some legitimate tools relying on unrestricted links break [S620]. Removing hard links altogether also removes a cheap snapshot/dedup idiom used by backup tools [unverified]. The restriction is a sysctl, so the old semantics persist where it is disabled [S619].

**Relevance to our OS.** medium. If names are references to objects, the authority to create a new name for an object should be tied to authority over the object, not just the directory.

**Sources.** [S619], [S620], [S621], [S161]

### DM-041: A single global mount namespace, with mounting reserved for root

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Traditional Unix, Linux (before 2.4.19 / 3.8), BSDs, macOS; Windows drive letters are a related global namespace
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Classic Unix has one file tree shared by all processes. `mount` alters that shared tree and therefore requires superuser privilege. Per-process views (chroot aside) did not exist.

**Why it was chosen.** One tree is simple to administer and explain. Because the tree is shared and setuid programs trust paths like `/etc/passwd`, letting an ordinary user mount over a directory would let them subvert privileged programs. Linux's docs say many features were restricted to privileged users because they could confuse set-user-ID-root programs [S155]. The constraint is partly self-inflicted (setuid trust in a shared tree), it has not gone away, and Plan 9 (described c. 1995) already had per-process namespaces and no super-user [S625], so this is treated as a tradeoff rather than right-at-the-time.

**Why it is a problem.** Users and applications cannot compose their own view of resources, such as sandboxes, per-application `/tmp`, or private device views, without privilege. That pushed sandboxing into setuid helpers and root daemons. Linux retrofitted mount namespaces (`CLONE_NEWNS`, 2.4.19) [S622]. Full isolation proved too strict, so shared-subtree propagation followed in 2.6.15, adding a notoriously complex state machine [S622]. Unprivileged mounting arrived only via user namespaces (3.8+), limited to specific filesystem types; block-device file systems still need init-namespace privilege [S155]. Making root-only kernel code reachable to unprivileged users widened the attack surface. Canonical cites a Google figure that 44% of observed exploits required unprivileged user namespaces, and Ubuntu now gates them with AppArmor [S156].

**Why it was not fixed.** Setuid binaries and global path assumptions are everywhere, so namespaces had to be opt-in and bolted on. Each relaxation needed new safety rules, such as locked mounts and flags that cannot be changed in less-privileged namespaces [S622].

**Alternatives that exist.** Plan 9 makes the name space a per-process attribute with no global name space. Processes build it with `mount`, `bind` and union directories, and each window gets its own `/dev/cons` [S161]. Plan 9 "has no super-user" and user processes construct their namespaces themselves [S625].

**Cost of the alternatives.** Plan 9 programs depend on agreed conventions for where things live, because nothing is global [S161]. On Linux, retrofitted unprivileged namespaces became an exploit enabler [S156]. Per-process namespaces complicate debugging and administration ("which tree is this process seeing?") [unverified as a measured cost].

**Relevance to our OS.** high. Whether the namespace is per-process and composable by unprivileged code is a foundational decision that interacts directly with the capability model and the absence of setuid.

**Sources.** [S622], [S155], [S161], [S625], [S156]

### DM-042: atime updated on every read

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Unix/POSIX, Linux (default changed 2.6.30), Windows NTFS (last-access time), Btrfs and other CoW file systems
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Every file records a last-access time. POSIX's model marks `st_atime` for update when data is read. The actual write may be deferred, but it must happen by the time the file is closed or stat'ed [S533].

**Why it was chosen.** On small, shared systems atime supports useful policies: detecting unread mail (mail tools such as mutt), cleaning unused temp files (tmpwatch), and backup decisions [S627][S628]. Disk writes for metadata were a modest cost relative to workloads of the time (inferred). The cost was not new, though: in 2007, on ext3, Molnar already rated it Linux's worst I/O deficiency [S627], so this is a tradeoff between a cheap usage signal and write amplification, made worse (not created) by copy-on-write and flash.

**Why it is a problem.** It turns every cached read into a disk write. Ingo Molnar called it "by far the biggest IO performance deficiency" in Linux in 2007 [S627]. On copy-on-write file systems the cost multiplies. On Btrfs, a recursive grep after a snapshot rewrites inode metadata and unshares it from the snapshot, so reading files consumes disk space, and relatime may not help much [S631]. Read-only workloads therefore also wear flash and fill snapshots.

**Why it was not fixed.** Programs depend on atime, so distributions avoided `noatime` as a default [S627]. Linux compromised with `relatime` (2.6.20): it updates atime only if it is older than mtime/ctime or more than a day old, and has been the default since 2.6.30 [S627][S628]. `lazytime` keeps timestamps in memory for up to 24 hours [S628]. NTFS similarly defers last-access writes by up to an hour and can disable them [S630].

**Alternatives that exist.** `noatime`/`relatime`/`lazytime` [S628]. NTFS's deferred and optional last-access [S630]. Commenters on LWN proposed opt-in per-file atime or a separate atime store [S631] (proposal only).

**Cost of the alternatives.** relatime still breaks "accessed within the last N" queries [S627]. Disabling last-access affects backup and remote-storage tools [S630]. Dropping atime removes a cheap usage signal that tiering and cleanup tools use.

**Relevance to our OS.** low. Easy to avoid from scratch (make access tracking opt-in), but the POSIX layer must decide what `st_atime` reports.

**Sources.** [S627], [S628], [S533], [S630], [S631]

### DM-043: Windows MAX_PATH and DOS-reserved device names

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows (Win32 API layer; NTFS itself is less restricted), applications ported to Windows (Git, Python)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Win32 path APIs historically limit paths to `MAX_PATH` (260 characters). Longer paths need the `\\?\` prefix (about 32,767 characters, absolute paths only), or, since Windows 10 1607, an opt-in that requires *both* a machine-wide registry setting (or the equivalent Group Policy [S635]) *and* a `longPathAware` manifest in each application [S632]. Names such as CON, PRN, AUX, NUL, COM1–9 and LPT1–9 are reserved in every directory, even with an extension (`NUL.txt` is NUL), and Windows also treats superscript digits as part of COM/LPT names [S633].

**Why it was chosen.** DOS 1.0 had no subdirectories. Magic device names let users redirect output to the printer or discard it without temporary files. Because tools appended extensions automatically, the names had to work with any extension [S634]. A fixed small path buffer suited 1980s memory budgets (inferred).

**Why it is a problem.** Fixed limits leak into every application. Cloning a Git repository with long names into a deep folder can fail [S632]. Relative paths cannot use the `\\?\` prefix and so stay limited to MAX_PATH [S632]. The shell and the file system disagree, so the API can create paths that Explorer cannot handle [S632][S633]. Names that are legal on NTFS through `\\?\` or other OSes become inaccessible or dangerous through Win32 (reserved names, trailing dots and spaces) [S633]. Windows may still generate 8.3 short-name aliases unless disabled, adding lookup cost and a second name for long-named files [S630][S633].

**Why it was not fixed.** Compatibility. Raymond Chen notes batch files and documentation still rely on the reserved names [S634]. Lifting MAX_PATH globally would break programs with fixed 260-character buffers, hence the dual opt-in [S632]. Python enabled long-path support in 3.6 but still needs an administrator to set the policy [S635].

**Alternatives that exist.** NT's native namespace sits below these Win32 conventions (inferred: without the 260-character and reserved-name rules). Win32 is a translation layer on top of it, and `\\.\` and `\\?\` expose it [S633]. Unix places devices in `/dev` rather than reserving names everywhere.

**Cost of the alternatives.** Unbounded paths need dynamic buffers in every API and still need *some* limit to bound kernel work. Raw namespace access bypasses normalization that some code relies on [S633].

**Relevance to our OS.** medium. A clear lesson for API design: avoid magic names and fixed buffer limits in a path ABI, and keep any compatibility translation layer from becoming the de-facto native API.

**Sources.** [S630], [S632], [S633], [S634], [S635]

### DM-044: Windows share modes: open files block deletion, rename and update

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows NT family (Win32 `CreateFile`); contrast Linux (mandatory locking removed)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Every Win32 open specifies a share mode. A handle opened without `FILE_SHARE_DELETE` prevents others from deleting *or renaming* the file, and without `FILE_SHARE_WRITE` or `FILE_SHARE_READ` it blocks those opens. A conflicting open fails with `ERROR_SHARING_VIOLATION` [S636]. Traditional deletion only takes effect when all handles close [S638].

**Why it was chosen.** It gives enforced, kernel-checked mutual exclusion that works for naive programs and for network file sharing. An open file cannot change underneath an editor or a running executable. This is a real integrity property, not merely an accident.

**Why it is a problem.** Updates of in-use files are blocked. Microsoft's own docs say the primary reason installs and updates require a restart is that files being updated are in use [S139]. Any third-party handle, such as an antivirus scanner, indexer or backup agent, can make deletes and renames fail transiently, which makes atomic replace-by-rename unreliable [unverified as a measured frequency]. Correctness depends on every opener choosing the right share flags.

**Why it was not fixed.** Applications rely on exclusivity for integrity. Microsoft added mitigations instead. Restart Manager shuts down and restarts applications holding files [S139]. POSIX-style delete (`FILE_DISPOSITION_POSIX_SEMANTICS`) removes the name immediately while existing handles keep the data, though a mapped view still blocks deletion [S638].

**Alternatives that exist.** Unix decouples names from open files: unlink and rename succeed while the file is open, and old handles keep the old inode. Linux tried mandatory locking but found it racy with `write()` and `mmap()` ("unreliable"), made it optional in 4.5 and removed it in 5.15 [S600].

**Cost of the alternatives.** Under Unix semantics, running processes keep using old binaries and libraries after an update, so "updated" does not mean "in effect". Advisory-only coordination means nothing protects a file from a careless writer. Mandatory locking failed in practice on Linux [S600].

**Relevance to our OS.** medium. The interaction between names, open handles and updates determines how live system updates work, and both extremes have documented failure modes.

**Sources.** [S600], [S636], [S139], [S638]

### DM-045: No usable transactional filesystem API (Windows TxF deprecated)

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows (TxF/TxR, Vista+), Linux Btrfs (user transaction ioctls), research TxOS; by absence: POSIX, Linux, BSDs, macOS
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Mainstream file APIs offer single-call atomicity at best (`rename`) and no way to group several file operations atomically. Attempts to add transactions exist but have not stuck.

**Why it was chosen.** Transactions across arbitrary kernel objects are hard: isolation, conflict handling, resource limits and interaction with non-transactional actors. The pragmatic position is that applications needing ACID use a database, and Microsoft's own guidance points to ESE or SQL Server [S639].

**Why it is a problem.** Without it, applications hand-roll crash consistency and get it wrong (DM-038) [S612]. Multi-file updates such as installers and package managers can leave systems half-upgraded. TxOS cites a Vista SP1 prerequisite patch that caused endless reboot cycles [S617]. TOCTOU races persist because check and use cannot be grouped [S617].

**Why it was not fixed.** Microsoft shipped TxF in Vista, but reports "extremely limited developer interest" because of "complexity and various nuances". It says the API "may not be available in future versions" and recommends `ReplaceFile`, Windows Installer, ESE or SQL FILESTREAM instead [S639]. `fsutil` documents TxF as deprecated but still present [S630]. Btrfs added userspace transaction ioctls for Ceph; they were deprecated in 2017 (slated for removal in 4.17) because Btrfs had become unsuitable as a Ceph backend, leaving no widespread legitimate use [S640]. That abandonment says more about Ceph's needs than about transactions in general.

**Alternatives that exist.** TxOS (SOSP 2009) put ACID system transactions over many kernel resources in Linux 2.6.22. It reported about 10% overhead for a transactional software install, negligible overhead for non-transactional work, and a transactional ext3 built in under a month [S617]. It remained a research prototype.

**Cost of the alternatives.** *Position A (transactions are the right abstraction):* TxOS shows that overheads are modest and that one mechanism fixes both crash-consistency and TOCTOU classes [S617]. *Position B (they don't pay for themselves):* two production attempts (TxF and Btrfs) were abandoned for complexity and lack of users [S639][S640], and simpler primitives such as atomic replace cover most needs [S639]. Unresolved.

**Relevance to our OS.** high. Whether the storage layer offers grouped atomic operations, and how small that interface can be, is open, and the record shows a broad API alone does not guarantee adoption.

**Sources.** [S612], [S617], [S630], [S639], [S640]
