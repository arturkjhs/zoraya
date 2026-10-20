# Stream C sources (S300–S399)

[S300] Michael Kerrisk (maintainer), signal-safety(7) — Linux manual page, man7.org / Linux man-pages project, current, https://man7.org/linux/man-pages/man7/signal-safety.7.html, primary, checked 2026-10-19
  - supports: DM-017 (definition of async-signal-safe; stdio reentrancy example; errno save/restore; glibc deviations)
[S301] Qualys Threat Research Unit, regreSSHion: RCE in OpenSSH's server, on glibc-based Linux systems (CVE-2024-6387), Qualys security advisory, 2024, https://www.qualys.com/2024/07/01/cve-2024-6387/regresshion.txt, primary, checked 2026-10-19
  - supports: DM-017 (SIGALRM handler calling syslog() -> heap corruption; regression of CVE-2006-5051; OpenBSD syslog_r not affected)
[S302] Linux man-pages project, signalfd(2) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man2/signalfd.2.html, primary, checked 2026-10-19
  - supports: DM-017 (fd-based signal delivery; must block signals; blocked mask inherited across exec; synchronous signals still need handlers)
[S303] Neil Brown, Ghosts of Unix past, part 3: Unfixable designs, LWN.net, 2010, https://lwn.net/Articles/414618/, secondary, checked 2026-10-19
  - supports: DM-017 (history of signal fixes: one-shot handlers, EINTR, sigaction, pselect, RT signal queue limits; signalfd as replacement; comment on exec inheritance problem)
[S304] Jochen Liedtke, On µ-Kernel Construction, 15th ACM SOSP, 1995, https://cgi.cse.unsw.edu.au/~cs9242/22/papers/Liedtke_95.pdf, primary, checked 2026-10-19
  - supports: DM-018 (inefficiency from overloading/implementation not concept; Mach 18 µs kernel call on 486 = ~800 cycles kernel overhead vs L3 15 cycles; Chen/Bershad MCPI reanalysis attributing cache misses to Mach's working set; processor dependence)
[S305] Gernot Heiser, Kevin Elphinstone, L4 Microkernels: The Lessons from 20 Years of Research and Deployment, ACM Transactions on Computer Systems 34(1), 2016, https://trustworthy.systems/publications/nicta_full_text/8988.pdf, primary, checked 2026-10-19
  - supports: DM-018 (early-90s one-way IPC ~100 µs; L4 10–20x faster; long IPC, IPC timeouts, clans & chiefs abandoned; thread IDs replaced by endpoints; Mach ports buffer, seL4 endpoints do not; synchronous IPC forces multithreading and is wrong across cores; notifications added)
[S306] Hermann Härtig, Michael Hohmuth, Jochen Liedtke, Sebastian Schönberg, Jean Wolter, The Performance of µ-Kernel-Based Systems, 16th ACM SOSP, 1997, https://os.inf.tu-dresden.de/pubs/sosp97, primary, checked 2026-10-19
  - supports: DM-018 (L4Linux within 5% of native Linux on AIM; MkLinux penalty ~5x (in-kernel) and ~7x (user-level) larger)
[S307] J. Bradley Chen, Brian N. Bershad, The Impact of Operating System Structure on Memory System Performance, 14th ACM SOSP (abstract page, CMU Mach project), 1993, https://www.cs.cmu.edu/afs/cs/project/mach/public/www/doc/abstracts/os-memorysys.html, primary, checked 2026-10-19
  - supports: DM-018 (Mach 3.0 + Unix server executes substantially more system instructions and more memory cycles per instruction than Ultrix)
[S308] Jochen Liedtke, Improving IPC by Kernel Design, 14th ACM SOSP, 1993, https://rcs.uwaterloo.ca/papers/l4.pdf, primary, checked 2026-10-19, unverified
  - supports: DM-018 (20x IPC improvement; cited only via S305 since fetch failed)
[S309] Linux man-pages project, sysvipc(7) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man7/sysvipc.7.html, primary, checked 2026-10-19
  - supports: DM-019 (get calls return identifiers used by the rest of the API; POSIX alternatives exist)
[S310] Linux man-pages project, ftok(3) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man3/ftok.3.html, primary, checked 2026-10-19
  - supports: DM-019 (keys derived from inode/device bits; no uniqueness guarantee; collision example)
[S311] Linux man-pages project, shmctl(2) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man2/shmctl.2.html, primary, checked 2026-10-19
  - supports: DM-019 (IPC_RMID marks for destruction; caller must ensure eventual destruction or pages remain; ipc_perm mode bits; Linux deviation on attaching deleted segments)
[S312] Linux man-pages project, ipc_namespaces(7) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man7/ipc_namespaces.7.html, primary, checked 2026-10-19
  - supports: DM-019 (separate namespace type needed to isolate SysV identifiers and POSIX mqueues; objects destroyed with namespace)
[S313] PostgreSQL Global Development Group, Managing Kernel Resources (Server Setup and Operation), PostgreSQL documentation, current, https://www.postgresql.org/docs/current/kernel-resources.html, primary, checked 2026-10-19
  - supports: DM-019 (system-wide SHMMAX/SEMMNI etc. limits, per-OS tuning; systemd RemoveIPC deleting a user's IPC objects at logout)
[S314] PostgreSQL Global Development Group, PostgreSQL 9.3 Release Notes, PostgreSQL documentation, 2013, https://www.postgresql.org/docs/release/9.3.0/, primary, checked 2026-10-19
  - supports: DM-019 (moved most shared memory to mmap to eliminate kernel parameter tuning)
[S315] Linux man-pages project, memfd_create(2) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man2/memfd_create.2.html, primary, checked 2026-10-19
  - supports: DM-019 (fd-based anonymous shared memory, reference-counted lifetime, sealing against untrusted peers; Linux 3.17)
[S316] M. D. McIlroy, E. N. Pinson, B. A. Tague, UNIX Time-Sharing System: Foreword, Bell System Technical Journal 57(6), 1978, https://archive.org/stream/bstj57-6-1899/bstj57-6-1899_djvu.txt, primary, checked 2026-10-19
  - supports: DM-020 (style maxims: output becomes input to unknown program; avoid columnar/binary input formats)
[S317] Jeffrey P. Snover, Monad Manifesto (v1.2), Microsoft (author's site), 2002, https://mail.jsnover.com/Docs/MonadManifesto.pdf, primary, checked 2026-10-19
  - supports: DM-020 (text pipelines force lossy re-parsing, "prayer-based parsing"; pipelines of .NET objects; legacy text as one object stream type)
[S318] David A. Wheeler, Fixing Unix/Linux/POSIX Filenames: Control Characters (such as Newline), Leading Dashes, and Other Problems, dwheeler.com (essay), 2009 (updated 2024), https://dwheeler.com/essays/fixing-unix-linux-filenames.html, weak, checked 2026-10-19
  - supports: DM-020 (newline/space/dash filenames breaking line-oriented pipelines; CVE pointers e.g. CVE-2013-7085)
[S319] Nushell project, Thinking in Nu (The Nushell Book), nushell.sh, current, https://www.nushell.sh/book/thinking_in_nu.html, primary, checked 2026-10-19
  - supports: DM-020 (structured-value pipelines; built-ins replace external text tools)
[S320] Andy Lutomirski, Re: [GIT PULL] kdbus for 4.1-rc1, linux-kernel mailing list, 2015-04-20, https://lkml.iu.edu/hypermail/linux/kernel/1504.2/02541.html, primary, checked 2026-10-19
  - supports: DM-021 (NACK; memfd over SCM_RIGHTS already handles large data; metadata could be a small UDS extension)
[S321] Andy Lutomirski, Re: [GIT PULL] kdbus for 4.1-rc1, linux-kernel mailing list, 2015-04-23, https://lkml.iu.edu/hypermail/linux/kernel/1504.2/04750.html, primary, checked 2026-10-19
  - supports: DM-021 (metadata objections incl. CRIU breakage; policy; memory accounting of pools)
[S322] Greg Kroah-Hartman, Re: [GIT PULL] kdbus for 4.1-rc1, linux-kernel mailing list, 2015-04-14, https://lkml.iu.edu/hypermail/linux/kernel/1504.1/04664.html, primary, checked 2026-10-19
  - supports: DM-021 (proponents' case: reliable metadata, fixing "almost unfixable races" in D-Bus; Biederman's objection to metadata code)
[S323] Andy Lutomirski quoting Greg Kroah-Hartman, Re: [!GIT PULL] kdbus for 4.2, linux-kernel mailing list, 2015-06-26, https://lkml.iu.edu/hypermail/linux/kernel/1506.3/02054.html, primary, checked 2026-10-19
  - supports: DM-021 (merge delayed because of continuing disagreement)
[S324] Jake Edge, Trying to head off kdbus, LWN.net, 2015, https://lwn.net/Articles/649111/, secondary, checked 2026-10-20
  - supports: DM-021 (Lutomirski: doable in userspace, memcg accounting; Torvalds unimpressed by performance argument, points at userspace)
[S325] Neil Brown, Bus1: a new Linux interprocess communication proposal, LWN.net, 2016, https://lwn.net/Articles/697191/, secondary, checked 2026-10-20
  - supports: DM-021 (bus1 capability handles, quotas, causal ordering, multicast; binder similarity)
[S326] Jonathan Corbet, IPC medley: message-queue peeking, io_uring, and bus1, LWN.net, 2026, https://lwn.net/Articles/1065490/, secondary, checked 2026-10-20
  - supports: DM-021 (bus1 stalled after 2016, reposted 2026 as a stripped-down Rust module; binder vs bus1 debate)
[S327] bus1 project, dbus-broker wiki, GitHub, current, https://github.com/bus1/dbus-broker/wiki, primary, checked 2026-10-19
  - supports: DM-021 (userspace D-Bus broker aimed at performance and reliability, Linux-only, compatible)
[S328] freedesktop.org, D-Bus Specification, freedesktop.org, current, https://dbus.freedesktop.org/doc/dbus-specification.html, primary, checked 2026-10-19
  - supports: DM-021 (bus daemon routes messages; UNIX_FD type; credentials API grew over revisions incl. ProcessFD)
[S329] Linux kernel documentation, Android Binderfs, docs.kernel.org, current, https://docs.kernel.org/admin-guide/binderfs.html, primary, checked 2026-10-19
  - supports: DM-021 (binder is an in-tree kernel IPC mechanism for Android)
[S330] Xingyu Jin (Android Security Research), The quantum state of Linux kernel garbage collection CVE-2021-0920 (Part I), Google Project Zero blog, 2022, https://projectzero.google/2022/08/the-quantum-state-of-linux-kernel.html, primary, checked 2026-10-19
  - supports: DM-022 (why SCM_RIGHTS needs a GC: in-flight refs and cycles; MSG_PEEK race; UAF exploited in the wild on Samsung devices; 2016 report not merged)
[S331] NIST NVD, CVE-2021-0920 (via NVD CVE API 2.0), NVD, 2021, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2021-0920, primary, checked 2026-10-19
  - supports: DM-022 (UAF in unix_scm_to_skb race; added to CISA KEV 2022-05-23)
[S332] NIST NVD, CVE-2024-26923 (via NVD CVE API 2.0), NVD, 2024, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2024-26923, primary, checked 2026-10-19
  - supports: DM-022 (another af_unix GC race, against connect(), dangling pointer in gc_inflight_list)
[S333] Debian Security Team, CVE-2021-0920 security tracker entry, Debian, 2021, https://security-tracker.debian.org/tracker/CVE-2021-0920, primary, checked 2026-10-19
  - supports: DM-022 (description and fix versions)
[S334] Linux man-pages project, unix(7) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man7/unix.7.html, primary, checked 2026-10-19
  - supports: DM-022 (SCM_MAX_FD; MSG_CTRUNC closes excess fds; ETOOMANYREFS in-flight limit only since 4.5; stream sockets need 1 data byte; SO_PEERCRED)
[S335] Kuniyuki Iwashima, af_unix: Allocate struct unix_vertex for each inflight AF_UNIX fd (Linux commit 1fbfdfaa5902), GitHub mirror of torvalds/linux, 2024, https://github.com/torvalds/linux/commit/1fbfdfaa590248c1d86407f578e40e5c65136330, primary, checked 2026-10-19
  - supports: DM-022 (GC rework models inflight sockets as graph vertices / fds as edges)
[S336] Kuniyuki Iwashima, af_unix: Replace garbage collection algorithm (Linux commit 4090fa373f0e), GitHub mirror of torvalds/linux, 2024, https://github.com/torvalds/linux/commit/4090fa373f0e763c43610853d2774b5979915959, primary, checked 2026-10-19
  - supports: DM-022 (SCC-based collector replaced old algorithm in 2024)
[S337] Fuchsia project, zx_channel_write, fuchsia.dev syscall reference, current, https://fuchsia.dev/reference/syscalls/channel_write, primary, checked 2026-10-19
  - supports: DM-022 (handles moved, consumed on success or failure; cannot send channel into itself; ZX_RIGHT_TRANSFER; 64 handles / 64 KiB)
[S338] Fuchsia project, Channel (kernel object), fuchsia.dev, current, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/channel, primary, checked 2026-10-19
  - supports: DM-022 (atomic bytes+handles message; endpoints not duplicable; queued handles closed when endpoint destroyed)
[S339] Microsoft, HandlerRoutine callback function (Windows Console), Microsoft Learn, 2018 (updated 2024), https://learn.microsoft.com/en-us/windows/console/handlerroutine, primary, checked 2026-10-20
  - supports: DM-017 (Windows runs console control handlers on a new system-created thread instead of interrupting one)
[S340] Arnd Bergmann, BKL: That's all, folks (Linux commit 4ba8216cd905), GitHub mirror of torvalds/linux, 2011, https://github.com/torvalds/linux/commit/4ba8216cd90560bc402f52076f64d8546e8aefcb, primary, checked 2026-10-19
  - supports: DM-058 (final removal of the big kernel lock implementation, January 2011, crediting many contributors)
[S341] Rick Lindsley, Dave Hansen, BKL: One Lock to Bind Them All, Ottawa Linux Symposium 2002, 2002, https://www.kernel.org/doc/ols/2002/ols2002-pages-301-309.pdf, primary, checked 2026-10-19
  - supports: DM-058 (BKL origin in 1.3.x SMP; recursive and "just to be safe" abuse; 2.1.23 went from 9 to 761 lock_kernel calls; 500+ calls in 2.4.18; unrelated subsystems serialized; BKL was the price of early concurrency)
[S342] Jonathan Corbet, The real BKL end game, LWN.net, 2011, https://lwn.net/Articles/424657/, secondary, checked 2026-10-20
  - supports: DM-058 (remaining users removed, moved to staging or fixed; completion "long in coming")
[S343] FreeBSD Project, SMPng Design Document (FreeBSD Architecture Handbook), docs.freebsd.org, current, https://docs.freebsd.org/en/books/arch-handbook/smp/, primary, checked 2026-10-19
  - supports: DM-058 (Giant as legacy global lock; subsystems moved out from under Giant incrementally; non-MPSAFE callouts still run under Giant)
[S344] Sean Peters, Adrian Danis, Kevin Elphinstone, Gernot Heiser, For a Microkernel, a Big Lock Is Fine, APSys 2015 (ACM), 2015, https://trustworthy.systems/publications/nictaabstracts/Peters_DEH_15.abstract, primary, checked 2026-10-19
  - supports: DM-058 (big lock adequate for microkernel with short syscalls on shared-cache cores)
[S345] Kevin Elphinstone, Amirreza Zarrabi, Adrian Danis, Yanyan Shen, Gernot Heiser, An Evaluation of Coarse-Grained Locking for Multicore Microkernels, arXiv:1609.08372, 2016, https://arxiv.org/abs/1609.08372, primary, checked 2026-10-19
  - supports: DM-058 (coarse lock gives scalability comparable to fine-grained on tightly coupled cores, avoids complexity, enables verification)
[S346] Linux man-pages project, pthread_cancel(3) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man3/pthread_cancel.3.html, primary, checked 2026-10-19
  - supports: DM-059 (deferred vs asynchronous cancellation; cleanup handler order; NPTL implements cancellation with a real-time signal)
[S347] Linux man-pages project, pthreads(7) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man7/pthreads.7.html, primary, checked 2026-10-19
  - supports: DM-059 (only 3 functions async-cancel-safe; long list of required cancellation points incl. close(), open(), read()); DM-060 (~90 POSIX functions exempt from thread-safety incl. strtok, localtime, getenv, asctime; errno per-thread)
[S348] Rich Felker (unsigned on page; EWONTFIX blog of musl author), Thread cancellation and resource leaks, ewontfix.com, 2012, https://ewontfix.com/2/, weak, checked 2026-10-19
  - supports: DM-059 (glibc async-cancel window around syscalls leaks fds from open/accept and makes close() outcome unknowable; musl PC-label approach)
[S349] Adhemerval Zanella, nptl: Fix Race conditions in pthread cancellation [BZ#12683] (glibc commit 89b53077d2a5), GitHub mirror bminor/glibc, 2024, https://github.com/bminor/glibc/commit/89b53077d2a58f00e7debdfe58afabe953dac60d, primary, checked 2026-10-19
  - supports: DM-059 (describes the two races: cancellation after syscall side effects; signal handlers running with async cancel enabled; five-point classification)
[S350] GNU C Library maintainers, glibc NEWS file, GitHub mirror bminor/glibc, current, https://raw.githubusercontent.com/bminor/glibc/master/NEWS, primary, checked 2026-10-19
  - supports: DM-059 (BZ #12683 listed as fixed in the 2.41 section; follow-up BZ #32782 cancellation race crash fixed in 2.42)
[S351] Microsoft, TerminateThread function (processthreadsapi.h), Microsoft Learn, 2018 (updated 2025), https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminatethread, primary, checked 2026-10-19
  - supports: DM-059 (asynchronous thread kill leaves critical sections and heap lock held, inconsistent kernel32/DLL state)
[S352] Oracle, Java Thread Primitive Deprecation, Java SE 21 documentation, current, https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/doc-files/threadPrimitiveDeprecation.html, primary, checked 2026-10-19
  - supports: DM-059 (Thread.stop inherently unsafe: unlocks monitors leaving damaged objects; recommend cooperative flag + interrupt)
[S353] Linux man-pages project, strtok(3) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man3/strtok.3.html, primary, checked 2026-10-19
  - supports: DM-060 (static buffer, MT-Unsafe race:strtok, use strtok_r)
[S354] Linux man-pages project, ctime(3) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man3/ctime.3.html, primary, checked 2026-10-19
  - supports: DM-060 (localtime/asctime return static data; MT-Unsafe race:tmbuf env locale; _r variants from SUSv2; localtime_r need not call tzset)
[S355] Linux man-pages project, errno(3) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man3/errno.3.html, primary, checked 2026-10-19
  - supports: DM-060 (errno is a per-thread lvalue/macro; old `extern int errno` declarations now harmful)
[S356] The Rust Project, Rust 2024: Unsafe functions (std::env::set_var, remove_var), The Rust Edition Guide, 2025, https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html, primary, checked 2026-10-19
  - supports: DM-060 (set_var/remove_var made unsafe because environment mutation is unsound in multithreaded programs on some platforms)
[S357] Glenn E. Reeves, What really happened on Mars? (authoritative account, email of 15 Dec 1997), copy hosted at UNC course page, 1997, https://www.cs.unc.edu/~anderson/teach/comp790/papers/mars_pathfinder_long_version.html, primary, checked 2026-10-19
  - supports: DM-061 (bus task blocked by meteorological task preempted by medium tasks; PI option off by default in VxWorks select() mutex for performance; reproduced on replica in <18h; fixed by uploading a global-variable change; "We just missed it")
[S358] Lui Sha, Ragunathan Rajkumar, John P. Lehoczky, Priority Inheritance Protocols: An Approach to Real-Time Synchronization, IEEE Transactions on Computers 39(9), 1990, https://www.math.unipd.it/~tullio/RTS/2009/SRL-1990.pdf, primary, checked 2026-10-19
  - supports: DM-061 (uncontrolled priority inversion with semaphores/monitors; basic PI and priority ceiling; ceiling bounds blocking to one critical section and prevents deadlock)
[S359] Steven Rostedt (updated Alex Shi), RT-mutex implementation design, Linux kernel documentation, 2006/2017, https://docs.kernel.org/locking/rt-mutex-design.html, primary, checked 2026-10-19
  - supports: DM-061 (unbounded inversion definition; PI chains; chain depth hard to bound, implementation caps it)
[S360] Linux kernel documentation, Lightweight PI-futexes, docs.kernel.org, current, https://docs.kernel.org/locking/pi-futex.html, primary, checked 2026-10-19
  - supports: DM-061 (PI for user-space pthread mutexes; fast path in user space, kernel rt-mutex on contention; strict owner rules); DM-120 (PI futex ops add kernel state to the futex interface)
[S361] Victor Yodaiken, Against priority inheritance, FSMLabs Technical Report, 2002, https://www.math.unipd.it/~tullio/SCD/2007/Materiale/Yodaiken-200207.pdf, primary, checked 2026-10-19
  - supports: DM-061 (PI unreliable/complex, original algorithm wrong, compensates for bad design; priority ceiling known since Lampson & Redell 1980; RTLinux omits PI)
[S362] Doug Locke, Priority Inheritance: The Real Story, LinuxDevices (guest editorial, archived copy), 2002, https://www.math.unipd.it/~tullio/SCD/2007/Materiale/Locke.pdf, secondary, checked 2026-10-19
  - supports: DM-061 (rebuttal: technical points correct but conclusions a strawman; PI is a legitimate tool; static analysis for critical systems)
[S363] The Open Group / IEEE, pthread_mutexattr_getprotocol, pthread_mutexattr_setprotocol, POSIX.1-2024 (Base Specifications Issue 8), 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_mutexattr_getprotocol.html, primary, checked 2026-10-20
  - supports: DM-061 (default protocol PTHREAD_PRIO_NONE; PI/PP are optional TPI/TPP features)
[S364] Jade Alglave, Luc Maranget, Paul E. McKenney, Andrea Parri, Alan Stern, Frightening Small Children and Disconcerting Grown-ups: Concurrency in the Linux Kernel, ASPLOS 2018, 2018, https://discovery.ucl.ac.uk/id/eprint/10070727/1/asplos18.pdf, primary, checked 2026-10-20
  - supports: DM-062 (30+ architectures with differing weak models; prose docs ambiguous; Torvalds rejects C11 model for kernel; executable cat/herd model; fixed bugs relying on lock-unlock ordering, ARM64 ordering)
[S365] David Howells, Paul E. McKenney, Will Deacon, Peter Zijlstra, Linux Kernel Memory Barriers (memory-barriers.txt), Linux kernel documentation, current, https://docs.kernel.org/core-api/wrappers/memory-barriers.html, primary, checked 2026-10-19
  - supports: DM-062 ("not a specification", incomplete; defers to tools/memory-model; Alpha dependency ordering; READ_ONCE barrier since v4.15)
[S366] Hans-J. Boehm, Threads Cannot Be Implemented As a Library, PLDI 2005, 2005, https://www.scs.stanford.edu/~zyedidia/docs/papers/boehm_threadlib.pdf, primary, checked 2026-10-19
  - supports: DM-062 (library-only threads on a thread-unaware compiler cannot guarantee correctness; C with Pthreads); DM-060 (threads retrofitted onto a language/library designed without them)
[S367] Ulrich Drepper, Futexes Are Tricky (v1.6), Red Hat (author's site akkadia.org), 2011, https://www.akkadia.org/drepper/futex.pdf, primary, checked 2026-10-20
  - supports: DM-120 (one multiplexing syscall; inventors' reference code was incorrect; FUTEX_REQUEUE "broken and unusable", kept for compatibility)
[S368] NIST NVD, CVE-2014-3153 (via NVD CVE API 2.0), NVD, 2014, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2014-3153, primary, checked 2026-10-19
  - supports: DM-120 (futex_requeue did not ensure distinct futex addresses -> local privilege escalation; in CISA KEV 2022-05-25)
[S369] Linux kernel documentation, futex2, docs.kernel.org, current, https://docs.kernel.org/userspace-api/futex2.html, primary, checked 2026-10-20
  - supports: DM-120 (new interface "designed to overcome limitations of the original"; futex_waitv waits on up to 128 futexes; only 32-bit size supported)
