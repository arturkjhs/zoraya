# Stream K sources (S1100–S1199)

[S1100] Baumann, Barham, Dagand, Harris, Isaacs, Peter, Roscoe, Schüpbach, Singhania, The Multikernel: A new OS architecture for scalable multicore systems, SOSP 2009, 2009, https://www.sigops.org/s/conferences/sosp/2009/papers/baumann-sosp09.pdf, primary, checked 2026-10-19
  - supports: Barrelfish (per-core kernels, message passing, replicated state; evaluation scope limited; queueing sensitivity; cache-coherence lessons)
[S1101] Barrelfish project, Barrelfish home page, ETH Zurich, 2020 (last release 2020-03-23), https://barrelfish.org/, primary, checked 2026-10-19
  - supports: Barrelfish (project marked no longer active; last release 2020)
[S1102] Andrew Baumann, Fish in a Barrel: an insider's retrospective of the SOSP'09 multikernel paper, ACM SIGOPS blog, 2021, https://www.sigops.org/2021/fish-in-a-barrel-an-insiders-retrospective-of-the-sosp09-multikernel-paper/, primary, checked 2026-10-19
  - supports: Barrelfish (core-count prediction wrong; heterogeneity mattered more; seL4 capability model a distraction; follow-on systems Arrakis, Popcorn, LegoOS, LXDs)
[S1103] Fuchsia project, Zircon (kernel concepts), fuchsia.dev, 2025, https://fuchsia.dev/fuchsia-src/concepts/kernel, primary, checked 2026-10-19
  - supports: Fuchsia (Zircon scope, ~100 syscalls, mostly non-blocking; larger OS built on top)
[S1104] Fuchsia project, Zircon Handles, fuchsia.dev, 2025, https://fuchsia.dev/fuchsia-src/concepts/kernel/handles, primary, checked 2026-10-19
  - supports: Fuchsia (handles with rights, per-process, transferred over channels, rights reducible on duplication)
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
[S1143] Simon Peter, Jialin Li, Irene Zhang, Dan R. K. Ports, Doug Woos, Arvind Krishnamurthy, Thomas Anderson, Timothy Roscoe, Arrakis: The Operating System is the Control Plane, OSDI 2014, 2014, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/peter, primary, checked 2026-10-20
  - supports: Exokernel (kernel as control plane, application data-plane via virtualized I/O; 2–5x latency, ~9x throughput vs Linux)
[S1144] Niklaus Wirth, Jürg Gutknecht, Project Oberon: The Design of an Operating System, a Compiler, and a Computer (Revised Edition 2013), ETH Zurich, 2013, https://people.inf.ethz.ch/wirth/ProjectOberon/PO.System.pdf, primary, checked 2026-10-20
  - supports: Oberon (1986–89 goal of whole-system understandability; single-process central loop, no protection mechanisms; GC; module sizes ~4,600 lines core; 2013 preface: competing with standards "preposterous")
[S1145] Niklaus Wirth, Project Oberon (New Edition 2013) index page, ETH Zurich, 2013, https://people.inf.ethz.ch/wirth/ProjectOberon/index.html, primary, checked 2026-10-20
  - supports: Oberon (OS, compiler and RISC5 Verilog for FPGA published together)
[S1146] Rob Pike, Acme: A User Interface for Programmers, Proc. Winter 1994 USENIX Conf., 1994, https://9p.io/sys/doc/acme/acme.html, primary, checked 2026-10-20
  - supports: Oberon (Acme/Help UI derived from Oberon; Oberon criticized as single-process with weak networking)
