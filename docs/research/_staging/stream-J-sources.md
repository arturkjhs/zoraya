# Stream J sources (S1000–S1099)

[S1000] Tom Van Vleck et al., Myths about Multics, multicians.org, n.d., https://multicians.org/myths.html, primary, checked 2026-10-19
  - supports: Multics (Bell Labs withdrawal not end; ~77 sites; last site 2000; early slowness; 250K-line kernel; Thompson took hierarchical FS; Bull cancellation attributed to politics)
[S1001] F. J. Corbató, V. A. Vyssotsky, Introduction and Overview of the Multics System, AFIPS Fall Joint Computer Conference, 1965, https://multicians.org/fjcc1.html, primary, checked 2026-10-19
  - supports: Multics (design goals: computer utility, segmentation, dynamic linking, hierarchical FS, protection, PL/I portability)
[S1002] Tom Van Vleck (ed.), Multics History, multicians.org, n.d., https://multicians.org/history.html, primary, checked 2026-10-19
  - supports: Multics (645 delays, Bell Labs exit 1969, Honeywell 1970, B2 1985, cancellation 1985, hardware cost, microcomputer shift, internal politics)
[S1003] Paul A. Karger, Roger R. Schell, Thirty Years Later: Lessons from the Multics Security Evaluation, ACSAC 2002, 2002, https://www.gwern.net/doc/cs/security/2002-karger.pdf, primary, checked 2026-10-19
  - supports: Multics (no buffer overflows: PL/I bounded strings, execute bits, upward stacks; trap-door subversion; kernel size vs SELinux policy)
[S1004] Rob Pike, Dave Presotto, Sean Dorward, Bob Flandrena, Ken Thompson, Howard Trickey, Phil Winterbottom, Plan 9 from Bell Labs, Bell Labs (Computing Systems 1995 version), 1995, https://9p.io/sys/doc/9.html, primary, checked 2026-10-19
  - supports: Plan 9 (Unix problems too deep; 9P, namespaces, unions, /proc, 8½, dump, no superuser; authors' list of weaknesses)
[S1005] Rob Pike, Systems Software Research is Irrelevant, talk at University of Utah, 2000, https://inf.ufes.br/~raulh/ufes/teaching/courses/sc/texts/utah200.pdf, primary, checked 2026-10-19
  - supports: Plan 9 (90-95% of work honoring standards; Unix emulation expected; PC monoculture; funding/startup horizon)
[S1006] Plan 9 authors, Plan 9 from Bell Labs — Overview (releases and licensing), 9p.io, n.d., https://9p.io/plan9/about.html, primary, checked 2026-10-19
  - supports: Plan 9 (edition/licensing history 1992-2021; mostly new software, not ported)
[S1007] Plan 9 Foundation, Plan 9 Download / Foundation home, p9f.org, 2025, https://p9f.org/dl/, primary, checked 2026-10-19
  - supports: Plan 9 (all historical releases re-released under MIT; foundation continues research/community)
[S1008] Rob Pike, UTF-8 history (email to Markus Kuhn), cl.cam.ac.uk, 2003, https://www.cl.cam.ac.uk/~mgk25/ucs/utf-8-history.txt, primary, checked 2026-10-19
  - supports: Plan 9 (UTF-8 designed by Thompson/Pike Sept 1992, Plan 9 first full user)
[S1009] Michael Kerrisk, Namespaces in operation, part 1: namespaces overview, LWN.net, 2013, https://lwn.net/Articles/531114/, secondary, checked 2026-10-19
  - supports: Plan 9 (Linux mount namespaces in 2.4.19; basis of containers)
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
[S1017] Hermann Härtig, Michael Hohmuth, Jochen Liedtke, Sebastian Schönberg, Jean Wolter, The Performance of µ-Kernel-Based Systems, SOSP 1997, 1997, https://os.inf.tu-dresden.de/pubs/sosp97, primary, checked 2026-10-19
  - supports: Mach (MkLinux in-kernel ~73% / user ~62% of native vs L4Linux ~95%)
  - supports: L4 (paravirtualised Linux on L4 with ~5% throughput loss)
[S1018] Jochen Liedtke, On µ-Kernel Construction, SOSP 1995, 1995, https://cgi.cse.unsw.edu.au/~cs9242/22/papers/Liedtke_95.pdf, primary, checked 2026-10-19
  - supports: Mach (inefficiency from overloading/poor implementation, not inherent)
  - supports: L4 (minimal concept set argument)
[S1019] Apple, XNU README, apple-oss-distributions/xnu on GitHub, 2026, https://github.com/apple-oss-distributions/xnu/blob/main/README.md, primary, checked 2026-10-19
  - supports: Mach (XNU hybrid kernel: CMU Mach + FreeBSD + IOKit; osfmk = Mach subsystems)
[S1020] CMU School of Computer Science, Project Mach home page, cs.cmu.edu, 1997, https://www.cs.cmu.edu/afs/cs/project/mach/public/www/mach.html, primary, checked 2026-10-19
  - supports: Mach (project 1985-1994; derivative projects incl. Hurd, Lites, RT-Mach)
[S1021] Gernot Heiser, Kevin Elphinstone, L4 Microkernels: The Lessons from 20 Years of Research and Deployment, ACM TOCS 34(1), 2016, https://trustworthy.systems/publications/nicta_full_text/8988.pdf, primary, checked 2026-10-19
  - supports: Mach (1990s IPC ~100 µs; trend of moving services back into kernel; Chen/Bershad read as inherent)
  - supports: L4 (10-20x faster IPC; Qualcomm/Apple/OKL4/PikeOS deployment; abandoned timeouts, long IPC, clans & chiefs, recursive mapping, C++; Linux VM as middleware; capabilities from KeyKOS/EROS)
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
[S1040] Ross McIlroy, Jaroslav Sevcík, Tobias Tebbi, Ben L. Titzer, Toon Verwaest, Spectre is here to stay: An analysis of side-channels and speculative execution, arXiv:1902.05178, 2019, https://arxiv.org/abs/1902.05178, primary, checked 2026-10-19
  - supports: Singularity (speculation defeats language-enforced confidentiality; universal read gadget; Chrome moved to process isolation)
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

## Attempted but not counted (unverified)
- Mary Jo Foley, Whatever happened to Microsoft's Midori operating system project?, ZDNet, 2015, https://www.zdnet.com/article/whatever-happened-to-microsofts-midori-operating-system-project/ — fetch blocked, unverified
- eWeek, Microsoft Midori: More Details on the Non-Windows OS Emerge, https://www.eweek.com/enterprise-apps/microsoft-midori-more-details-on-the-non-windows-os-emerge.html — 403, unverified
- Redmond Magazine, Will Microsoft Take a Sip of Midori, 2008 — 403, unverified
- Accetta et al., Mach: A New Kernel Foundation for UNIX Development, USENIX Summer 1986 — TLS/403 on all mirrors, unverified
- Dr. Dobb's Dec 1992 GNOSIS/Key Logic history (archive.retro.co.za) — 402, unverified
