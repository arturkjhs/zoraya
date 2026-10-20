# Stream J — Alternatives postmortem (Multics, Plan 9/Inferno, Mach, L4/seL4, QNX, KeyKOS/EROS/CapROS/Coyotos, Singularity/Midori)

Draft for `alternatives-postmortem.md`. Source IDs S1000–S1099 (see `stream-J-sources.md`). Claims without a
verified source are marked `[unverified]`. "Lessons" are observations, not decisions.

## Multics

**What it tried to fix.** Batch-era computing: Multics aimed to be a "computer utility" that runs continuously, serves
many interactive users, and shares programs and data between them safely. The 1965 design paper lists: segmented,
paged virtual memory with named segments; dynamic linking at run time by symbolic name; reentrant shared code; a
hierarchical file system with automatic storage movement and backup; files private by default with selective
sharing enforced by hardware descriptor bits; delegated administration; and being written mostly in PL/I so it could
move to other hardware [S1001].

**What worked.**
- The hierarchical file system, dynamic linking, shared reentrant code and a kernel written in a high-level language
  became standard. Thompson credited Multics as the source of the hierarchical file system he took for Unix [S1000].
- Security held up well for its time. Karger and Schell, looking back at the 1974 Air Force penetration test, say
  Multics had essentially no buffer overflows. They give three reasons: PL/I strings with declared maximum lengths,
  hardware execute-permission bits that stopped data being run as code, and stacks that grew upward [S1003]. Multics
  was the first system rated B2, in 1985 [S1002].
- The kernel stayed fairly small: about 250K source lines in 1978 [S1000]. Karger and Schell note that SELinux's
  example policy alone is more than 2.5 times the size of the whole 1973 Multics kernel [S1003].

**What did not work.**
- Late and slow at first. The GE-645 arrived in January 1967, milestones slipped, and early versions had poor
  performance, frequent crashes and a slow compiler [S1002]. Van Vleck accepts "big and slow" for the period before
  release, but says it was not true of later Multics [S1000].
- Tied to hardware. The design needed special segmentation hardware (GE-645, then Honeywell 6180), so the
  portability promised by PL/I did not happen in practice. When the vendor stopped making competitive hardware,
  sites ended up on slow, obsolete machines [S1000][S1002].
- Even a security-focused design could be subverted. The 1974 evaluation's main finding was that malicious software
  (trap doors) could be planted in the system and shipped to customer sites [S1003].

**Why it did not achieve wide adoption.**
- *Technical reasons:* it needed expensive proprietary mainframe hardware, each sale ran to millions of dollars, and
  it could not follow the 1980s move to cheap microcomputers. The 645 had reliability problems and fell behind
  competitors built with integrated circuits [S1002].
- *Non-technical reasons:* Bell Labs left in April 1969 because it had no usable system and no end date for the
  costs [S1002][S1000]. GE sold its computer business to Honeywell in 1970. Honeywell cancelled Multics development
  in 1985 after repeated internal attempts to kill it, and Bull built no new hardware [S1002]. A participant account
  blames corporate politics (no champion among executives) rather than technical flaws or weak sales [S1000]. At
  peak there were about 77 sites. The last one shut down in October 2000 [S1000][S1002].
- *Niche:* government, military, car makers and universities. Some sites kept running for years after cancellation
  because Multics had features they could not get elsewhere [S1000][S1002].

**Lessons for our OS.**
- Language choice and hardware protection (bounded strings, no execution of data) mattered more to Multics' memory
  safety than any single kernel feature. Rust plus aarch64 permissions can play a similar role.
- A design that depends on unusual hardware (rich segmentation) lives or dies with its hardware vendor.
  Mainstream aarch64 has no Multics-style segments, so similar ideas would have to be built from paging,
  capabilities or language mechanisms.
- A security story can be mostly right and still be beaten by subverting the supply chain. Build and release
  integrity is part of the TCB.
- Adoption depended on corporate sponsorship more than technical merit. Staying "a research system" did not save
  it from decisions made in executive suites.

**Sources.** [S1000] (participant retrospective), [S1001] (primary design paper), [S1002] (participant-maintained
history), [S1003] (primary retrospective).

## Plan 9 / Inferno

**What it tried to fix.** The authors saw Unix as an old timesharing system that handled graphics, networking and
multi-machine use poorly. Their problems were "too deep to fix" in place [S1004]. Their goal was a single system
built from many machines (central file and CPU servers, cheap terminals), with every resource exposed through one
file protocol (9P), per-process namespaces, and union directories [S1004]. Inferno (Bell Labs, 1995–97) brought
the same namespace-and-9P model to small networked devices. Its programs ran on a portable VM (Dis) in a safe
language (Limbo), either hosted on another OS or as a native kernel [S1011][S1012].

**What worked.**
- UTF-8 was designed by Thompson and Pike in 1992 for Plan 9, and Plan 9 was the first system to use it throughout
  [S1008].
- Per-process namespaces and 9P. Linux added mount namespaces in 2.4.19, which led to containers [S1009]. 9P
  itself lives on in Linux v9fs, including as the virtio transport for sharing host files with QEMU guests [S1010].
- Text-file `/proc`, windows as file servers that give each window its own `/dev` files (so no tty code in the
  kernel), a dump file system with daily read-only snapshots in the namespace, and no superuser in the
  authentication model [S1004].

**What did not work (the authors' own list, [S1004]).**
- The "everything is a file" idea had limits. Process creation still used fork/exec rather than file writes, and
  network setup needed more structure than files give.
- A process cannot pass its namespace to another, so `cpu` cannot fully reproduce a terminal's changes to its
  environment.
- The file server was a separate kernel with duplicated drivers and bug fixes. The WORM jukebox, the least reliable
  hardware, held the file system metadata, and restoring from it took days. The central server limited how big an
  installation could get.
- Files in the dump could never be deleted. The authors list this among the costs of permanence.

**Why it did not achieve wide adoption.**
- *Technical reasons:* Pike estimated that 90–95% of the Plan 9 work went into honoring external standards (TCP/IP,
  HTTP, HTML, POSIX, NFS, X, ...), which left little room for new ideas. He also says new systems are expected to
  ship a Unix emulation layer first [S1005]. Because the software was mostly new rather than ported from Unix
  [S1006], users had few familiar applications. By 2000 hardware had settled on the PC, so Plan 9's portability no
  longer gave it an edge [S1005].
- *Non-technical reasons:* licensing and timing. The 1st edition (1992) was for universities only. The 2nd (1995)
  cost $350. Only the 3rd (2000) was free on the Internet, under a new Plan 9 License. The 4th (2002) used the
  Lucent Public License, and MIT relicensing came only in 2021 [S1006][S1007]. By 2000 Linux had taken the role of
  a free Unix. Pike also points to funding and short-horizon business pressure as reasons such large systems
  projects are no longer built [S1005]. Inferno was spun out to Vita Nuova in 2000. Its CEO compared it to Java and
  claimed embedded customers, but these are company claims [S1013].
- *Niche:* research and a hobbyist community (9front and the Plan 9 Foundation [S1007]). Its ideas, rather than
  its code, are what spread: UTF-8, namespaces, 9P.

**Lessons for our OS.**
- Per-process namespaces composed from file servers are the Plan 9 idea that spread most (containers, 9P in VMs).
  Unix inherited them late, as an add-on.
- The authors themselves found that files are a poor fit for process creation and for structured control (network
  setup). A uniform name space and a uniform operation set are different claims.
- The cost of external standards (network protocols, web, POSIX for a C toolchain) can take up most of the effort
  of a new OS. That budget is worth setting on purpose.
- Restrictive early licensing during the years when the free-Unix niche was open seems to have mattered as much as
  any technical factor. This is our inference, not the authors' stated conclusion.

**Sources.** [S1004] (primary), [S1005] (primary, author polemic), [S1006] (primary), [S1007] (primary), [S1008]
(primary), [S1009] (secondary), [S1010] (primary), [S1011] (primary), [S1012] (primary), [S1013] (weak, vendor interview).

## Mach (incl. NeXTSTEP / XNU and OSF/1)

**What it tried to fix.** Mach (CMU, 1985–1994 [S1020]) aimed to give Unix a new base for shared-memory
multiprocessors and networks of machines, while staying compatible with Unix. Its abstractions were tasks, threads,
ports (capability-like message endpoints), messages, and memory objects with external pagers. The plan was for much
of Unix to run as user-level servers [S1015].

**What worked.**
- Its abstractions outlived the research project. XNU (macOS/iOS) describes itself as a hybrid kernel that combines
  CMU's Mach with FreeBSD components and the IOKit driver API, and keeps Mach in its `osfmk` tree [S1019]. NeXTSTEP
  used an older Mach version [S1015].
- Mach 2.5 became the base of OSF/1 from 1989, and Mach 3.0 the base of MkLinux [S1015].
- Ports as named, transferable message rights, and user-level pagers, shaped later microkernels. Even L4 defines
  itself by how it differs from Mach. seL4 endpoints are "essentially ports" but without kernel buffering [S1021].

**What did not work.**
- In Mach 2.5, the first major release, all the Unix compatibility code was still inside the kernel. The
  microkernel split arrived only with 3.0 [S1015].
- IPC was slow. A typical one-way microkernel message cost about 100 µs in the early 1990s, which led to services
  being moved back into the kernel [S1021]. Chen and Bershad traced Mach 3.0 + Unix server against Ultrix. For the
  same work, Mach ran substantially more non-idle system instructions, each instruction cost more memory cycles, and
  system locality was worse [S1014].
- The workaround was to put the servers back in the kernel. The Utah in-kernel-server work improved RPC about 3x,
  Unix syscalls by 20%–2x, and large benchmarks by 4–13% [S1016]. But a co-located MkLinux still lost about 27% of
  native Linux AIM throughput, against about 5% for L4Linux. User-mode MkLinux lost about 38% [S1017].
- Whether this cost was inherent was disputed. Chen and Bershad's results were read as structural [S1021]. Liedtke
  argued that Mach was slow because it was overloaded and implemented poorly, not because it was a microkernel
  [S1018]. L4's results support Liedtke for IPC [S1017].

**Why it did not achieve wide adoption as a microkernel.**
- *Technical reasons:* the IPC and cache-footprint costs above [S1014][S1017]. Vendors who shipped Mach (OSF/1,
  NeXT, Apple) used it as a single-server or co-located kernel, so the microkernel's isolation benefits largely did
  not reach users [S1015][S1019].
- *Non-technical reasons:* OSF/1 was a product of the Unix wars, positioned as the rival to System V Release 4
  [S1015]. Its later commercial fate (Digital UNIX/Tru64) was decided by vendor consolidation `[unverified]`. CMU's
  project ended in 1994 [S1020]. Mach still reached very wide adoption through NeXT and Apple, but as a component
  of a monolithic hybrid, not as the multiserver design it was meant to be [S1019].
- *Niche:* Mach-derived code runs on every Apple device via XNU [S1019].

**Lessons for our OS.**
- Message passing designed for the network case (rich messages, kernel buffering, transparent remote ports) cost
  so much locally that it pushed implementers back into a monolithic structure.
- "Microkernel" turned out to be a claim about implementation quality as much as structure. Chen/Bershad's numbers
  and Liedtke's rebuttal concern the same design space [S1014][S1018].
- Mach's abstractions survived in XNU because Apple kept them as an internal structuring tool, not as an isolation
  boundary. Abstractions can last longer than the architecture they were built for.

**Sources.** [S1014] (primary), [S1015] (secondary, textbook), [S1016] (primary), [S1017] (primary), [S1018]
(primary), [S1019] (primary), [S1020] (primary), [S1021] (primary).

## L4 family and seL4

**What it tried to fix.** By the early 1990s, IPC (about 100 µs one way) was the weak point of first-generation
microkernels [S1021]. Liedtke argued that a minimal kernel offering only address spaces, threads and IPC could be
fast, and that Mach's slowness came from implementation, not structure [S1018]. His L4 IPC was 10–20x faster than
contemporary microkernels [S1021]. seL4 later added capability-based access control and a machine-checked proof
that the C implementation matches its specification, plus proofs of integrity and confidentiality enforcement
[S1021][S1022].

**What worked.**
- Performance. Linux paravirtualised on L4 lost only about 5% AIM throughput [S1017].
- Commercial deployment at very large scale. NICTA's L4-embedded became Qualcomm's protected-mode RTOS for modem
  firmware, and the authors report it running on the security processor of recent Apple iOS devices. OKL4 shipped on
  billions of devices, and PikeOS (a commercial L4 clone) is certified for avionics [S1021].
- Formal verification of a real, high-performance kernel, together with analysis of worst-case execution time
  [S1021]. seL4 is also a hypervisor. Its main deployment pattern is "incremental cyber-retrofit": run the existing
  stack in a Linux VM, then move critical pieces out into verified native components. The Boeing ULB work under
  DARPA HACMS was done this way [S1022].
- Ideas that survived are those that held up across 20 years of rewrites [S1021]: minimality, fast synchronous IPC
  for client–server calls, user-mode drivers, and capabilities (influenced by KeyKOS/EROS).

**What did not work (from the authors' retrospective [S1021]).**
- *IPC timeouts* were abandoned. There was no theory for choosing values, and in practice only zero and infinity
  were used.
- *Long IPC* (in-kernel copying of bulk data, with page faults handled during IPC) was abandoned. It added
  significant kernel complexity and was rarely used, since shared buffers did the job.
- *Clans and chiefs* (kernel-enforced IPC policy) were abandoned. The model collapsed into one chief per process, and
  many builds disabled it, which left no way to control IPC until capabilities arrived.
- *Recursive mapping database*: in real deployments it used 25–50% of kernel memory and was a potential
  denial-of-service vector. It was removed.
- *Thread IDs as IPC destinations* were replaced by port-like endpoints. *C++* was abandoned for seL4 and OKL4. The
  verification toolchain needed C, and good C++ compilers for embedded targets were scarce.
- The proofs have limits. They do not cover timing channels, some architectures are only partly verified (with the
  missing parts "waiting for funding"), and the Microkit/CapDL proofs are incomplete [S1022].

**Why it did not achieve wide adoption as a general-purpose OS.**
- *Technical reasons:* L4 is a kernel, not an OS. In practice "virtualised Linux became the de facto OS middleware"
  and most L4 systems needed at least one Linux VM [S1021]. Building native systems is hard. The whitepaper calls
  CAmkES painful to build with and costly at runtime. It also says Genode works only at a least-common-denominator
  level and has no assurance story. The Microkit is easier but requires a static architecture fixed at build time
  [S1022]. Verification also limits what the kernel can do: it must be in C, with no kernel concurrency [S1021].
- *Non-technical reasons:* business and licensing. OK Labs was sold to General Dynamics in 2012. GD closed its Sydney
  engineering office in February 2014, and the engineers spun out as Cog Systems [S1024]. seL4 was open-sourced only
  in July 2014 (GPLv2 kernel, BSD for parts), five years after the proof [S1023][S1021]. Most deployments are under
  NDA or embedded, so they do not create a visible developer community `[unverified]`.
- *Niche (successful):* baseband/modem firmware, secure enclaves, avionics/rail (PikeOS), and defense retrofit
  (HACMS) [S1021][S1022]. These buyers value isolation and certification and accept small, static systems.

**Lessons for our OS.**
- Many of L4's "abandoned" features were mechanisms that put policy in the kernel or handled rare cases
  (timeouts, long IPC, clans). They added complexity without adding generality.
- When a microkernel needs a guest Linux to be useful, compatibility becomes the bottleneck. This is relevant to how
  much our POSIX layer has to carry.
- Verification puts concrete constraints on design: language subset, no kernel concurrency, a static system
  architecture. It also leaves timing channels unproven. These are tradeoffs, not free benefits.
- Commercial success came in places where buyers paid for isolation (modems, enclaves). That did not carry over to
  general-purpose computing.

**Sources.** [S1017] (primary), [S1018] (primary), [S1021] (primary retrospective), [S1022] (primary), [S1023]
(secondary), [S1024] (primary, participant blog).

## QNX

**What it tried to fix.** QNX targets real-time and embedded systems that cannot tolerate a large, monolithic
kernel. Message passing is its basic IPC, and its documentation calls it the first commercial OS of this kind. The
microkernel provides thread-level services and message passing. File systems, networking and drivers run as
user-level "resource managers" that can be started and stopped dynamically and are reached through the POSIX
library. Qnet extends message passing transparently across a network of nodes [S1025].

**What worked.**
- Synchronous, copying send/receive/reply also handles scheduling: processes change state as they send and reply,
  and the kernel uses that state and their priorities [S1025]. This keeps the real-time model simple.
- POSIX on top of message passing. Applications see a POSIX API and the library turns calls into messages
  [S1025]. This let QNX host conventional software without making POSIX the kernel's native interface.
- Commercial longevity in safety-critical markets. BlackBerry reports QNX in more than 255 million vehicles
  (TechInsights estimate, Oct 2024), certified to ISO 26262 and ISO 21434, and licensed mostly per-unit royalty
  [S1029].
- The architecture kept evolving. QNX OS 8.0 (2024) introduced a redesigned, fully preemptible, multithreaded kernel
  for 1–64 cores, with kernel calls running in parallel unless they touch shared resources [S1026].

**What did not work.**
- Older kernels did not scale to many cores. The vendor's own 8.0 redesign implies this. The release notes do not
  say what the old bottleneck was, so the exact mechanism is `[unverified]` [S1026].
- 8.0 broke compatibility: interrupt service routines were removed (replaced by interrupt threads), shared
  synchronization objects must now be declared shared, and the mqueue implementations were replaced [S1026].
  Evolving a decades-old ABI had real costs.
- Synchronous copying IPC costs a copy on every message [S1025]. Whether that is a problem in general-purpose use
  (e.g. large-buffer I/O) is not measured in our sources `[unverified]`.

**Why it did not achieve wide adoption outside embedded.**
- *Technical reasons:* QNX was optimized for determinism and small systems. It had few desktop and server
  applications, and its own GUI (Photon) never built an ecosystem `[unverified]`. The PlayBook tablet (2010) ran a
  QNX-based OS [S1028], but our sources do not give the technical reasons the BlackBerry platform failed
  `[unverified]`.
- *Non-technical reasons:* licensing and ownership. QNX opened much of its source (2007 `[unverified]`). After RIM bought it from
  Harman in April 2010, access to the microkernel, userland, libraries and Photon was withdrawn [S1027][S1028].
  This cut off the hobbyist and academic community. Commercial royalty licensing [S1029] suits automotive buyers but
  not general-purpose adoption. Its consumer future was also tied to BlackBerry's handset business.
- *Niche (successful):* automotive ECUs and infotainment, industrial and medical. In these markets certification,
  determinism and fault containment are worth paying for [S1029].

**Lessons for our OS.**
- A message-passing microkernel with a POSIX-translating library is a proven production design. "POSIX as a
  library over native IPC" is not hypothetical.
- Certification and determinism, not raw throughput, were where QNX's design paid off commercially.
- Closing source after an acquisition ended community use overnight. Long-term governance of the code matters
  as much as its license on day one.
- Even a well-regarded microkernel needed a major kernel redesign for many cores, and its 8.0 release broke
  compatibility. This suggests planning for SMP scalability from the start.

**Sources.** [S1025] (primary), [S1026] (primary), [S1027] (secondary), [S1028] (secondary), [S1029] (primary,
vendor claim).

## KeyKOS / EROS / CapROS (and Coyotos)

**What it tried to fix.** KeyKOS (GNOSIS at Tymshare from about 1975 [S1031], then Key Logic) was a pure
capability microkernel for running a commercial service on a network. All authority was held as "keys" to objects
(pages, nodes, domains, gates), and there was no ambient authority. Formal reasoning about security (confinement)
was a design goal. Everything was persistent: the whole system was checkpointed, and the kernel kept no state that
could not be rebuilt from a checkpoint [S1030]. EROS (Shapiro et al., Penn) was the third implementation of the
architecture. It set out to show that capabilities plus a transparent single-level store could be fast on commodity
x86 hardware [S1035]. CapROS continues EROS [S1036]. Coyotos was Shapiro's successor project [S1037].

**What worked.**
- It worked in production. KeyKOS/370 ran production applications from January 1983. It hosted UNIX, VM/CMS and a
  transaction processor (KeyTXF) as guest environments, with performance described as comparable to the native
  systems [S1030]. On the Motorola 88000 it offered a binary-compatible POSIX emulation [S1035].
- IPC was fast. EROS microbenchmarks were generally comparable to similar Linux operations on a 400 MHz Pentium II
  [S1035].
- The ideas lived on: capabilities in seL4, OKL4 and Fiasco.OC, and seL4's badged endpoints copy KeyKOS
  "distinguished" capabilities [S1021]. Midori cites KeyKOS/EROS/Coyotos as influences [S1043]. Hardy notes the
  message primitives never needed critical changes [S1046].

**What did not work.**
- Hardy's own list of "warts" [S1032]:
  - The single-level store hides the RAM/disk distinction. That hurts when the kernel's paging guesses are wrong, or
    when recomputing data would be cheaper than reading it from disk.
  - Batched kernel work grows roughly linearly with RAM size and cannot be kept out of critical periods, so
    real-time support was poor.
  - There were abstraction faults and a covert channel in the space bank.
- Persistence makes things hard to get out and to upgrade. KeyKOS objects are generally not extractable from the
  platform, and Hardy's tradeoffs note puts application upgrades on a persistent platform aside as a separate issue
  [S1033]. In EROS, changing the size of checkpointed kernel tables makes earlier checkpoints unusable [S1035].
- EROS's microbenchmark results were not proof of application-level performance. The authors say a full validation
  needed application measurements [S1035].

**Why it did not achieve wide adoption.**
- *Technical reasons:* applications had to be written against the capability discipline to get its benefits.
  Ported UNIX/VM code ran as legacy guests [S1030]. Hardy himself observes that any simple application is better
  done without a new OS. The value appears only when many mutually suspicious parties interact, and that market had
  to be created [S1034].
- *Non-technical reasons:* corporate ownership and funding. GNOSIS passed from Tymshare to McDonnell Douglas, which
  judged it not strategic, and then to the start-up Key Logic `[unverified: secondary snippet only]`. The KeyKOS
  hardware ran on IBM 370 and 88000 [S1030], and the 88000 platform itself faded `[unverified]`. EROS/Coyotos was
  effectively a one-leader project. Shapiro stopped work on Coyotos and BitC when he joined Microsoft's Midori in
  2009 [S1037]. CapROS is still maintained by a very small group [S1036].
- *Niche:* transaction processing on 370 mainframes in the 1980s [S1030]. Today its ideas live on rather than its
  code.

**Lessons for our OS.**
- Capabilities without ambient authority can be implemented efficiently. Neither KeyKOS in production nor the EROS
  measurements show capabilities as the performance problem [S1030][S1035].
- Transparent orthogonal persistence moves hard problems elsewhere: paging policy, real-time behavior, schema and
  code upgrades, and getting data out. It is not a free simplification.
- Making legacy OSes guests let KeyKOS run existing software. It also meant most code got none of the capability
  benefits. This is the same tension our POSIX layer will face.
- A small team with a single leader is fragile. Coyotos and BitC ended when their leader changed jobs.

**Sources.** [S1021] (primary), [S1030] (primary), [S1031] (primary), [S1032] (primary), [S1033] (primary),
[S1034] (primary), [S1035] (primary), [S1036] (primary), [S1037] (secondary), [S1043] (primary), [S1046] (primary).

## Singularity / Midori

**What it tried to fix.** Singularity (Microsoft Research, started about 2003–04 per the 2007 paper) revisited OS design decisions in light of
safe languages and verification. It had three features [S1038]:
- *Software-isolated processes (SIPs):* isolated by type-safe code (Sing#, compiled from MSIL at install time)
  rather than by the MMU. SIPs share no writable memory and cannot load code dynamically.
- *Contract-based channels:* messages pass ownership of data through an exchange heap, enforced by a linear type
  system.
- *Manifest-based programs:* manifests declare everything a program is and needs.

Midori (about 2008–2015) was the incubation project that tried to turn this into a complete stack: language,
compiler, OS, services and applications, with asynchrony everywhere, capability-based security and safe concurrency
[S1041][S1042][S1043].

**What worked.**
- Measuring the "unsafe code tax." In the WebFiles benchmark, adding hardware protection step by step slowed it down
  by up to 33% (separate domains) and 37.7% (full ring-3 microkernel). The runtime checks for safe code cost under 5%
  [S1038]. The authors say this may be the worst case.
- A kernel mostly in a safe language: over 90% Sing#. The garbage collector was 48% of the remaining unsafe code
  [S1038].
- Midori showed a whole OS, browser and compiler in safe code. Duffy reports safety checks "within the noise" after
  optimization, and buffer overflows, data races and deadlocks largely gone [S1042].
- Object capabilities with no ambient authority. There were no mutable statics, and even the clock was a capability
  [S1043].
- Ideas moved into products: asynchrony, slices, safe concurrency, and C++ safety profiles. Duffy says existing
  ecosystems slowed adoption [S1041][S1042]. Analysts in 2010 linked Midori's concurrency principles to .NET
  [S1044].

**What did not work.**
- The trusted computing base still included a large optimizing compiler (Bartok). The authors admit it probably has
  bugs that could turn safe code unsafe, and they planned typed-assembly verification to remove it from the TCB
  [S1038].
- Sealed processes cannot load or generate code at run time [S1038]. That rules out JITs and plugins as they are
  normally built.
- Software isolation is now undermined by hardware. Google's V8 team concluded that speculative-execution attacks
  defeat all language-enforced confidentiality on current hardware. Untrusted code in the same address space can
  build a universal read gadget, so Chrome moved to process isolation [S1040]. SIPs sharing one address space (the
  low-overhead configuration) now face the same problem. This weakens the main performance argument for software
  isolation. It is "right at the time, wrong now".
- Midori's capability model had usability problems. Constructors ended up with long capability lists, downcasting
  leaked authority, and the end-user permission UX was underexplored. Duffy says it was never tested at real-world
  scale [S1043].

**Why it did not achieve wide adoption.**
- *Technical reasons:* everything had to be rewritten in the new language. Existing Windows/Win32 and C/C++ code
  could not run as SIPs (our inference from the sealed, verified-MSIL model in [S1038]), and Duffy notes that existing ecosystems made the ideas slow to move [S1042].
  Spectre later removed part of the security case for one shared address space [S1040].
- *Non-technical reasons:* Singularity was released only under an academic, non-commercial research license
  (2008) [S1039]. Midori was an incubation project. Duffy says decisions about its core technology were "not
  entirely technology-driven" nor entirely business-driven. He regrets not open-sourcing it and not publishing more
  [S1041]. It was reported as shut down by late 2015 [S1045]. A press report that it was folded into the Windows OS
  group (2013) could not be fetched `[unverified]`. Staff turnover played a part: Shapiro left in 2010 [S1044].
- *Niche:* none as a product. Its influence came through people and language features [S1041][S1042].

**Lessons for our OS.**
- Hardware isolation costs something, and the cost can be measured. Singularity's numbers are a rare apples-to-apples
  comparison, but they predate Spectre [S1038][S1040].
- Language-based isolation still adds value inside one protection domain: safety and fewer bugs. As a boundary
  between mutually untrusting code, on current CPUs, it needs to be combined with hardware isolation [S1040].
- Midori's experience suggests a full capability system needs ergonomic patterns (grouping, attenuation,
  avoiding downcasts) as much as a kernel mechanism [S1043].
- A clean-slate stack without a compatibility path stayed inside the company. Its ideas spread only by being
  retrofitted into existing ecosystems [S1041][S1042].

**Sources.** [S1038] (primary), [S1039] (secondary), [S1040] (primary), [S1041] (primary, author retrospective),
[S1042] (primary), [S1043] (primary), [S1044] (secondary), [S1045] (weak).

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
- Apple's use of L4 on the iOS security processor comes from Heiser & Elphinstone [S1021], who cite an Apple
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

**Cross-stream pointers.** The L4 "abandoned features" list [S1021] and Singularity's unsafe-code-tax numbers
[S1038] + Spectre [S1040] are likely useful for IPC (cat. 3) and kernel structure (cat. 7) entries. The Multics
buffer-overflow analysis [S1003] is relevant to security/ABI entries.
