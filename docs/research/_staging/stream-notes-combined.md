# stream-A-entries.md

## Stream notes

**Searched.** HotOS 2019 fork paper (full PDF text extracted locally) and the Drepper/Krieger follow-up; POSIX Issue 8 `posix_spawn` and `open`; Linux man pages (open, close_range, pidfd_open, pidfd_send_signal, nptl, pthreads, signal, wait, PR_SET_CHILD_SUBREAPER, openat, umask, unshare, setsid, ld.so, dpkg); LWN on pidfd, clone3, and the cgroup.kill patch; Microsoft Learn (CreateProcessW, process handles, processes and threads, job objects, DLL search order, advisory 2269637, MoveFileEx, Restart Manager, MSVC argv parsing, KB5025885); Fuchsia process creation and namespaces docs; PEP 446; Go issue 1435; Rust CVE-2024-24576; Ubuntu CVE-2017-1000117; Android A/B and Verified Boot; ChromiumOS verified boot; kernel dm-verity, livepatch and cgroup-v2 docs; OSTree; Nix thesis abstract and NixOS JFP abstract; systemd offline updates; needrestart; tini; Eclypsium BootHole; CERT/CC VU#811862 (LogoFAIL); Oxide phbl and RFD 284; Poettering's trusted-boot post; FSF statement.

**Dropped/merged.** None dropped. All 12 index rows survived.

**New entries.** DM-100 (no inescapable process-tree containment: process groups vs job objects vs cgroups). Note for coordinator: the seed-list item "Linux namespaces and cgroups retrofitted for containers; cgroups v1 → v2" has **no row in the index**. DM-100 covers only the process-containment slice; namespaces and the cgroup v1→v2 redesign remain uncovered (category 2 or 7?).

**Cross-references / overlap.** DM-001↔DM-030 (overcommit); DM-002/DM-003↔GR-005 and DM-055 (clone3); DM-004↔DM-017, DM-059; DM-006↔DM-039, DM-066, DM-085, DM-097; DM-007↔DM-011, DM-085 (`LD_PRELOAD`); DM-008↔DM-095 (leading `-`); DM-091↔DM-044 (share modes); DM-090/DM-093↔GR-017 (stream I should share or cite S134, S146, S148).

**Open questions.** (1) Could not fetch Ritchie's "Evolution of the Unix Time-sharing System" (bell-labs URL returns 410, Nokia 403). The 27-line fork claim is cited via S100's quotation only. (2) FreeBSD process descriptors (`pdfork`) and Mach task/thread split are marked [unverified]; streams J/K may have sources. (3) Erlang-style hot code loading as a DM-091 alternative is unverified. (4) The NixOS JFP paper and Nix thesis: only the abstracts were read, so rollback mechanics are attributed only at abstract level. (5) Oxide RFD 284 does not state the rationale for avoiding UEFI (it defers to RFD 75, which was not fetched), so S145 supports only what phbl does, not why. (6) No macOS/dyld or Windows-side verified-boot material was researched.

**Weak or caveated sources.** S121 (tini README) is used only as an anecdotal/practice pointer. S149 is a blog post, by the systemd lead developer, so it is authoritative on systemd design but still a blog. S150 is an advocacy statement, used only to state the FSF position. S101 is an informal blog conversation. S140 (needrestart README) describes tool behaviour, not kernel semantics. NVD and cve.org pages for CVE-2020-10713 did not render, so BootHole rests on the discoverer's advisory (S142).

**Addendum (DM-101, coordinator request).** Added DM-101 from the reserve block under category 2 (Security, authority and identity); reasoning is in its Category field. Narrowed at coordinator request to the umbrella question (containers as an assembly of retrofitted primitives vs first-class designs); user-namespace attack-surface detail and cgroup v1/v2 hierarchy detail moved to stream B's DM-110 and DM-111. Fetched sources S156 (Canonical userns post) and S157 (CVE-2022-0185 report), plus the S152 'Issues with v1' section, are no longer cited by DM-101 but remain verified and available for DM-110/DM-111. Searched: user_namespaces(7), LWN namespaces overview and container-ID articles, CVE-2022-0185 oss-sec report, Canonical's Ubuntu 23.10 userns post, Kamp/Watson jails paper, FreeBSD handbook, Price/Tucker zones paper (full text), Zircon job docs, Plan 9 namespaces paper. Open: Google's June 2023 kCTF post (the '44% of exploits need unprivileged userns' figure) did not render and is known only via S156. Weak/caveated: S162/S163 are LWN (secondary).


# stream-B-entries.md

## Stream notes

**Searched / fetched.** Hardy (cap-lore copy, via curl due to TLS error); Miller/Yee/Shapiro (JHU URL dead; Agoric-hosted copy, text extracted locally); Capsicum USENIX 2010; FreeBSD capsicum(4); OpenBSD pledge/unveil; kernel seccomp, LSM, cgroup-v2 docs; capabilities(7), user_namespaces(7), ld.so(8), setenv(3); LWN (CAP_SYS_ADMIN, Capsicum for Linux, LSM stacking 2012, Debian systemd 2014, The Journal 2011); Setuid Demystified (Berkeley copy; the usenix.org copy is encrypted); Qualys PwnKit; CVE records via cveawg API; Android 4.3 docs commit; run0(1); Plan 9 security paper; Apple SIP; Rust set_var docs and 2024 edition guide; glibc environ patch (via AlmaLinux packaging); Microsoft registry, token, MIC, ETW docs; Raymond Chen on INI; Project Zero registry series #1; Yin et al. SOSP 2011; Augeas; NixOS ICFP 2008; Rethinking PID 1; Debian tech-ctte and 2014/2019 GRs; skarnet critiques and s6; illumos smf(7); RFC 3164/5424; journal design doc and file format.

**Dropped/merged.** None dropped. All 8 category-2 and 5 category-14 candidates survived.

**New entries.** DM-110 (unprivileged user namespaces as kernel attack surface — the seed list's "namespaces/cgroups retrofitted for containers" had no index row). DM-111 (cgroups v1 → v2), placed in category 14 as resource management.

**Open questions / unresolved.**
- Whether Capsicum for Linux was ever merged (believed not; marked [unverified] in DM-014).
- Current mainline status of full LSM stacking (SELinux + AppArmor together); kernel docs say only "close approximation".
- The 44% "exploits needing unprivileged userns" figure is Canonical's attribution to Google; the Google kCTF post I fetched gives a 60% io_uring figure and does not mention 44%. Primary not found.
- No peer-reviewed source for "admins disable SELinux" — left as [unverified] anecdote.
- launchd not fetched (named only, marked [unverified]); Windows AppContainer and MSIX per-package registry not fetched.
- glibc commit 7a61e7f5 ("Make getenv thread-safe in more cases") known only through the Weimer follow-up patch; sourceware.org blocked by bot protection.
- The 2014 Debian tech-ctte vote tally (4–4 tie) is not stated in my sources; only "casting vote" (LWN) is cited.

**Possibly weak sources.** S249 (skarnet critique, opinion page; marked weak, used only as the critic position). S256 comments (weak; the article body is secondary). S240 (Chen blog; secondary, author is a Microsoft insider). S243 Augeas home page is thin. S230 is a vendor blog repeating a third-party statistic.


# stream-C-entries.md

## Stream notes

**Searched.**
- Signals: signal-safety(7), signalfd(2), the Qualys regreSSHion advisory, LWN "Ghosts of Unix past", Windows console handlers.
- Mach/L4: Liedtke 1993/1995, Härtig 1997, Chen and Bershad 1993, Heiser and Elphinstone 2016.
- SysV IPC: man pages and PostgreSQL docs and release notes.
- Byte streams: the BSTJ 1978 foreword, the Monad Manifesto, Wheeler, Nushell.
- kdbus/bus1: LKML 2015 threads, LWN 2015/2016/2026, dbus-broker, the D-Bus spec, binderfs.
- SCM_RIGHTS: Project Zero, the NVD API, the Debian tracker, unix(7), 2024 GC rework commits, Fuchsia channel docs.
- BKL: the OLS 2002 paper, the removal commit, LWN, FreeBSD SMPng, seL4 big-lock papers.
- Cancellation: man pages, glibc BZ 12683 (bugzilla blocked by an anti-bot wall, so I used the glibc commit and NEWS via the GitHub mirror instead), EWONTFIX, TerminateThread, Java Thread.stop.
- Non-reentrant libc: man pages and the Rust 2024 edition guide.
- Priority inversion: Reeves, Sha et al., kernel rt-mutex and PI-futex docs, Yodaiken vs Locke, POSIX.
- Memory model: Alglave et al. 2018, memory-barriers.txt, Boehm 2005.
- Futex: Drepper, the futex2 docs, CVE-2014-3153.

**Dropped/merged.** None dropped. DM-018 was reclassified as `contested`; the index listed no classification.

**New entries.** DM-120 (futex interface accretion, tradeoff). It complements GR-011 rather than contradicting it.

**Open questions.**
- Whether Zircon move-only handles can still form unreachable cycles; the docs are silent.
- seL4 scheduling-context donation as a PI alternative, not sourced.
- Whether macOS XPC/launchd is a better bus-design data point for DM-021.
- Rust's atomics model formally deferring to C++20, not sourced.
- Original Linux futex man page and Liedtke 1993 could not be fetched (connection refused); S308 is marked unverified and not counted.
- The glibc fix in 2.41 is inferred from the NEWS fixed-bug list.

**Weak or caution sources.**
- S318 (Wheeler essay) is weak.
- S348 (EWONTFIX, unsigned, attributed to the musl author) is weak; its claims are corroborated by the glibc commit S349.
- S362 (Locke) is an editorial.
- The S330 Project Zero post is a vendor write-up, treated as primary for exploit analysis.
- S357 Reeves is an email reproduced on a course page.
- The Sha et al. S358 copy is a course mirror of the IEEE paper.
- The NVD records were fetched through the NVD JSON API endpoint (the detail page rendered empty).
- Several PDFs were text-extracted locally after fetch.


# stream-D-entries.md

## Stream notes

**What was searched.** USENIX proceedings (Banga/Mogul/Druschel 1999, Lemon 2001, Arrakis and IX at OSDI 2014), man7.org and FreeBSD man pages, POSIX Issue 8, LWN (AIO, TASK_KILLABLE, non-blocking buffered reads, io_uring restrictions), LKML (Torvalds on AIO, Rizzo's io_uring sysctl, Kirby's hung-task report), Google's kCTF post and its oss-security repost, Axboe's io_uring paper and Kernel Recipes slides, Microsoft Learn (IOCP, IRP cancellation, WaitForMultipleObjects, GetAddrInfoExW), Go and Rust issue trackers, RFC 3493 and RFC 9621, the Plan 9 networking paper, Fuchsia docs, moby PR #41030, the Linux commit adding ip_unprivileged_port_start, the XDP paper (CoNEXT 2018), Snap (SOSP 2019) and DPDK docs. The full text of PDFs (kqueue, io_uring, XDP, Kernel Recipes) was extracted locally after downloading. A WebFetch rate limit hit partway through, so some later pages (pipe(7), send(2), POSIX write, FreeBSD setsockopt, Python signal docs, Rust issue #62569 via the GitHub API, signalfd(2)) were fetched with curl instead.

**Dropped/merged.** Nothing was dropped. DM-023 covers the multithreaded epoll problems (thundering herd, EPOLLEXCLUSIVE) rather than a separate entry.

**New entries.** DM-130: waitable events outside the descriptor model (signals, timers, process exit) and the signalfd/timerfd/pidfd retrofits, contrasted with NT waitable objects, kqueue filters and Zircon ports. It may overlap with DM-017 (signals, stream C), DM-019 (SysV IPC outside the fd model) and stream A's PID-reuse/pidfd entry. The coordinator may want to merge it, or keep it as the "unified wait" angle.

**Open questions.**
- I could not verify a source for Linux's internal `restart_block` mechanism or for NT alertable-wait/APC semantics. Both are marked [unverified] in DM-027.
- Whether microkernel file-server clients really avoid D-state hangs in practice is [unverified] in DM-028; I found no specific system's documentation.
- Socket activation as a per-port grant is [unverified] in DM-076.
- I could not confirm whether Plan 9 kills writers to a closed pipe with a note, so it is omitted from DM-077.
- Search results mentioned a 2026 epoll use-after-free CVE ("Bad Epoll", CVE-2026-46242), but only through news sites. I did not verify it and did not use it.
- The Google kCTF blog page body did not render. The 60% figure and the ChromeOS/Android/servers/GKE statements are verified through the blog author's own oss-security repost [S413].

**Weak or caveated sources.** S404 and S405 (Majkowski's personal blog), S419 (without.boats blog), S421 (wepoll README) and S461 (Cloudflare blog) are labelled weak. Each is used only for practitioner viewpoints or anecdotal failure modes, and each entry also has primary sources. S448 (Apple Developer Forums) is an Apple engineer's statement and is labelled secondary. Apple never documented the Mojave change. S410 was fetched through an Internet Archive copy because kernel.dk's TLS certificate failed. S407 is Torvalds's LKML email as mirrored on LWN. S411's Torvalds quotes are as reproduced on Axboe's slides.


# stream-E-entries.md

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


# stream-F-entries.md

## Stream notes

**Searched / fetched.** fcntl/fcntl_locking man pages, POSIX.1-2024 fcntl and XBD ch.3/ch.4, Layton's LWN article and LKML patch threads, SQLite os_unix.c. LWN fsyncgate coverage, Ringer's pgsql-hackers post, PostgreSQL wiki, Rebello ATC'20, LWN errseq_t, Apple fsync(2). LWN ext4 delalloc coverage, Ts'o's Launchpad comment, Pillai OSDI'14 (PDF text extracted locally). openat2(2), LWN 2026 path-traversal article, CVE-2018-15664, Fuchsia "dotdot", Pike's lexical names paper, TxOS SOSP'09 (PDF extracted locally). Kernel sysctl docs and Kees Cook link-restriction patches. mount_namespaces(7), user_namespaces(7), Plan 9 papers, Canonical userns post. LWN atime articles, mount(8), NTFS fsutil. Microsoft MAX_PATH / naming / CreateFile / Restart Manager / FILE_DISPOSITION_INFORMATION_EX / TxF deprecation / WSL case sensitivity / UTF-8 code page / A-W prototypes / surrogates docs. Raymond Chen, Michael Kaplan. Btrfs transaction-ioctl deprecation commit. Apple APFS FAQ, CVE-2014-9390, LWN case-insensitive ext4, 2025 LKML bcachefs thread (Torvalds/Overstreet/Anvin), Austin Group bug 251, Colomar/Ts'o LKML 2025, Wheeler essay. Unicode UTF FAQ, PEP 529, PEP 538, P0067R5, Python locale docs, glibc setlocale/uselocale/newlocale man pages, Rust OsString docs.

**Fetch method note.** A tool rate-limit interrupted WebFetch partway through. Some later sources (Austin bug 251, Microsoft UTF-8/A-W/surrogate docs, man pages for setlocale/uselocale/newlocale, PEPs, P0067R5, Rust docs, Python locale docs, POSIX XBD ch.3, btrfs commit, LWN btrfs-atime) were fetched with curl and their HTML stripped and read locally. Two PDFs (Pillai, TxOS) were text-extracted with Ghostscript. All are genuinely fetched; none are marked unverified.

**Dropped/merged.** None dropped. DM-040 was narrowed to the security problems of hard links (the hardlink-to-inaccessible-file issue); broader "semantic complications" claims (backup tools, directory hard links) could not be sourced and are marked [unverified] or omitted.

**New entries.** None added (reserve DM-150–DM-159 unused). Candidates considered but not written for lack of time/sources: Darwin `fsync` vs `F_FULLFSYNC` (folded into DM-037), 8.3 short-name aliases (folded into DM-043).

**Open questions.**
- Whether Plan 9 file servers support hard links at all (DM-040) — not verified.
- Plan 9 native UTF-8 (DM-096) — not verified in this stream; streams J/K may have a source.
- Exact merge version of protected_hardlinks/symlinks (3.6?) not confirmed.
- Windows reparse-point TOCTOU specifics (DM-039) and frequency of sharing-violation failures caused by AV/indexers (DM-044) not sourced; marked [unverified].
- DM-045 contested: TxOS's low overhead vs two abandoned production attempts; not resolvable from sources.

**Weak/caution sources.** S648 (Wheeler essay) is weak and is used only as a catalogue of failure patterns; the binding claims cite S649/S651. S626 (Canonical blog) relays a Google statistic (44%) that was not checked against Google's report. S608 (PostgreSQL wiki) is a living community page with no fixed date. S657 (Kaplan) is an archived mirror of an MSDN blog. S621 is the patch text as posted on LWN, labelled primary.


# stream-G-entries.md

## Stream notes

**Searched.** OS fault studies (Chou 2001, Palix 2011/2014), Nooks and shadow drivers, SDV, MINIX 3 (IEEE Computer 2006; ;login: 2007 not extractable), the Tanenbaum–Torvalds debate (1992), Torvalds RWT 2006, Biggs et al. 2018, memory-safety statistics (MSRC, Chromium, Android), Rust-for-Linux docs and the LWN Hellwig/DMA dispute, stable-api-nonsense, the CrowdStrike RCA and Microsoft posts (July/Nov 2024), Windows TDR/UMDF docs, Linux BUG_ON threads, man pages (errno, syscall, openat2, clone3, stat, statx, statmount, proc_pid_mountinfo), kernel docs (ioctl, adding-syscalls, sysfs), PROCMAP_QUERY LKML, Debian 64-bit time, glibc feature macros, Go 1.11/1.16 notes, OpenBSD pinsyscalls, Apple QA1118, the j00ru NT syscall table, double-fetch papers, Thunderclap and the Linux Thunderbolt IOMMU patch.

**Dropped/merged.** None of DM-046–DM-057 were dropped. The time_t 2038 material in DM-056 points to DM-063 (stream E) instead of duplicating it.

**New entries.** DM-160 (double-fetch races from raw user pointers; category 8). DM-161 (driver isolation without DMA/IOMMU isolation; category 7).

**Open questions.**
- I found no Linux-kernel-specific breakdown of CVEs by memory-safety class (marked [unverified] in DM-048).
- The scale of out-of-tree vendor driver pain (Android vendor kernels, GPU drivers) in DM-049 is unverified.
- Torn reads of multi-read procfs files (DM-054) are unverified.
- FIDL and specific message-passing copy-in behaviour (DM-053, DM-160) were not fetched.
- The EU interoperability angle in DM-050 is unverified.
- The 2001 Palix ASPLOS site (faultlinux.lip6.fr) refused connections, so the TOCS/arXiv version [S701] is used and [S742] is marked unverified.

**Weak or suspect sources.**
- The widely quoted "85% of Windows XP crashes are drivers" goes back only to a private communication cited in Nooks [S702]. SDV [S703] cites Nooks for it, so the second paper adds no independent evidence.
- Biggs et al. [S704] is a counterfactual analysis by microkernel proponents, with self-declared ±9 pp uncertainty.
- The MINIX 3 <10% overhead [S707] is a claim by the authors in a magazine article.
- [S744] is a community-maintained table (secondary).
- [S741] is LWN (secondary).
- PDF text for several papers was extracted with a crude local zlib/Tj parser, so short quotes were checked against that extracted text, not a rendered PDF.


# stream-H-entries.md

## Stream notes

- **Searched:** man7 credentials/setsid; POSIX `_exit`; Åkesson TTY article; kernel TIOCSTI/ldisc-autoload commits and kernel-hardening posts; oss-security threads (2016–2023); CVE records via the cveawg.mitre.org JSON API (NVD pages render empty); HD Moore 2003 bugtraq; dgl.cx 2023; ECMA-48; Microsoft console devblogs and Learn docs; Plan 9 cons(3)/rio(4); LWN on systemd KillUserProcesses and Stone's LCA 2013 talk; X SECURITY/XACE specs; the Scheifler–Gettys 1986 paper (draft copy); Wayland FAQ, architecture and spec; waypipe; NT 4.0 kernel-mode USER/GDI white paper; Chromium sandbox doc; Project Zero "Breaking the Chain"; UIPI docs; Session 0 paper; MSRC servicing criteria; Packard Present blog; Dänzer's GNOME blog; wayland.app protocol pages; GOVERNANCE.md (AOSP mirror, since gitlab.freedesktop.org is behind Anubis); frog-protocols.
- **Dropped/merged:** none. TIOCLINUX (CVE-2023-28100) and "shared TTY across su/sudo" are folded into DM-072. Line-discipline autoload is folded into DM-069. Console "wrong-way verbs" are folded into DM-073.
- **New entries:** none (DM-170–179 unused). A candidate: "in-kernel VT/fbcon console" (TIOCLINUX, fbcon scrollback removal) — not researched enough to add.
- **Open questions:** (1) Paget's shatter paper could not be fetched (host is gone; archive.org is blocked for this agent), so the mechanism in DM-082 rests on CVE-2002-1230. (2) The EWS full PDF was fetched but could not be text-extracted; only abstract claims are used. (3) Distribution defaults for `CONFIG_LEGACY_TIOCSTI` after 6.2 are not verified. (4) I found no primary statement of GNOME's SSD position; the evidence is protocol support tables. (5) I found no primary source for Høgsberg coining "every frame is perfect"; it is attributed to the Wayland community via Dänzer.
- **Weak or secondary sources to watch:** S831 (Rutkowska blog, credible practitioner with a reproducible demo); S821 (Linux Magazine news); S856 (Japanese news summary); the support tables in S851/S857 are wayland.app's compilation, not the protocol text; S835 is a pre-publication draft, not the TOG version of record; S811 is a researcher blog but is the primary disclosure.


# stream-I-entries.md

## Stream notes

**What was searched.** Primary pointers from the brief:
- Ritchie & Thompson 1974 (MIT recreation; the Bell Labs/Nokia copies returned 403/410)
- Plan 9 namespaces and UTF papers
- Microsoft object manager, handle, IOCP, CreateProcess attribute and WaitOnAddress docs
- seL4 SOSP 2009, FAQ and verified-configurations pages, and the Aug 2026 AArch64 confidentiality announcement
- McKenney's RCU survey and the kernel RCU docs
- DTrace USENIX 2004
- OpenZFS docs plus the FAST 2008/2010 corruption studies
- Franke/Russell/Kirkwood OLS 2002 and Drepper
- Torvalds' LKML posts (via the lkml.iu.edu mirror; lore.kernel.org returned 403) and kernel regression docs
- Chromium OS verified boot and Android A/B/Virtual A/B docs
- Zircon handles and process creation

Also used:
- Baumann et al. (fork), Capsicum, Crotty et al. (mmap)
- Seltzer/Murphy and WinFS (hierarchy critique and failed alternative)
- Google's kCTF io_uring post, LWN on eBPF verifier bugs
- QNX 7.1 docs and Hildebrand 1992
- "Spectre is here to stay", Site Isolation, Deconstructing Process Isolation
- virtio paper/spec and LWN on virtio hardening

PDFs were fetched with curl and converted with Ghostscript where WebFetch could not read them. A mid-run rate limit forced curl for a few HTML pages; each URL cited was retrieved.

**Downgrades and classification.** Only five entries are `held-up-well`: GR-005, GR-012, GR-015, GR-017, GR-019. Fifteen are `held-up-with-caveats`. GR-008 (object capabilities) is `contested`: the evidence for the model is strong, but mainstream adoption is thin and vendors have chosen ACL plus sandbox instead. Unix-origin items (GR-001 fds, GR-002 pipes, GR-006 hierarchy) were deliberately kept at `with-caveats`, with their known failure modes cross-referenced to DM entries. Nothing was dropped or merged.

**New entries.** GR-021 (virtio as a standard paravirtual device interface). It was added because QEMU `virt` makes it directly relevant to our first drivers.

**Open questions / unverified.**
- (a) Whether the Capsicum Linux port was ever merged. I believe not, but found only secondary pointers (linuxfr.org, a mailing-list forward), which I did not fetch. Marked [unverified] in GR-008.
- (b) APFS checksum scope (Apple's page returned no body text). APFS claims are marked [unverified].
- (c) Exact date and NT version for the object manager's origin.
- (d) The scale of CVE-2014-3153 exploitation.
- (e) Whether seL4's 2026 AArch64 confidentiality proof covers MCS or SMP configurations. The announcement doesn't say, and the verified-configurations page lists MCS as in progress.
- (f) Several adoption claims (Wayland default on desktops, DTrace ports, QNX market) are marked [unverified] rather than sourced.

**Possibly weak sources.**
- [S929] is an LWN comment thread, labelled weak and used only to show that practitioners dispute the scope of the rule.
- [S951] (W3Techs) is a commercial survey, labelled secondary.
- [S921] is a forum announcement by a seL4/Proofcraft principal. It is primary, but it is a press-style claim and is backed by [S920].
- [S938] (ebpf.io) is an advocacy site, labelled secondary.

**For the coordinator.** GR-001/GR-002/GR-003 cross-reference DM-002, DM-009, DM-016, DM-020, DM-022, DM-023, DM-036, DM-053, DM-077 and DM-081. GR-009 relates to DM-025/026/029, GR-013 to DM-049/072, GR-016 to DM-079/082/084, and GR-017 to DM-090. Please check that those DM entries do not contradict the GR claims.


# stream-J-postmortem.md

## Stream notes

**Searched.** multicians.org (myths, history, FJCC 1965); Karger & Schell ACSAC 2002; 9p.io Plan 9 paper and
overview; Pike utah2000; Pike UTF-8 email; Plan 9 Foundation; LWN namespaces; kernel v9fs doc; Inferno BLTJ paper,
Vita Nuova site and 2002 CEO interview; Chen & Bershad SOSP'93; Liedtke SOSP'95; Härtig et al. SOSP'97; Lepreau et
al. Mach III '93; Coulouris Mach chapter; CMU Mach page; XNU README; Heiser & Elphinstone TOCS'16; seL4 whitepaper
(rev 1.4.1, 2026); seL4 open-source news; Heiser's OK Labs post; QNX 7.1 sys-arch and 8.0 release notes; QNX
source-closure news; BlackBerry 2024 vehicle press release; Hardy KeyKOS architecture and cap-lore notes; EROS
SOSP'99; CapROS site; Shapiro/Midori news; Hunt & Larus OSR'07; Singularity RDK news; McIlroy et al. 2019; three Joe
Duffy Midori posts; SD Times 2010; Pixel Envy pointer to Foley/ZDNet.

**Dropped / not used.** Accetta et al. 1986 Mach paper (all mirrors failed: TLS error or 403). Mach goals are cited
from the Coulouris chapter (secondary) and the CMU page instead. The ZDNet, eWeek and Redmond Magazine Midori
articles were blocked. The Dr. Dobb's 1992 Key Logic history was paywalled (402). The Condict 1994 OSF co-location
paper was not found.

**Process notes for the verifier.**
- WebFetch hit a session limit partway through. Later pages (LWN, UTF-8 email, Plan 9 overview/Foundation, kernel
  v9fs doc, Inferno paper/site, OSNews Vita Nuova interview, cap-lore pages) were fetched with `curl` and read
  locally. The cap-lore.com pages also failed WebFetch TLS verification, so they were fetched with curl over http.
- PDFs (Pike, Chen/Bershad, L4 TOCS, Singularity, seL4 whitepaper, KeyKOS, EROS, Karger/Schell, Coulouris) were
  downloaded and text-extracted locally with pypdf.
- MkLinux figures in the Mach section are throughput as a percentage of native from the SOSP'97 body (73% / 62%
  vs 95%). The abstract states them as 5x / 7x the L4Linux penalty.
- Apple's use of L4 on the iOS security processor comes from Heiser & Elphinstone [S305], who cite an Apple
  security guide. We did not fetch the Apple document.

**Weak or vendor sources.** [S1013] (Vita Nuova CEO interview: commercial claims), [S1029] (BlackBerry vehicle
count, vendor-commissioned estimate), [S1045] (link blog). [S1000]/[S1002] are by Multics participants, so they may
favor Multics. They were used for facts and the participants' own account of causes. [S1005] is self-described as
a polemic.

**Open questions.**
- Why exactly Honeywell/Bull cancelled Multics: the "corporate politics" account comes from participants only.
- What scalability limit in pre-8.0 QNX kernels prompted the redesign. The release notes do not say.
- The real reason Midori was ended or folded into Windows: no fetchable primary source. Duffy says only that it was
  not purely technical or business.
- How much Key Logic's commercial failure was due to McDonnell Douglas strategy vs. market: unverified.
- Whether QNX's synchronous copy IPC limits general-purpose I/O throughput: no measurement found.
- OSF/1 → Digital UNIX/Tru64 commercial history: not sourced.

**Cross-stream pointers.** The L4 "abandoned features" list [S305] and Singularity's unsafe-code-tax numbers
[S1038] + Spectre [S963] are likely useful for IPC (cat. 3) and kernel structure (cat. 7) entries. The Multics
buffer-overflow analysis [S1003] is relevant to security/ABI entries.


# stream-K-postmortem.md

## Stream notes

**Searched.** For each system: the system's own docs, papers and retrospectives (fuchsia.dev Zircon, handles,
Starnix and RFC-0082; Genode Foundations, release notes, roadmap and licence news; the Redox book and news;
haiku-os.org; the Theseus OSDI paper and repository; the Hubris reference and Biffle's OSFC talk; Herder et al.
ACSAC 2006; the Amoeba CWI report; Ousterhout's Sprite retrospective and Douglis's thesis; the exokernel SOSP 1995
and 1997 papers; Project Oberon 2013; Baumann's SIGOPS retrospective). Status was checked through the GitHub API
for Theseus, Hubris and MINIX. News for Fuchsia came from 9to5Google, OSNews and XDA; for BeOS from Computerworld,
The Register and TechCrunch.

**Dropped or not added.** No extra systems were added. Spring (Sun, the origin of Solaris doors) and Nemesis
(QoS and self-paging, an ancestor of Barrelfish per [S1102]) are relevant, but I could not research them to
standard in this pass. They are candidates for a follow-up. The Microsoft Research Barrelfish feature, Roscoe's
;login: articles, the MINIX 3 OSR 2006 paper (PDF text not extractable) and Wirth's "Plea for Lean Software"
(scanned PDF) were found but not used.

**Weak or limited sources.**
- [S1125] (TechCrunch opinion piece) is the only source for the Apple–Be price story, and figures differ across
  outlets ($125M–$300M asks).
- [S1108] is opinion on Fuchsia's direction.
- Genode, Redox, Hubris, Sprite and Oberon rest almost only on the projects' own documents. They are primary,
  but self-reported, with no independent assessment.
- The MINIX ME letter page is undated; 2017 is inferred from the coverage it links to.

**Open questions.**
1. Does Starnix run in production on any shipping device, and what does it cost in syscall overhead now? Its
   trapping mechanism is reported as "restricted mode" but this was not verified.
2. Fuchsia's actual 2026 roadmap content: the 2025 overview URL returned 404.
3. Why MINIX 3 development stopped after 2018 (funding end is assumed, not sourced).
4. BeOS's technical limitations at the time, such as drivers, networking and applications: no primary source was
   found.
5. Whether Amoeba's early licensing terms limited uptake.
6. Primary evidence for exokernel lineage into DPDK/SPDK and unikernels.

**Cross-stream note.** Theseus [S1126] includes a fault-injection comparison with MINIX 3's IPC. This is useful
to stream J (microkernel and L4 entries) and to any DM entry on driver isolation and recovery.
