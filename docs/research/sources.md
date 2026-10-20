# Sources

Bibliography for TASK-01. Format: `[S<N>] Authors, Title, Venue/Publisher, Year, URL, type, checked <date>`.
IDs were allocated in blocks per research stream (S100–S199 stream A, … S1100–S1199 stream K), so gaps are
expected. Where two streams fetched the same URL the lower ID was kept and the other merged into it (see
the merge table at the end). Indented `supports:` lines record which claim each source was cited for, so the
citation can be audited.

[S100] Andrew Baumann, Jonathan Appavoo, Orran Krieger, Timothy Roscoe, A fork() in the road, HotOS '19 (ACM), 2019, https://www.microsoft.com/en-us/research/wp-content/uploads/2019/04/fork-hotos19.pdf, primary, checked 2026-10-19
  - supports: DM-001 (fork history and Ritchie quotes; thread-unsafety, insecurity, ASLR, cost numbers, overcommit, composability, K42 experience, spawn/cross-process alternatives, snapshot use cases)
  - supports: GR-005 (fork problems; posix_spawn ~0.5ms constant; spawn + cross-process ops; posix_spawn gaps; Cygwin fork unreliability)
[S101] Mairin Duffy (ed.), Ulrich Drepper, Orran Krieger, A fork() in the road – conversation between Uli Drepper & Orran Krieger, Boston University Red Hat Collaboratory blog, 2019, https://www.bu.edu/rhcollab/2019/04/11/a-fork-in-the-road, secondary, checked 2026-10-19
  - supports: DM-001 (Drepper's dissent: fork+exec bad, but fork/clone worth keeping; no consensus)
[S102] The Open Group / IEEE, posix_spawn, posix_spawnp — spawn a process (Base Specifications Issue 8, IEEE Std 1003.1-2024), The Open Group, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_spawn.html, primary, checked 2026-10-19
  - supports: DM-001 (rationale: fork hard without MMU; limits: no arbitrary child code, exit-127 error reporting, optional feature)
  - supports: DM-002 (file actions give explicit child fd table; FD_CLOEXEC/FD_CLOFORK handling)
  - supports: DM-008 (ARG_MAX limit on args plus environment)
  - supports: GR-005 (rationale: no-MMU/realtime; not a full fork/exec replacement; exit 127 errors)
[S103] Fuchsia project, Process creation, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/process/process_creation, primary, checked 2026-10-19
  - supports: DM-001 (launcher builds process from caller-supplied kernel objects; job policy forbids direct construction)
  - supports: DM-002 (child receives only the handles it is given)
  - supports: DM-003 (processes reached through handles/kernel objects)
  - supports: DM-004 (distinct process objects with startup message; thread split [weak support])
  - supports: DM-005 (process lifetime via handles)
  - supports: DM-008 (handles passed at spawn)
  - supports: DM-100 (process creation scoped by job policy)
  - supports: GR-005 (Launcher/fdio_spawn; caller supplies job, executable, handles; no direct creation in fuchsia job)
  - supports: GR-004 (namespace supplied by creator)
[S104] Microsoft, CreateProcessW function (processthreadsapi.h), Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw, primary, checked 2026-10-19
  - supports: DM-001 (explicit-parameter process creation as alternative)
  - supports: DM-002 (bInheritHandles, PROC_THREAD_ATTRIBUTE_HANDLE_LIST for concurrent spawns)
  - supports: DM-008 (single command-line string; unquoted path ambiguity security remark)
[S105] Victor Stinner, PEP 446 – Make newly created file descriptors non-inheritable, Python Software Foundation, 2013, https://peps.python.org/pep-0446/, primary, checked 2026-10-19
  - supports: DM-002 (leaked-fd bugs and security cases; fcntl race; Python 3.4 default change)
[S106] Linux man-pages project, open(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/open.2.html, primary, checked 2026-10-19
  - supports: DM-002 (O_CLOEXEC since 2.6.23; F_SETFD race in multithreaded programs)
[S107] The Open Group / IEEE, open, openat — open file (Base Specifications Issue 8), The Open Group, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/open.html, primary, checked 2026-10-19
  - supports: DM-002 (O_CLOEXEC/O_CLOFORK needed to avoid data race; O_CLOFORK added in Issue 8)
[S108] Linux man-pages project, close_range(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/close_range.2.html, primary, checked 2026-10-19
  - supports: DM-002 (Linux 5.9, CLOSE_RANGE_CLOEXEC 5.11; replaces close loops and /proc/self/fd scanning)
[S109] Jonathan Corbet, Completing the pidfd API, LWN.net, 2019, https://lwn.net/Articles/794707/, secondary, checked 2026-10-19
  - supports: DM-003 (PID reuse as the main problem; pidfd_open in 5.3; waitid P_PIDFD)
  - supports: DM-005 (pidfd exit polling; waitid limited to children)
  - supports: GR-001 (processes brought into the fd model; PID reuse races)
[S110] Linux man-pages project, pidfd_open(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/pidfd_open.2.html, primary, checked 2026-10-19
  - supports: DM-003 (5.3; conditions under which PIDs are not recycled; CLONE_PIDFD; setns/pidfd_getfd/process_madvise uses)
  - supports: DM-005 (pollable exit; waitid only for own children)
  - supports: DM-130 (Linux 5.3; process exit waitable via poll/epoll; avoids PID recycling; caveats with SIGCHLD SIG_IGN)
[S111] Linux man-pages project, pidfd_send_signal(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html, primary, checked 2026-10-19
  - supports: DM-003 (kill by PID can signal the wrong process after reuse; pidfd is stable reference; Linux 5.1)
[S112] Jonathan Corbet, clone3(), fchmodat4(), and fsinfo(), LWN.net, 2019, https://lwn.net/Articles/792628/, secondary, checked 2026-10-19
  - supports: DM-003 (CLONE_PIDFD took the last clone flag bit; clone3 extensible struct)
[S113] Microsoft, Process Handles and Identifiers, Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers, primary, checked 2026-10-19
  - supports: DM-003 (handles valid until closed even after termination; PID valid only while process lives)
  - supports: DM-005 (handle-based waiting/lifetime as alternative)
[S114] Linux man-pages project, nptl(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/nptl.7.html, primary, checked 2026-10-19
  - supports: DM-004 (kernel credentials per-thread; glibc signal broadcast to apply setuid to all threads)
[S115] Go project, syscall: Setuid/Setgid doesn't apply to all threads on Linux (issue #1435), GitHub golang/go, 2011, https://github.com/golang/go/issues/1435, primary, checked 2026-10-19
  - supports: DM-004 (Go setuid changed only one thread; open 2011, fixed in Go 1.16)
[S116] Linux man-pages project, signal(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/signal.7.html, primary, checked 2026-10-19
  - supports: DM-004 (process-directed signal goes to an arbitrary unblocked thread; dispositions per-process)
  - supports: DM-006 (signal dispositions are process-wide)
  - supports: DM-027 (SA_RESTART lists; calls never restarted: poll/epoll_wait/select/nanosleep/SO_RCVTIMEO sockets; Linux EINTR after SIGSTOP/SIGCONT with no handler)
[S117] Microsoft, About Processes and Threads, Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/win32/procthread/about-processes-and-threads, primary, checked 2026-10-19
  - supports: DM-004 (NT process as resource container, thread as schedulable unit, per-thread security context)
[S118] Linux man-pages project, pthreads(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/pthreads.7.html, primary, checked 2026-10-19
  - supports: DM-004 (POSIX process-wide vs per-thread attributes; LinuxThreads nonconformances; NPTL fixes through 2.6.16)
  - supports: DM-006 (cwd, umask, dispositions required to be process-wide)
  - supports: DM-059 (only 3 functions async-cancel-safe; long list of required cancellation points incl. close(), open(), read()); DM-060 (~90 POSIX functions exempt from thread-safety incl. strtok, localtime, getenv, asctime; errno per-thread)
[S119] Linux man-pages project, PR_SET_CHILD_SUBREAPER(2const), man7.org, n.d., https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html, primary, checked 2026-10-19
  - supports: DM-005 (subreaper since Linux 3.4; rationale: session/service managers, double-forked daemons)
  - supports: DM-100 (subreaper retrofit for supervisors)
[S120] Linux man-pages project, wait(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/wait.2.html, primary, checked 2026-10-19
  - supports: DM-005 (zombies occupy process-table slots; reparenting to init/subreaper; SIG_IGN behaviour and Linux 2.4 difference)
[S121] Thomas Orozco (krallin), tini — A tiny but valid init for containers (README), GitHub, n.d., https://github.com/krallin/tini, weak, checked 2026-10-19
  - supports: DM-005 (PID 1 in containers fails to reap zombies and lacks default signal handling; docker --init)
[S122] Linux man-pages project, openat(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/openat.2.html, primary, checked 2026-10-19
  - supports: DM-006 (rationale: races with path components; emulating a per-thread cwd)
[S123] Linux man-pages project, umask(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/umask.2.html, primary, checked 2026-10-19
  - supports: DM-006 (umask cannot be read without being set; race in multithreaded programs; /proc Umask field since 4.7)
[S124] Linux man-pages project, unshare(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/unshare.2.html, primary, checked 2026-10-19
  - supports: DM-006 (CLONE_FS unshares root, cwd and umask)
[S125] Fuchsia project, Namespaces, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/process/namespaces, primary, checked 2026-10-19
  - supports: DM-006 (no global root; per-component namespace of handles; client-side `..` and `/` emulation in fdio; no ambient authority)
[S126] Microsoft, Dynamic-link library search order, Microsoft Learn, 2023, https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order, primary, checked 2026-10-19
  - supports: DM-007 (search order incl. current folder and PATH; planting warning; safe search mode, KnownDLLs, LOAD_LIBRARY_SEARCH flags; packaged-app dependency graph)
[S127] Linux man-pages project, ld.so(8), man7.org, n.d., https://man7.org/linux/man-pages/man8/ld.so.8.html, primary, checked 2026-10-19
  - supports: DM-007 (DT_RPATH/LD_LIBRARY_PATH/DT_RUNPATH/cache/default order; secure-execution mode; $ORIGIN)
  - supports: DM-011 (secure-execution mode/AT_SECURE: loader must scrub environment for setuid/capability binaries)
  - supports: DM-085 (LD_PRELOAD/LD_LIBRARY_PATH semantics; list of stripped variables incl. GCONV_PATH, TZDIR)
[S128] Microsoft, Microsoft Security Advisory 2269637: Insecure Library Loading Could Allow Remote Code Execution, Microsoft Learn (Security Updates), 2010 (updated 2014), https://learn.microsoft.com/en-us/security-updates/securityadvisories/2010/2269637, primary, checked 2026-10-19
  - supports: DM-007 (binary planting; root cause insufficiently qualified paths; bulletins 2010–2014; opt-in mitigation tool)
[S129] Eelco Dolstra, The Purely Functional Software Deployment Model (PhD thesis), Utrecht University, 2006, https://research-portal.uu.nl/en/publications/the-purely-functional-software-deployment-model/, primary, checked 2026-10-19
  - supports: DM-007 (interference and undeclared dependencies as deployment failure causes; hashed store paths never overwrite each other) [abstract only read]
  - supports: DM-090 (isolated components; atomic upgrades/rollback via user environments) [abstract only read]
[S130] Eelco Dolstra, Andres Löh, Nicolas Pierron, NixOS: A purely functional Linux distribution, Journal of Functional Programming 20(5–6), Cambridge University Press, 2010, https://www.cambridge.org/core/journals/journal-of-functional-programming/article/nixos-a-purely-functional-linux-distribution/C1ACBA2A51D2E5466820F5B5086EA2CE, primary, checked 2026-10-19
  - supports: DM-007 (immutable, purely functional package composition) [abstract only read]
  - supports: DM-090 (imperative destructive updates cause rollback/upgrade problems) [abstract only read]
[S131] Rust Security Response WG, Security advisory for the standard library (CVE-2024-24576), Rust Blog, 2024, https://blog.rust-lang.org/2024/04/09/cve-2024-24576/, primary, checked 2026-10-19
  - supports: DM-008 (Windows single command string; cmd.exe splitting defeated escaping; no escaping is complete; fix in 1.77.2)
[S132] Microsoft, Parsing C command-line arguments, Microsoft Learn, 2021, https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments, primary, checked 2026-10-19
  - supports: DM-008 (MSVC runtime splitting rules, backslash/quote parity)
[S133] Ubuntu Security Team, CVE-2017-1000117, Canonical Ubuntu CVE tracker, 2017, https://people.canonical.com/~ubuntu-security/cve/2017/CVE-2017-1000117, primary, checked 2026-10-19
  - supports: DM-008 (crafted ssh:// host passed as option to ssh via .gitmodules → command execution)
[S134] Android Open Source Project, A/B (seamless) system updates, source.android.com, n.d., https://source.android.com/docs/core/ota/ab, primary, checked 2026-10-19
  - supports: DM-090 (background update to inactive slot; fallback after failed boots; downtime only a reboot; Virtual A/B in Android 10)
  - supports: DM-091 (switch at planned reboot)
  - supports: GR-017 (two slots; fallback on failed boot; background install; ChromeOS also uses A/B)
[S135] OSTree project, OSTree Overview (Introduction), ostreedev.github.io, n.d., https://ostreedev.github.io/ostree/introduction/, primary, checked 2026-10-19
  - supports: DM-090 (atomic tree deployments via hardlinks; read-only /usr; /etc 3-way merge; /var shared; image replication rigidity)
[S136] systemd project, systemd.offline-updates — Implementation of offline updates in systemd, freedesktop.org, n.d., https://www.freedesktop.org/software/systemd/man/latest/systemd.offline-updates.html, primary, checked 2026-10-19
  - supports: DM-090 (updating running system conflicts with running libraries/services; offline update boot)
  - supports: DM-091 (offline updates as alternative to live replacement)
[S137] Debian dpkg maintainers, dpkg(1), man7.org, n.d., https://man7.org/linux/man-pages/man1/dpkg.1.html, primary, checked 2026-10-19
  - supports: DM-090 (half-installed/half-configured states; --configure -a and --audit recovery)
[S138] Microsoft, MoveFileExW function (winbase.h), Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw, primary, checked 2026-10-19
  - supports: DM-091 (MOVEFILE_DELAY_UNTIL_REBOOT, PendingFileRenameOperations; admin only; return value does not reflect the eventual move)
[S139] Microsoft, About Restart Manager, Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/win32/rstmgr/about-restart-manager, primary, checked 2026-10-19
  - supports: DM-091 (restarts needed chiefly because files are in use; Restart Manager in Windows Installer 4.0; critical services still need reboot)
  - supports: DM-044 ("primary reason" installs need restart is files in use; Restart Manager shuts apps down)
[S140] Thomas Liske, needrestart (README), GitHub, n.d., https://github.com/liske/needrestart, secondary, checked 2026-10-19
  - supports: DM-091 (detecting daemons running outdated libraries; reliance on /proc/<pid>/maps; interpreter heuristic imperfect)
[S141] Linux kernel documentation, Livepatch, docs.kernel.org, n.d., https://docs.kernel.org/livepatch/livepatch.html, primary, checked 2026-10-19
  - supports: DM-091 (livepatch motivation; consistency model; only traceable functions; transitions can stall)
[S142] Mickey Shkatov, Jesse Michael (Eclypsium), There's a Hole in the Boot (BootHole, CVE-2020-10713), Eclypsium, 2020, https://eclypsium.com/research/theres-a-hole-in-the-boot/, primary, checked 2026-10-19
  - supports: DM-092 (overflow parsing unsigned grub.cfg bypasses Secure Boot; >80 shims; dbx ordering and bricking risk; failed-reboot regressions)
[S143] CERT/CC, VU#811862: Image files in UEFI can be abused to modify boot behavior (LogoFAIL), CERT Coordination Center, 2023 (revised 2025), https://kb.cert.org/vuls/id/811862, primary, checked 2026-10-19
  - supports: DM-092 (logo image parsers in privileged firmware; AMI/Insyde/Phoenix CVEs; shared supply-chain libraries)
[S144] Microsoft, KB5025885: How to manage the Windows Boot Manager revocations for Secure Boot changes associated with CVE-2023-24932, Microsoft Support, 2023 (updated 2026), https://support.microsoft.com/en-us/topic/kb5025885-how-to-manage-the-windows-boot-manager-revocations-for-secure-boot-changes-associated-with-cve-2023-24932-41a975df-beb2-40c1-99a3-b3ff139f832d, primary, checked 2026-10-19
  - supports: DM-092 (multi-year staged revocation; irreversible; breaks old media; firmware DB/DBX update failures)
[S145] Oxide Computer Company, phbl — Pico Host Boot Loader (README), GitHub, n.d., https://github.com/oxidecomputer/phbl, primary, checked 2026-10-19
  - supports: DM-092 (minimal loader from reset vector loading embedded host OS; describes what, not why)
[S146] Chromium OS project, Verified Boot (design document), chromium.org, n.d., https://www.chromium.org/chromium-os/chromiumos-design-docs/verified-boot/, primary, checked 2026-10-19
  - supports: DM-092 (read-only firmware stub verifies RW firmware and kernel)
  - supports: DM-093 (on-demand hash-tree rootfs verification; developer mode; explicit limitations incl. rollback replay and stateful data)
  - supports: GR-017 (RO firmware root of trust; RSA-signed hashes; on-demand block hashing; full verify would add >=5s; stateful partition not protected; users own computers)
[S147] Linux kernel documentation, dm-verity, docs.kernel.org, n.d., https://docs.kernel.org/admin-guide/device-mapper/verity.html, primary, checked 2026-10-19
  - supports: DM-093 (hash tree; root hash is sole trust anchor; read-only target)
[S148] Android Open Source Project, Verified Boot, source.android.com, n.d., https://source.android.com/docs/security/features/verifiedboot, primary, checked 2026-10-19
  - supports: DM-093 (chain from hardware root of trust; rollback protection; strict enforcement since 7.0; AVB in 8.0)
[S149] Lennart Poettering, Brave New Trusted Boot World, Pid Eins blog (0pointer.net), 2022, https://0pointer.net/blog/brave-new-trusted-boot-world.html, secondary, checked 2026-10-19
  - supports: DM-092 (unified kernel images remove unsigned initrd/config pieces)
  - supports: DM-093 (distro initrd unsigned and outside Secure Boot/PCRs; UKI + TPM measured boot proposal)
[S150] Free Software Foundation, Will your computer's "Secure Boot" turn out to be "Restricted Boot"? (statement), fsf.org, n.d., https://www.fsf.org/campaigns/secure-boot-vs-restricted-boot/statement, secondary, checked 2026-10-19
  - supports: DM-093 (owner-control position: must be able to disable restrictions or install free OS)
[S151] Microsoft, Job Objects, Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects, primary, checked 2026-10-19
  - supports: DM-100 (children join job by default; unbreakable association; TerminateJobObject; kill-on-close; nesting only since Windows 8; breakaway flags)
[S152] Linux kernel documentation, Control Group v2, docs.kernel.org, n.d., https://docs.kernel.org/admin-guide/cgroup-v2.html, primary, checked 2026-10-19
  - supports: DM-100 (cgroup.kill semantics incl. concurrent forks; single v2 hierarchy vs v1)
  - supports: (available for DM-111; not cited by DM-101 after narrowing) ("Issues with v1": multiple hierarchies, thread granularity, inner-node competition, racy delegation, non-hierarchical memory soft limit; v1 still bindable)
  - supports: DM-111 ("Issues with v1": multiple hierarchies impractical, thread granularity, inner-node competition, inconsistent controller interfaces; no-internal-process rule; delegation)
[S153] Christian Brauner, [PATCH v3 1/5] cgroup: introduce cgroup.kill (patch posting mirrored on LWN), LWN.net, 2021, https://lwn.net/Articles/855924/, primary, checked 2026-10-19
  - supports: DM-100 (motivation: systemd recursive killing, container managers lack single PID, OOM handlers; fork-race handling)
  - supports: DM-101 (no single handle to kill a container; no single PID when PID namespace/cgroup is shared)
[S154] Linux man-pages project, setsid(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/setsid.2.html, primary, checked 2026-10-19
  - supports: DM-005 (fork-then-setsid detach idiom)
  - supports: DM-100 (any process can leave its group/session)
  - supports: DM-070 (EPERM for group leaders; fork-then-setsid idiom; SIGHUP on hangup and on session-leader exit)
[S155] Linux man-pages project, user_namespaces(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/user_namespaces.7.html, primary, checked 2026-10-20
  - supports: DM-101 (unprivileged user namespaces enable sandboxes without setuid helpers; capabilities scoped to the namespace)
  - supports: DM-110 (unprivileged creation since 3.8; full caps inside; per-subsystem support; later fixes e.g. setgroups, CAP_SETFCAP in 5.12)
  - supports: DM-041 (unprivileged userns since 3.8; which filesystems mountable; block filesystems still need init-ns CAP_SYS_ADMIN; privilege restricted to protect set-user-ID programs)
[S156] Ijlal Loutfi (Canonical), Restricted unprivileged user namespaces are coming to Ubuntu 23.10, Canonical blog, 2023, https://canonical.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces, secondary, checked 2026-10-20
  - supports: (available for DM-110; not cited by DM-101 after narrowing) (userns widen attack surface, used in exploit chains; cites Google 44% figure; AppArmor gating, opt-in, incomplete coverage)
  - supports: DM-041 (unprivileged userns exposes root-only kernel interfaces; cited Google figure: 44% of exploits needed them; AppArmor gating)
[S157] Will (willsroot) et al., CVE-2022-0185: Linux kernel slab out-of-bounds write (legacy_parse_param), oss-security mailing list, 2022, https://seclists.org/oss-sec/2022/q1/55, primary, checked 2026-10-20
  - supports: (available for DM-110; not cited by DM-101 after narrowing) (heap overflow needing only namespaced CAP_SYS_ADMIN via unshare; Ubuntu LPE and COS container escape)
[S158] Poul-Henning Kamp, Robert N. M. Watson, Jails: Confining the omnipotent root, SANE 2000 (FreeBSD papers archive), 2000, https://papers.freebsd.org/2000/phk-jails/, primary, checked 2026-10-20
  - supports: DM-101 (jail partitions OS while keeping simple root model; delegated root confined to jail)
[S159] Daniel Price, Andrew Tucker, Solaris Zones: Operating System Support for Consolidating Commercial Workloads, LISA '04 (USENIX), 2004, https://www.usenix.org/legacy/publications/library/proceedings/lisa04/tech/full_papers/price/price.pdf, primary, checked 2026-10-20
  - supports: DM-101 (integrated namespace, security and resource isolation as design principles; reduced privilege set, "double-hulled"; fixed privilege set limitation)
[S160] Fuchsia project, Job (Zircon kernel object), fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/reference/kernel_objects/job, primary, checked 2026-10-20
  - supports: DM-101 (job tree; every process in one job; per-job policy, privileges and resource limits)
[S161] Rob Pike, Dave Presotto, Ken Thompson, Howard Trickey, Phil Winterbottom, The Use of Name Spaces in Plan 9, Operating Systems Review 27(2) (ACM), 1993, https://9p.io/sys/doc/names.html, primary, checked 2026-10-20
  - supports: DM-101 (per-process name spaces via bind/mount; rfork share-or-copy; resources as file servers)
  - supports: DM-041 (per-process name spaces, no global namespace, bind/mount/union dirs, per-window /dev/cons; reliance on conventions)
  - supports: GR-001 (everything-as-file extension; process creation/shared memory not files)
  - supports: GR-002 (composition via namespaces)
  - supports: GR-004 (bind/mount/union, 8½, /net, import/cpu, limitations, ~10% X slowdown)
  - supports: GR-006 (file trees as organizing idea for non-disk resources)
[S162] Michael Kerrisk, Namespaces in operation, part 1: namespaces overview, LWN.net, 2013, https://lwn.net/Articles/531114/, secondary, checked 2026-10-20
  - supports: DM-101 (namespace goals: containers, migration; introduction dates 2.4.19 to 3.8; incremental decade-long development)
  - supports: Plan 9 (Linux mount namespaces in 2.4.19; basis of containers)
[S163] Jake Edge, Container IDs for the audit subsystem, LWN.net, 2017, https://lwn.net/Articles/740621/, secondary, checked 2026-10-20
  - supports: DM-101 (kernel has no container concept; Howells' kernel-object proposal stalled; Biederman objection; audit container-ID retrofit)
[S164] FreeBSD Documentation Project, FreeBSD Handbook: Jails and Containers, docs.freebsd.org, n.d., https://docs.freebsd.org/en/books/handbook/jails/, primary, checked 2026-10-20
  - supports: DM-101 (jails extend chroot to users, processes and networking; per-jail permission settings)
[S200] Norm Hardy, The Confused Deputy (or why capabilities might have been invented), Operating Systems Review 22(4) (copy at cap-lore.com), 1988, http://www.cap-lore.com/CapTheory/ConfusedDeputy.html, primary, checked 2026-10-19
  - supports: DM-009 (compiler with "home files license" overwrote billing file; authority from two sources cannot be kept apart)
  - supports: DM-010 (access decided by program's license plus invoker identity, not by the request; rules grew to 14 boolean operators)
[S201] Mark S. Miller, Ka-Ping Yee, Jonathan Shapiro, Capability Myths Demolished, technical report (copy hosted by Agoric), 2003, https://papers.agoric.com/assets/pdf/papers/capability-myths-demolished.pdf, primary, checked 2026-10-19
  - supports: DM-009 (defines "No Ambient Authority" property; ACL systems lack it; capabilities avoid confused deputy)
[S202] Robert N. M. Watson, Jonathan Anderson, Ben Laurie, Kris Kennaway, Capsicum: practical capabilities for UNIX, USENIX Security Symposium, 2010, https://www.usenix.org/legacy/event/sec10/tech/full_papers/Watson.pdf, primary, checked 2026-10-19
  - supports: DM-009 (capability mode removes global namespaces; fork/exec rely on global namespaces and had to be reworked)
  - supports: DM-010 (DAC designed to protect users from each other, not one app processing data for one user)
  - supports: DM-014 (tcpdump ~10 lines, Chromium ~100 lines; other Chromium sandboxes up to 22,000 lines, porous; Linux suid sandbox needs privilege)
  - supports: DM-015 (MAC designed for system policy, not application compartmentalisation)
  - supports: GR-008 (hybrid capability model; tcpdump ~2 lines, Chromium ~100 lines; DNS lookup failure in capability mode)
[S203] FreeBSD Project, capsicum(4) manual page, FreeBSD, 2026 (current), https://man.freebsd.org/cgi/man.cgi?query=capsicum&sektion=4, primary, checked 2026-10-19
  - supports: DM-009 (capability mode restricts global namespaces; first in FreeBSD 9.0)
  - supports: DM-014 (shipping since FreeBSD 9.0; sysctl limited to ~60 of 15,000 parameters)
[S204] Fuchsia project, Sandboxing, fuchsia.dev, 2026 (last updated 2026-10-12), https://fuchsia.dev/fuchsia-src/concepts/process/sandboxing, primary, checked 2026-10-19
  - supports: DM-009 (new process has nothing; authority comes only from initial handles and namespace)
[S205] Hao Chen, David Wagner, Drew Dean, Setuid Demystified, USENIX Security Symposium, 2002, https://people.eecs.berkeley.edu/~daw/papers/setuid-usenix02.pdf, primary, checked 2026-10-19
  - supports: DM-011 (uid-setting calls poorly designed, divergent across Linux/Solaris/FreeBSD; sendmail vulnerabilities; setuid bit 1973)
  - supports: DM-012 (uid 0 reserved for root who can access all resources)
[S206] Qualys Research Team, PwnKit: Local Privilege Escalation Vulnerability in polkit's pkexec (CVE-2021-4034), Qualys security advisory, 2022, https://www.qualys.com/2022/01/25/cve-2021-4034/pwnkit.txt, primary, checked 2026-10-19
  - supports: DM-011 (argc==0 lets attacker reintroduce GCONV_PATH into a setuid process; vulnerable since May 2009; OpenBSD kernel refuses argc 0)
  - supports: DM-085 (environment is attacker-controlled input to privileged programs)
[S207] Red Hat (CNA), CVE-2021-4034 record, CVE Program (cveawg API), 2022, https://cveawg.mitre.org/api/cve/CVE-2021-4034, primary, checked 2026-10-19
  - supports: DM-011 (pkexec setuid tool executes environment variables as commands; LPE)
[S208] MITRE CVE Program, CVE-2021-3156 record (sudo "Baron Samedit"), CVE Program (cveawg API), 2021, https://cveawg.mitre.org/api/cve/CVE-2021-3156, primary, checked 2026-10-19
  - supports: DM-011 (heap overflow in setuid sudo before 1.9.5p2 gives root)
[S210] Nick Kralevich (Google), Security Enhancements in Android 4.3 (source.android.com docs commit cb4c927), android.googlesource.com, 2013, https://android.googlesource.com/platform/docs/source.android.com/+/cb4c927%5E%21, primary, checked 2026-10-19
  - supports: DM-011 (Android removed all setuid/setgid programs; /system nosuid for zygote children; file capabilities instead)
[S211] systemd project, run0(1) manual page, freedesktop.org, 2026 (current; added in v256), https://www.freedesktop.org/software/systemd/man/latest/run0.html, primary, checked 2026-10-19
  - supports: DM-011 (privilege elevation without setuid via service manager; no credentials/context inherited from caller)
  - supports: DM-085 (caller environment not inherited except TERM)
[S212] OpenBSD, pledge(2) manual page, man.openbsd.org, 2026 (current; first in OpenBSD 5.9), https://man.openbsd.org/pledge.2, primary, checked 2026-10-19
  - supports: DM-014 (promise-based subsystem restriction; violation = uncatchable SIGABRT; only reducible)
  - supports: DM-011 (pledged processes ignore setuid bits; execpromises refuse setuid targets)
[S213] OpenBSD, unveil(2) manual page, man.openbsd.org, 2026 (current; first in OpenBSD 6.4), https://man.openbsd.org/unveil.2, primary, checked 2026-10-19
  - supports: DM-014 (path-based filesystem view restriction; cleared by execve; warns against applying to arbitrary programs)
[S214] Russ Cox, Eric Grosse, Rob Pike, Dave Presotto, Sean Quinlan, Security in Plan 9, USENIX Security Symposium (revised copy at 9p.io), 2002, https://9p.io/sys/doc/auth.html, primary, checked 2026-10-19
  - supports: DM-012 (host owner is the closest analogue to root but far weaker; factotum is the only process creating capabilities)
  - supports: DM-011 (su/login would not need setuid with a capability device)
[S215] Apple, About System Integrity Protection on your Mac, support.apple.com, 2026 (current), https://support.apple.com/en-us/102149, primary, checked 2026-10-19
  - supports: DM-012 (SIP restricts root from modifying protected system locations since OS X El Capitan)
[S216] Linux man-pages project, capabilities(7), man7.org, 2026 (current), https://man7.org/linux/man-pages/man7/capabilities.7.html, primary, checked 2026-10-19
  - supports: DM-012 (capabilities divide superuser power)
  - supports: DM-013 (CAP_SYS_ADMIN "the new root"; developer guidance to avoid it; file caps since 2.6.24; ambient set since 4.3)
[S217] Michael Kerrisk, CAP_SYS_ADMIN: the new root, LWN.net, 2012, https://lwn.net/Articles/486306/, secondary, checked 2026-10-19
  - supports: DM-013 (451 of 1,167 capability checks in 3.2 are CAP_SYS_ADMIN; 16% in 2.2; escalation from CAP_SYS_ADMIN to root)
[S218] Linux kernel documentation, Seccomp BPF (SECure COMPuting with filters), docs.kernel.org, 2026 (current), https://docs.kernel.org/userspace-api/seccomp_filter.html, primary, checked 2026-10-19
  - supports: DM-014 ("System call filtering isn't a sandbox"; no pointer dereference; arch/ABI checks; no_new_privs required)
[S219] Jonathan Corbet, Capsicum for Linux, LWN.net, 2014, https://lwn.net/Articles/604015/, secondary, checked 2026-10-20
  - supports: DM-014 (Linux port posted 2014, invasive, "hard sell"; question whether seccomp-BPF could subsume it)
  - supports: DM-013 (Capsicum "capabilities" entirely different from Linux capabilities — naming collision)
  - supports: DM-015 (LSM stacking not expected soon in 2014)
[S220] Tamás Koczka (Google), Learnings from kCTF VRP's 42 Linux kernel exploits submissions, Google Security Blog, 2023, https://security.googleblog.com/2023/06/learnings-from-kctf-vrps-42-linux.html, primary, checked 2026-10-19
  - supports: DM-014 (Android uses seccomp-bpf to make io_uring unreachable to apps — filters as attack-surface reduction)
  - supports: DM-110 (large reachable kernel surface exploited; 60% of submissions used io_uring)
  - supports: DM-025 (existence, title and date of Google's post; body content verified via S413)
  - supports: GR-009 (60% of submissions exploited io_uring; disabled on ChromeOS; unreachable for Android apps)
[S221] Linux kernel documentation, Linux Security Modules: General Security Hooks for Linux, docs.kernel.org, 2026 (current), https://docs.kernel.org/security/lsm.html, primary, checked 2026-10-19
  - supports: DM-015 (2001 Kernel Summit origin; Torvalds asked for generic hook framework; merged Dec 2003; void* blobs; "close approximation" of stacking)
[S222] Stephen Smalley, Robert Craig, Security Enhanced (SE) Android: Bringing Flexible MAC to Android, NDSS, 2013, https://www.ndss-symposium.org/wp-content/uploads/2017/09/02_4.pdf, primary, checked 2026-10-19
  - supports: DM-010 (Android reuses DAC uids to represent apps, not users; DAC weaknesses remain)
  - supports: DM-015 (SELinux successfully deployed on Android; mitigated published exploits)
[S223] Wenjuan Xu, Mohamed Shehab, Gail-Joon Ahn, Visualization-based policy analysis for SELinux: framework and user study, International Journal of Information Security 12:155–171, 2013, https://sefcom.asu.edu/publications/visualization-based-policy-security2013.pdf, primary, checked 2026-10-19
  - supports: DM-015 (SELinux policy over 30,000 statements; policy misconfiguration makes access control ineffective)
[S224] Jake Edge, Another LSM stacking approach, LWN.net, 2012, https://lwn.net/Articles/518345/, secondary, checked 2026-10-20
  - supports: DM-015 (only one LSM active; "perennial" stacking problem; slot occupied by SELinux or AppArmor)
[S225] Microsoft, Access Tokens, Microsoft Learn (Win32 docs), 2025, https://learn.microsoft.com/en-us/windows/win32/secauthz/access-tokens, primary, checked 2026-10-19
  - supports: DM-016 (token contents: user/group/logon SIDs, privileges, restricting SIDs, impersonation)
[S226] Microsoft, Mandatory Integrity Control, Microsoft Learn (Win32 docs), 2025, https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control, primary, checked 2026-10-19
  - supports: DM-016 (integrity levels layered before DACL check; no-write-up default; AppContainer interaction)
[S227] Sudhakar Govindavajhala, Andrew W. Appel, Windows Access Control Demystified, Princeton University technical report (TR-744-06), 2006, https://www.cs.princeton.edu/~appel/papers/winval.pdf, primary, checked 2026-10-19
  - supports: DM-016 (~30 rights on ~15 object kinds; vendors misconfigure ACLs; several user-to-admin escalations; CERT VU#953860)
[S228] Microsoft, Registry Virtualization, Microsoft Learn (Win32 docs), 2025, https://learn.microsoft.com/en-us/windows/win32/sysinfo/registry-virtualization, primary, checked 2026-10-19
  - supports: DM-016 (pre-Vista apps assumed admin; virtualization as interim compat)
  - supports: DM-086 (writes to HKLM\Software redirected to per-user VirtualStore; breaks registry-as-IPC between service and app)
[S230] Ijlal Loutfi (Canonical), Restricted unprivileged user namespaces are coming to Ubuntu 23.10, ubuntu.com blog, 2023, https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces, primary, checked 2026-10-19
  - supports: DM-110 (unprivileged userns widely used in LPE chains; 44% figure attributed to Google; sysctl is all-or-nothing; AppArmor per-app gate)
[S231] Synacktiv, Old bug, shallow bug: Exploiting Ubuntu at Pwn2Own Vancouver 2023, synacktiv.com, 2023, https://synacktiv.com/publications/old-bug-shallow-bug-exploiting-ubuntu-at-pwn2own-vancouver-2023, secondary, checked 2026-10-19
  - supports: DM-110 (CVE-2023-35001 nf_tables reachable by unprivileged users via user namespaces)
[S233] Lennart Poettering, Rethinking PID 1, 0pointer.de blog, 2010, http://0pointer.de/blog/projects/systemd.html, primary, checked 2026-10-19
  - supports: DM-088 (SysV init serial, shell-heavy — grep called 77 times; socket activation; parallel start)
  - supports: DM-111 (cgroups used for process tracking because double-forked daemons escape supervision)
  - supports: DM-085 (shell behaviour depends heavily on environment variables)
[S234] Linux man-pages project, setenv(3), man7.org, 2026 (current), https://man7.org/linux/man-pages/man3/setenv.3.html, primary, checked 2026-10-19
  - supports: DM-085 (setenv/unsetenv MT-Unsafe const:env; POSIX does not require reentrancy)
  - supports: DM-066 (setenv/unsetenv MT-Unsafe const:env)
[S235] Rust project, std::env::set_var documentation, doc.rust-lang.org, 2026 (current), https://doc.rust-lang.org/std/env/fn.set_var.html, primary, checked 2026-10-19
  - supports: DM-085 (unsound in multithreaded programs on non-Windows; libc getenv unsynchronized; libraries read env undocumented)
[S236] Rust project, Rust 2024 Edition Guide: Unsafe functions, doc.rust-lang.org, 2025, https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html, primary, checked 2026-10-19
  - supports: DM-085 (set_var/remove_var made unsafe in 2024 edition after being found incorrect)
  - supports: DM-060 (set_var/remove_var made unsafe because environment mutation is unsound in multithreaded programs on some platforms)
  - supports: DM-066 (set_var/remove_var made unsafe; unsound in multithreaded programs)
[S237] Florian Weimer (Red Hat), stdlib: Support malloc-managed environ arrays for compatibility (glibc commit, as packaged in AlmaLinux glibc-RHEL-75809.patch), git.almalinux.org, 2025, https://git.almalinux.org/rpms/glibc/raw/branch/a10/glibc-RHEL-75809.patch, primary, checked 2026-10-19
  - supports: DM-085 (glibc made getenv thread-safe "in more cases" and broke applications that manage environ themselves)
[S238] Microsoft, Registry (overview), Microsoft Learn (Win32 docs), 2025, https://learn.microsoft.com/en-us/windows/win32/sysinfo/registry, primary, checked 2026-10-19
  - supports: DM-086 (system-defined config database; do not edit data not belonging to your app)
[S239] Microsoft, Structure of the Registry, Microsoft Learn (Win32 docs), 2025, https://learn.microsoft.com/en-us/windows/win32/sysinfo/structure-of-the-registry, primary, checked 2026-10-19
  - supports: DM-086 (hierarchical keys/values, case-insensitive names, 512 levels)
[S240] Raymond Chen, Why are INI files deprecated in favor of the registry?, The Old New Thing (Microsoft devblogs), 2007, https://devblogs.microsoft.com/oldnewthing/20071126-00/?p=24383, secondary, checked 2026-10-19
  - supports: DM-086 (steelman: INI problems — concurrency, per-file security, 32KB limit, parsing, location)
  - supports: DM-087 (writable text config files reopen INI problems; read-only XML sidesteps many)
[S241] Mateusz Jurczyk, The Windows Registry Adventure #1: Introduction and research results, Google Project Zero, 2024, https://projectzero.google/2024/04/the-windows-registry-adventure-1.html, primary, checked 2026-10-19
  - supports: DM-086 (50 CVEs 2022–2024; hives parsed in kernel; RegLoadAppKey loads arbitrary hives unprivileged; 30-year-old C code)
[S242] Zuoning Yin, Xiao Ma, Jing Zheng, Yuanyuan Zhou, Lakshmi N. Bairavasundaram, Shankar Pasupathy, An Empirical Study on Configuration Errors in Commercial and Open Source Systems, SOSP, 2011, https://www.sigops.org/s/conferences/sosp/2011/current/2011-Cascais/12-yin-online.pdf, primary, checked 2026-10-19
  - supports: DM-087 (546 misconfigurations; 70–85.5% parameter mistakes; 38–54% illegal values; 12–30% cross-parameter inconsistencies)
[S243] Augeas project, Augeas (home page), augeas.net, 2026 (current), https://augeas.net/, primary, checked 2026-10-19
  - supports: DM-087 (tool exists to parse many native config formats into a tree because sed/awk editing is unsafe)
[S244] Eelco Dolstra, Andres Löh, NixOS: A Purely Functional Linux Distribution, ICFP, 2008, https://kosmikus.org/NixOS.pdf, primary, checked 2026-10-19
  - supports: DM-087 (imperative config management: no rollback, not reproducible; functional whole-system config)
[S245] Don Armstrong for the Debian Technical Committee, Bug#727708: Decision on default init system for jessie, debian-devel-announce, 2014, https://lists.debian.org/debian-devel-announce/2014/02/msg00005.html, primary, checked 2026-10-19
  - supports: DM-088 (tech-ctte resolved systemd as default for jessie, overridable by GR)
[S246] Jake Edge, Debian decides on systemd—for now, LWN.net, 2014, https://lwn.net/Articles/585319/, secondary, checked 2026-10-19
  - supports: DM-088 (chair's casting vote; Ian Jackson's loose vs tight coupling concern)
[S247] Debian Project, General Resolution: init system coupling, debian.org, 2014, https://www.debian.org/vote/2014/vote_003, primary, checked 2026-10-19
  - supports: DM-088 ("General Resolution is not required" won)
[S248] Debian Project, General Resolution: Init systems and systemd, debian.org, 2019, https://www.debian.org/vote/2019/vote_002, primary, checked 2026-10-19
  - supports: DM-088 ("Systemd but we support exploring alternatives" won; eight options)
[S249] Laurent Bercot (skarnet.org; author identified as s6 developer, name not on page), Why not just use systemd?, skarnet.org, undated, https://skarnet.org/software/systemd.html, weak, checked 2026-10-19
  - supports: DM-088 (practitioner critique: scope creep, monolithic, PID 1 risk, interface coupling, non-portable)
[S250] illumos project, smf(7) manual page, illumos.org, 2026 (current), https://illumos.org/man/7/smf, primary, checked 2026-10-20
  - supports: DM-088 (Solaris SMF: declarative service model with instances, dependencies, FMRIs)
[S251] skarnet.org, s6: an overview, skarnet.org, 2026 (current), https://skarnet.org/software/s6/overview.html, primary, checked 2026-10-20
  - supports: DM-088 (process supervision suite in daemontools/runit lineage as the minimal alternative)
[S252] C. Lonvick, RFC 3164: The BSD syslog Protocol, IETF (Informational), 2001, https://datatracker.ietf.org/doc/html/rfc3164, primary, checked 2026-10-19
  - supports: DM-089 (documents observed practice; no year/timezone in timestamp; UDP 1024 bytes; no auth/integrity)
[S253] R. Gerhards, RFC 5424: The Syslog Protocol, IETF (Proposed Standard), 2009, https://datatracker.ietf.org/doc/html/rfc5424, primary, checked 2026-10-19
  - supports: DM-089 (STRUCTURED-DATA, MSGID, RFC 3339 timestamps; implementations shared little beyond PRI)
[S254] Lennart Poettering, Kay Sievers (attributed via LWN S256; doc itself unsigned), systemd Journal design document, Google Docs (published), 2011, https://docs.google.com/document/pub?id=1IC9yOXj7j6cdLLxWEBAGRL6wl97tFxgjLUEHIX3MSTs, primary, checked 2026-10-19
  - supports: DM-089 (syslog unauthenticated, unstructured, no indexing, tamperable; trusted underscore fields from kernel credentials)
[S255] systemd project, Journal File Format, systemd.io, 2026 (current), https://systemd.io/JOURNAL_FILE_FORMAT/, primary, checked 2026-10-19
  - supports: DM-089 (indexed, append-mostly, FSS; corruption → rotate; format not a stable interface, use the C API)
[S256] Jonathan Corbet (post) and LWN commenters, The Journal - a proposed syslog replacement, LWN.net, 2011, https://lwn.net/Articles/468049/, secondary (comments: weak), checked 2026-10-20
  - supports: DM-089 (hash-chain motivation after kernel.org break-in; early FAQ refused to standardize format; commenter objections to binary format)
[S257] Microsoft, About Event Tracing, Microsoft Learn (Win32 docs), 2026 (current), https://learn.microsoft.com/en-us/windows/win32/etw/about-event-tracing, primary, checked 2026-10-20
  - supports: DM-089 (ETW: kernel-level structured tracing with controllers/providers/consumers, dynamically enabled in production)
[S300] Michael Kerrisk (maintainer), signal-safety(7) — Linux manual page, man7.org / Linux man-pages project, current, https://man7.org/linux/man-pages/man7/signal-safety.7.html, primary, checked 2026-10-19
  - supports: DM-017 (definition of async-signal-safe; stdio reentrancy example; errno save/restore; glibc deviations)
[S301] Qualys Threat Research Unit, regreSSHion: RCE in OpenSSH's server, on glibc-based Linux systems (CVE-2024-6387), Qualys security advisory, 2024, https://www.qualys.com/2024/07/01/cve-2024-6387/regresshion.txt, primary, checked 2026-10-19
  - supports: DM-017 (SIGALRM handler calling syslog() -> heap corruption; regression of CVE-2006-5051; OpenBSD syslog_r not affected)
[S302] Linux man-pages project, signalfd(2) — Linux manual page, man7.org, current, https://man7.org/linux/man-pages/man2/signalfd.2.html, primary, checked 2026-10-19
  - supports: DM-017 (fd-based signal delivery; must block signals; blocked mask inherited across exec; synchronous signals still need handlers)
  - supports: DM-130 (signals must be blocked with sigprocmask for signalfd; mask and pending signals persist across execve)
[S303] Neil Brown, Ghosts of Unix past, part 3: Unfixable designs, LWN.net, 2010, https://lwn.net/Articles/414618/, secondary, checked 2026-10-19
  - supports: DM-017 (history of signal fixes: one-shot handlers, EINTR, sigaction, pselect, RT signal queue limits; signalfd as replacement; comment on exec inheritance problem)
[S304] Jochen Liedtke, On µ-Kernel Construction, 15th ACM SOSP, 1995, https://cgi.cse.unsw.edu.au/~cs9242/22/papers/Liedtke_95.pdf, primary, checked 2026-10-19
  - supports: DM-018 (inefficiency from overloading/implementation not concept; Mach 18 µs kernel call on 486 = ~800 cycles kernel overhead vs L3 15 cycles; Chen/Bershad MCPI reanalysis attributing cache misses to Mach's working set; processor dependence)
  - supports: Mach (inefficiency from overloading/poor implementation, not inherent)
  - supports: L4 (minimal concept set argument)
[S305] Gernot Heiser, Kevin Elphinstone, L4 Microkernels: The Lessons from 20 Years of Research and Deployment, ACM Transactions on Computer Systems 34(1), 2016, https://trustworthy.systems/publications/nicta_full_text/8988.pdf, primary, checked 2026-10-19
  - supports: DM-018 (early-90s one-way IPC ~100 µs; L4 10–20x faster; long IPC, IPC timeouts, clans & chiefs abandoned; thread IDs replaced by endpoints; Mach ports buffer, seL4 endpoints do not; synchronous IPC forces multithreading and is wrong across cores; notifications added)
  - supports: Mach (1990s IPC ~100 µs; trend of moving services back into kernel; Chen/Bershad read as inherent)
  - supports: L4 (10-20x faster IPC; Qualcomm/Apple/OKL4/PikeOS deployment; abandoned timeouts, long IPC, clans & chiefs, recursive mapping, C++; Linux VM as middleware; capabilities from KeyKOS/EROS)
[S306] Hermann Härtig, Michael Hohmuth, Jochen Liedtke, Sebastian Schönberg, Jean Wolter, The Performance of µ-Kernel-Based Systems, 16th ACM SOSP, 1997, https://os.inf.tu-dresden.de/pubs/sosp97, primary, checked 2026-10-19
  - supports: DM-018 (L4Linux within 5% of native Linux on AIM; MkLinux penalty ~5x (in-kernel) and ~7x (user-level) larger)
  - supports: Mach (MkLinux in-kernel ~73% / user ~62% of native vs L4Linux ~95%)
  - supports: L4 (paravirtualised Linux on L4 with ~5% throughput loss)
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
  - supports: DM-031 (peer can shrink shared file causing SIGBUS; F_SEAL_SHRINK; Linux 3.17)
[S316] M. D. McIlroy, E. N. Pinson, B. A. Tague, UNIX Time-Sharing System: Foreword, Bell System Technical Journal 57(6), 1978, https://archive.org/stream/bstj57-6-1899/bstj57-6-1899_djvu.txt, primary, checked 2026-10-19
  - supports: DM-020 (style maxims: output becomes input to unknown program; avoid columnar/binary input formats)
[S317] Jeffrey P. Snover, Monad Manifesto (v1.2), Microsoft (author's site), 2002, https://mail.jsnover.com/Docs/MonadManifesto.pdf, primary, checked 2026-10-19
  - supports: DM-020 (text pipelines force lossy re-parsing, "prayer-based parsing"; pipelines of .NET objects; legacy text as one object stream type)
[S318] David A. Wheeler, Fixing Unix/Linux/POSIX Filenames: Control Characters (such as Newline), Leading Dashes, and Other Problems, dwheeler.com (essay), 2009 (updated 2024), https://dwheeler.com/essays/fixing-unix-linux-filenames.html, weak, checked 2026-10-19
  - supports: DM-020 (newline/space/dash filenames breaking line-oriented pipelines; CVE pointers e.g. CVE-2013-7085)
  - supports: DM-095 (catalogue of failure patterns: newline, leading dash, non-UTF-8, find|while read; proposed fixes) — used as pointer/anecdote only
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
  - supports: DM-052 (errno thread-local, never reset to zero, meaningful only after failure; pthreads return codes instead; values differ across archs; aliased names)
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
  - supports: GR-011 (naive futex mutexes have correctness and performance bugs)
[S368] NIST NVD, CVE-2014-3153 (via NVD CVE API 2.0), NVD, 2014, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2014-3153, primary, checked 2026-10-19
  - supports: DM-120 (futex_requeue did not ensure distinct futex addresses -> local privilege escalation; in CISA KEV 2022-05-25)
  - supports: GR-011 (requeue/PI futex privilege escalation)
[S369] Linux kernel documentation, futex2, docs.kernel.org, current, https://docs.kernel.org/userspace-api/futex2.html, primary, checked 2026-10-20
  - supports: DM-120 (new interface "designed to overcome limitations of the original"; futex_waitv waits on up to 128 futexes; only 32-bit size supported)
[S400] Gaurav Banga, Jeffrey C. Mogul, Peter Druschel, A scalable and explicit event delivery mechanism for UNIX, USENIX Annual Technical Conference, 1999, https://www.usenix.org/legacy/events/usenix99/full_papers/banga/banga.pdf, primary, checked 2026-10-19
  - supports: DM-023 (select/poll restate interest set every call; cost scales with descriptors; declare_interest/get_next_event; select ~25% CPU in proxy)
[S401] Jonathan Lemon, Kqueue: A generic and scalable event notification facility, Proceedings of the FREENIX Track, USENIX Annual Technical Conference, 2001, https://www.read.seas.harvard.edu/~kohler/class/aosref/lemon01kqueue.pdf, primary, checked 2026-10-19
  - supports: DM-023 (select/poll do not scale and cannot watch signals/fs/AIO; knotes deleted on close; RT-signal queues give stale events and overflow; /dev/poll limitations)
  - supports: DM-130 (single generic queue for heterogeneous event sources; signal-based notification unsuitable for libraries)
[S402] FreeBSD Project, kqueue(2) manual page, FreeBSD, current, https://man.freebsd.org/cgi/man.cgi?query=kqueue&sektion=2, primary, checked 2026-10-19
  - supports: DM-023 (close() removes kevents referencing the descriptor; changelist applied in same call as wait)
  - supports: DM-029 (EVFILT_READ on vnodes reports offset to EOF, unlike poll which always fires for regular files)
  - supports: DM-130 (EVFILT_PROC/PROCDESC/SIGNAL/TIMER/VNODE/USER filters)
[S403] Linux man-pages project, epoll(7), man7.org, current, https://man7.org/linux/man-pages/man7/epoll.7.html, primary, checked 2026-10-19
  - supports: DM-023 (interest key is fd plus open file description; fd removed only after all duplicates closed; ET starvation and must-drain-to-EAGAIN rules)
[S404] Marek Majkowski, Epoll is fundamentally broken 1/2, idea.popcount.org (personal blog), 2017, https://idea.popcount.org/2017-02-20-epoll-is-fundamentally-broken-12/, weak, checked 2026-10-19
  - supports: DM-023 (multithreaded load balancing; thundering herd; EPOLLEXCLUSIVE/EPOLLONESHOT as later fixes)
[S405] Marek Majkowski, Epoll is fundamentally broken 2/2, idea.popcount.org (personal blog), 2017, https://idea.popcount.org/2017-03-20-epoll-is-fundamentally-broken-22/, weak, checked 2026-10-19
  - supports: DM-023 (events on already-closed fds; EPOLL_CTL_DEL impossible after close of original; illumos declined to copy)
[S406] Jonathan Corbet, Fixing asynchronous I/O, again, LWN.net, 2016, https://lwn.net/Articles/671649/, secondary, checked 2026-10-19
  - supports: DM-024 (native AIO: O_DIRECT only, can still block, few ops, not exposed by glibc; glibc POSIX AIO uses threads; syslets/fibrils/acall never merged)
[S407] Linus Torvalds, Re: [PATCH 09/13] aio: add support for async openat() (LKML message, mirrored by LWN), LWN.net, 2016, https://lwn.net/Articles/671657/, primary, checked 2026-10-19
  - supports: DM-024 (Torvalds calls AIO a "horrible ad-hoc design"; wants generic async syscalls instead)
[S408] Linux man-pages project, aio(7), man7.org, current, https://man7.org/linux/man-pages/man7/aio.7.html, primary, checked 2026-10-19
  - supports: DM-024 (glibc POSIX AIO in user space; threads expensive and scale poorly; kernel AIO not mature enough to back POSIX AIO)
[S409] Linux man-pages project, io_submit(2), man7.org, current, https://man7.org/linux/man-pages/man2/io_submit.2.html, primary, checked 2026-10-19
  - supports: DM-024 (no glibc wrapper; libaio wrapper with different prototype; RWF_NOWAIT retrofit)
[S410] Jens Axboe, Efficient IO with io_uring (v0.4), kernel.dk (author's site; fetched via Internet Archive copy), 2019, https://web.archive.org/web/2023id_/https://kernel.dk/io_uring.pdf, primary, checked 2026-10-19
  - supports: DM-024 (AIO limitations: O_DIRECT only, blocking submission on metadata/request slots, 104 bytes copied per IO, two syscalls; attempts to extend AIO abandoned)
  - supports: DM-025 (io_uring design: shared SQ/CQ rings, registered files and buffers, polled IO)
  - supports: DM-026 (completion model, shared rings)
[S411] Jens Axboe, Faster IO through io_uring (slides), Kernel Recipes 2019, 2019, https://archives.kernel-recipes.org/wp-content/uploads/2025/01/kernel-recipes-2019-axboe.pdf, primary, checked 2026-10-19
  - supports: DM-024 (buffered AIO unsupported; quoted Torvalds objections about async code being hard and racing with fd close)
  - supports: DM-025 (io_uring merged 5.1, posted Jan 2019 merged Mar 2019)
[S413] Tamás Koczka, Learnings from kCTF VRP's 42 Linux kernel exploits submissions (repost to oss-security), Openwall oss-security list, 2023, https://openwall.com/lists/oss-security/2023/06/17/2, primary, checked 2026-10-19
  - supports: DM-025 (60% of submissions exploited io_uring; disabled on ChromeOS and Google production servers; blocked for Android apps via seccomp; GKE Autopilot considering disabling)
[S414] Matteo Rizzo, [PATCH 0/1] Add a sysctl to disable io_uring system-wide, LKML, 2023, https://lkml.rescloud.iu.edu/2306.3/02831.html, primary, checked 2026-10-19
  - supports: DM-025 (many unprivileged-exploitable io_uring bugs; seccomp only per-process; only option was compiling it out)
[S415] Linux kernel documentation, Documentation for /proc/sys/kernel/ (io_uring_disabled, io_uring_group), docs.kernel.org, current, https://docs.kernel.org/admin-guide/sysctl/kernel.html, primary, checked 2026-10-19
  - supports: DM-025 (io_uring_disabled 0/1/2 and io_uring_group semantics)
[S416] Jonathan Corbet, Task-level io_uring restrictions, LWN.net, 2026, https://lwn.net/Articles/1054225/, secondary, checked 2026-10-19
  - supports: DM-025 (seccomp has no visibility into io_uring operations; per-ring restrictions; cBPF filter work in flux)
[S417] Microsoft, I/O Completion Ports, Microsoft Learn (Win32 docs), 2025, https://learn.microsoft.com/en-us/windows/win32/fileio/i-o-completion-ports, primary, checked 2026-10-19
  - supports: DM-026 (completion packets per handle; concurrency value; LIFO thread release; works for files, sockets, pipes)
  - supports: DM-024 (Windows offers one completion mechanism across files and sockets)
  - supports: DM-029 (overlapped I/O on files completes via the same port as sockets)
  - supports: GR-009 (IOCP model, concurrency value, LIFO wake, works for files/sockets/pipes)
[S418] libuv project, Design overview, docs.libuv.org, current, https://docs.libuv.org/en/v1.x/design.html, primary, checked 2026-10-19
  - supports: DM-026 (one library over epoll/kqueue/event ports/IOCP)
  - supports: DM-029 (file I/O done on a thread pool because no usable platform async file primitives)
[S419] without.boats (Saoirse Shirley), Notes on io-uring, without.boats (personal blog of Rust async designer), 2020, https://without.boats/blog/io-uring/, weak, checked 2026-10-19
  - supports: DM-026 (completion I/O needs kernel-owned buffers; cancellation on drop unsound with borrowed buffers; AsyncRead mismatch)
[S420] liburing / Linux man-pages, io_uring_register_buf_ring(3), man7.org, current, https://man7.org/linux/man-pages/man3/io_uring_register_buf_ring.3.html, primary, checked 2026-10-19
  - supports: DM-026 (provided buffers chosen when data is ready; more operations in flight than buffers)
[S421] Bert Belder, wepoll README, GitHub, current, https://github.com/piscisaureus/wepoll, weak, checked 2026-10-19
  - supports: DM-026 (cross-platform software designed for readiness finds IOCP impractical; epoll emulation for Windows)
[S422] Richard P. Gabriel, The Rise of "Worse is Better" (from Lisp: Good News, Bad News, How to Win Big), dreamsongs.com (author's site), 1989/1991, https://www.dreamsongs.com/RiseOfWorseIsBetter.html, primary, checked 2026-10-19
  - supports: DM-027 (PC loser-ing problem; Unix returns error and makes caller retry; implementation simplicity over interface simplicity)
[S424] Go project, Go 1.14 Release Notes, go.dev, 2020, https://go.dev/doc/go1.14, primary, checked 2026-10-19
  - supports: DM-027 (async preemption sends more signals; programs see more EINTR and must loop)
[S425] Go project, os: write should handle EINTR (issue #38033), GitHub golang/go, 2020, https://github.com/golang/go/issues/38033, primary, checked 2026-10-19
  - supports: DM-027 (writes to cgroupfs failing with EINTR under Go 1.14 despite SA_RESTART)
[S426] Go project, os: write should handle EINTR [1.14 backport] (issue #39026), GitHub golang/go, 2020, https://github.com/golang/go/issues/39026, primary, checked 2026-10-19
  - supports: DM-027 (Ian Lance Taylor: 1.14 made existing EINTR problem occur more often; fix too risky to backport)
[S427] Jonathan Corbet, TASK_KILLABLE, LWN.net, 2008, https://lwn.net/Articles/288056/, secondary, checked 2026-10-19
  - supports: DM-028 (D-state unkillable hangs; Alan Cox: apps assume file writes not interruptible; Wilcox's killable sleep in 2.6.25; little adoption, NFS converted)
  - supports: DM-027 (interruptible sleeps require kernel cleanup paths and apps mishandle interrupted calls)
[S428] Linux man-pages / nfs-utils, nfs(5), man7.org, current, https://man7.org/linux/man-pages/man5/nfs.5.html, primary, checked 2026-10-19
  - supports: DM-028 (hard mounts retry indefinitely; intr ignored after 2.6.25; soft timeouts can cause silent data corruption)
[S429] Simon Kirby, Hung task detector versus NFS (TASK_KILLABLE), LKML, 2016, https://lkml.iu.edu/hypermail/linux/kernel/1603.0/06299.html, primary, checked 2026-10-19
  - supports: DM-028 (VFS paths take mutexes in uninterruptible sleep behind a killable NFS waiter; collateral D-state victims)
[S430] Microsoft, Canceling IRPs, Microsoft Learn (Windows driver docs), 2017, https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/canceling-irps, primary, checked 2026-10-19
  - supports: DM-028 (NT drivers with indefinitely queued IRPs must support cancel; IRPs cancelled on thread exit; 5-minute timeout disassociates IRP)
  - supports: DM-027 (NT models I/O cancellation as an explicit request operation rather than a signal side effect)
[S431] The Open Group, pselect, select (POSIX.1-2024, XSH), The Open Group Base Specifications Issue 8, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/pselect.html, primary, checked 2026-10-19
  - supports: DM-029 (regular files shall always select true for read, write and error)
[S432] Jonathan Corbet, Non-blocking buffered file read operations, LWN.net, 2014, https://lwn.net/Articles/612483/, secondary, checked 2026-10-19
  - supports: DM-029 (buffered reads block on page cache; thread pools as workaround; O_NONBLOCK on files cannot be repurposed because Samba/squid rely on it being ignored; fincore race)
[S433] Linux man-pages project, preadv2(2) (readv(2)), man7.org, current, https://man7.org/linux/man-pages/man2/preadv2.2.html, primary, checked 2026-10-19
  - supports: DM-029 (RWF_NOWAIT since 4.14, reads only, EAGAIN; 5.9/5.10 bug returning 0)
[S434] T. Pauly, B. Trammell, A. Brunstrom, G. Fairhurst, C. S. Perkins (eds./authors), RFC 9621: Architecture and Requirements for Transport Services, IETF / RFC Editor, 2025, https://www.rfc-editor.org/rfc/rfc9621.html, primary, checked 2026-10-19
  - supports: DM-074 (sockets: protocol-specific calls, single address/interface, stream-only TCP, try-and-fail async, racing reimplemented per app)
[S435] Dave Presotto, Phil Winterbottom, The Organization of Networks in Plan 9, USENIX Winter Conference 1993 (Plan 9 docs), 1993, https://plan9.io/sys/doc/net/net.html, primary, checked 2026-10-19
  - supports: DM-074 (/net clone/ctl/data files; dial() with textual addresses; only name resolution differs between networks, done by user-level cs)
  - supports: DM-075 (name resolution in a user-level connection server rather than linked into every program)
[S436] R. Gilligan, S. Thomson, J. Bound, J. McCann, W. Stevens, RFC 3493: Basic Socket Interface Extensions for IPv6, IETF / RFC Editor, 2003, https://www.rfc-editor.org/rfc/rfc3493.html, primary, checked 2026-10-19
  - supports: DM-074 (sockaddr_in cannot hold IPv6; sockaddr_storage aligned for casting; gethostbyname inadequate; minimal-change goal)
[S437] Linux man-pages project, accept(2), man7.org, current, https://man7.org/linux/man-pages/man2/accept.2.html, primary, checked 2026-10-19
  - supports: DM-074 (addrlen type history int -> size_t draft -> socklen_t; silent truncation of returned address)
[S438] Linux man-pages project, getaddrinfo(3), man7.org, current, https://man7.org/linux/man-pages/man3/getaddrinfo.3.html, primary, checked 2026-10-19
  - supports: DM-074 (getaddrinfo removes IPv4/IPv6 dependencies; returns sockaddr usable with connect/bind)
  - supports: DM-075 (synchronous interface; "potentially lengthy" lookups; RFC 3484 sorting via gai.conf)
[S439] Linux man-pages project, getaddrinfo_a(3), man7.org, current, https://man7.org/linux/man-pages/man3/getaddrinfo_a.3.html, primary, checked 2026-10-19
  - supports: DM-075 (GNU-only async lookup since glibc 2.2.3; signal or new-thread notification; in-progress requests cannot be cancelled)
[S440] Go project, package net (Name Resolution section), pkg.go.dev, current, https://pkg.go.dev/net, primary, checked 2026-10-19
  - supports: DM-075 (blocked C lookup consumes an OS thread; pure-Go resolver preferred; falls back to cgo for nsswitch features and on macOS; 500-lookup cap)
[S441] Linux man-pages project, nsswitch.conf(5), man7.org, current, https://man7.org/linux/man-pages/man5/nsswitch.conf.5.html, primary, checked 2026-10-19
  - supports: DM-075 (services provided by libnss_SERVICE.so.X shared libraries loaded by name)
[S442] musl libc project, Functional differences from glibc (Name Resolver/DNS), musl wiki, current, https://wiki.musl-libc.org/functional-differences-from-glibc.html, primary, checked 2026-10-19
  - supports: DM-075 (different resolver semantics: parallel nameserver queries; no DNS-over-TCP until 1.2.4)
[S443] Solar Designer, Re: The GNU C Library security advisories update for 2025-05-16 (CVE-2025-4802), Openwall oss-security list, 2025, https://seclists.org/oss-sec/2025/q2/161, primary, checked 2026-10-19
  - supports: DM-075 (static setuid binaries calling getaddrinfo/NSS dlopen could load attacker library via LD_LIBRARY_PATH, glibc 2.27-2.39)
[S444] Microsoft, GetAddrInfoExW function, Microsoft Learn (Win32 API), 2018, https://learn.microsoft.com/en-us/windows/win32/api/ws2tcpip/nf-ws2tcpip-getaddrinfoexw, primary, checked 2026-10-19
  - supports: DM-075 (async name resolution with OVERLAPPED/completion routine and GetAddrInfoExCancel, only from Windows 8)
[S445] Linux kernel documentation, IP Sysctl (ip_unprivileged_port_start, ping_group_range), docs.kernel.org, current, https://docs.kernel.org/networking/ip-sysctl.html, primary, checked 2026-10-19
  - supports: DM-076 (per-namespace first unprivileged port; root or CAP_NET_BIND_SERVICE; 0 disables; default 1024)
[S446] Linux man-pages project, rcmd(3), man7.org, current, https://man7.org/linux/man-pages/man3/rcmd.3.html, primary, checked 2026-10-19
  - supports: DM-076 (rcmd authentication "based on privileged port numbers"; rresvport; ruserok/.rhosts)
[S447] Krister Johansen (committed by David S. Miller), commit 4548b683b781: Introduce a sysctl that modifies the value of PROT_SOCK, Linux kernel git (android-kvm mirror), 2017, https://android-kvm.googlesource.com/linux/+/4548b683b78137f8eadeb312b94e20bb0d4a7141, primary, checked 2026-10-19
  - supports: DM-076 (sysctl added so container managers can lift privileged ports without giving up netns control)
[S448] Quinn "The Eskimo!" (Apple DTS) et al., Binding on priviledged ports on macOS, Apple Developer Forums, 2021, https://developer.apple.com/forums/thread/674179, secondary, checked 2026-10-19
  - supports: DM-076 (Apple engineer: iOS never had the restriction, lifted on macOS Mojave intentionally, never documented)
[S449] Justin Cormack, Add default sysctls to allow ping sockets and privileged ports with no capabilities (moby PR #41030), GitHub moby/moby, 2020, https://github.com/moby/moby/pull/41030, primary, checked 2026-10-19
  - supports: DM-076 (Docker sets ip_unprivileged_port_start=0 in containers by default; host-network caveat)
[S450] Linux man-pages project, pipe(7), man7.org, current, https://man7.org/linux/man-pages/man7/pipe.7.html, primary, checked 2026-10-19
  - supports: DM-077 (write to pipe with no readers raises SIGPIPE; EPIPE if ignored)
[S451] Linux man-pages project, send(2), man7.org, current, https://man7.org/linux/man-pages/man2/send.2.html, primary, checked 2026-10-19
  - supports: DM-077 (MSG_NOSIGNAL since Linux 2.2, per-call vs process-wide ignore; added to POSIX.1-2008)
[S452] The Open Group, write (POSIX.1-2024, XSH), The Open Group Base Specifications Issue 8, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/write.html, primary, checked 2026-10-19
  - supports: DM-077 (SIGPIPE sent for pipes with no reader and for SOCK_STREAM sockets no longer connected)
[S453] FreeBSD Project, setsockopt(2) manual page (SO_NOSIGPIPE), FreeBSD, current, https://man.freebsd.org/cgi/man.cgi?query=setsockopt&sektion=2, primary, checked 2026-10-19
  - supports: DM-077 (per-socket SO_NOSIGPIPE option)
[S454] Python Software Foundation, signal — Set handlers for asynchronous events, Python 3 documentation, current, https://docs.python.org/3/library/signal.html, primary, checked 2026-10-19
  - supports: DM-077 (Python ignores SIGPIPE at startup so errors become exceptions; BrokenPipeError when piping to head)
[S455] Josh Triplett et al., Should Rust still ignore SIGPIPE by default? (issue #62569), GitHub rust-lang/rust, 2019, https://github.com/rust-lang/rust/issues/62569, primary, checked 2026-10-19
  - supports: DM-077 (Rust startup ignores SIGPIPE since 2014; process-wide surprise; libraries vs applications differ)
[S456] Simon Peter, Jialin Li, Irene Zhang, Dan R. K. Ports, Doug Woos, Arvind Krishnamurthy, Thomas Anderson, Timothy Roscoe, Arrakis: The Operating System is the Control Plane, OSDI 2014, 2014, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/peter, primary, checked 2026-10-19
  - supports: DM-078 (kernel as control plane; applications get virtualized devices; 2-5x latency, 9x throughput vs Linux)
  - supports: Exokernel (kernel as control plane, application data-plane via virtualized I/O; 2–5x latency, ~9x throughput vs Linux)
[S457] Adam Belay, George Prekas, Ana Klimovic, Samuel Grossman, Christos Kozyrakis, Edouard Bugnion, IX: A Protected Dataplane Operating System for High Throughput and Low Latency, OSDI 2014, 2014, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/belay, primary, checked 2026-10-19
  - supports: DM-078 (protected dataplane via virtualization; run to completion; up to 3.6x throughput vs Linux)
[S458] Michael Marty et al., Snap: a Microkernel Approach to Host Networking, SOSP 2019, 2019, https://research.google/pubs/snap-a-microkernel-approach-to-host-networking/, primary, checked 2026-10-19
  - supports: DM-078 (userspace networking service in production >3 years, >half of Google fleet; transparent upgrades; 3x Gbps/core)
[S459] DPDK Project, Programmer's Guide: Overview, doc.dpdk.org, current, https://doc.dpdk.org/guides/prog_guide/overview.html, primary, checked 2026-10-19
  - supports: DM-078 (poll-mode drivers without interrupts; run-to-completion model)
[S460] Toke Høiland-Jørgensen, Jesper Dangaard Brouer, Daniel Borkmann, John Fastabend, Tom Herbert, David Ahern, David Miller, The eXpress Data Path: Fast Programmable Packet Processing in the Operating System Kernel, ACM CoNEXT 2018, 2018, https://courses.grainger.illinois.edu/ece598hpn/fa2023/papers/xdp.pdf, primary, checked 2026-10-19
  - supports: DM-078 (bypass loses isolation, security and tooling; dedicated cores; reimplementing routing; XDP/AF_XDP middle ground)
[S461] Marek Majkowski, Why we use the Linux kernel's TCP stack, Cloudflare blog, 2016, https://blog.cloudflare.com/why-we-use-the-linux-kernels-tcp-stack/, weak, checked 2026-10-19
  - supports: DM-078 (practitioner view: bypass breaks tcpdump/netstat, monopolises NIC, lacks DDoS features; ~5-10% CPU gain)
[S462] Fuchsia project, Netstack3 – A Fuchsia owned rust based netstack (2021 roadmap), fuchsia.dev, 2021, https://fuchsia.dev/fuchsia-src/contribute/roadmap/2021/netstack3, primary, checked 2026-10-19
  - supports: DM-078 (Fuchsia replacing gVisor-derived Go netstack with Rust netstack; GC language and ownership concerns; performance parity a risk)
[S463] Microsoft, WaitForMultipleObjects function, Microsoft Learn (Win32 API), 2018, https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects, primary, checked 2026-10-19
  - supports: DM-130 (one wait over processes, threads, events, mutexes, semaphores, timers, change notifications; MAXIMUM_WAIT_OBJECTS limit)
[S466] Fuchsia project, Port (Zircon kernel object), fuchsia.dev, current, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/port, primary, checked 2026-10-19
  - supports: DM-130 (port receives user packets, async waits on any bound handle, IPC delivery)
[S467] Fuchsia project, zx_object_wait_async, fuchsia.dev, current, https://fuchsia.dev/fuchsia-src/reference/syscalls/object_wait_async, primary, checked 2026-10-19
  - supports: DM-130 (any waitable object's signals delivered as packets to a port; one packet per wait; edge mode can miss transitions)
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
  - supports: DM-042 (marked-for-update model; periodic update allowed; required update points)
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
[S544] RustSec, RUSTSEC-2020-0159: chrono: Potential segfault in localtime_r invocations, rustsec.org, 2021, https://rustsec.org/advisories/RUSTSEC-2020-0159.html, primary, checked 2026-10-19
  - supports: DM-066 (env var set in another thread => dangling pointer segfault in localtime_r path; related CVE-2020-26235)
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
[S600] Linux man-pages project, fcntl_locking(2) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man2/fcntl_locking.2.html, primary, checked 2026-10-19
  - supports: DM-036 (process-owned locks released on close of any fd; not inherited by fork; threads share locks; OFD locks since 3.15; no deadlock detection for OFD)
  - supports: DM-044 (Linux mandatory locking unreliable, optional since 4.5, removed in 5.15)
[S601] Jeffrey T. Layton, File-private POSIX locks, LWN.net, 2014, https://lwn.net/Articles/586904/, secondary, checked 2026-10-19
  - supports: DM-036 (close semantics, library code silently dropping locks, threads, flock whole-file only, Samba/Ganesha/SQLite/OpenJDK interest, rename to OFD)
[S602] SQLite developers, src/os_unix.c (comment "Posix Advisory Locking"), SQLite source tree (GitHub mirror), current trunk, https://raw.githubusercontent.com/sqlite/sqlite/master/src/os_unix.c, primary, checked 2026-10-19
  - supports: DM-036 (POSIX locks "broken by design"; unixInodeInfo per-inode tracking; deferred close via pUnused list)
[S603] The Open Group / IEEE, fcntl — POSIX.1-2024 (IEEE Std 1003.1-2024), pubs.opengroup.org, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/fcntl.html, primary, checked 2026-10-19
  - supports: DM-036 (process-owned locks removed when any fd for the file is closed; F_OFD_* now standardized)
[S604] Jonathan Corbet, PostgreSQL's fsync() surprise, LWN.net, 2018, https://lwn.net/Articles/752063/, secondary, checked 2026-10-19
  - supports: DM-037 (PG assumption; pages marked clean; errors before open() not seen; Ts'o USB rationale; Chinner/Ts'o direct-I/O advice; OpenBSD/NetBSD similar)
[S605] Craig Ringer, PostgreSQL's handling of fsync() errors is unsafe and risks data loss at least on XFS, pgsql-hackers mailing list, 2018, https://www.postgresql.org/message-id/CAMsr%2BYHh%2B5Oq4xziwwoEfhoTZgr07vdGG%2Bhu%3D1adXx59aTeaoQ%40mail.gmail.com, primary, checked 2026-10-19
  - supports: DM-037 (retry-after-EIO silently succeeds; observed XFS corruption; proposal to PANIC; POSIX silent on post-failure state)
[S606] Anthony Rebello, Yuvraj Patel, Ramnatthan Alagappan, Andrea C. Arpaci-Dusseau, Remzi H. Arpaci-Dusseau, Can Applications Recover from fsync Failures?, USENIX ATC 2020, 2020, https://www.usenix.org/conference/atc20/presentation/rebello, primary, checked 2026-10-19
  - supports: DM-037 (ext4/XFS/Btrfs mark pages clean after failure; none of PostgreSQL, LMDB, LevelDB, SQLite, Redis handle it adequately)
[S607] Jonathan Corbet, Improved block-layer error handling, LWN.net, 2017, https://lwn.net/Articles/724307/, secondary, checked 2026-10-19
  - supports: DM-037 (errseq_t: error reported once per open file description; "will not be perfect")
[S608] PostgreSQL community, Fsync Errors, PostgreSQL Wiki, n.d. (living page), https://wiki.postgresql.org/wiki/Fsync_Errors, primary, checked 2026-10-19
  - supports: DM-037 (PANIC fix in 12 backpatched to 9.4; per-OS table: FreeBSD keeps buffers dirty, Linux/macOS/NetBSD/OpenBSD invalidate)
[S609] Apple Inc., fsync(2) — Mac OS X / iOS Manual Page, Apple Developer Documentation Archive, n.d., https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html, primary, checked 2026-10-19
  - supports: DM-037 (Darwin fsync does not flush drive cache; F_FULLFSYNC needed)
[S610] Jonathan Corbet, ext4 and data loss, LWN.net, 2009, https://lwn.net/Articles/322823/, secondary, checked 2026-10-19
  - supports: DM-038 (zero-length files after crash; ext3 data=ordered protection accidental; 2.6.30 rename/truncate heuristics)
[S611] Theodore Ts'o, Comment #45 on Ubuntu bug 317781 (ext4 data loss), Launchpad, 2009, https://bugs.launchpad.net/ubuntu/+source/linux/+bug/317781/comments/45, primary, checked 2026-10-19
  - supports: DM-038 (POSIX leaves unclean-shutdown state undefined; Firefox fsync cost; heuristics in 2.6.30; advice to use SQLite-like stores)
[S612] Thanumalayan Sankaranarayana Pillai, Vijay Chidambaram, Ramnatthan Alagappan, Samer Al-Kiswany, Andrea C. Arpaci-Dusseau, Remzi H. Arpaci-Dusseau, All File Systems Are Not Created Equal: On the Complexity of Crafting Crash-Consistent Applications, OSDI 2014, 2014, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/pillai, primary, checked 2026-10-19
  - supports: DM-038 (60 vulnerabilities in 11 applications; persistence properties differ across 6 Linux file systems; Git append-then-rename example)
  - supports: DM-045 (applications hand-roll crash consistency and get it wrong)
[S613] Linux man-pages project, openat2(2) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man2/openat2.2.html, primary, checked 2026-10-19
  - supports: DM-039 (RESOLVE_* semantics; Linux 5.6; extensible open_how; EAGAIN on possible ".." race)
  - supports: DM-055 (openat ignores unknown flag bits, openat2 rejects them; size as version; E2BIG for unsupported nonzero extension fields)
[S614] Daroc Alden, The difficulty of safe path traversal, LWN.net, 2026, https://lwn.net/Articles/1050887/, secondary, checked 2026-10-19
  - supports: DM-039 (14 runc CVEs since 2017; procfs overmounts; openat2 often blocked by seccomp; libpathrs; move to fd-based designs)
[S615] NIST NVD, CVE-2018-15664 (Docker cp symlink-exchange race), National Vulnerability Database, 2019, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2018-15664, primary, checked 2026-10-19
  - supports: DM-039 (symlink-exchange + traversal race in docker cp gives root read-write host access)
[S616] Fuchsia project, Dot Dot Considered Harmful, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/filesystems/dotdot, primary, checked 2026-10-19
  - supports: DM-039 (no ".." on servers; directory handle confines to subtree; client-side canonicalization relies on absence of symlinks)
[S617] Donald E. Porter, Owen S. Hofmann, Christopher J. Rossbach, Alexander Benn, Emmett Witchel, Operating System Transactions, SOSP 2009, 2009, https://www.cs.utexas.edu/users/witchel/pubs/porter09sosp-txos.pdf, primary, checked 2026-10-19
  - supports: DM-039 (>600 NVD "symlink attack" hits; TOCTTOU eliminable only by API change; Linux added close-on-exec flag to 15 syscalls)
  - supports: DM-045 (TxOS: ACID system transactions over Linux 2.6.22; ~10% overhead on transactional dpkg install; Windows TxF/TxR mentioned)
[S618] Rob Pike, Lexical File Names in Plan 9 or Getting Dot-Dot Right, USENIX ATC 2000, 2000, https://www.usenix.org/legacy/event/usenix2000/general/full_papers/pikelex/pikelex_html/index.html, primary, checked 2026-10-19
  - supports: DM-039 (symlinks make ".." ambiguous; Plan 9 lexical ".."; kernel-tracked names; fd2path)
[S619] Linux kernel documentation, Documentation for /proc/sys/fs/ (protected_hardlinks, protected_symlinks, protected_fifos, protected_regular), docs.kernel.org, current, https://docs.kernel.org/admin-guide/sysctl/fs.html, primary, checked 2026-10-19
  - supports: DM-039 ("long-standing class" of symlink TOCTOU races; sticky-dir restrictions)
  - supports: DM-040 (protected_hardlinks semantics)
[S620] Kees Cook, [PATCH v2] fs: hardlink creation restrictions, linux-kernel mailing list (lkml.iu.edu archive), 2012, https://lkml.iu.edu/hypermail/linux/kernel/1202.2/02340.html, primary, checked 2026-10-19
  - supports: DM-040 (hardlink TOCTOU across privilege boundaries; pinning vulnerable setuid files; POSIX permits restriction; "at" breakage; Ubuntu 1.5 years)
[S621] Kees Cook (posted by LWN), [PATCH 1/2] fs: add link restrictions, LWN.net patch posting, 2012, https://lwn.net/Articles/503671/, primary, checked 2026-10-19
  - supports: DM-040 (history of discussions 1996–2010; objections and rebuttals: POSIX, breakage, mkstemp, LSM-vs-VFS; based on Openwall/grsecurity)
[S622] Linux man-pages project, mount_namespaces(7) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man7/mount_namespaces.7.html, primary, checked 2026-10-19
  - supports: DM-041 (CLONE_NEWNS in 2.4.19; shared subtrees in 2.6.15 because full isolation too strict; locked mounts in less-privileged namespaces)
[S625] Rob Pike, Dave Presotto, Sean Dorward, Bob Flandrena, Ken Thompson, Howard Trickey, Phil Winterbottom, Plan 9 from Bell Labs, Bell Labs, n.d. (c. 1995), https://9p.io/sys/doc/9.html, primary, checked 2026-10-19
  - supports: DM-041 (user processes build name spaces with mount/bind/unmount; "Plan 9 has no super-user")
  - supports: Plan 9 (Unix problems too deep; 9P, namespaces, unions, /proc, 8½, dump, no superuser; authors' list of weaknesses)
[S627] Jonathan Corbet, Once upon atime, LWN.net, 2007, https://lwn.net/Articles/244829/, secondary, checked 2026-10-19
  - supports: DM-042 (every read becomes a write; Molnar quotes; mutt/tmpwatch/backup dependence; relatime since 2.6.20; Linus's one-day interval)
[S628] Linux man-pages project / util-linux, mount(8) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man8/mount.8.html, primary, checked 2026-10-19
  - supports: DM-042 (relatime default since 2.6.30; 1-day rule; strictatime; lazytime)
[S630] Microsoft, fsutil behavior, Microsoft Learn, 2017 (updated 2026), https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/fsutil-behavior, primary, checked 2026-10-19
  - supports: DM-042 (NTFS disablelastaccess; NTFS defers last-access writes up to one hour)
  - supports: DM-043 (disable8dot3; 8.3 short-name generation cost)
  - supports: DM-045 ("TxF is presently deprecated, but the functionality is still available")
[S631] Jonathan Corbet, Atime and btrfs: a bad combination?, LWN.net, 2012, https://lwn.net/Articles/499293/, secondary, checked 2026-10-19
  - supports: DM-042 (atime on CoW + snapshots unshares metadata; reads consume space; relatime may not help)
[S632] Microsoft, Maximum Path Length Limitation, Microsoft Learn, 2024, https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation, primary, checked 2026-10-19
  - supports: DM-043 (MAX_PATH 260; \\?\ 32,767; opt-in requires registry AND manifest; relative paths always limited; shell vs FS mismatch; git example)
  - supports: DM-094 (Windows FS treats names as opaque WCHAR sequences; no normalization)
[S633] Microsoft, Naming Files, Paths, and Namespaces, Microsoft Learn, 2024, https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file, primary, checked 2026-10-19
  - supports: DM-043 (reserved names incl. with extension and superscript digits; no trailing space/period; \\?\ disables parsing; NT vs Win32 namespaces)
  - supports: DM-094 (case-insensitive by default; NTFS supports case-sensitive semantics)
  - supports: DM-095 (Windows forbids bytes 1–31 and reserved punctuation in names)
[S634] Raymond Chen, What's the deal with those reserved filenames like NUL and CON?, The Old New Thing (Microsoft devblogs), 2003, https://devblogs.microsoft.com/oldnewthing/20031022-00/?p=42073, secondary, checked 2026-10-19
  - supports: DM-043 (DOS 1.0 had no directories; magic names had to work with extensions; kept because batch files rely on them)
[S635] Python Software Foundation, Using Python on Windows (sections 4.6 Removing the MAX_PATH Limitation, 4.7 UTF-8 mode), docs.python.org, 2026, https://docs.python.org/3/using/windows.html, primary, checked 2026-10-19
  - supports: DM-043 (long paths need admin policy/registry; Python enabled support in 3.6)
  - supports: DM-096 (Windows still uses legacy ANSI code page as system encoding; UTF-8 mode default from 3.15; PEP 528/529)
  - supports: DM-097 (UTF-8 mode ignores locale/system encoding for text files)
[S636] Microsoft, CreateFileW function (fileapi.h), Microsoft Learn, 2022, https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew, primary, checked 2026-10-19
  - supports: DM-044 (dwShareMode semantics; 0 = no subsequent opens; ERROR_SHARING_VIOLATION; FILE_SHARE_DELETE governs delete and rename)
[S638] Microsoft, FILE_DISPOSITION_INFORMATION_EX structure (ntddk.h), Microsoft Learn, 2021, https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex, primary, checked 2026-10-19
  - supports: DM-044 (FILE_DISPOSITION_POSIX_SEMANTICS: name removed while existing handles keep data; STATUS_CANNOT_DELETE with mapped view)
[S639] Microsoft, Alternatives to using Transactional NTFS, Microsoft Learn, 2018, https://learn.microsoft.com/en-us/windows/win32/fileio/deprecation-of-txf, primary, checked 2026-10-19
  - supports: DM-045 (TxF since Vista; "extremely limited developer interest"; complexity; may be removed; ReplaceFile/ESE/SQL alternatives)
[S640] Nikolay Borisov (reviewed by David Sterba), btrfs: Deprecate userspace transaction ioctls, Linux kernel commit 3558d4f88ec8 (GitHub mirror), 2017, https://github.com/torvalds/linux/commit/3558d4f88ec8, primary, checked 2026-10-20
  - supports: DM-045 (btrfs user transactions were added for Ceph; no widespread legitimate use; slated for removal in 4.17)
[S641] Apple Inc., Apple File System Guide — Frequently Asked Questions (retired document), Apple Developer Documentation Archive, n.d. (c. 2017–2018), https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html, primary, checked 2026-10-19
  - supports: DM-094 (HFS+ stores NFD-normalized names (Unicode 3.2); APFS preserves and hashes normalized form (Unicode 9.0); iOS 10.3 APFS was normalization-sensitive causing ENOENT; case-insensitive default on macOS)
  - supports: DM-095 (APFS stores names in UTF-8; rejects unassigned Unicode 9.0 code points)
  - supports: DM-096 (APFS UTF-8 vs HFS UTF-16 on-disk names)
[S642] NIST NVD, CVE-2014-9390 (Git .git/config overwrite on case-insensitive / normalizing file systems), National Vulnerability Database, 2014, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2014-9390, primary, checked 2026-10-19
  - supports: DM-094 (ignorable Unicode codepoint, git~1 8.3 alias, mixed case on case-insensitive FS lead to arbitrary command execution)
[S643] Vicent Martí, Vulnerability announced: update your Git clients, GitHub Blog, 2014, https://github.blog/news-insights/company-news/vulnerability-announced-update-your-git-clients/, secondary, checked 2026-10-19
  - supports: DM-094 (affects HFS+, NTFS, FAT clients; fixed versions; Linux case-sensitive unaffected)
[S644] Jake Edge, Case-insensitive ext4, LWN.net, 2019, https://lwn.net/Articles/784041/, secondary, checked 2026-10-19
  - supports: DM-094 (motivations: Android /sdcard, Samba interoperability; per-directory flag; NFD internal comparison; Torvalds objection; invalid UTF-8 treated as opaque)
[S645] Kent Overstreet, Re: [GIT PULL] bcachefs fixes for 6.15-rc4 (quoting Linus Torvalds), linux-kernel mailing list (lkml.rescloud.iu.edu archive), 2025, https://lkml.rescloud.iu.edu/2504.3/02103.html, primary, checked 2026-10-19
  - supports: DM-094 (Torvalds: case-insensitive names "horribly wrong"; Overstreet: user/application needs justify them)
[S646] H. Peter Anvin, Re: [GIT PULL] bcachefs fixes for 6.15-rc4 (quoting Linus Torvalds), linux-kernel mailing list (lkml.rescloud.iu.edu archive), 2025, https://lkml.rescloud.iu.edu/2504.3/05029.html, primary, checked 2026-10-19
  - supports: DM-094 (Torvalds: compatibility only valid reason, prefer fixed 1:1 table, HFS "almost NFD" rejected; Anvin: Microsoft froze its upcase table, 1:1 BMP only)
[S647] Microsoft, Case Sensitivity (WSL), Microsoft Learn, 2021 (updated 2026), https://learn.microsoft.com/en-us/windows/wsl/case-sensitivity, primary, checked 2026-10-19
  - supports: DM-094 (per-directory case sensitivity since build 17107; Windows apps that change case break; files differing only by case inaccessible to Windows apps)
[S649] David A. Wheeler (reporter), Austin Group, Bug 0000251: Forbid newline, or even bytes 1 through 31 (inclusive), in filenames, Austin Group Defect Tracker, 2010–2024, https://www.austingroupbugs.net/view.php?id=251, primary, checked 2026-10-19
  - supports: DM-095 (rationale: vulnerabilities and erroneous programs; NTFS already forbids 1–31; accepted resolution: implementations "encouraged" to fail with EILSEQ on newline; future version may require it)
[S650] Alejandro Colomar, Re: newlines in filenames; POSIX.1-2024 (reply to Theodore Ts'o), linux-kernel mailing list (lkml.rescloud.iu.edu archive), 2025, https://lkml.rescloud.iu.edu/2504.3/00150.html, primary, checked 2026-10-19
  - supports: DM-095 (Ts'o objections: fragmentation, control chars printable in cp437, kernel doesn't know locale; Colomar: optional mode forbidding 0–31)
  - supports: DM-094 (Ts'o: adding Unicode case-folding tables caused ongoing headaches)
[S651] The Open Group / IEEE, Base Definitions ch. 3 Definitions (3.264 Portable Filename, 3.265 Portable Filename Character Set, Pathname) — POSIX.1-2024, pubs.opengroup.org, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap03.html, primary, checked 2026-10-20
  - supports: DM-095 (portable set is A–Z a–z 0–9 . _ -; avoid leading hyphen; pathname "might only be a string" rather than character string)
[S652] The Rust Project, std::ffi::OsString, Rust standard library documentation, current, https://doc.rust-lang.org/std/ffi/struct.OsString.html, primary, checked 2026-10-19
  - supports: DM-095 (Unix strings are arbitrary non-zero bytes; separate OS string type needed)
  - supports: DM-096 (Windows strings are arbitrary 16-bit sequences, stored internally in a less-strict UTF-8 variant)
[S653] Microsoft, Use UTF-8 code pages in Windows apps, Microsoft Learn, current, https://learn.microsoft.com/en-us/windows/apps/design/globalizing/use-utf8-code-page, primary, checked 2026-10-19
  - supports: DM-096 (activeCodePage UTF-8 since 1903; GDI ignores per-process setting; -A APIs reused for UTF-8; Windows natively UTF-16; CP_ACP legacy)
  - supports: GR-015 (Microsoft recommends UTF-8; activeCodePage UTF-8 since Windows 10 1903)
[S654] Microsoft, Conventions for Function Prototypes, Microsoft Learn, current, https://learn.microsoft.com/en-us/windows/win32/intl/conventions-for-function-prototypes, primary, checked 2026-10-19
  - supports: DM-096 (generic/A/W triple via UNICODE macro; some functions W-only)
[S655] Microsoft, Surrogates and Supplementary Characters, Microsoft Learn, current, https://learn.microsoft.com/en-us/windows/win32/intl/surrogates-and-supplementary-characters, primary, checked 2026-10-19
  - supports: DM-096 (Windows uses UTF-16; 16 bits insufficient; surrogate pairs required for supplementary characters)
[S656] Unicode Consortium, FAQ — UTF-8, UTF-16, UTF-32 & BOM, unicode.org, current, https://www.unicode.org/faq/utf_bom.html, primary, checked 2026-10-19
  - supports: DM-096 (UCS-2 = implementations up to Unicode 1.1, before surrogates/UTF-16 added in 2.0; "UCS-2" implementations don't handle supplementary characters)
[S657] Michael S. Kaplan, UCS-2 vs. UTF-16 (not quite Kramer vs. Kramer), Sorting It All Out (MSDN blog, archived), 2005, https://archives.miloush.net/michkap/archive/2005/05/11/416552.html, secondary, checked 2026-10-20
  - supports: DM-096 (CreateFileW level takes arbitrary WCHAR arrays incl. unpaired surrogates; Windows 2000 "surrogate neutral"; XP UTF-16 support)
[S658] Steve Dower, PEP 529 — Change Windows filesystem encoding to UTF-8, Python Enhancement Proposals, 2016, https://peps.python.org/pep-0529/, primary, checked 2026-10-20
  - supports: DM-096 (ANSI APIs encode via active code page; lossy round-trip for characters outside the code page; OS represents all text as UTF-16)
[S659] Linux man-pages project, setlocale(3) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man3/setlocale.3.html, primary, checked 2026-10-19
  - supports: DM-097 (program-wide locale; "C" at startup; env-var driven; MT-Unsafe; C89/POSIX.1-2001 origin)
[S660] Linux man-pages project, uselocale(3) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man3/uselocale.3.html, primary, checked 2026-10-19
  - supports: DM-097 (per-thread locale; glibc 2.3; POSIX.1-2008)
[S661] Linux man-pages project, newlocale(3) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man3/newlocale.3.html, primary, checked 2026-10-19
  - supports: DM-097 (locale objects; base may be freed on modify; LC_GLOBAL_LOCALE base undefined)
[S662] Alyssa Coghlan, PEP 538 — Coercing the legacy C locale to a UTF-8 based locale, Python Enhancement Proposals, 2016, https://peps.python.org/pep-0538/, primary, checked 2026-10-20
  - supports: DM-097 (C locale implies ASCII; divergence between components in one process and subprocesses; readline interop)
[S663] Jens Maurer, P0067R5: Elementary string conversions, revision 5, ISO/IEC JTC1/SC22/WG21, 2016, https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2016/p0067r5.html, primary, checked 2026-10-20
  - supports: DM-097 (sprintf/strtol/atol/stringstream all locale-dependent; to_chars/from_chars designed locale-independent for JSON/XML)
[S664] Python Software Foundation, locale — Internationalization services, docs.python.org, 2026, https://docs.python.org/3/library/locale.html, primary, checked 2026-10-20
  - supports: DM-097 (C standard makes locale program-wide; setlocale not thread-safe; calling it in a library affects whole program; some implementations crash on frequent changes)
[S665] Microsoft, Unicode in the Windows API, Microsoft Learn, 2021, https://learn.microsoft.com/en-us/windows/win32/intl/unicode-in-the-windows-api, primary, checked 2026-10-19
  - supports: DM-096 (three formats: generic, A ("ANSI" code page), W ("wide"); some newer functions Unicode-only)
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
[S800] Linus Åkesson, The TTY demystified, linusakesson.net, 2008, https://www.linusakesson.net/programming/tty/, secondary, checked 2026-10-19
  - supports: DM-069 (line discipline in kernel; cooked/raw mode; "twisty little mess of special cases")
  - supports: DM-070 (session/foreground process group; SIGHUP, SIGTTIN/SIGTTOU, SIGWINCH roles)
[S801] Michael Kerrisk (maintainer), credentials(7), Linux man-pages project, n.d. (current), https://man7.org/linux/man-pages/man7/credentials.7.html, primary, checked 2026-10-19
  - supports: DM-070 (two-level session/process-group hierarchy; controlling terminal acquisition, O_NOCTTY; foreground job, SIGTTIN/SIGTTOU)
[S803] Kees Cook (committed by Greg Kroah-Hartman), tty: Allow TIOCSTI to be disabled (commit 83efeeeb3d04), Linux kernel git (googlesource mirror), 2022, https://android-kvm.googlesource.com/linux/+/83efeeeb3d04b22aaed1df99bc70a48fe9d22c4d%5E%21, primary, checked 2026-10-19
  - supports: DM-072 (LEGACY_TIOCSTI Kconfig + sysctl, default y; "long history" of privilege escalation; OpenBSD removed, Android filters; earlier LSM-filter proposals stalled)
[S804] Hanno Böck, TTY pushback vulnerabilities / TIOCSTI (oss-security post), Openwall oss-security list, 2023, https://openwall.com/lists/oss-security/2023/03/14/2, primary, checked 2026-10-19
  - supports: DM-072 (su/sudo/bubblewrap affected; pty options in su/sudo are optional; kernel disable is the only real fix)
[S805] MITRE/CVE Program, CVE-2017-5226 (bubblewrap TIOCSTI sandbox escape), CVE record (cveawg API), 2017, https://cveawg.mitre.org/api/cve/CVE-2017-5226, primary, checked 2026-10-19
  - supports: DM-072 (sandboxed process escapes to parent session via TIOCSTI)
[S806] brynet (contributor), On the Insecurity of TIOCSTI, undeadly.org (OpenBSD Journal), 2017, https://undeadly.org/cgi?action=article;sid=20170701132619, secondary, checked 2026-10-19
  - supports: DM-072 (OpenBSD makes TIOCSTI return EIO; csh/mailx/ksh uses removed; de Raadt's rationale; Linux had rejected removal)
[S807] MITRE/CVE Program, CVE-2016-7545 (SELinux policycoreutils sandbox TIOCSTI escape), CVE record (cveawg API), 2017, https://cveawg.mitre.org/api/cve/CVE-2016-7545, primary, checked 2026-10-19
  - supports: DM-072 (sandbox escape via crafted TIOCSTI; fix was "create a new session for sandboxed processes")
  - supports: DM-070 (setsid used as the security fix for a shared controlling terminal)
[S808] Solar Designer, TIOCSTI / su-like programs and pty allocation (oss-security post), Openwall oss-security list, 2017, https://www.openwall.com/lists/oss-security/2017/06/03/9, primary, checked 2026-10-19
  - supports: DM-072 (Alan Cox NAKs of TIOCSTI-disable patch and his reasoning; util-linux CVE-2016-2779 left unfixed; recommendation to allocate new ptys)
  - supports: DM-070 (setsid would break su/runuser)
[S809] Flatpak developers, Flatpak CVE-2023-28100 advisory (TIOCLINUX), Openwall oss-security list, 2023, http://www.openwall.com/lists/oss-security/2023/03/17/1, primary, checked 2026-10-19
  - supports: DM-072 (TIOCLINUX copy/paste gives TIOCSTI-like injection on VTs; blocked by seccomp)
[S810] H D Moore, Terminal Emulator Security Issues (bugtraq post), Bugtraq (MIT archive), 2003, https://diswww.mit.edu/menelaus/bt/29027, primary, checked 2026-10-19
  - supports: DM-071 (title reporting onto command line; Eterm/rxvt screen dump writes files; delivery via logs, wget/curl, banners; affected emulators)
[S811] David Leadbeater, ANSI Terminal security in 2023 and finding 10 CVEs, dgl.cx (researcher's own disclosure write-up), 2023, https://dgl.cx/term, primary, checked 2026-10-19
  - supports: DM-071 (echoback class; DECRQSS bug copied from xterm docs; CVE list across iTerm2, mintty, SwiftTerm, ConEmu, Windows Terminal, urxvt, less; root causes; recommendations incl. mosh)
[S812] MITRE/CVE Program, CVE-2022-45063 (xterm OSC 50 font ops code execution), CVE record (cveawg API), 2022, https://cveawg.mitre.org/api/cve/CVE-2022-45063, primary, checked 2026-10-19
  - supports: DM-071 (concrete reply-into-input code execution with zsh vi mode)
[S813] Ecma International, ECMA-48 Control functions for coded character sets (5th ed.), Ecma International, 1991, https://ecma-international.org/publications-and-standards/standards/ecma-48/, primary, checked 2026-10-19
  - supports: DM-071 (control functions embedded in character-coded data)
[S814] Rich Turner, Windows Command-Line: Introducing the Windows Pseudo Console (ConPTY), Microsoft devblogs (Command Line), 2018, https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/, primary, checked 2026-10-19
  - supports: DM-073 (no PTY on Windows; Console API as IOCTLs via ConDrv; off-screen console screen-scraping and its instability; ConPTY API; no Win7 support)
  - supports: DM-069 (Windows' terminal plumbing as alternative to kernel TTY)
  - supports: DM-071 (out-of-band console API needed ConPTY to be remoted)
[S815] Michael Niksa (miniksa), Classic Console APIs versus Virtual Terminal Sequences, Microsoft Learn, 2018 (updated 2025), https://learn.microsoft.com/en-us/windows/console/classic-vs-vt, primary, checked 2026-10-19
  - supports: DM-073 (recommendation to move to VT; remoting; "wrong-way verbs" crossing privilege; direct window handle fragility; no plan to remove; pseudoconsole translation imperfect)
  - supports: DM-071 (VT advantages: carried over any byte channel; in-band)
  - supports: DM-069 (line editing better done in client than in host)
  - supports: DM-070 (Windows console signals via SetConsoleCtrlHandler/GenerateConsoleCtrlEvent)
[S816] Rich Turner, Windows Command-Line: Inside the Windows Console, Microsoft devblogs (Command Line), 2018, https://devblogs.microsoft.com/commandline/windows-command-line-inside-the-windows-console/, primary, checked 2026-10-19
  - supports: DM-073 (history: CSRSS, conhost/condrv in Win7 for security and reliability; object vs text philosophy; compatibility constraints; UCS-2)
  - supports: DM-069 (console host in user mode)
  - supports: DM-071 (out-of-band typed API as the alternative to in-band VT)
[S817] MITRE/CVE Program, CVE-2014-0196 (n_tty_write race), CVE record (cveawg API), 2014, https://cveawg.mitre.org/api/cve/CVE-2014-0196, primary, checked 2026-10-19
  - supports: DM-069 (line-discipline race giving memory corruption / privilege escalation; in CISA KEV)
[S818] Greg Kroah-Hartman, tty: ldisc: add sysctl to prevent autoloading of ldiscs (patch posting), kernel-hardening list (Openwall archive), 2019, https://openwall.com/lists/kernel-hardening/2019/01/21/6, primary, checked 2026-10-19
  - supports: DM-069 (any user can trigger loading of any ldisc module; default kept y for compatibility)
[S819] MITRE/CVE Program (Red Hat CNA), CVE-2023-6546 (n_gsm use-after-free), CVE record (cveawg API), 2023, https://cveawg.mitre.org/api/cve/CVE-2023-6546, primary, checked 2026-10-19
  - supports: DM-069 (obscure line discipline reachable by unprivileged user; UAF -> privilege escalation)
[S820] Arnd Bergmann, BKL conversion in tty layer (LKML patch series v2, reprinted), LWN.net, 2010, https://lwn.net/Articles/386190, primary, checked 2026-10-19
  - supports: DM-069 (TTY layer's dependence on BKL; second attempt; 54 files touched)
[S821] Linux Magazine staff, Cox Quits as TTY Maintainer, Linux Magazine, 2009, https://www.linux-magazine.com/Online/News/Cox-Quits-as-TTY-Maintainer, secondary, checked 2026-10-19
  - supports: DM-069 (maintainer resignation after TTY regression broke kdesu; compatibility pressure)
[S822] Plan 9 authors (Bell Labs), cons(3), Plan 9 manual (9p.io), n.d., https://9p.io/magic/man2html/3/cons, primary, checked 2026-10-19
  - supports: DM-069 (minimal console line editing; rawon/rawoff via consctl file instead of termios ioctls)
[S823] Plan 9 authors (Bell Labs), rio(4), Plan 9 manual (9p.io), n.d., https://9p.io/magic/man2html/4/rio, primary, checked 2026-10-19
  - supports: DM-069 (per-window cons/consctl served by user-space window system)
  - supports: DM-070 (DEL/hangup become notes to a named note group; no job-control mechanism described)
[S824] Jonathan Corbet, Distributors ponder a systemd change, LWN.net, 2016, https://lwn.net/Articles/690151/, secondary, checked 2026-10-19
  - supports: DM-070 (KillUserProcesses default flip; SIGHUP/nohup no longer sufficient; screen/tmux breakage; distro build-time opt-outs; Poettering's security rationale)
[S825] The Open Group / IEEE, _exit (POSIX.1-2024 System Interfaces), The Open Group Base Specifications Issue 8, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/_exit.html, primary, checked 2026-10-19
  - supports: DM-070 (SIGHUP to foreground pgrp on controlling-process exit; orphaned-pgrp SIGHUP+SIGCONT; SysV vs 4.2BSD history)
[S826] David P. Wiggins, Security Extension Specification, X Consortium (X11R7.7 docs), 1996, https://xorg.freedesktop.org/archive/X11R7.7/doc/xextproto/security.txt, primary, checked 2026-10-19
  - supports: DM-079 (trusted/untrusted clients; goals: stop keyboard theft and image reading; limitations incl. ICCCM focus breakage)
[S827] Eamon F. Walsh (NSA), X Access Control Extension Specification (XACE) v2.2, X.Org, 2009, https://xorg.freedesktop.org/archive/X11R7.7/doc/xorg-docs/Xserver/XACE-Spec.xml, primary, checked 2026-10-19
  - supports: DM-079 (LSM-like hooks; incomplete event-delivery coverage; GLX/DRI not secured; lifecycle problems)
[S828] Wayland project, Wayland FAQ, freedesktop.org, n.d., https://wayland.freedesktop.org/faq.html, primary, checked 2026-10-19
  - supports: DM-080 (remote rendering out of scope; separate remoting server)
  - supports: DM-084 (display server+WM+compositor in one; rendering API "whatever you want"; X as compatibility option)
[S829] Wayland project, Wayland Architecture, freedesktop.org, n.d., https://wayland.freedesktop.org/architecture.html, primary, checked 2026-10-19
  - supports: DM-079 (compositor picks input target from its scenegraph)
  - supports: DM-080 (X server-side rendering vs client-side buffers)
  - supports: DM-083 (clients send damage, compositor schedules page flips)
  - supports: DM-081 (user-space compositor as alternative to in-kernel window manager)
  - supports: DM-082 (compositor, not clients, routes input)
  - supports: GR-016 (compositor is display server; compositor routes input to target surface; client-side rendering)
[S830] OpenBSD project, ssh_config(5), OpenBSD manual, n.d. (current), https://man.openbsd.org/ssh_config, primary, checked 2026-10-19
  - supports: DM-079 (keystroke monitoring via trusted forwarding; untrusted mode uses SECURITY extension; 20-minute timeout)
[S831] Joanna Rutkowska, The Linux Security Circus: On GUI isolation, The Invisible Things Lab blog, 2011, https://theinvisiblethings.blogspot.com/2011/04/linux-security-circus-on-gui-isolation.html, secondary, checked 2026-10-19
  - supports: DM-079 (xinput test keylogger demo across users/SELinux; XACE unused in practice; Qubes per-domain isolation)
[S832] XDG Desktop Portal project, XDG Desktop Portal documentation, flatpak.github.io, n.d., https://flatpak.github.io/xdg-desktop-portal/docs/, primary, checked 2026-10-19
  - supports: DM-079 (screenshot/screencast/global shortcuts/input capture/remote desktop moved to mediated portals)
  - supports: DM-084 (desktop-specific backends: GTK, GNOME, KDE, wlroots)
[S833] Nathan Willis, LCA: The ways of Wayland, LWN.net, 2013, https://lwn.net/Articles/536862/, secondary, checked 2026-10-19
  - supports: DM-080 (X server reduced to IPC bus; SHM/DRI2 not network-transparent; poor over Internet; core frozen; VNC-like remoting proposal)
  - supports: DM-083 (X frames assembled from separate requests -> inconsistent output; DRI2 limited)
  - supports: DM-084 (X itself accumulated four input stacks, five display-management extensions, four buffer models)
[S834] Robert W. Scheifler, X Window System Protocol, X Version 11 (X11R7.7), X Consortium / X.Org, 1986–2004, https://xorg.freedesktop.org/archive/X11R7.7/doc/xproto/x11protocol.html, primary, checked 2026-10-19
  - supports: DM-080 (protocol over any reliable byte stream; server-side resources; "mechanism, not policy")
[S835] Robert W. Scheifler, Jim Gettys, The X Window System (pre-publication draft of ACM TOG 5(2) 1986 paper), datamuseum.dk archive, 1986, https://datamuseum.dk/aa/dkuug/b0/b0c8c354e.html, primary, checked 2026-10-19
  - supports: DM-079 (access control only at connection time; per-resource control judged unnecessary)
  - supports: DM-080 (network transparency goal; server-side rendering to save bandwidth; async protocol for 5–50 ms RTT)
  - supports: DM-083 (server not required to retain window contents; 1 MB per full-screen image; clients redraw on exposure)
[S836] Manuel Stoeckl (author), waypipe(1), Arch Linux manual pages, n.d. (current), https://man.archlinux.org/man/waypipe.1.en, primary, checked 2026-10-19
  - supports: DM-080 (Wayland remoting via proxy with lz4/zstd and video encoding; security caveats)
  - supports: DM-084 (does not filter protocols, e.g. compositor screenshot protocols)
[S837] Microsoft, MS Windows NT Kernel-mode User and GDI White Paper, Microsoft TechNet (archived on Microsoft Learn), 1996, https://learn.microsoft.com/en-us/previous-versions//cc750820(v=technet.10)?redirectedfrom=MSDN, primary, checked 2026-10-19
  - supports: DM-081 (NT 4.0 moved USER/GDI/drivers to kernel; 60–70 µs vs 4–5 µs transitions; 256 KB–1 MB savings; CSRSS crash already fatal; claimed no security change)
[S838] Microsoft, PROCESS_MITIGATION_SYSTEM_CALL_DISABLE_POLICY structure, Microsoft Learn, 2018, https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-process_mitigation_system_call_disable_policy, primary, checked 2026-10-19
  - supports: DM-081 (DisallowWin32kSystemCalls; Windows 8 minimum)
[S839] Chromium authors, Sandbox (design document, docs/design/sandbox.md), Chromium source, n.d. (current), https://chromium.googlesource.com/chromium/src/+/main/docs/design/sandbox.md, primary, checked 2026-10-19
  - supports: DM-081 (renderers use Win32k lockdown; "significantly reduces the kernel attack surface")
[S840] James Forshaw, Breaking the Chain, Google Project Zero blog, 2016, https://projectzero.google/2016/11/breaking-chain.html, primary, checked 2026-10-19
  - supports: DM-081 (win32k as sandbox-escape surface; Hacking Team chain; DirectWrite/font/HDCP rework; ~10 months; Win10-only due to Win8.1 instability; Edge Win32k filter)
[S841] MITRE/CVE Program (Microsoft CNA), CVE-2021-1732 (Win32k EoP), CVE record (cveawg API), 2021, https://cveawg.mitre.org/api/cve/CVE-2021-1732, primary, checked 2026-10-19
  - supports: DM-081 (win32k EoP exploited in the wild; in CISA KEV)
[S842] Microsoft, ChangeWindowMessageFilterEx function, Microsoft Learn, 2018 (updated 2025), https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-changewindowmessagefilterex, primary, checked 2026-10-19
  - supports: DM-082 (UIPI blocks lower-IL senders; MSGFLT_ALLOW opt-out; messages < WM_USER always pass)
[S843] Microsoft, Using the UIPIEnableCustomMsgs Fix, Microsoft Learn (archived, Windows 7), 2010, https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-7/dd638320(v=ws.10), primary, checked 2026-10-19
  - supports: DM-082 (UIPI filter semantics; compatibility shim to re-open messages; advice to use other IPC)
[S844] Chris Paget, Shatter attacks announcement (bugtraq post), Bugtraq (MIT archive), 2002, https://diswww.mit.edu/menelaus/bt/26552, primary, checked 2026-10-19
  - supports: DM-082 (new attack class against Win32 messaging; present since NT 3.1; claimed unfixable by patch)
[S845] Microsoft, Impact of Session 0 Isolation on Services and Drivers in Windows, Microsoft Learn (archived), 2006 (page 2017), https://learn.microsoft.com/en-us/previous-versions/windows/hardware/design/dn653293(v=vs.85), primary, checked 2026-10-19
  - supports: DM-082 (pre-Vista services shared Session 0 with apps: security risk; Vista isolates services)
[S846] Microsoft Security Response Center, Microsoft Security Servicing Criteria for Windows, Microsoft, n.d. (current), https://www.microsoft.com/en-us/msrc/windows-security-servicing-criteria, primary, checked 2026-10-19
  - supports: DM-082 (UAC is defense-in-depth, not a serviced boundary; integrity levels not listed as boundaries)
[S847] Keith Packard, Implementing Vblank Synchronization in the Present Extension, keithp.com, 2013, https://keithp.com/blogs/present_sync/, primary, checked 2026-10-19
  - supports: DM-083 (Present design accepts occasional tearing to avoid blocking)
[S848] Jake Edge, DRI3 and Present, LWN.net, 2013, https://lwn.net/Articles/569265, secondary, checked 2026-10-19
  - supports: DM-083 (Present as VBLANK-synchronized retrofit; DRI2 pain)
[S849] Wayland project, Wayland protocol specification, Appendix A (wl_surface), freedesktop.org, n.d. (current), https://wayland.freedesktop.org/docs/html/apa.html, primary, checked 2026-10-19
  - supports: DM-083 (pending/double-buffered state applied on commit; frame callbacks)
[S850] Michel Dänzer, Ensuring steady frame rates with GPU-intensive clients, GNOME Shell & Mutter developers blog, 2023, https://blogs.gnome.org/shell-dev/?author=423, primary, checked 2026-10-19
  - supports: DM-083 ("every frame is perfect" motto; transactions; GPU-late buffers cause missed refresh; ~2 years to fix in Mutter)
[S851] Xaver Hugl (protocol author); wayland.app (renderer), tearing-control-v1 protocol, wayland.app, 2021 (protocol), https://wayland.app/protocols/tearing-control-v1, primary, checked 2026-10-19
  - supports: DM-083 (opt-in async/tearing for latency; compositor may ignore)
  - supports: DM-084 (support table: listed in KWin, Sway, Weston etc., not Mutter — support table is the site's compilation, treat as secondary)
[S852] wayland-protocols project, wayland-protocols GOVERNANCE.md (AOSP mirror, commit 8763980d), android.googlesource.com, n.d., https://android.googlesource.com/platform/external/wayland-protocols/+/8763980daa8a599e5735e32656824874862fb757/freedesktop.org/GOVERNANCE.md, primary, checked 2026-10-19
  - supports: DM-084 (project membership; wp/xdg/ext namespaces; 3 ACKs and single-NACK veto for xdg/wp; implementation requirements)
[S853] Chris Paget, Shatter Attacks – How to break Windows (white paper), security.tombom.co.uk, 2002, http://security.tombom.co.uk/shatter.html, primary, checked 2026-10-20, unverified (host no longer resolves; archive.org not reachable by this agent)
  - supports: DM-082 (technical mechanism of shatter attacks; not fetched — web.archive.org unavailable to this agent)
[S854] MITRE/CVE Program, CVE-2002-1230 (NetDDE Agent WM_COPYDATA + WM_TIMER shatter attack), CVE record (cveawg API), 2002 (published 2004), https://cveawg.mitre.org/api/cve/CVE-2002-1230, primary, checked 2026-10-20
  - supports: DM-082 (concrete shatter mechanism: WM_COPYDATA then WM_TIMER -> code exec as LocalSystem; MS02-071)
[S855] Joshua Ashton (misyltoad), frog-protocols README, GitHub, 2024, https://github.com/misyltoad/frog-protocols, primary, checked 2026-10-20
  - supports: DM-084 (protocols wait "months, to years"; parallel fast-iteration repository; frog-fifo-v1 for VSync/GPU starvation)
[S856] gihyo.jp Linux Daily Topics, Frog Protocols announcement report (Japanese), gihyo.jp, 2024, https://gihyo.jp/article/2024/10/daily-linux-241003, secondary, checked 2026-10-20
  - supports: DM-084 (launch 2024-09-24 by Valve engineer; Blumenkrantz "experimental" stage proposal; objections to bypassing upstream consensus)
[S857] wayland-protocols project; wayland.app (renderer), xdg-decoration-unstable-v1 protocol, wayland.app, n.d., https://wayland.app/protocols/xdg-decoration-unstable-v1, primary, checked 2026-10-20
  - supports: DM-084 (compositor may override client's decoration mode; still unstable; support table: not in Mutter/Weston — table is the site's compilation, treat as secondary)
[S858] Jonathan S. Shapiro, John Vanderburgh, Eric Northup, David Chizmadia, Design of the EROS Trusted Window System (abstract page), USENIX Security Symposium, 2004, https://www.usenix.org/legacy/event/sec04/tech/shapiro.html, primary, checked 2026-10-20
  - supports: DM-079 (window system is an IPC/trust mediator; X11 and Windows inherit Alto-era trust assumptions; EWS < 4,500 lines, ~10x smaller than Trusted X)
  - supports: DM-082 (window system as IPC mechanism relevant to security policy)
  - note: full PDF (https://usenix.org/event/sec04/tech/full_papers/shapiro/shapiro.pdf) was fetched but could not be text-extracted; only abstract-level claims are used.
[S900] D. M. Ritchie, K. Thompson, The UNIX Time-Sharing System, Communications of the ACM 17(7), 1974 (electronic recreation by E. Brewer), https://pdos.csail.mit.edu/6.1810/2004/readings/ritchie74unix.pdf, primary, checked 2026-10-19
  - supports: GR-001 (compatible file/device/IPC I/O; fds; std fds 0/1; small-system goals)
  - supports: GR-002 (pipes; filters rationale; pipe needs common ancestor)
  - supports: GR-006 (rooted directory tree, mountable volumes)
[S904] Microsoft, About Handles and Objects, Microsoft Learn (Win32), 2018 (updated 2025), https://learn.microsoft.com/en-us/windows/win32/sysinfo/about-handles-and-objects, primary, checked 2026-10-19
  - supports: GR-003 (objects behind handles; ACL checked on handle creation)
[S905] Microsoft, Windows Kernel-Mode Object Manager, Microsoft Learn (drivers), 2023, https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/windows-kernel-mode-object-manager, primary, checked 2026-10-19
  - supports: GR-003 (25+ object types; object manager duties)
  - supports: GR-006 (NT object namespace is hierarchical)
[S906] Microsoft, Access Rights and Access Masks, Microsoft Learn (Win32), 2025, https://learn.microsoft.com/en-us/windows/win32/secauthz/access-rights-and-access-masks, primary, checked 2026-10-19
  - supports: GR-003 (access mask requested at open)
[S907] J. Forshaw, Windows 10^H^H Symbolic Link Mitigations, Google Project Zero blog, 2015, https://projectzero.google/2015/08/windows-10hh-symbolic-link-mitigations.html, primary, checked 2026-10-19
  - supports: GR-003 (object-manager symlink planting bug class; CVE-2015-2428 mitigation)
[S908] Microsoft, UpdateProcThreadAttribute function, Microsoft Learn (Win32 API), 2021 (updated 2025), https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute, primary, checked 2026-10-19
  - supports: GR-003 (explicit handle list, AppContainer, mitigation attributes)
  - supports: GR-005 (extensible CreateProcess attribute list since Vista)
[S911] Fuchsia project, Zircon Handles, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/kernel/handles, primary, checked 2026-10-19
  - supports: GR-001 (handle values reused; per-process)
  - supports: GR-002 (channels carry handles)
  - supports: GR-003 (rights reduction, transfer moves handle)
  - supports: GR-008 (rights, duplicate/replace, in-transit handles)
  - supports: GR-018 (Zircon channels as asynchronous contrast)
  - supports: Fuchsia (handles with rights, per-process, transferred over channels, rights reducible on duplication)
[S912] R. C. Daley, P. G. Neumann, A General-Purpose File System For Secondary Storage, AFIPS Fall Joint Computer Conference, 1965, https://multicians.org/fjcc4.html, primary, checked 2026-10-19
  - supports: GR-006 (tree of directories, path names, links, per-branch ACLs)
[S913] M. Seltzer, N. Murphy, Hierarchical File Systems are Dead, HotOS 2009 (USENIX), 2009, https://www.usenix.org/legacy/event/hotos09/tech/full_papers/seltzer/seltzer.pdf, primary, checked 2026-10-19
  - supports: GR-006 (critique: hierarchy ill-suited to large personal data; tag/search proposal)
[S914] Q. Clark, WinFS Update, Microsoft WinFS team blog (archived on Microsoft Learn), 2006, https://learn.microsoft.com/en-nz/archive/blogs/winfs/winfs-update, primary, checked 2026-10-19
  - supports: GR-006 (WinFS not shipped separately; folded into SQL Server/ADO.NET)
[S915] A. Bensoussan, C. T. Clingen, R. C. Daley, The Multics Virtual Memory: Concepts and Design, Communications of the ACM 15(5), 1972, https://multicians.org/multics-vm.html, primary, checked 2026-10-19
  - supports: GR-007 (segments, demand paging, sharing without copying, "no longer uses files")
[S916] A. Crotty, V. Leis, A. Pavlo, Are You Sure You Want to Use MMAP in Your Database Management System?, CIDR 2022, 2022, https://db.cs.cmu.edu/papers/2022/cidr2022-p13-crotty.pdf, primary, checked 2026-10-19
  - supports: GR-007 (mmap problems: transactional safety, I/O stalls, SIGBUS error handling, TLB shootdowns)
[S918] G. Klein et al., seL4: Formal Verification of an OS Kernel, SOSP 2009, 2009, https://www.sigops.org/s/conferences/sosp/2009/papers/klein-sosp09.pdf, primary, checked 2026-10-19
  - supports: GR-020 (8,700 lines C; ~20 py proof, 11 py seL4-specific; assumptions: compiler, asm, hardware)
[S919] seL4 Foundation, Frequently Asked Questions on seL4, sel4.systems, n.d., https://sel4.systems/About/FAQ.html, primary, checked 2026-10-19
  - supports: GR-020 (proof scope per arch; assumptions; DMA; timing channels excluded; multicore not verified)
  - supports: GR-008 (all operations need a capability; user-level resource management via Untyped)
  - supports: GR-018 (seL4 IPC performance claim)
[S920] seL4 Foundation, Verified Configurations, docs.sel4.systems, n.d., https://docs.sel4.systems/projects/sel4/verified-configurations.html, primary, checked 2026-10-19
  - supports: GR-020 (AArch64 functional correctness + integrity + confidentiality; no binary proof; SMMU/boot not covered)
  - supports: GR-008 (integrity/confidentiality proofs rest on capability model)
[S921] J. Andronick, seL4 security proofs now complete on AArch64, seL4 Discourse, 2026, https://sel4.discourse.group/t/sel4-security-proofs-now-complete-on-aarch64/1074, primary, checked 2026-10-19
  - supports: GR-020 (AArch64 confidentiality proof completed Aug 2026)
[S923] J. Corbet, Ringing in a new asynchronous I/O API, LWN.net, 2019, https://lwn.net/Articles/776703/, secondary, checked 2026-10-19
  - supports: GR-009 (Linux AIO criticized; io_uring ring design)
[S924] Linux man-pages project, io_uring(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/io_uring.7.html, primary, checked 2026-10-19
  - supports: GR-009 (shared SQ/CQ rings; batching; SQPOLL; syscall cost after Spectre/Meltdown)
[S926] Linux kernel documentation, Reporting regressions ("no regressions" rule), docs.kernel.org, n.d., https://docs.kernel.org/admin-guide/reporting-regressions.html, primary, checked 2026-10-19
  - supports: GR-010 (scope: userland interfaces; performance; exceptions incl. security fixes; internal APIs excluded)
[S927] L. Torvalds, Re: OOM changes in Linux 3.7 lead to udev warnings and total breakage of KDE 3.5.10/TDE, LKML (lkml.iu.edu mirror), 2012, https://lkml.iu.edu/hypermail/linux/kernel/1211.1/02016.html, primary, checked 2026-10-19
  - supports: GR-010 ("We don't break user space"; no time limit)
[S928] J. Corbet, A way out for a.out, LWN.net, 2022, https://lwn.net/Articles/888741/, secondary, checked 2026-10-19
  - supports: GR-010 (decades-obsolete a.out support resisted removal; one user blocked it)
[S929] LWN.net readers (J. Corbet et al.), "Never break userspace" comment thread on "A turning point for CVE numbers", LWN.net, 2024, https://lwn.net/Articles/962531/, weak, checked 2026-10-19
  - supports: GR-010 (practitioner dispute over whether the rule is honored; sysfs/tracepoints contested)
  - supports: GR-013 (dispute over whether tracepoints are stable ABI)
[S930] H. Franke, R. Russell, M. Kirkwood, Fuss, Futexes and Furwocks: Fast Userlevel Locking in Linux, Ottawa Linux Symposium 2002, 2002, https://www.kernel.org/doc/ols/2002/ols2002-pages-479-495.pdf, primary, checked 2026-10-19
  - supports: GR-011 (kernel only on contention; motivation vs SysV semaphores/fcntl)
[S933] Microsoft, WaitOnAddress function, Microsoft Learn (Win32 API), 2024, https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitonaddress, primary, checked 2026-10-19
  - supports: GR-011 (Windows 8+ adopted address-based waiting; 1/2/4/8-byte sizes; spurious wakes)
[S934] J. Corbet, Short subjects: Realtime, Futexes, and ntfs3, LWN.net, 2021, https://lwn.net/Articles/866112/, secondary, checked 2026-10-19
  - supports: GR-011 (futex multiplexer; futex2/futex_waitv for multi-wait)
[S935] Linux kernel documentation, What is RCU? – "Read, Copy, Update", docs.kernel.org, n.d., https://docs.kernel.org/RCU/whatisRCU.html, primary, checked 2026-10-19
  - supports: GR-012 (mechanism; read-mostly fit; update-heavy unsuitable; deferred reclamation; no blocking in readers)
[S936] P. E. McKenney, S. Boyd-Wickizer, J. Walpole, RCU Usage In the Linux Kernel: One Decade Later, technical report, 2012, http://www.rdrop.com/~paulmck/techreports/survey.2012.09.17a.pdf, primary, checked 2026-10-19
  - supports: GR-012 (6,258 uses by 2012; locking still >10x more common; beats rwlock by orders of magnitude on reads)
[S937] B. M. Cantrill, M. W. Shapiro, A. H. Leventhal, Dynamic Instrumentation of Production Systems, USENIX ATC 2004, 2004, https://www.usenix.org/legacy/event/usenix04/tech/general/full_papers/cantrill/cantrill.pdf, primary, checked 2026-10-19
  - supports: GR-013 (zero disabled-probe effect; safety; ~30,000 probes; aggregation)
[S938] eBPF Foundation, What is eBPF?, ebpf.io, n.d., https://ebpf.io/what-is-ebpf/, secondary, checked 2026-10-19
  - supports: GR-013 (verifier checks termination/bounds, not intent; JIT; uses)
[S939] J. Edge, Privilege escalation via eBPF in Linux 4.9 and beyond, LWN.net, 2017, https://lwn.net/Articles/742170/, secondary, checked 2026-10-19
  - supports: GR-013 (eight verifier bugs; arbitrary kernel R/W; disable unprivileged BPF)
[S940] P. Gupta, [PATCH ebpf v3] bpf: Disallow unprivileged bpf by default, LKML (lkml.iu.edu mirror), 2021, https://lkml.iu.edu/hypermail/linux/kernel/2110.3/07085.html, primary, checked 2026-10-19
  - supports: GR-013 (unprivileged BPF default-off due to speculative-execution risk; distros already did so)
[S941] OpenZFS, Checksums and Their Use in ZFS, openzfs.github.io, n.d., https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Data%20Storage/Checksums.html, primary, checked 2026-10-19
  - supports: GR-014 (checksum in block pointer; self-heal with redundancy; scrub; fletcher4/sha256)
[S942] L. N. Bairavasundaram, G. R. Goodson, B. Schroeder, A. C. Arpaci-Dusseau, R. H. Arpaci-Dusseau, An Analysis of Data Corruption in the Storage Stack, FAST 2008 (USENIX), 2008, https://www.usenix.org/legacy/event/fast08/tech/full_papers/bairavasundaram/bairavasundaram.pdf, primary, checked 2026-10-19
  - supports: GR-014 (1.53M drives, 41 months, >400,000 checksum mismatches; silent corruption is real)
[S943] Y. Zhang, A. Rajimwale, A. C. Arpaci-Dusseau, R. H. Arpaci-Dusseau, End-to-end Data Integrity for File Systems: A ZFS Case Study, FAST 2010 (USENIX), 2010, https://www.usenix.org/legacy/event/fast10/tech/full_papers/zhang.pdf, primary, checked 2026-10-19
  - supports: GR-014 (ZFS robust to disk faults, less resilient to memory corruption)
[S944] Btrfs project, Status, btrfs.readthedocs.io, n.d., https://btrfs.readthedocs.io/en/latest/Status.html, primary, checked 2026-10-19
  - supports: GR-014 (RAID5/6 "unstable")
[S945] Btrfs project, Administration (mount options), btrfs.readthedocs.io, n.d., https://btrfs.readthedocs.io/en/latest/Administration.html, primary, checked 2026-10-19
  - supports: GR-014 (nodatacow implies nodatasum for overwrite-heavy workloads)
[S946] R. Pike, K. Thompson, Hello World or Καλημέρα κόσμε or こんにちは 世界, USENIX Winter 1993 (revised), 1993, https://9p.io/sys/doc/utf.pdf, primary, checked 2026-10-19
  - supports: GR-015 (why UTF; FSS-UTF/X/Open modification; ASCII self-representation; error handling; size tradeoffs)
[S947] F. Yergeau, UTF-8, a transformation format of ISO 10646 (RFC 3629), IETF, 2003, https://www.rfc-editor.org/rfc/rfc3629, primary, checked 2026-10-19
  - supports: GR-015 (properties; U+10FFFF limit; overlong/invalid sequence security; normalization caveat)
[S949] Wayland project, Wayland Protocol and Model of Operation (docs ch. 4), wayland.freedesktop.org, n.d., https://wayland.freedesktop.org/docs/html/ch04.html, primary, checked 2026-10-20
  - supports: GR-016 (seat focus; no explicit client grabs; selection only to focused client; security section a stub)
[S950] Flatpak project, org.freedesktop.portal.ScreenCast, xdg-desktop-portal docs, n.d., https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html, primary, checked 2026-10-19
  - supports: GR-016 (screen capture via user-mediated portal + PipeWire)
[S951] Q-Success, Usage statistics of UTF-8 for websites, W3Techs, 2026, https://w3techs.com/technologies/details/en-utf8, secondary, checked 2026-10-20
  - supports: GR-015 (99.1% of websites with known encoding use UTF-8)
[S953] Microsoft, about_Pipelines (PowerShell), Microsoft Learn, n.d., https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_pipelines, primary, checked 2026-10-20
  - supports: GR-002 (object pipelines as alternative to byte streams)
[S954] J. Edge, The status of Wayland security, LWN.net, 2014, https://lwn.net/Articles/589727/, secondary, checked 2026-10-20
  - supports: GR-016 (no input snooping/injection/global grabs; privileged clients unsolved; GEM handle guessing; LD_PRELOAD)
[S955] J. Edge, A Wayland full-screen shell, LWN.net, 2014, https://lwn.net/Articles/617807/, secondary, checked 2026-10-20
  - supports: GR-016 (X11 global namespace: any client reads/moves/injects; Wayland per-client isolation; capture via compositor)
[S958] Android Open Source Project, Virtual A/B overview, source.android.com, n.d., https://source.android.com/docs/core/ota/virtual_ab, primary, checked 2026-10-20
  - supports: GR-017 (space overhead of A/B motivated snapshot-based Virtual A/B; compression 45–55%)
[S959] BlackBerry QNX, Synchronous message passing (QNX Neutrino 7.1 System Architecture), qnx.com, 2026 (page update), https://www.qnx.com/developers/docs/7.1/com.qnx.doc.neutrino.sys_arch/topic/ipc_Sync_messaging.html, primary, checked 2026-10-20
  - supports: GR-018 (Send/Receive/Reply states; direct copy; pulses)
[S960] BlackBerry QNX, Priority inheritance and messages (QNX Neutrino 7.1 System Architecture), qnx.com, 2026 (page update), https://www.qnx.com/developers/docs/7.1/com.qnx.doc.neutrino.sys_arch/topic/ipc_Priority_inheritance_messages.html, primary, checked 2026-10-20
  - supports: GR-018 (server boosted at send time; priority not reverted on reply; can be disabled per channel)
[S961] BlackBerry QNX, Robust implementations with Send/Receive/Reply (QNX Neutrino 7.1 System Architecture), qnx.com, 2026 (page update), https://www.qnx.com/developers/docs/7.1/com.qnx.doc.neutrino.sys_arch/topic/ipc_Robust.html, primary, checked 2026-10-20
  - supports: GR-018 (deadlock-freedom requires send hierarchy rules; MsgDeliverEvent for upward-to-downward notification)
[S962] D. Hildebrand, An Architectural Overview of QNX, USENIX Workshop on Micro-Kernels and Other Kernel Architectures, 1992, https://cseweb.ucsd.edu/~voelker/cse221/papers/qnx-paper92.pdf, primary, checked 2026-10-20
  - supports: GR-018 (message-driven priority inheritance in 1992; POSIX on microkernel)
[S963] R. Mcilroy, J. Sevcik, T. Tebbi, B. L. Titzer, T. Verwaest, Spectre is here to stay: An analysis of side-channels and speculative execution, arXiv:1902.05178, 2019, https://arxiv.org/abs/1902.05178, primary, checked 2026-10-20
  - supports: GR-019 (speculation defeats language-enforced confidentiality in one address space; Chrome moved to process isolation)
  - supports: GR-007 (single-address-space language isolation undermined)
  - supports: Singularity (speculation defeats language-enforced confidentiality; universal read gadget; Chrome moved to process isolation)
[S964] C. Reis, A. Moshchuk, N. Oskov, Site Isolation: Process Separation for Web Sites within the Browser, USENIX Security 2019, 2019, https://www.usenix.org/system/files/sec19-reis.pdf, primary, checked 2026-10-20
  - supports: GR-019 (deployed process-per-site; 9–13% memory overhead, <2.25% page-load latency)
[S965] R. Russell, virtio: Towards a De-Facto Standard For Virtual I/O Devices, ACM SIGOPS Operating Systems Review, 2008, https://ozlabs.org/~rusty/virtio-spec/virtio-paper.pdf, primary, checked 2026-10-20
  - supports: GR-021 (8 virtualization systems each with own drivers; vring transport)
[S966] R. Russell, M. S. Tsirkin, C. Huck, P. Moll (eds.), Virtual I/O Device (VIRTIO) Version 1.0, OASIS Committee Specification 04, 2016, https://docs.oasis-open.org/virtio/virtio/v1.0/virtio-v1.0.html, primary, checked 2026-10-20
  - supports: GR-021 (standard, extensible mechanism; feature bits; looks like physical devices)
[S967] M. Rybczyńska, Hardening virtio, LWN.net, 2021, https://lwn.net/Articles/865216/, secondary, checked 2026-10-20
  - supports: GR-021 (virtio drivers trusted the device; hardening for confidential VMs; performance dispute)
[S968] BlackBerry QNX, Message copying (QNX Neutrino 7.1 System Architecture), qnx.com, 2026 (page update), https://www.qnx.com/developers/docs/7.1/com.qnx.doc.neutrino.sys_arch/topic/ipc_Message_copying.html, primary, checked 2026-10-20
  - supports: GR-018 (POSIX calls like lseek implemented as MsgSend; short messages double-copied as optimization)
[S969] M. Aiken, M. Fähndrich, C. Hawblitzel, G. Hunt, J. Larus, Deconstructing Process Isolation, ACM SIGPLAN Workshop on Memory Systems Performance and Correctness (MSPC), 2006, https://research.cs.wisc.edu/areas/os/Seminar/papers/Deconstructing_Process_Isolation_final.pdf, primary, checked 2026-10-20
  - supports: GR-019 (hardware isolation costs up to 25–33%; software-isolated processes <5% overhead; hardware kept as defense in depth)
[S1000] Tom Van Vleck et al., Myths about Multics, multicians.org, n.d., https://multicians.org/myths.html, primary, checked 2026-10-19
  - supports: Multics (Bell Labs withdrawal not end; ~77 sites; last site 2000; early slowness; 250K-line kernel; Thompson took hierarchical FS; Bull cancellation attributed to politics)
[S1001] F. J. Corbató, V. A. Vyssotsky, Introduction and Overview of the Multics System, AFIPS Fall Joint Computer Conference, 1965, https://multicians.org/fjcc1.html, primary, checked 2026-10-19
  - supports: Multics (design goals: computer utility, segmentation, dynamic linking, hierarchical FS, protection, PL/I portability)
[S1002] Tom Van Vleck (ed.), Multics History, multicians.org, n.d., https://multicians.org/history.html, primary, checked 2026-10-19
  - supports: Multics (645 delays, Bell Labs exit 1969, Honeywell 1970, B2 1985, cancellation 1985, hardware cost, microcomputer shift, internal politics)
[S1003] Paul A. Karger, Roger R. Schell, Thirty Years Later: Lessons from the Multics Security Evaluation, ACSAC 2002, 2002, https://www.gwern.net/doc/cs/security/2002-karger.pdf, primary, checked 2026-10-19
  - supports: Multics (no buffer overflows: PL/I bounded strings, execute bits, upward stacks; trap-door subversion; kernel size vs SELinux policy)
[S1005] Rob Pike, Systems Software Research is Irrelevant, talk at University of Utah, 2000, https://inf.ufes.br/~raulh/ufes/teaching/courses/sc/texts/utah200.pdf, primary, checked 2026-10-19
  - supports: Plan 9 (90-95% of work honoring standards; Unix emulation expected; PC monoculture; funding/startup horizon)
[S1006] Plan 9 authors, Plan 9 from Bell Labs — Overview (releases and licensing), 9p.io, n.d., https://9p.io/plan9/about.html, primary, checked 2026-10-19
  - supports: Plan 9 (edition/licensing history 1992-2021; mostly new software, not ported)
[S1007] Plan 9 Foundation, Plan 9 Download / Foundation home, p9f.org, 2025, https://p9f.org/dl/, primary, checked 2026-10-19
  - supports: Plan 9 (all historical releases re-released under MIT; foundation continues research/community)
[S1008] Rob Pike, UTF-8 history (email to Markus Kuhn), cl.cam.ac.uk, 2003, https://www.cl.cam.ac.uk/~mgk25/ucs/utf-8-history.txt, primary, checked 2026-10-19
  - supports: Plan 9 (UTF-8 designed by Thompson/Pike Sept 1992, Plan 9 first full user)
[S1010] Linux kernel developers, v9fs: Plan 9 Resource Sharing for Linux, docs.kernel.org, 2026, https://docs.kernel.org/filesystems/9p.html, primary, checked 2026-10-19
  - supports: Plan 9 (9P in Linux; virtio transport for QEMU host sharing)
[S1011] Sean Dorward, Rob Pike, David Leo Presotto, Dennis M. Ritchie, Howard Trickey, Phil Winterbottom, The Inferno Operating System, Bell Labs Technical Journal 2(1), 1997, http://www.vitanuova.com/inferno/papers/bltj.html, primary, checked 2026-10-19
  - supports: Plan 9/Inferno (Inferno as OS for distributed services from Bell Labs CSRC)
[S1012] Vita Nuova, Inferno — What is Inferno?, vitanuova.com, n.d., http://www.vitanuova.com/inferno/, primary, checked 2026-10-19
  - supports: Plan 9/Inferno (hosted or native; Limbo; Dis VM; free software)
[S1013] Thom Holwerda, Interview with Vita Nuova CEO Michael Jeffrey, OSNews, 2002, https://www.osnews.com/story/750/interview-with-vita-nuova-ceo-michael-jeffrey/, weak, checked 2026-10-19
  - supports: Plan 9/Inferno (2000 spin-out to Vita Nuova; Java comparison; embedded customer claims — vendor statements)
[S1014] J. Bradley Chen, Brian N. Bershad, The Impact of Operating System Structure on Memory System Performance, SOSP 1993, 1993, https://pages.cs.wisc.edu/~dusseau/Classes/CS736/Papers/chen-bershad.pdf, primary, checked 2026-10-19
  - supports: Mach (Mach 3.0 executes more system instructions, higher MCPI, worse system locality than Ultrix)
[S1015] George Coulouris, Jean Dollimore, Tim Kindberg, Mach Case Study (ch. 18, Distributed Systems 3rd ed.), Pearson / coulouris.net archive, 2001, https://www.coulouris.net/cdk4/wo/additional/oss/Ed3/Mach.pdf, secondary, checked 2026-10-19
  - supports: Mach (design goals; Mach 2.5 had UNIX in kernel; OSF/1 base from 1989 as SVR4 rival; NeXT used older Mach; MkLinux on 3.0)
[S1016] Jay Lepreau, Mike Hibler, Bryan Ford, Jeffrey Law, Douglas Orr, In-Kernel Servers on Mach 3.0: Implementation and Performance, USENIX Mach III Symposium, 1993, https://www.usenix.org/conference/usenix-mach-iii-symposium/kernel-servers-mach-30-implementation-and-performance, primary, checked 2026-10-19
  - supports: Mach (co-location gains: RPC ~3x, syscalls 20%-2x, benchmarks 4-13%)
[S1019] Apple, XNU README, apple-oss-distributions/xnu on GitHub, 2026, https://github.com/apple-oss-distributions/xnu/blob/main/README.md, primary, checked 2026-10-19
  - supports: Mach (XNU hybrid kernel: CMU Mach + FreeBSD + IOKit; osfmk = Mach subsystems)
[S1020] CMU School of Computer Science, Project Mach home page, cs.cmu.edu, 1997, https://www.cs.cmu.edu/afs/cs/project/mach/public/www/mach.html, primary, checked 2026-10-19
  - supports: Mach (project 1985-1994; derivative projects incl. Hurd, Lites, RT-Mach)
[S1022] Gernot Heiser, The seL4 Microkernel — An Introduction (whitepaper rev. 1.4.1), seL4 Foundation, 2026, https://sel4.systems/About/seL4-whitepaper.pdf, primary, checked 2026-10-19
  - supports: L4 (proof scope; timing channels not covered; partial arch coverage; Microkit static architecture; CAmkES/Genode drawbacks; HACMS incremental retrofit)
[S1023] Phoronix (author not checked), "The World's Most Highly-Assured OS" Kernel Open-Sourced, Phoronix, 2014, https://www.phoronix.com/news/MTc1MTg, secondary, checked 2026-10-19
  - supports: L4 (seL4 open-sourced 29 July 2014 by GD C4 Systems and NICTA, GPLv2 + BSD)
[S1024] Gernot Heiser, RIP Open Kernel Labs, Welcome Cog Systems, microkerneldude.org, 2014, https://microkerneldude.org/2014/08/22/rip-open-kernel-labs-welcome-cog-systems/, primary, checked 2026-10-19
  - supports: L4 (GD acquisition of OK Labs; Sydney office closed Feb 2014; Cog Systems spin-out)
[S1025] QNX Software Systems / BlackBerry, QNX Neutrino as a message-passing operating system (System Architecture guide, 7.1), qnx.com, n.d., https://china.qnx.com/developers/docs/7.1/com.qnx.doc.neutrino.sys_arch/topic/intro_Message_passing_OS.html, primary, checked 2026-10-19
  - supports: QNX (message passing as fundamental IPC; services as user processes via POSIX library; Qnet; sync/copy semantics drive scheduling)
[S1026] BlackBerry QNX, QNX SDP 8.0 General Availability Release Notes, qnx.com, 2024, https://www.qnx.com/developers/docs/relnotes8.0/com.qnx.doc.release_notes/topic/sdp8_rn.html, primary, checked 2026-10-19
  - supports: QNX (redesigned scalable preemptible multithreaded kernel, 1-64 cores; ISRs removed; shared sync objects must be declared; mqueue replaced)
[S1027] "brynet" (guest author), QNX6 is Closed Source Once Again, OSNews, 2010, https://www.osnews.com/story/23565/qnx6-is-closed-source-once-again/, secondary, checked 2026-10-19
  - supports: QNX (April 2010 withdrawal of source access to microkernel, userland, libraries, Photon; possibly due to RIM acquisition)
[S1028] The Register (author not checked), BlackBerry tablet runs QNX, The Register, 2010, https://www.theregister.com/2010/09/28/blackberry_tablet_runs_qnx/, secondary, checked 2026-10-19
  - supports: QNX (RIM bought QNX from Harman April 2010 and closed source; PlayBook built on Neutrino)
[S1029] BlackBerry, QNX Embedded Technology Powers 255 Million Vehicles On The Road Today (press release), Newswire, 2024, https://www.newswire.com/news/qnx-embedded-technology-powers-255-million-vehicles-on-the-road-today, primary, checked 2026-10-19
  - supports: QNX (255M vehicles per TechInsights; per-unit royalties; ISO 26262 / ISO 21434)
[S1030] Norman Hardy, The KeyKOS Architecture (8th ed.; orig. Operating Systems Review Sept 1985), Key Logic, 1990, https://pdos.csail.mit.edu/6.828/2009/readings/keykos-osr.pdf, primary, checked 2026-10-19
  - supports: KeyKOS (keys/capabilities; checkpoint of all state; production on 370 since 1983; hosts UNIX, VM/CMS, KeyTXF; 88000 port)
[S1031] Norman Hardy, Notes on Keykos (index), cap-lore.com, n.d., http://cap-lore.com/CapTheory/KK/, primary, checked 2026-10-19 (fetched with curl; WebFetch failed on TLS certificate)
  - supports: KeyKOS (developed at Tymshare from ~1975; EROS/CapROS/Coyotos successors; ancestor of seL4)
[S1032] Norman Hardy, Warts, cap-lore.com, n.d., http://cap-lore.com/CapTheory/KK/Warts.html, primary, checked 2026-10-19 (curl)
  - supports: KeyKOS (single-level store heuristics; real-time batching linear in RAM; abstraction faults; space-bank covert channel)
[S1033] Norman Hardy, Keykos Tradeoffs, cap-lore.com, 2015, http://cap-lore.com/CapTheory/KK/Tradeoffs, primary, checked 2026-10-19 (curl)
  - supports: KeyKOS (objects not extractable from platform; app upgrade issue set aside)
[S1034] Norman Hardy, Commercial Bootstrap, cap-lore.com, n.d., http://cap-lore.com/CapTheory/KK/CBB.html, primary, checked 2026-10-19 (curl)
  - supports: KeyKOS (simple apps better without new OS; value only in community of mutually suspicious interests)
[S1035] Jonathan S. Shapiro, Jonathan M. Smith, David J. Farber, EROS: a fast capability system, SOSP 1999 (OSR 34(5)), 1999, https://www.cs.yale.edu/flint/cs428/doc/eros.pdf, primary, checked 2026-10-19
  - supports: KeyKOS/EROS (third implementation of GNOSIS/KeyKOS; IPC comparable to Linux; periodic checkpoint; checkpoint invalidated by kernel table resize; 88000 POSIX emulation; need for app-level validation)
[S1036] Charles Landau / Strawberry Development Group, CapROS home page, capros.org, 2022, http://www.capros.org/, primary, checked 2026-10-19
  - supports: KeyKOS/EROS (CapROS continues EROS; small persistent real-time OS; small maintainer base)
[S1037] OSNews (author not checked), Jonathan Shapiro of Coyotos, BitC Joins Microsoft, OSNews, 2009, https://www.osnews.com/story/21262/jonathan-shapiro-of-coyotos-bitc-joins-microsoft/, secondary, checked 2026-10-19
  - supports: KeyKOS/EROS (Shapiro ends Coyotos/BitC work to join Midori, 2009)
[S1038] Galen C. Hunt, James R. Larus, Singularity: Rethinking the Software Stack, ACM SIGOPS Operating Systems Review 41(2), 2007, https://pdos.csail.mit.edu/6.828/2008/readings/hunt07singularity.pdf, primary, checked 2026-10-19
  - supports: Singularity (SIPs, channels, manifests; sealed processes; >90% Sing#, GC 48% of unsafe code; unsafe code tax up to 37.7% vs <5% safe-code cost; Bartok in TCB; versioned ABI)
[S1039] OSNews (author not checked), Singularity Source Code Released to CodePlex, OSNews, 2008, https://www.osnews.com/story/19424/singularity-source-code-released-to-codeplex/, secondary, checked 2026-10-19
  - supports: Singularity (RDK released March 2008 for academic non-commercial use only)
[S1041] Joe Duffy, Blogging about Midori, joeduffyblog.com, 2015, http://joeduffyblog.com/2015/11/03/blogging-about-midori/, primary, checked 2026-10-19
  - supports: Midori (scope: language to apps; 2009-2014 timeframe; lessons list; decisions not entirely technical or business; regret not open-sourcing)
[S1042] Joe Duffy, A Tale of Three Safeties, joeduffyblog.com, 2015, http://joeduffyblog.com/2015/11/03/a-tale-of-three-safeties/, primary, checked 2026-10-19
  - supports: Midori (safe whole stack; safety cost within noise; bugs classes eliminated; ideas into C++/.NET slowed by ecosystems)
[S1043] Joe Duffy, Objects as Secure Capabilities, joeduffyblog.com, 2015, http://joeduffyblog.com/2015/11/10/objects-as-secure-capabilities/, primary, checked 2026-10-19
  - supports: Midori (no ambient authority; Clock capability; KeyKOS/EROS/Coyotos influence; capability-bag ergonomics; downcasting; never tested at scale)
  - supports: KeyKOS/EROS (influence on Midori)
[S1044] David Worthington, Midori concepts materialize in .NET, SD Times, 2010, https://sdtimes.com/microsoft/midori-concepts-materialize-in-net, secondary, checked 2026-10-19
  - supports: Midori (analysts link .NET concurrency to Midori; Shapiro left March 2010; "one of many incubation projects")
[S1045] Nick Heer, Whatever Happened to Midori? (link post to Mary Jo Foley, ZDNet), Pixel Envy, 2015, https://pxlnv.com/linklog/whatever-happened-to-midori/, weak, checked 2026-10-19
  - supports: Midori (reported shut down internally, 2015; pointer to ZDNet article which could not be fetched)
[S1046] Norman Hardy, Some Reflections on Keykos (Retrospective), cap-lore.com, ~2010, http://cap-lore.com/CapTheory/KK/Retrospective.html, primary, checked 2026-10-19 (curl)
  - supports: KeyKOS (message primitives never needed critical change; no Internet interface built)
  - Mary Jo Foley, Whatever happened to Microsoft's Midori operating system project?, ZDNet, 2015, https://www.zdnet.com/article/whatever-happened-to-microsofts-midori-operating-system-project/ — fetch blocked, unverified
  - eWeek, Microsoft Midori: More Details on the Non-Windows OS Emerge, https://www.eweek.com/enterprise-apps/microsoft-midori-more-details-on-the-non-windows-os-emerge.html — 403, unverified
  - Redmond Magazine, Will Microsoft Take a Sip of Midori, 2008 — 403, unverified
  - Accetta et al., Mach: A New Kernel Foundation for UNIX Development, USENIX Summer 1986 — TLS/403 on all mirrors, unverified
  - Dr. Dobb's Dec 1992 GNOSIS/Key Logic history (archive.retro.co.za) — 402, unverified
[S1100] Baumann, Barham, Dagand, Harris, Isaacs, Peter, Roscoe, Schüpbach, Singhania, The Multikernel: A new OS architecture for scalable multicore systems, SOSP 2009, 2009, https://www.sigops.org/s/conferences/sosp/2009/papers/baumann-sosp09.pdf, primary, checked 2026-10-19
  - supports: Barrelfish (per-core kernels, message passing, replicated state; evaluation scope limited; queueing sensitivity; cache-coherence lessons)
[S1101] Barrelfish project, Barrelfish home page, ETH Zurich, 2020 (last release 2020-03-23), https://barrelfish.org/, primary, checked 2026-10-19
  - supports: Barrelfish (project marked no longer active; last release 2020)
[S1102] Andrew Baumann, Fish in a Barrel: an insider's retrospective of the SOSP'09 multikernel paper, ACM SIGOPS blog, 2021, https://www.sigops.org/2021/fish-in-a-barrel-an-insiders-retrospective-of-the-sosp09-multikernel-paper/, primary, checked 2026-10-19
  - supports: Barrelfish (core-count prediction wrong; heterogeneity mattered more; seL4 capability model a distraction; follow-on systems Arrakis, Popcorn, LegoOS, LXDs)
[S1103] Fuchsia project, Zircon (kernel concepts), fuchsia.dev, 2025, https://fuchsia.dev/fuchsia-src/concepts/kernel, primary, checked 2026-10-19
  - supports: Fuchsia (Zircon scope, ~100 syscalls, mostly non-blocking; larger OS built on top)
[S1105] Fuchsia project, Starnix, fuchsia.dev, 2025, https://fuchsia.dev/fuchsia-src/concepts/starnix, primary, checked 2026-10-19
  - supports: Fuchsia (Starnix runs unmodified Linux binaries by implementing Linux ABI in a userspace program)
[S1106] Adam Barth, RFC-0082: Running unmodified Linux programs on Fuchsia, fuchsia.dev, 2021, https://fuchsia.dev/fuchsia-src/contribute/governance/rfcs/0082_starnix, primary, checked 2026-10-19
  - supports: Fuchsia (motivation: POSIX Lite insufficient, Android binaries cannot be recompiled; alternatives VM/in-process/in-kernel; exception-based trapping expected too slow; WSL1 filesystem warning; futex/signal/inotify mismatches)
[S1107] Kyle Bradshaw, Google abandons work to move Assistant smart speakers to Fuchsia, 9to5Google, 2023, https://9to5google.com/2023/07/25/google-abandons-assistant-speakers-fuchsia/, secondary, checked 2026-10-19
  - supports: Fuchsia (speaker port abandoned; >16-20% of ~400-person team laid off; Nest Hubs keep Fuchsia)
[S1108] Thom Holwerda, Google is bringing Fuchsia OS to Android devices, but not in the way you'd think, OSNews, 2024, https://www.osnews.com/story/140162/google-is-bringing-fuchsia-os-to-android-devices-but-not-in-the-way-youd-think/, secondary, checked 2026-10-19
  - supports: Fuchsia (microfuchsia under pKVM/QEMU; ambitions faded after layoffs — opinion)
[S1109] Kishan Vyas, Google's Fuchsia OS is making its long-awaited debut on the Nest Hub, XDA Developers, 2021, https://www.xda-developers.com/google-fuchsia-os-debut-nest-hub/, secondary, checked 2026-10-19
  - supports: Fuchsia (Cast OS replaced by Fuchsia on 1st-gen Nest Hub, May–Aug 2021, invisible to users)
[S1110] Genode Labs, Genode Foundations 26.05 — Introduction, genode.org, 2026, https://genode.org/documentation/genode-foundations/26.05/introduction/index.html, primary, checked 2026-10-19
  - supports: Genode (goals: TCB size, recursive component tree, capabilities, resource budgets and trading)
[S1111] Genode Labs, Genode Foundations 26.05 — Licensing and commercial support, genode.org, 2026, https://genode.org/documentation/genode-foundations/26.05/introduction/Licensing_and_commercial_support.html, primary, checked 2026-10-19
  - supports: Genode (AGPLv3 plus linking clause; commercial licence from Genode Labs)
[S1112] Genode Labs, Open-source license update, genode.org news, 2017, https://genode.org/news/open-source-license-update, primary, checked 2026-10-19
  - supports: Genode (GPLv2 2008 → AGPLv3 from 17.02; reasons)
[S1113] Genode Labs, Release notes for the Genode OS Framework 21.08, genode.org, 2021, https://genode.org/documentation/release-notes/21.08, primary, checked 2026-10-19
  - supports: Genode (dependence on ported Linux drivers; per-driver port cost, e.g. two months for i.MX8 graphics; old DDE approach unsustainable for ARM)
[S1114] Genode Labs, Road map, genode.org, 2026, https://genode.org/about/road-map, primary, checked 2026-10-19
  - supports: Genode (2026 goals; Sculpt status; progress depends on community support; XML→HID migration)
[S1115] Redox OS project, The Redox Operating System (book), redox-os.org, 2026, https://doc.redox-os.org/book/, primary, checked 2026-10-19
  - supports: Redox (Rust microkernel, Unix-like, modest POSIX compatibility, started 2015 as unikernel, schemes)
[S1116] Redox OS project, Comparing Redox to Other OSes, redox-os.org, 2026, https://doc.redox-os.org/book/how-redox-compares.html, primary, checked 2026-10-19
  - supports: Redox (code moved to userspace not removed; syscall interface unstable; POSIX via relibc)
[S1117] Ron Williams, Development Priorities for 2025/26, redox-os.org news, 2025, https://www.redox-os.org/news/development-priorities-2025-09/, primary, checked 2026-10-19
  - supports: Redox (funding by donations and NLnet/NGI grants; driver gaps — no WiFi, limited USB, no IOMMU; no Wayland; self-hosting incomplete; full POSIX not a goal)
[S1118] Ribbon, Ron Williams, This Month in Redox — August 2026, redox-os.org news, 2026, https://www.redox-os.org/news/this-month-260831/, primary, checked 2026-10-19
  - supports: Redox (2026 status: daily images, ring-buffer I/O API gave ~14-15x NVMe gain, kernel leak made GCC test build take ~10h, ARM64 SMP)
[S1119] Haiku project, About Haiku, haiku-os.org, 2026, https://www.haiku-os.org/about/, primary, checked 2026-10-19
  - supports: Haiku (goals; single team writes kernel to apps; threaded design; BFS indexed metadata)
[S1120] Haiku project, History, haiku-os.org, 2026, https://www.haiku-os.org/about/history/, primary, checked 2026-10-19
  - supports: Haiku (OpenBeOS started 2001; renamed 2004; alpha 2009; beta1 2018)
[S1121] Haiku project, Haiku R1/beta6 has been released!, haiku-os.org news, 2026, https://www.haiku-os.org/news/2026-08-26_haiku_r1_beta6/, primary, checked 2026-10-19
  - supports: Haiku (beta6 August 2026, two years after beta5; still no R1 final)
[S1122] Haiku, Inc., Haiku, Inc. Financial Report for 2024 is now available, haiku-os.org news, 2025, https://www.haiku-os.org/news/2025-02-01_2024_financial_report/, primary, checked 2026-10-19
  - supports: Haiku (record donations; one paid contract developer)
[S1123] Matt Berger (IDG News Service), Be files antitrust suit against Microsoft, Computerworld, 2002, https://www.computerworld.com/article/1354584/be-files-antitrust-suit-against-microsoft.html, secondary, checked 2026-10-19
  - supports: Haiku/BeOS (Hitachi dual-boot deal collapsed over Windows OEM terms; Be sold assets to Palm)
[S1124] Andrew Orlowski, Fireworks cancelled as Be, Microsoft settle, The Register, 2003, https://www.theregister.com/2003/09/09/fireworks_cancelled_be_microsoft_settle/, secondary, checked 2026-10-19
  - supports: Haiku/BeOS (settlement ~$23.5M, no admission of wrongdoing)
[S1125] Michael Arrington, What If Steve Jobs Hadn't Returned To Apple In 1997?, TechCrunch, 2009, https://techcrunch.com/?p=123523, weak, checked 2026-10-19
  - supports: Haiku/BeOS (Apple–Be talks failed over price; Apple bought NeXT ~$400M — opinion piece, figures rumoured)
[S1126] Kevin Boos, Namitha Liyanage, Ramla Ijaz, Lin Zhong, Theseus: an Experiment in Operating System Structure and State Management, OSDI 2020, 2020, https://www.usenix.org/system/files/osdi20-boos.pdf, primary, checked 2026-10-19
  - supports: Theseus (SAS/SPL; cells; intralingual design; ~38k LoC, ~4 person-years; 31% of manifested faults unrecovered due to no async unwinding; limitations: unsafe code, legacy/unsafe languages need hardware isolation, spillful POSIX FS)
[S1127] Theseus OS project, Theseus README, GitHub, 2024, https://github.com/theseus-os/Theseus, primary, checked 2026-10-19
  - supports: Theseus (self-described experimental, not mature)
[S1128] Theseus OS project, commit history (GitHub API, theseus_main), GitHub, 2024, https://api.github.com/repos/theseus-os/Theseus/commits?per_page=3, primary, checked 2026-10-19
  - supports: Theseus (most recent commit 2024-09-22: dormant for ~2 years as of 2026-10)
[S1129] Oxide Computer Company, Hubris Reference, hubris.oxide.computer, 2026, https://hubris.oxide.computer/reference/, primary, checked 2026-10-19
  - supports: Hubris (static task set; sync IPC + leases; supervisor restart; generation numbers; no capabilities yet; whole-image updates; no shared code)
[S1130] Cliff Biffle, On Hubris and Humility (OSFC 2021 talk transcript), cliffle.com, 2021, https://cliffle.com/blog/on-hubris-and-humility/, primary, checked 2026-10-19
  - supports: Hubris (why built; evaluated systems lacked memory protection or had serious problems; tradeoffs: sync vs throughput, one outstanding message; service processor deployment)
[S1131] Oxide Computer Company, Hubris commit history (GitHub API), GitHub, 2026, https://api.github.com/repos/oxidecomputer/hubris/commits?per_page=2, primary, checked 2026-10-19
  - supports: Hubris (active development, commits October 2026)
[S1132] Jorrit N. Herder, Herbert Bos, Ben Gras, Philip Homburg, Andrew S. Tanenbaum, Reorganizing UNIX for Reliability, ACSAC 2006 (Asia-Pacific Computer Systems Architecture Conf., LNCS), 2006, https://www.minix3.org/doc/ACSAC-2006.pdf, primary, checked 2026-10-20
  - supports: MINIX 3 (user-mode drivers/servers; reincarnation server; 5–10% overhead; recovery limited to drivers, PM/FS crash not recoverable; transient-fault assumption; 75,000 downloads)
[S1133] MINIX 3 project, MINIX 3 home page, minix3.org, 2026 (undated content), https://www.minix3.org/, primary, checked 2026-10-20
  - supports: MINIX 3 (download still points to 3.3.0; no news)
[S1134] Stichting MINIX Research Foundation, minix repository commits and tags (GitHub API), GitHub, 2018, https://api.github.com/repos/Stichting-MINIX-Research-Foundation/minix/commits?per_page=3, primary, checked 2026-10-20
  - supports: MINIX 3 (last commits November 2018; latest tag v3.3.0)
[S1135] Andrew S. Tanenbaum, An Open Letter to Intel, cs.vu.nl, 2017 (undated page; links to 2017 coverage), https://www.cs.vu.nl/~ast/intel/, primary, checked 2026-10-20
  - supports: MINIX 3 (MINIX runs in Intel ME-11; BSD licence attracted Intel; Intel gave no notice)
[S1136] A.S. Tanenbaum, R. van Renesse, H. van Staveren, G.J. Sharp, S.J. Mullender, A.J. Jansen, G. van Rossum, Experiences with the Amoeba distributed operating system, CWI report CS-R8929 (later CACM Dec 1990), 1989, https://ir.cwi.nl/pub/5841/5841D.pdf, primary, checked 2026-10-20
  - supports: Amoeba (processor pool; sparse cryptographic capabilities with rights; RPC point-to-point limits; non-preemptive threads "worst mistake"; immutable bullet file server; UNIX emulation via V7/MINIX server and Ajax library)
[S1137] Vrije Universiteit, The Amoeba Distributed Operating System (project page), cs.vu.nl, 1998, https://www.cs.vu.nl/pub/amoeba/, primary, checked 2026-10-20
  - supports: Amoeba (latest release 5.3; page last updated 1998)
[S1138] Python Software Foundation, General Python FAQ ("Why was Python created in the first place?"), docs.python.org, 2026, https://docs.python.org/3/faq/general.html, primary, checked 2026-10-20
  - supports: Amoeba (Python created for Amoeba sysadmin because syscalls not reachable from Bourne shell)
[S1139] John Ousterhout, A Brief Retrospective on the Sprite Network Operating System, Stanford/UC Berkeley, c.1994, https://web.stanford.edu/~ouster/cgi-bin/spriteRetrospective.php, primary, checked 2026-10-20
  - supports: Sprite (goals; caching FS; migration; single system image; LFS; wind-down decision late 1991; ~10 outside sites; vendors outpaced; all-or-nothing adoption; ideas reached NetApp/flash/VMware)
[S1140] Frederick Douglis, Transparent Process Migration in the Sprite Operating System (PhD thesis, UCB/CSD-90-598), UC Berkeley, 1990, https://www2.eecs.berkeley.edu/Pubs/TechRpts/1990/6382.html, primary, checked 2026-10-20
  - supports: Sprite (31% of processing via migration; evictions in seconds; 3–6x compile speedups limited by file server)
[S1141] Dawson R. Engler, M. Frans Kaashoek, James O'Toole Jr., Exokernel: An Operating System Architecture for Application-Level Resource Management, SOSP 1995, 1995, https://pdos.csail.mit.edu/6.828/2008/readings/engler95exokernel.pdf, primary, checked 2026-10-20
  - supports: Exokernel (secure bindings, visible revocation, abort protocol; libOS; Aegis primitives 10–100x faster than Ultrix)
[S1142] M. Frans Kaashoek, Dawson R. Engler, Gregory R. Ganger, Héctor M. Briceño, Russell Hunt, David Mazières, Thomas Pinckney, Robert Grimm, John Jannotti, Kenneth Mackenzie, Application Performance and Flexibility on Exokernel Systems, SOSP 1997, 1997, https://pdos.csail.mit.edu/papers/exo-sosp97/exo-sosp97.html, primary, checked 2026-10-20
  - supports: Exokernel (Xok/ExOS results vs BSD; Cheetah 8x; gains mostly from C-FFS; shared-state protection hard, unprotected tables; XN four designs; slow fork; information loss; unstable libOS API)
[S1144] Niklaus Wirth, Jürg Gutknecht, Project Oberon: The Design of an Operating System, a Compiler, and a Computer (Revised Edition 2013), ETH Zurich, 2013, https://people.inf.ethz.ch/wirth/ProjectOberon/PO.System.pdf, primary, checked 2026-10-20
  - supports: Oberon (1986–89 goal of whole-system understandability; single-process central loop, no protection mechanisms; GC; module sizes ~4,600 lines core; 2013 preface: competing with standards "preposterous")
[S1145] Niklaus Wirth, Project Oberon (New Edition 2013) index page, ETH Zurich, 2013, https://people.inf.ethz.ch/wirth/ProjectOberon/index.html, primary, checked 2026-10-20
  - supports: Oberon (OS, compiler and RISC5 Verilog for FPGA published together)
[S1146] Rob Pike, Acme: A User Interface for Programmers, Proc. Winter 1994 USENIX Conf., 1994, https://9p.io/sys/doc/acme/acme.html, primary, checked 2026-10-20
  - supports: Oberon (Acme/Help UI derived from Oberon; Oberon criticized as single-process with weak networking)

## Merged duplicate IDs

- S209 → S127
- S229 → S155
- S232 → S152
- S347 → S118
- S356 → S236
- S412 → S220
- S423 → S116
- S464 → S110
- S465 → S302
- S511 → S315
- S543 → S234
- S545 → S236
- S623 → S155
- S624 → S161
- S626 → S156
- S629 → S533
- S637 → S139
- S648 → S318
- S723 → S355
- S728 → S613
- S802 → S154
- S901 → S109
- S902 → S100
- S903 → S161
- S909 → S102
- S910 → S103
- S917 → S202
- S922 → S417
- S925 → S220
- S931 → S367
- S932 → S368
- S948 → S829
- S952 → S653
- S956 → S146
- S957 → S134
- S1004 → S625
- S1009 → S162
- S1017 → S306
- S1018 → S304
- S1021 → S305
- S1040 → S963
- S1104 → S911
- S1143 → S456
