# Stream B sources (S200–S299)

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
[S209] Linux man-pages project, ld.so(8), man7.org, 2026 (current), https://man7.org/linux/man-pages/man8/ld.so.8.html, primary, checked 2026-10-19
  - supports: DM-011 (secure-execution mode/AT_SECURE: loader must scrub environment for setuid/capability binaries)
  - supports: DM-085 (LD_PRELOAD/LD_LIBRARY_PATH semantics; list of stripped variables incl. GCONV_PATH, TZDIR)
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
[S229] Linux man-pages project, user_namespaces(7), man7.org, 2026 (current), https://man7.org/linux/man-pages/man7/user_namespaces.7.html, primary, checked 2026-10-19
  - supports: DM-110 (unprivileged creation since 3.8; full caps inside; per-subsystem support; later fixes e.g. setgroups, CAP_SETFCAP in 5.12)
[S230] Ijlal Loutfi (Canonical), Restricted unprivileged user namespaces are coming to Ubuntu 23.10, ubuntu.com blog, 2023, https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces, primary, checked 2026-10-19
  - supports: DM-110 (unprivileged userns widely used in LPE chains; 44% figure attributed to Google; sysctl is all-or-nothing; AppArmor per-app gate)
[S231] Synacktiv, Old bug, shallow bug: Exploiting Ubuntu at Pwn2Own Vancouver 2023, synacktiv.com, 2023, https://synacktiv.com/publications/old-bug-shallow-bug-exploiting-ubuntu-at-pwn2own-vancouver-2023, secondary, checked 2026-10-19
  - supports: DM-110 (CVE-2023-35001 nf_tables reachable by unprivileged users via user namespaces)
[S232] Linux kernel documentation (Tejun Heo et al.), Control Group v2, docs.kernel.org, 2026 (current), https://docs.kernel.org/admin-guide/cgroup-v2.html, primary, checked 2026-10-19
  - supports: DM-111 ("Issues with v1": multiple hierarchies impractical, thread granularity, inner-node competition, inconsistent controller interfaces; no-internal-process rule; delegation)
[S233] Lennart Poettering, Rethinking PID 1, 0pointer.de blog, 2010, http://0pointer.de/blog/projects/systemd.html, primary, checked 2026-10-19
  - supports: DM-088 (SysV init serial, shell-heavy — grep called 77 times; socket activation; parallel start)
  - supports: DM-111 (cgroups used for process tracking because double-forked daemons escape supervision)
  - supports: DM-085 (shell behaviour depends heavily on environment variables)
[S234] Linux man-pages project, setenv(3), man7.org, 2026 (current), https://man7.org/linux/man-pages/man3/setenv.3.html, primary, checked 2026-10-19
  - supports: DM-085 (setenv/unsetenv MT-Unsafe const:env; POSIX does not require reentrancy)
[S235] Rust project, std::env::set_var documentation, doc.rust-lang.org, 2026 (current), https://doc.rust-lang.org/std/env/fn.set_var.html, primary, checked 2026-10-19
  - supports: DM-085 (unsound in multithreaded programs on non-Windows; libc getenv unsynchronized; libraries read env undocumented)
[S236] Rust project, Rust 2024 Edition Guide: Unsafe functions, doc.rust-lang.org, 2025, https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html, primary, checked 2026-10-19
  - supports: DM-085 (set_var/remove_var made unsafe in 2024 edition after being found incorrect)
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
