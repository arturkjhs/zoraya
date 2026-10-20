# Stream I sources (S900–S999)

[S900] D. M. Ritchie, K. Thompson, The UNIX Time-Sharing System, Communications of the ACM 17(7), 1974 (electronic recreation by E. Brewer), https://pdos.csail.mit.edu/6.1810/2004/readings/ritchie74unix.pdf, primary, checked 2026-10-19
  - supports: GR-001 (compatible file/device/IPC I/O; fds; std fds 0/1; small-system goals)
  - supports: GR-002 (pipes; filters rationale; pipe needs common ancestor)
  - supports: GR-006 (rooted directory tree, mountable volumes)
[S901] J. Corbet, Completing the pidfd API, LWN.net, 2019, https://lwn.net/Articles/794707/, secondary, checked 2026-10-19
  - supports: GR-001 (processes brought into the fd model; PID reuse races)
[S902] A. Baumann, J. Appavoo, O. Krieger, T. Roscoe, A fork() in the road, HotOS 2019 (ACM), 2019, https://www.microsoft.com/en-us/research/wp-content/uploads/2019/04/fork-hotos19.pdf, primary, checked 2026-10-19
  - supports: GR-005 (fork problems; posix_spawn ~0.5ms constant; spawn + cross-process ops; posix_spawn gaps; Cygwin fork unreliability)
[S903] R. Pike, D. Presotto, K. Thompson, H. Trickey, P. Winterbottom, The Use of Name Spaces in Plan 9, Operating Systems Review, 1993, https://9p.io/sys/doc/names.html, primary, checked 2026-10-19
  - supports: GR-001 (everything-as-file extension; process creation/shared memory not files)
  - supports: GR-002 (composition via namespaces)
  - supports: GR-004 (bind/mount/union, 8½, /net, import/cpu, limitations, ~10% X slowdown)
  - supports: GR-006 (file trees as organizing idea for non-disk resources)
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
[S909] The Open Group / IEEE, posix_spawn, posix_spawnp — spawn a process, IEEE Std 1003.1-2024 (Issue 8), 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_spawn.html, primary, checked 2026-10-19
  - supports: GR-005 (rationale: no-MMU/realtime; not a full fork/exec replacement; exit 127 errors)
[S910] Fuchsia project, Process creation, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/process/process_creation, primary, checked 2026-10-19
  - supports: GR-005 (Launcher/fdio_spawn; caller supplies job, executable, handles; no direct creation in fuchsia job)
  - supports: GR-004 (namespace supplied by creator)
[S911] Fuchsia project, Zircon Handles, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/kernel/handles, primary, checked 2026-10-19
  - supports: GR-001 (handle values reused; per-process)
  - supports: GR-002 (channels carry handles)
  - supports: GR-003 (rights reduction, transfer moves handle)
  - supports: GR-008 (rights, duplicate/replace, in-transit handles)
  - supports: GR-018 (Zircon channels as asynchronous contrast)
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
[S917] R. N. M. Watson, J. Anderson, B. Laurie, K. Kennaway, Capsicum: practical capabilities for UNIX, USENIX Security 2010, 2010, https://www.usenix.org/legacy/event/sec10/tech/full_papers/Watson.pdf, primary, checked 2026-10-19
  - supports: GR-008 (hybrid capability model; tcpdump ~2 lines, Chromium ~100 lines; DNS lookup failure in capability mode)
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
[S922] Microsoft, I/O Completion Ports, Microsoft Learn (Win32), 2025, https://learn.microsoft.com/en-us/windows/win32/fileio/i-o-completion-ports, primary, checked 2026-10-19
  - supports: GR-009 (IOCP model, concurrency value, LIFO wake, works for files/sockets/pipes)
[S923] J. Corbet, Ringing in a new asynchronous I/O API, LWN.net, 2019, https://lwn.net/Articles/776703/, secondary, checked 2026-10-19
  - supports: GR-009 (Linux AIO criticized; io_uring ring design)
[S924] Linux man-pages project, io_uring(7), man7.org, n.d., https://man7.org/linux/man-pages/man7/io_uring.7.html, primary, checked 2026-10-19
  - supports: GR-009 (shared SQ/CQ rings; batching; SQPOLL; syscall cost after Spectre/Meltdown)
[S925] Google (Security Blog), Learnings from kCTF VRP's 42 Linux kernel exploits submissions, Google Security Blog, 2023, https://security.googleblog.com/2023/06/learnings-from-kctf-vrps-42-linux.html, primary, checked 2026-10-19
  - supports: GR-009 (60% of submissions exploited io_uring; disabled on ChromeOS; unreachable for Android apps)
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
[S931] U. Drepper, Futexes Are Tricky, Red Hat (author's site), 2011, https://www.akkadia.org/drepper/futex.pdf, primary, checked 2026-10-19
  - supports: GR-011 (naive futex mutexes have correctness and performance bugs)
[S932] NIST NVD, CVE-2014-3153 (futex_requeue), National Vulnerability Database (API record), 2014, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2014-3153, primary, checked 2026-10-19
  - supports: GR-011 (requeue/PI futex privilege escalation)
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
[S948] Wayland project, Wayland Architecture, wayland.freedesktop.org, n.d., https://wayland.freedesktop.org/architecture.html, primary, checked 2026-10-19
  - supports: GR-016 (compositor is display server; compositor routes input to target surface; client-side rendering)
[S949] Wayland project, Wayland Protocol and Model of Operation (docs ch. 4), wayland.freedesktop.org, n.d., https://wayland.freedesktop.org/docs/html/ch04.html, primary, checked 2026-10-20
  - supports: GR-016 (seat focus; no explicit client grabs; selection only to focused client; security section a stub)
[S950] Flatpak project, org.freedesktop.portal.ScreenCast, xdg-desktop-portal docs, n.d., https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html, primary, checked 2026-10-19
  - supports: GR-016 (screen capture via user-mediated portal + PipeWire)
[S951] Q-Success, Usage statistics of UTF-8 for websites, W3Techs, 2026, https://w3techs.com/technologies/details/en-utf8, secondary, checked 2026-10-20
  - supports: GR-015 (99.1% of websites with known encoding use UTF-8)
[S952] Microsoft, Use UTF-8 code pages in Windows apps, Microsoft Learn, n.d., https://learn.microsoft.com/en-us/windows/apps/design/globalizing/use-utf8-code-page, primary, checked 2026-10-20
  - supports: GR-015 (Microsoft recommends UTF-8; activeCodePage UTF-8 since Windows 10 1903)
[S953] Microsoft, about_Pipelines (PowerShell), Microsoft Learn, n.d., https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_pipelines, primary, checked 2026-10-20
  - supports: GR-002 (object pipelines as alternative to byte streams)
[S954] J. Edge, The status of Wayland security, LWN.net, 2014, https://lwn.net/Articles/589727/, secondary, checked 2026-10-20
  - supports: GR-016 (no input snooping/injection/global grabs; privileged clients unsolved; GEM handle guessing; LD_PRELOAD)
[S955] J. Edge, A Wayland full-screen shell, LWN.net, 2014, https://lwn.net/Articles/617807/, secondary, checked 2026-10-20
  - supports: GR-016 (X11 global namespace: any client reads/moves/injects; Wayland per-client isolation; capture via compositor)
[S956] Chromium project, Verified Boot (Chromium OS design document), chromium.org, n.d., https://www.chromium.org/chromium-os/chromiumos-design-docs/verified-boot/, primary, checked 2026-10-20
  - supports: GR-017 (RO firmware root of trust; RSA-signed hashes; on-demand block hashing; full verify would add >=5s; stateful partition not protected; users own computers)
[S957] Android Open Source Project, A/B (seamless) system updates, source.android.com, n.d., https://source.android.com/docs/core/ota/ab, primary, checked 2026-10-20
  - supports: GR-017 (two slots; fallback on failed boot; background install; ChromeOS also uses A/B)
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
