# Stream G sources (S700–S799)

[S700] Andy Chou, Junfeng Yang, Benjamin Chelf, Seth Hallem, Dawson Engler, An Empirical Study of Operating Systems Errors, SOSP 2001 (ACM), 2001, https://www.read.seas.harvard.edu/~kohler/class/aosref/chou01empirical.pdf, primary, checked 2026-10-19
  - supports: DM-047 (drivers have 3–7x higher error rates than rest of kernel; bugs live ~1.8 years; static analysis of Linux and OpenBSD)
  - supports: DM-048 (bug classes found include use of freed memory, null dereference, lock misuse)
[S701] Nicolas Palix, Gaël Thomas, Suman Saha, Christophe Calvès, Gilles Muller, Julia Lawall, Faults in Linux 2.6 (extended version of ASPLOS 2011 "Faults in Linux: Ten Years Later"), ACM TOCS 32(2) / arXiv 1407.4346, 2014, https://arxiv.org/abs/1407.4346, primary, checked 2026-10-19
  - supports: DM-047 (fault rate per line fell over Linux 2.6; driver fault rate now below arch and fs directories)
  - supports: DM-049 (in-tree drivers improved over time — used only as context, not as proof of causation)
[S702] Michael M. Swift, Brian N. Bershad, Henry M. Levy, Improving the Reliability of Commodity Operating Systems (Nooks), SOSP 2003 (ACM), 2003, https://nooks.cs.washington.edu/nooks-sosp.pdf, primary, checked 2026-10-19
  - supports: DM-047 (85% of Windows XP failures attributed to drivers — sourced to a private communication from Microsoft; extensions >70% of Linux code; 99% recovery in 2000 fault injections)
  - supports: DM-050 (Nooks explicitly trusts extensions not to be malicious; in-kernel isolation easily circumvented by malicious code)
  - supports: DM-051 (in-kernel driver restart; 60% throughput loss for kHTTPd, ~10% for network streams)
[S703] Thomas Ball, Ella Bounimova, Byron Cook, Vladimir Levin, Jakob Lichtenberg, Con McGarvey, Bohus Ondrusek, Sriram K. Rajamani, Abdullah Ustuner, Thorough Static Analysis of Device Drivers, EuroSys 2006 (ACM), 2006, https://cs.uwaterloo.ca/~brecht/courses/epfl/Possible-Readings/device-drivers/static-anal-device-drivers-eurosys-2006.pdf, primary, checked 2026-10-19
  - supports: DM-047 (SDV: 60+ API usage rules, run on 126 WDM and 20 KMDF drivers; driver API complexity as an error source; re-cites the 85% XP figure from Nooks)
  - supports: DM-049 (Windows driver API rules are complex enough to need a model checker)
[S704] Simon Biggs, Damon Lee, Gernot Heiser, The Jury Is In: Monolithic OS Design Is Flawed: Microkernel-based Designs Improve Security, APSys 2018 (ACM), 2018, https://www.trustworthy.systems/publications/full_text/Biggs_LH_18.pdf, primary, checked 2026-10-19
  - supports: DM-046 (115 critical Linux CVEs; 29% eliminated by microkernel design, 96% no longer critical; ~9 percentage-point uncertainty; argument that IPC cost is no longer prohibitive)
  - supports: DM-048 (privileged code size growth: Linux ~26 MSLOC)
[S705] Andrew S. Tanenbaum, Linus Torvalds et al., "The Tanenbaum–Torvalds Debate" (comp.os.minix posts, Jan–Feb 1992), appendix A of Open Sources: Voices from the Open Source Revolution, O'Reilly, 1999, https://www.oreilly.com/openbook/opensources/book/appa.html, primary, checked 2026-10-19
  - supports: DM-046 (Tanenbaum: monolithic design a step backward; Torvalds: Linux available now, multithreaded FS natural in monolithic kernel)
[S706] Linus Torvalds, post in thread "Hybrid kernel, not NT", Real World Technologies forum, 2006-05-09, https://www.realworldtech.com/forum/?threadid=65915&curpostid=65936, primary, checked 2026-10-19
  - supports: DM-046 (separate address spaces prevent sharing data structures; distributed algorithms harder; "hybrid" is marketing)
  - supports: DM-051 (restarting a failed service helps little because of mutual dependencies)
[S707] Andrew S. Tanenbaum, Jorrit N. Herder, Herbert Bos, Can We Make Operating Systems Reliable and Secure?, IEEE Computer 39(5), 2006, https://none.cs.umass.edu/~shenoy/courses/spring15/readings/ast-Computer06.pdf, primary, checked 2026-10-19
  - supports: DM-046 (MINIX 3 multiserver design; user-mode driver overhead under 10%)
  - supports: DM-051 (reincarnation server restarts crashed drivers/servers; only servers with little internal state restartable; restarted drivers return to initial state)
  - supports: DM-047 (bug-density estimates cited from older literature — treat as rough)
[S708] Michael M. Swift, Muthukaruppan Annamalai, Brian N. Bershad, Henry M. Levy, Recovering Device Drivers (shadow drivers), OSDI 2004 (USENIX), 2004, https://static.usenix.org/events/osdi04/tech/swift/swift_html/index.html, primary, checked 2026-10-19
  - supports: DM-051 (shadow drivers mask driver failures to applications; only transient fail-stop faults; 4–5 s recovery pauses; irreversible side effects not undoable)
  - supports: DM-047 (only 18% of injected faults visible; detection is a limit)
[S709] The Chromium Projects, Memory safety, chromium.org, n.d. (accessed), https://www.chromium.org/Home/chromium-security/memory-safety/, primary, checked 2026-10-19
  - supports: DM-048 (~70% of 912 high/critical Chromium security bugs since 2015 are memory-safety issues)
[S710] MSRC Team (Gavin Thomas), A proactive approach to more secure code, Microsoft Security Response Center blog, 2019, https://www.microsoft.com/en-us/msrc/blog/2019/07/a-proactive-approach-to-more-secure-code, primary, checked 2026-10-19
  - supports: DM-048 (~70% of Microsoft CVEs each year are memory-safety issues)
[S711] Google (Android security team), Memory Safe Languages in Android 13, Google Security Blog, 2022, https://security.googleblog.com/2022/12/memory-safe-languages-in-android-13.html, primary, checked 2026-10-19
  - supports: DM-048 (memory-safety vulns fell 223→85 per year 2019–2022 as new code shifted to memory-safe languages; focus on new code, correlation not proven causation)
[S712] Linux kernel documentation, Rust: General Information, docs.kernel.org, n.d. (accessed), https://docs.kernel.org/rust/general-information.html, primary, checked 2026-10-19
  - supports: DM-048 (leaf modules must not use C bindings directly; safe abstractions encapsulate unsafe)
[S713] Rust for Linux project, Rust for Linux (project home page), rust-for-linux.com, n.d. (accessed), https://rust-for-linux.com/, primary, checked 2026-10-19
  - supports: DM-048 (Rust drivers in mainline: Android Binder, Nova, Tyr, null block)
[S714] Linux kernel documentation, The Linux Kernel Driver Interface (stable-api-nonsense.rst), kernel.org, n.d. (accessed), https://www.kernel.org/doc/html/latest/process/stable-api-nonsense.html, primary, checked 2026-10-19
  - supports: DM-049 (no stable in-kernel ABI or API; USB reworked 3+ times; merge drivers upstream)
  - supports: DM-057 (syscall interface is stable and will not break)
[S715] David Weston (Microsoft), Helping our customers through the CrowdStrike outage, Official Microsoft Blog, 2024, https://blogs.microsoft.com/blog/2024/07/20/helping-our-customers-through-the-crowdstrike-outage/, primary, checked 2026-10-19
  - supports: DM-050 (~8.5 million Windows devices affected, under 1% of Windows machines)
[S716] Microsoft Security, Windows Security best practices for integrating and managing security tools, Microsoft Security Blog, 2024, https://www.microsoft.com/en-us/security/blog/2024/07/27/windows-security-best-practices-for-integrating-and-managing-security-tools/, primary, checked 2026-10-19
  - supports: DM-050 (why security products use kernel drivers: visibility, tamper resistance, ELAM early boot; kernel drivers give these "at the cost of resilience"; recommend minimal kernel sensors, parsing in user mode)
  - supports: DM-051 (kernel code cannot fail and restart like user-mode code)
  - supports: DM-048 (crash was an out-of-bounds read in CSagent.sys)
[S717] CrowdStrike, External Technical Root Cause Analysis — Channel File 291, crowdstrike.com, 2024-08-06, https://crowdstrike.com/wp-content/uploads/2024/08/Channel-File-291-Incident-Root-Cause-Analysis-08.06.2024.pdf, primary, checked 2026-10-19
  - supports: DM-050 (21-field template vs 20 inputs; out-of-bounds read in Content Interpreter; no runtime bounds check; no staged deployment of content)
  - supports: DM-048 (memory-safety bug class in privileged code)
[S718] David Weston (Microsoft), Windows security and resiliency: Protecting your business, Windows Experience Blog, 2024-11-19, https://blogs.windows.com/windowsexperience/2024/11/19/windows-security-and-resiliency-protecting-your-business/, primary, checked 2026-10-19
  - supports: DM-050 (Microsoft building capability for AV to run in user mode; private preview July 2025; deployment rings mandatory; Quick Machine Recovery)
[S719] Microsoft Learn, WDDM Support for Timeout Detection and Recovery (TDR), learn.microsoft.com, 2025 (page date), https://learn.microsoft.com/en-us/windows-hardware/drivers/display/timeout-detection-and-recovery, primary, checked 2026-10-19
  - supports: DM-051 (GPU reset and driver reinit without reboot; bug-check after 5+ hangs within a minute; legacy apps may render black)
[S720] Microsoft Learn, Overview of UMDF, learn.microsoft.com, 2017 (page date), https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/overview-of-the-umdf, primary, checked 2026-10-19
  - supports: DM-047 (user-mode driver framework with host processes and kernel reflector)
  - supports: DM-049 (Windows ships a stable vendor-facing driver framework, UMDF v1/v2)
  - supports: DM-051 (file system, display and print drivers cannot be UMDF)
[S721] Linus Torvalds, Re: [PATCH v10 08/14] mm: multi-gen LRU: support page table walks, LKML (lkml.iu.edu archive), 2022-04-15, https://lkml.iu.edu/hypermail/linux/kernel/2204.1/11440.html, primary, checked 2026-10-19
  - supports: DM-051 (BUG_ON kills the machine with locks held, undebuggable; prefer WARN_ON_ONCE)
[S722] Jason Gunthorpe (quoting Linus Torvalds), Re: [PATCH v1] mm/gup: fix FOLL_FORCE COW security issue and remove FOLL_COW, LKML (lkml.iu.edu archive), 2022-08-09, https://lkml.iu.edu/hypermail/linux/kernel/2208.1/01957.html, primary, checked 2026-10-19
  - supports: DM-051 ("absolutely no new BUG_ON() calls" rule; panic_on_warn)
[S723] Michael Kerrisk et al., errno(3), Linux man-pages project, n.d. (accessed), https://man7.org/linux/man-pages/man3/errno.3.html, primary, checked 2026-10-19
  - supports: DM-052 (errno thread-local, never reset to zero, meaningful only after failure; pthreads return codes instead; values differ across archs; aliased names)
[S724] Linux man-pages project, syscall(2), man7.org, n.d. (accessed), https://man7.org/linux/man-pages/man2/syscall.2.html, primary, checked 2026-10-19
  - supports: DM-052 (per-arch error signalling: MIPS/Alpha a3, SPARC carry bit, PowerPC cr0)
  - supports: DM-057 (raw syscall conventions are per architecture)
[S725] Fuchsia project, Zircon kernel errors, fuchsia.dev, n.d. (accessed), https://fuchsia.dev/fuchsia-src/concepts/kernel/errors, primary, checked 2026-10-19
  - supports: DM-052 (zx_status_t categories; precise semantics such as INVALID_ARGUMENT vs OUT_OF_RANGE, WRONG_TYPE, PEER_CLOSED)
[S726] Linux kernel documentation, ioctl based interfaces, docs.kernel.org, n.d. (accessed), https://docs.kernel.org/driver-api/ioctl.html, primary, checked 2026-10-19
  - supports: DM-053 (compat_ioctl requirement, padding/alignment differences, pointers as __u64, uninitialised-data leaks, versioned commands discouraged, ENOTTY vs ENOSYS/EINVAL misuse)
  - supports: DM-056 (struct timespec/timeval incompatible after 64-bit time_t move)
  - supports: DM-052 (subsystems return wrong error codes for unknown commands)
[S727] Linux kernel documentation, Adding a New System Call, docs.kernel.org, n.d. (accessed), https://docs.kernel.org/process/adding-syscalls.html, primary, checked 2026-10-19
  - supports: DM-055 (reject unknown flags with EINVAL; extensible struct with size; list of *2/*3 follow-up syscalls)
  - supports: DM-053 (ioctl leads to opaque API; fcntl/prctl multiplexers hide complexity)
  - supports: DM-056 (compat layer needed for loff_t/__u64 args on 32-bit)
[S728] Linux man-pages project, openat2(2), man7.org, n.d. (accessed), https://man7.org/linux/man-pages/man2/openat2.2.html, primary, checked 2026-10-19
  - supports: DM-055 (openat ignores unknown flag bits, openat2 rejects them; size as version; E2BIG for unsupported nonzero extension fields)
[S729] Linux kernel documentation, sysfs — The filesystem for exporting kernel objects, docs.kernel.org, n.d. (accessed), https://docs.kernel.org/filesystems/sysfs.html, primary, checked 2026-10-19
  - supports: DM-054 (ASCII text, preferably one value per file; mixing types and multi-line formatting frowned upon)
[S730] Linux man-pages project, proc_pid_mountinfo(5), man7.org, n.d. (accessed), https://man7.org/linux/man-pages/man5/proc_pid_mountinfo.5.html, primary, checked 2026-10-19
  - supports: DM-054 (variable optional fields terminated by "-"; parsers must scan; mount IDs reused)
[S731] Linux man-pages project, statmount(2), man7.org, n.d. (accessed), https://man7.org/linux/man-pages/man2/statmount.2.html, primary, checked 2026-10-19
  - supports: DM-054 (binary struct with mask of filled fields as structured mount-info interface)
  - supports: DM-055 (mask-based extensibility)
[S732] Andrii Nakryiko, [PATCH v3 0/9] ioctl()-based API to query VMAs from /proc/<pid>/maps, LKML (lkml.iu.edu archive), 2024-06-04, https://lkml.iu.edu/hypermail/linux/kernel/2406.0/03928.html, primary, checked 2026-10-19
  - supports: DM-054 (binary PROCMAP_QUERY: ~5x end-to-end, ~35–40x microbenchmark vs text parsing for profiling)
[S733] Linux man-pages project, clone3(2) (clone.2), man7.org, n.d. (accessed), https://man7.org/linux/man-pages/man2/clone3.2.html, primary, checked 2026-10-19
  - supports: DM-055 (size argument for extensibility; clone flags low byte is exit signal; CLONE_PID/CLONE_STOPPED silently ignored then bits reused)
[S734] Linux man-pages project, statx(2), man7.org, n.d. (accessed), https://man7.org/linux/man-pages/man2/statx.2.html, primary, checked 2026-10-19
  - supports: DM-056 (statx added Linux 4.11 / glibc 2.28)
  - supports: DM-055 (request/response mask; kernel does not reject unknown mask bits)
[S735] Linux man-pages project, stat(2), man7.org, n.d. (accessed), https://man7.org/linux/man-pages/man2/stat.2.html, primary, checked 2026-10-19
  - supports: DM-056 (three generations of stat on 32-bit: oldstat, stat, stat64; fstatat64 vs newfstatat; glibc repacking)
[S736] Debian project, ReleaseGoals/64bit-time, wiki.debian.org, 2024 (accessed), https://wiki.debian.org/ReleaseGoals/64bit-time, primary, checked 2026-10-19
  - supports: DM-056 (~495 library packages changed ABI; 5–6k rebuilds; t64 renames; i386 excluded to keep legacy binaries running; 58 ABI changes from enabling LFS)
[S737] GNU C Library manual, Feature Test Macros, sourceware.org, n.d. (accessed), https://sourceware.org/glibc/manual/latest/html_node/Feature-Test-Macros.html, primary, checked 2026-10-19
  - supports: DM-056 (_LARGEFILE64_SOURCE parallel *64 API; _FILE_OFFSET_BITS=64 redirect, default still 32; _TIME_BITS=64 requires _FILE_OFFSET_BITS=64)
[S738] The Go Authors, Go 1.11 Release Notes, go.dev, 2018, https://go.dev/doc/go1.11, primary, checked 2026-10-19
  - supports: DM-057 (Go runtime on macOS/iOS calls libSystem instead of kernel for forward compatibility)
[S739] The Go Authors, Go 1.16 Release Notes, go.dev, 2021, https://go.dev/doc/go1.16, primary, checked 2026-10-19
  - supports: DM-057 (OpenBSD 6.9+ requires syscalls via libc for non-static binaries; Go switched)
[S740] OpenBSD project, pinsyscalls(2), man.openbsd.org, n.d. (accessed), https://man.openbsd.org/pinsyscalls.2, primary, checked 2026-10-19
  - supports: DM-057 (OpenBSD 7.5 pins each syscall number to its libc call site; mismatch → SIGABRT)
[S741] Jonathan Corbet, Resistance to Rust abstractions for DMA mapping, LWN.net, 2025-01-30, https://lwn.net/Articles/1006805/, secondary, checked 2026-10-20
  - supports: DM-048 (Hellwig: "No rust code in kernel/dma"; multi-language kernel "impossible to maintain"; Rust developers offered to maintain wrapper layer)
[S742] Nicolas Palix et al., Faults in Linux: Ten Years Later (project site), ASPLOS 2011, 2011, https://faultlinux.lip6.fr, primary, checked 2026-10-19, unverified
  - supports: DM-047 (connection refused; claims taken from S701 instead — not counted)
[S743] Linus Torvalds, Re: [Regression w/ patch] Media commit causes user space to misbahave (was: Re: Linux 3.8-rc1), LKML (lkml.iu.edu archive), 2012-12-23, https://lkml.iu.edu/hypermail/linux/kernel/1212.2/03058.html, primary, checked 2026-10-20
  - supports: DM-057 ("WE DO NOT BREAK USERSPACE"; breaking user programs is a kernel bug)
  - supports: DM-053 (an ioctl returned an error code userspace did not expect — ENOENT "not a valid error return from an ioctl")
  - supports: DM-052 (error-code values are part of the ABI)
[S744] Mateusz Jurczyk (j00ru), windows-syscalls: x64 nt.csv (Windows x86-64 NT system call table), GitHub / j00ru.vexillium.org, accessed 2026, https://raw.githubusercontent.com/j00ru/windows-syscalls/master/x64/csv/nt.csv, secondary, checked 2026-10-20
  - supports: DM-057 (NtCreateFile number 0x52 → 0x53 → 0x54 → 0x55 across Windows 7/8/8.1/10)
[S745] Apple, Technical Q&A QA1118: Statically linked binaries on Mac OS X, Apple Developer Documentation Archive, 2011 (rev.), https://developer.apple.com/library/archive/qa/qa1118/_index.html, primary, checked 2026-10-20
  - supports: DM-057 (no guarantee of syscall-level compatibility; static binaries unsupported; compatibility promised at dynamic system libraries)
[S746] Pengfei Wang, Jens Krinke, Kai Lu, Gen Li, Steve Dodier-Lazaro, How Double-Fetch Situations turn into Double-Fetch Vulnerabilities: A Study of Double Fetches in the Linux Kernel, USENIX Security 2017, 2017, https://www.usenix.org/conference/usenixsecurity17/technical-sessions/presentation/wang-pengfei, primary, checked 2026-10-20
  - supports: DM-160 (90 double fetches in Linux, 57 in drivers; 6 new bugs, 3 exploitable; Linux, FreeBSD, Android)
  - supports: DM-047 (drivers dominate the double-fetch population)
[S747] Mateusz Jurczyk, Gynvael Coldwind, Identifying and Exploiting Windows Kernel Race Conditions via Memory Access Patterns (Bochspwn), SyScan 2013 (Google Research publication page), 2013, https://research.google/pubs/identifying-and-exploiting-windows-kernel-race-conditions-via-memory-access-patterns/, primary, checked 2026-10-20
  - supports: DM-160 (races in fetching user-mode input in syscall handlers on Windows; CPU-level instrumentation found exploitable cases)
[S748] A. Theodore Markettos, Colin Rothwell, Brett F. Gutstein, Allison Pearce, Peter G. Neumann, Simon W. Moore, Robert N. M. Watson, Thunderclap: Exploring Vulnerabilities in Operating System IOMMU Protection via DMA from Untrustworthy Peripherals, NDSS 2019, 2019, https://www.ndss-symposium.org/ndss-paper/thunderclap-exploring-vulnerabilities-in-operating-system-iommu-protection-via-dma-from-untrustworthy-peripherals/, primary, checked 2026-10-20
  - supports: DM-161 (macOS, FreeBSD, Linux compromised despite IOMMU via shared-memory attack surface; Windows used IOMMU only in limited cases; root shell within seconds)
[S749] Mika Westerberg, [PATCH 0/4] PCI / iommu / thunderbolt: IOMMU based DMA protection, LKML (lkml.iu.edu archive), 2018-11-12, https://lkml.iu.edu/hypermail/linux/kernel/1811.1/05486.html, primary, checked 2026-10-20
  - supports: DM-161 (Linux did not enable IOMMU automatically for Thunderbolt; follows Windows 10 1803 Kernel DMA Protection; ATS disabled for external devices; internal devices identity-mapped)
