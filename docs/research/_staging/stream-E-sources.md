# Stream E sources (S500–S599)

[S500] Linux kernel developers, Overcommit Accounting (Documentation/mm/overcommit-accounting), kernel.org, n.d. (current 7.x docs), https://docs.kernel.org/mm/overcommit-accounting.html, primary, checked 2026-10-19
  - supports: DM-030 (modes 0/1/2; heuristic default; mode 2 returns allocation errors instead of kills; MAP_NORESERVE ignored in mode 2)
[S501] Goldwyn Rodrigues, Taming the OOM killer, LWN.net, 2009, https://lwn.net/Articles/317814/, secondary, checked 2026-10-19
  - supports: DM-030 (distros default to overcommit; rationale that allocated memory often goes unused; badness heuristic; wrong-victim complaints; Android low-memory notifications as alternative)
[S502] Microsoft, Introduction to the page file, Microsoft Learn, 2026 (ms.date 2026-02-12), https://learn.microsoft.com/windows/client-management/introduction-page-file, primary, checked 2026-10-19
  - supports: DM-030 (commit limit = RAM + page files; commit charge reaching limit makes allocations fail; system-managed page file growth)
[S503] Jonathan Corbet, Microsoft Research: A fork() in the road, LWN.net, 2019, https://lwn.net/Articles/785430/, weak, checked 2026-10-19
  - supports: DM-030 (reader comments only: fork-heavy workloads push toward overcommit; without it fork can fail with ENOMEM — anecdotal)
[S504] Johannes Weiner, PSI - Pressure Stall Information (Documentation/accounting/psi), kernel.org, 2018, https://docs.kernel.org/accounting/psi.html, primary, checked 2026-10-19
  - supports: DM-034 (motivation: no accurate measure of contention; some/full metrics; pollable triggers; unprivileged 2s-multiple windows)
  - supports: DM-030 (PSI-based userspace OOM handling as the retrofit)
[S505] Jonathan Corbet, Tracking pressure-stall information, LWN.net, 2018, https://lwn.net/Articles/759781/, secondary, checked 2026-10-19
  - supports: DM-034 (load average inadequate; Facebook fleet use; Android interest in earlier OOM detection)
[S506] Android Open Source Project, Low memory killer daemon (lmkd), source.android.com, n.d., https://source.android.com/docs/core/perf/lmkd, primary, checked 2026-10-19
  - supports: DM-034 (in-kernel LMK deprecated: fixed thresholds, thrashing without kills, vendor forks; vmpressure false positives; PSI default since Android 10)
  - supports: DM-030 (kernel 4.12 removed in-kernel LMK; userspace killer)
[S507] Microsoft, CreateMemoryResourceNotification function (memoryapi.h), Microsoft Learn, 2018, https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-creatememoryresourcenotification, primary, checked 2026-10-19
  - supports: DM-034 (waitable low/high memory notification objects since Windows XP; system-wide)
[S508] Apple, Responding to low-memory warnings, Apple Developer Documentation, n.d., https://developer.apple.com/documentation/xcode/responding-to-low-memory-warnings, primary, checked 2026-10-19
  - supports: DM-034 (four delivery paths incl. DISPATCH_SOURCE_TYPE_MEMORYPRESSURE; best-effort; jetsam when demand outpaces warnings; NSPurgeableData; compressor caveat)
[S509] Michael Kerrisk et al., mmap(2), Linux man-pages (man7.org), n.d., https://man7.org/linux/man-pages/man2/mmap.2.html, primary, checked 2026-10-19
  - supports: DM-031 (SIGBUS beyond EOF; effect of file size change unspecified)
  - supports: DM-030 (MAP_NORESERVE may SIGSEGV on write; BUGS: Linux gives no such guarantees, any process can be killed)
[S510] Andrew Crotty, Viktor Leis, Andrew Pavlo, Are You Sure You Want to Use MMAP in Your Database Management System?, CIDR 2022, 2022, https://www.pdl.cmu.edu/ftp/Database/p13-crotty.pdf (abstract: https://vldb.org/cidrdb/2022/are-you-sure-you-want-to-use-mmap-in-your-database-management-system.html), primary, checked 2026-10-19
  - supports: DM-031 (SIGBUS error handling via signal handlers; no async reads / unexpected I/O stalls; TLB shootdowns, single-threaded eviction; MongoDB MMAPv1 and InfluxDB moved away)
[S511] Michael Kerrisk et al., memfd_create(2), Linux man-pages (man7.org), n.d., https://man7.org/linux/man-pages/man2/memfd_create.2.html, primary, checked 2026-10-19
  - supports: DM-031 (peer can shrink shared file causing SIGBUS; F_SEAL_SHRINK; Linux 3.17)
[S512] Jonathan Corbet, Sealed files, LWN.net, 2014, https://lwn.net/Articles/593918/, secondary, checked 2026-10-19
  - supports: DM-031 (shared memory assumes mutual trust; peer truncation => fatal signals; sealing design; open review questions)
[S513] Simon Ser, Sealed memfd & no-fault mmap, linux-kernel mailing list, 2021, https://lkml.iu.edu/hypermail/linux/kernel/2104.3/02179.html, primary, checked 2026-10-19
  - supports: DM-031 (seven years on, no Wayland compositor enforces sealing; clients use shm_open; proposal for OpenBSD-style __MAP_NOFAULT)
[S514] OpenBSD project, OpenBSD 3.3 release notes, openbsd.org, 2003, https://www.openbsd.org/33.html, primary, checked 2026-10-19
  - supports: DM-032 (W^X in 3.3 only on archs with execute bit: sparc, sparc64, alpha, hppa; i386/powerpc later)
[S515] OpenBSD project, OpenBSD 6.0 release notes, openbsd.org, 2016, https://www.openbsd.org/60.html, primary, checked 2026-10-19
  - supports: DM-032 (W^X strictly enforced by default; PT_OPENBSD_WXNEEDED + wxallowed escape hatch; ports still violate W^X)
[S516] Microsoft, Data Execution Prevention, Microsoft Learn, 2022, https://learn.microsoft.com/en-us/windows/win32/memory/data-execution-prevention, primary, checked 2026-10-19
  - supports: DM-032 (DEP since XP/2003; JIT and ATL thunk compatibility breakage; executable code in data sections)
[S517] "stein", W^X (isopenbsdsecu.re mitigations page), isopenbsdsecu.re, n.d., https://isopenbsdsecu.re/mitigations/wx/, weak, checked 2026-10-19
  - supports: DM-032 (timeline 2002–2003; PaX earlier (2000); OpenBSD does not prevent W->X transitions unlike PaX MPROTECT/iOS)
[S518] Nicolas Brito (quoting Theo de Raadt), Re: Buffer overflow prevention, Bugtraq, 2003, https://diswww.mit.edu/menelaus.mit.edu/bt/31191, primary, checked 2026-10-19
  - supports: DM-032 (de Raadt's definition of W^X as default page policy; critic: return-into-existing-code bypasses)
[S519] Kees Cook, [PATCH v4 2/6] x86/elf: Split READ_IMPLIES_EXEC from executable GNU_STACK, linux-kernel mailing list, 2020, https://lkml.iu.edu/hypermail/linux/kernel/2002.3/01551.html, primary, checked 2026-10-19
  - supports: DM-032 (binaries lacking PT_GNU_STACK still get all readable mappings executable on x86 in 2020)
[S520] GNU C Library maintainers, The GNU C Library version 2.41 is now available, libc-alpha mailing list, 2025, https://gcc.gnu.org/pipermail/libc-alpha/2025-January/164379.html, primary, checked 2026-10-19
  - supports: DM-032 (dlopen no longer makes stack executable; glibc.rtld.execstack tunable; exec stacks still allowed by default for main program)
[S521] Michael Larabel, GNU C Library Lands Workaround After Breaking Various Steam Games, Discord & Other Apps, Phoronix, 2025, https://www.phoronix.com/news/Glibc-WA-Steam-Exec-Stack, secondary, checked 2026-10-19
  - supports: DM-032 (2.41 dlopen change broke Steam games, Discord, Julia, MATLAB; workaround extends tunable to force exec stack)
[S522] Apple, Porting just-in-time compilers to Apple silicon, Apple Developer Documentation, n.d., https://developer.apple.com/documentation/apple-silicon/porting-just-in-time-compilers-to-apple-silicon, primary, checked 2026-10-19
  - supports: DM-032 (MAP_JIT, per-thread W/X toggling, allow-jit entitlement, one MAP_JIT region, Apple silicon enforces for all apps; icache invalidation)
[S523] Moritz Lipp, Michael Schwarz, Daniel Gruss, Thomas Prescher, Werner Haas, Anders Fogh, Jann Horn, Stefan Mangard, Paul Kocher, Daniel Genkin, Yuval Yarom, Mike Hamburg, Meltdown: Reading Kernel Memory from User Space, USENIX Security 2018, 2018, https://usenix.org/conference/usenixsecurity18/presentation/lipp, primary, checked 2026-10-19
  - supports: DM-033 (out-of-order execution reads kernel memory from user space without software bug; KAISER impedes it, must be deployed)
[S524] Daniel Gruss, [RFC, PATCH] x86_64: KAISER - do not map kernel in user mode, linux-kernel mailing list, 2017, https://lkml.iu.edu/hypermail/linux/kernel/1705.0/02174.html, primary, checked 2026-10-19
  - supports: DM-033 (KAISER proposed May 2017 to fix KASLR side channels, before Meltdown disclosure)
[S525] Linux kernel developers, Page Table Isolation (PTI) (Documentation/arch/x86/pti), kernel.org, n.d., https://docs.kernel.org/arch/x86/pti.html, primary, checked 2026-10-19
  - supports: DM-033 (two page-table sets; CR3 writes ~100 cycles per entry/exit; TLB flush costs without PCID; memory overhead)
[S526] Jonathan Corbet, KAISER: hiding the kernel from user space, LWN.net, 2017, https://lwn.net/Articles/738975/, secondary, checked 2026-10-19
  - supports: DM-033 (kernel mapped for performance to avoid TLB flushes; single-digit typical regression, ~30% worst case; PCID mitigates)
[S527] Mark Brown (committed by Will Deacon), arm64: Add initial support for E0PD (commit 3e6c69a058de), Linux kernel git (GitHub mirror), 2019/2020, https://github.com/torvalds/linux/commit/3e6c69a058deaa50d33c3dac36cde80b4ce590e8, primary, checked 2026-10-19
  - supports: DM-033 (KPTI expensive and incompatible with SPE on arm64; ARMv8.5 E0PD makes EL0 access to kernel half fault in constant time)
[S528] Michael Kerrisk et al., brk(2), Linux man-pages (man7.org), n.d., https://man7.org/linux/man-pages/man2/brk.2.html, primary, checked 2026-10-19
  - supports: DM-035 (4.3BSD origin; LEGACY in SUSv2, removed in POSIX.1-2001; avoid in favour of malloc; inconsistent sbrk argument types)
[S529] Otto Moerbeek, A new malloc(3) for OpenBSD, EuroBSDCon 2009, 2009, https://www.openbsd.org/papers/eurobsdcon2009/otto-malloc.pdf, primary, checked 2026-10-19
  - supports: DM-035 (sbrk heap: predictable, memory rarely returnable; mmap-based design with randomization, out-of-band metadata, ~30% faster; sbrk still supported)
[S530] Michael Kerrisk et al., mallopt(3), Linux man-pages (man7.org), n.d., https://man7.org/linux/man-pages/man3/mallopt.3.html, primary, checked 2026-10-19
  - supports: DM-035 (glibc trims via sbrk only at heap top; mmap threshold for large blocks; dynamic threshold)
[S531] Arnd Bergmann, [GIT PULL] y2038: core, driver and file system changes, linux-kernel mailing list, 2020, https://lkml.iu.edu/hypermail/linux/kernel/2001.3/05499.html, primary, checked 2026-10-19
  - supports: DM-063 (5.6 first base for post-2038 32-bit systems; filesystems with 32-bit timestamps; userspace must rebuild; direct-syscall runtimes; interfaces like input_event cannot be fixed compatibly, 2106 limit)
[S532] Carlos O'Donell / GNU C Library maintainers, The GNU C Library version 2.34 is now available, libc-alpha mailing list, 2021, https://sourceware.org/pipermail/libc-alpha/2021-August/129718.html, primary, checked 2026-10-19
  - supports: DM-063 (opt-in _TIME_BITS=64 requiring _FILE_OFFSET_BITS=64; default stays 32-bit; Linux-only; kernel 5.1)
[S533] IEEE/The Open Group, POSIX.1-2024 Base Definitions §4.19 Seconds Since the Epoch, pubs.opengroup.org, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html, primary, checked 2026-10-19
  - supports: DM-064 (every day is exactly 86400 seconds; relation to actual UTC unspecified)
[S534] Google, Leap Smear, Google Public NTP docs, n.d., https://developers.google.com/time/smear, primary, checked 2026-10-19
  - supports: DM-064 (smearing since 2008; 24h linear noon-to-noon, 11.6 ppm; different orgs smear differently, ~0.5 s offsets)
[S535] CGPM, Resolution 4 of the 27th CGPM (2022): On the use and future development of UTC, BIPM, 2022, https://www.bipm.org/en/cgpm-2022/resolution-4, primary, checked 2026-10-19
  - supports: DM-064 (leap seconds disrupt infrastructure; uncoordinated handling; untested negative leap second; UT1-UTC tolerance to be raised by 2035)
[S536] John Stultz, [PATCH 0/3] Fix for leapsecond caused hrtimer/futex issue, linux-kernel mailing list, 2012, https://lkml.iu.edu/hypermail/linux/kernel/1207.0/02318.html, primary, checked 2026-10-19
  - supports: DM-064 (2012 leap second: CLOCK_REALTIME hrtimers fired early; futex load spikes in MySQL, Java, Firefox)
[S537] Cloudflare, How and why the leap second affected Cloudflare DNS, Cloudflare Blog, 2017, https://blog.cloudflare.com/how-and-why-the-leap-second-affected-cloudflare-dns/, primary, checked 2026-10-19
  - supports: DM-064 (2017 leap second outage incident report)
  - supports: DM-065 (negative duration from wall-clock subtraction; Go had no monotonic source)
[S538] Michael Kerrisk et al., clock_getres(2) / clock_gettime(2), Linux man-pages (man7.org), n.d., https://man7.org/linux/man-pages/man2/clock_gettime.2.html, primary, checked 2026-10-19
  - supports: DM-065 (REALTIME jumps; MONOTONIC excludes suspend; BOOTTIME 2.6.39; MONOTONIC_RAW 2.6.28; TAI 3.10; MONOTONIC mandatory only in POSIX.1-2024)
  - supports: DM-064 (CLOCK_TAI exists but depends on NTP to set offset)
[S539] Russ Cox, Proposal: Monotonic Elapsed Time Measurements in Go (design/12914-monotonic), Go project, 2017, https://go.googlesource.com/proposal/+/master/design/12914-monotonic.md, primary, checked 2026-10-19
  - supports: DM-065 (29% of time.Now calls measure elapsed time; dual-reading Time; rationale against separate API; costs)
[S540] Thomas Gleixner, Revert: Unify CLOCK_MONOTONIC and CLOCK_BOOTTIME (commit a3ed0e4393), Linux kernel git (GitHub mirror), 2018, https://github.com/torvalds/linux/commit/a3ed0e4393, primary, checked 2026-10-19
  - supports: DM-065 (making MONOTONIC include suspend broke systemd watchdogs, journald, NetworkManager, MATE; reverted)
[S541] Microsoft, Acquiring high-resolution time stamps, Microsoft Learn, 2018, https://learn.microsoft.com/en-us/windows/win32/sysinfo/acquiring-high-resolution-time-stamps, primary, checked 2026-10-19
  - supports: DM-065 (QPC monotonic, independent of UTC/DST/leap seconds; pre-Vista TSC inconsistencies; discourage raw RDTSC; Arm Generic Timer counter)
[S542] Michael Kerrisk et al., tzset(3), Linux man-pages (man7.org), n.d., https://man7.org/linux/man-pages/man3/tzset.3.html, primary, checked 2026-10-19
  - supports: DM-066 (TZ env lookup; tzname/timezone/daylight globals; caveat: UB in multithreaded programs; called implicitly by conversion functions)
[S543] Michael Kerrisk et al., setenv(3), Linux man-pages (man7.org), n.d., https://man7.org/linux/man-pages/man3/setenv.3.html, primary, checked 2026-10-19
  - supports: DM-066 (setenv/unsetenv MT-Unsafe const:env)
[S544] RustSec, RUSTSEC-2020-0159: chrono: Potential segfault in localtime_r invocations, rustsec.org, 2021, https://rustsec.org/advisories/RUSTSEC-2020-0159.html, primary, checked 2026-10-19
  - supports: DM-066 (env var set in another thread => dangling pointer segfault in localtime_r path; related CVE-2020-26235)
[S545] Rust project, Rust 2024 edition guide: Unsafe functions (std::env::set_var / remove_var), doc.rust-lang.org, 2025, https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html, primary, checked 2026-10-19
  - supports: DM-066 (set_var/remove_var made unsafe; unsound in multithreaded programs)
[S546] NetBSD project, tzset(3) (tzalloc, tzfree, localtime_rz), NetBSD manual pages, n.d., https://man.netbsd.org/tzset.3, primary, checked 2026-10-19
  - supports: DM-066 (explicit timezone_t objects for multiple zones without changing global TZ)
[S547] Linux kernel developers (Paul E. McKenney), NO_HZ: Reducing Scheduling-Clock Ticks (Documentation/timers/no_hz), kernel.org, n.d., https://docs.kernel.org/timers/no_hz.html, primary, checked 2026-10-19
  - supports: DM-067 (periodic tick costs: battery 2-3x drain, mainframe guests losing ~half CPU; NO_HZ_IDLE/FULL costs, housekeeping CPU, residual 1 Hz tick, reboot to reconfigure)
[S548] Microsoft, timeBeginPeriod function (timeapi.h), Microsoft Learn, 2018 (updated 2024), https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod, primary, checked 2026-10-19
  - supports: DM-067 (pre-2004 global timer resolution = minimum requested by any process; power cost; Win10 2004 and Win11 changes)
[S549] Linux kernel developers, include/linux/jiffies.h, Linux kernel source (GitHub mirror), n.d., https://raw.githubusercontent.com/torvalds/linux/master/include/linux/jiffies.h, primary, checked 2026-10-19
  - supports: DM-067 (INITIAL_JIFFIES set to wrap 32-bit jiffies 5 minutes after boot so wrap bugs show early; time_after macros)
[S550] Linux kernel developers, Extensible Scheduler Class (Documentation/scheduler/sched-ext), kernel.org, n.d., https://docs.kernel.org/scheduler/sched-ext.html, primary, checked 2026-10-20
  - supports: DM-068 (BPF-defined scheduler class; runtime enable/disable; fallback to fair class on error, stall, SysRq-S)
[S551] Linux kernel developers, EEVDF Scheduler (Documentation/scheduler/sched-eevdf), kernel.org, n.d., https://docs.kernel.org/scheduler/sched-eevdf.html, primary, checked 2026-10-20
  - supports: DM-068 (EEVDF from 1995 paper, proposed by Zijlstra 2023, replaces CFS from 6.6; lag, deadlines, per-task slice via sched_setattr)
[S552] Jean-Pierre Lozi, Baptiste Lepers, Justin Funston, Fabien Gaud, Vivien Quema, Alexandra Fedorova, The Linux Scheduler: a Decade of Wasted Cores, EuroSys 2016, 2016, https://people.ece.ubc.ca/sasha/papers/eurosys16-final29.pdf, primary, checked 2026-10-20
  - supports: DM-068 (four CFS bugs leaving cores idle; 13-24% typical, up to 138x corner case; evade conventional tools; complexity from NUMA/multicore workarounds)
[S553] Jonathan Corbet, An EEVDF CPU scheduler for Linux, LWN.net, 2023, https://lwn.net/Articles/925371/, secondary, checked 2026-10-20
  - supports: DM-068 (CFS cannot express latency needs; fragile heuristics; EEVDF slices replace heuristics; CFS merged 2.6.23 in 2007)
[S554] Thomas Gleixner, Re: [PATCHSET v6] sched: Implement BPF extensible scheduler class, linux-kernel mailing list (lore mirror), 2024, https://lore.jethome.com/lkml/87ed8sps71.ffs@tglx/, primary, checked 2026-10-20
  - supports: DM-068 (maintainer dispute over sched_ext; Torvalds intended to merge for 6.11 over objections; request for collaborative integration)
[S555] Marcel Hilzinger, Con Kolivas Introduces New BFS Scheduler, Linux Magazine, 2009, https://www.linux-magazine.com/Online/News/Con-Kolivas-Introduces-New-BFS-Scheduler, weak, checked 2026-10-20
  - supports: DM-068 (BFS: simple desktop-oriented scheduler; claims vs CFS; no NUMA / large-SMP scaling; not aimed at mainline)
