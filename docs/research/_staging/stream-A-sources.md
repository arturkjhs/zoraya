# Stream A sources (S100–S199)

Years marked n.d. are living documents with no publication year shown on the page; the online version as of the check date was used.

[S100] Andrew Baumann, Jonathan Appavoo, Orran Krieger, Timothy Roscoe, A fork() in the road, HotOS '19 (ACM), 2019, https://www.microsoft.com/en-us/research/wp-content/uploads/2019/04/fork-hotos19.pdf, primary, checked 2026-10-19
  - supports: DM-001 (fork history and Ritchie quotes; thread-unsafety, insecurity, ASLR, cost numbers, overcommit, composability, K42 experience, spawn/cross-process alternatives, snapshot use cases)
[S101] Mairin Duffy (ed.), Ulrich Drepper, Orran Krieger, A fork() in the road – conversation between Uli Drepper & Orran Krieger, Boston University Red Hat Collaboratory blog, 2019, https://www.bu.edu/rhcollab/2019/04/11/a-fork-in-the-road, secondary, checked 2026-10-19
  - supports: DM-001 (Drepper's dissent: fork+exec bad, but fork/clone worth keeping; no consensus)
[S102] The Open Group / IEEE, posix_spawn, posix_spawnp — spawn a process (Base Specifications Issue 8, IEEE Std 1003.1-2024), The Open Group, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_spawn.html, primary, checked 2026-10-19
  - supports: DM-001 (rationale: fork hard without MMU; limits: no arbitrary child code, exit-127 error reporting, optional feature)
  - supports: DM-002 (file actions give explicit child fd table; FD_CLOEXEC/FD_CLOFORK handling)
  - supports: DM-008 (ARG_MAX limit on args plus environment)
[S103] Fuchsia project, Process creation, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/process/process_creation, primary, checked 2026-10-19
  - supports: DM-001 (launcher builds process from caller-supplied kernel objects; job policy forbids direct construction)
  - supports: DM-002 (child receives only the handles it is given)
  - supports: DM-003 (processes reached through handles/kernel objects)
  - supports: DM-004 (distinct process objects with startup message; thread split [weak support])
  - supports: DM-005 (process lifetime via handles)
  - supports: DM-008 (handles passed at spawn)
  - supports: DM-100 (process creation scoped by job policy)
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
[S110] Linux man-pages project, pidfd_open(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/pidfd_open.2.html, primary, checked 2026-10-19
  - supports: DM-003 (5.3; conditions under which PIDs are not recycled; CLONE_PIDFD; setns/pidfd_getfd/process_madvise uses)
  - supports: DM-005 (pollable exit; waitid only for own children)
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
[S117] Microsoft, About Processes and Threads, Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/win32/procthread/about-processes-and-threads, primary, checked 2026-10-19
  - supports: DM-004 (NT process as resource container, thread as schedulable unit, per-thread security context)
[S118] Linux man-pages project, pthreads(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/pthreads.7.html, primary, checked 2026-10-19
  - supports: DM-004 (POSIX process-wide vs per-thread attributes; LinuxThreads nonconformances; NPTL fixes through 2.6.16)
  - supports: DM-006 (cwd, umask, dispositions required to be process-wide)
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
[S153] Christian Brauner, [PATCH v3 1/5] cgroup: introduce cgroup.kill (patch posting mirrored on LWN), LWN.net, 2021, https://lwn.net/Articles/855924/, primary, checked 2026-10-19
  - supports: DM-100 (motivation: systemd recursive killing, container managers lack single PID, OOM handlers; fork-race handling)
  - supports: DM-101 (no single handle to kill a container; no single PID when PID namespace/cgroup is shared)
[S154] Linux man-pages project, setsid(2), man7.org, n.d., https://man7.org/linux/man-pages/man2/setsid.2.html, primary, checked 2026-10-19
  - supports: DM-005 (fork-then-setsid detach idiom)
  - supports: DM-100 (any process can leave its group/session)
[S155] Linux man-pages project, user_namespaces(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/user_namespaces.7.html, primary, checked 2026-10-20
  - supports: DM-101 (unprivileged user namespaces enable sandboxes without setuid helpers; capabilities scoped to the namespace)
[S156] Ijlal Loutfi (Canonical), Restricted unprivileged user namespaces are coming to Ubuntu 23.10, Canonical blog, 2023, https://canonical.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces, secondary, checked 2026-10-20
  - supports: (available for DM-110; not cited by DM-101 after narrowing) (userns widen attack surface, used in exploit chains; cites Google 44% figure; AppArmor gating, opt-in, incomplete coverage)
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
[S162] Michael Kerrisk, Namespaces in operation, part 1: namespaces overview, LWN.net, 2013, https://lwn.net/Articles/531114/, secondary, checked 2026-10-20
  - supports: DM-101 (namespace goals: containers, migration; introduction dates 2.4.19 to 3.8; incremental decade-long development)
[S163] Jake Edge, Container IDs for the audit subsystem, LWN.net, 2017, https://lwn.net/Articles/740621/, secondary, checked 2026-10-20
  - supports: DM-101 (kernel has no container concept; Howells' kernel-object proposal stalled; Biederman objection; audit container-ID retrofit)
[S164] FreeBSD Documentation Project, FreeBSD Handbook: Jails and Containers, docs.freebsd.org, n.d., https://docs.freebsd.org/en/books/handbook/jails/, primary, checked 2026-10-20
  - supports: DM-101 (jails extend chroot to users, processes and networking; per-jail permission settings)
