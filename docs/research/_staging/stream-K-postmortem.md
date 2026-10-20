# Stream K — alternatives postmortem

Systems: Barrelfish; Fuchsia / Zircon (incl. Starnix); Genode; Redox; Haiku / BeOS; Theseus; Hubris; MINIX 3;
Amoeba and Sprite; Exokernel (Aegis/XOK); Oberon. Source IDs refer to `stream-K-sources.md`.

## Barrelfish

**What it tried to fix.** Shared-memory kernels like Linux and Windows scale by adding locks and fine-grained
sharing, and they assume homogeneous, cache-coherent cores. Barrelfish (ETH Zurich and MSR Cambridge, 2007 to about 2020)
proposed the *multikernel*. Each core runs its own CPU driver plus a monitor. OS state is replicated per core and
kept consistent by explicit asynchronous messages, never by shared kernel data structures. The machine is treated as a
distributed system [S1100].

**What worked.**
- The paper named a new point in the design space. Its evaluation showed that URPC messaging could match the best
  microkernel IPC without the cache and TLB costs of context switches, and that multicast tuned to the interconnect
  topology beat point-to-point protocols for TLB shootdown [S1100].
- The ideas lived on in research, not in products. "An Analysis of Linux Scalability to Many Cores" (2010) was
  partly a response to it. Popcorn Linux turned Linux into a multikernel, and LegoOS applied the model to
  disaggregated servers. Arrakis and FlexNIC were built on the Barrelfish codebase, and LXDs reused its
  message-passing protocol [S1102].
- The author's retrospective says the multikernel still fits rack-scale and disaggregated hardware, and
  heterogeneous hardware such as big.LITTLE cores and accelerators [S1102].

**What did not work.**
- The motivating prediction was wrong. The team expected hundreds to thousands of cores. Core counts grew
  gradually, and Linux and Windows kept up through incremental changes. The author now calls the emphasis on
  scalability "perhaps a mistake" [S1102].
- A purely message-oriented system was very sensitive to message-queue lengths, which caused performance anomalies.
  Fast URPC needed detailed knowledge of the cache-coherence protocol [S1100].
- Adopting the seL4 capability model for memory management was the project's biggest distraction. That model
  suited static embedded memory layouts, not PCs with dynamic memory and hot-plug, and it made heap allocation very
  complex. It was not simplified until 2016 [S1102].
- The paper itself admits that it did not evaluate complex applications, a storage system, or heterogeneous
  cores. Building a whole OS from scratch limited how far the architecture could be tested [S1100].

**Why it did not achieve wide adoption.**
- *Technical reasons:* commodity hardware stayed cache-coherent with moderate core counts, so the problem it
  solved never became urgent [S1102]. There was no binary compatibility, only partial POSIX (no thread-local storage
  in 2009) [S1100].
- *Non-technical reasons:* it was a research vehicle, never a product. The first year lacked a clear research
  agenda, and engineering effort went to work that did not advance the research [S1102]. The project is now marked
  "no longer active", and its last release was in March 2020 [S1101].

**Lessons for our OS.**
- Design premises that rest on hardware forecasts can expire. Barrelfish's lasting value came from heterogeneity
  and explicit communication, not from the core-count bet.
- A capability model built for one memory environment (static, embedded) can impose heavy costs in another.
  Memory-management capabilities need to fit dynamic allocation on aarch64 SoCs.
- A message-only design moves the performance problem into queueing and cache-line behaviour. Plan for
  measurement and tracing from day one.

**Sources.** [S1100] (primary), [S1101] (primary), [S1102] (primary, author retrospective).

## Fuchsia / Zircon (incl. Starnix)

**What it tried to fix.** Google built a new, non-Linux OS stack, publicly visible from 2016. Its kernel, Zircon,
offers about 100 syscalls, most of them non-blocking. Access to kernel objects is through per-process handles that
carry rights. Rights can only be reduced when a handle is duplicated, and handles move between processes over
channels [S1103][S1104]. Drivers, filesystems and most services run in userspace, and a "much larger OS" is
built on top [S1103]. The aims were capability-style authority, updatability, and freedom from Linux's ABI and GPL
kernel. The licensing motive is commonly reported but is not stated in the sources fetched here [unverified].

**What worked.**
- It shipped at scale. In 2021 Fuchsia replaced the Linux-based Cast OS on the first-generation Nest Hub through an
  over-the-air update that users did not notice [S1109]. Nest Hubs still ran and received Fuchsia updates in
  2023 [S1107].
- The handle and rights model is the clearest production example of a capability-leaning kernel interface [S1104].
- Starnix is a userspace "runner" that implements the Linux ABI by translating Linux syscalls into Fuchsia
  protocol messages. For example, `write` becomes `fuchsia.io.File#Write`. Starnix keeps the fd tables, pid
  allocation and AF_UNIX state itself, so Linux binaries run unmodified without a Linux kernel [S1105][S1106].
  It is written in Rust because it is a trust boundary [S1106].

**What did not work.**
- Source-level compatibility was not enough. POSIX Lite deliberately omitted global-state calls such as `kill`, and
  some important software, such as Android native modules, cannot be recompiled. This forced the project to build
  Starnix for binary compatibility [S1106]. A non-Unix kernel did not remove the need for the Linux ABI; it
  moved that need into a large userspace emulator.
- The Starnix RFC itself names semantic mismatches: futexes, asynchronous signal handlers (which have no Fuchsia
  equivalent), and inotify. It also expected the first syscall-trapping design (Zircon exceptions plus
  cross-process memory reads) to be too slow, and it cited WSL1's filesystem-performance problems as a warning
  [S1106]. Starnix's current trapping mechanism and its production performance were not verified for this report
  [unverified].
- The move of Assistant speakers to Fuchsia was abandoned in 2023. Reporting suggests one target SoC did not meet
  Fuchsia's CPU requirements, but that is the reporter's inference, not Google's [S1107].

**Why it did not achieve wide adoption.** *(Fuchsia is still active; its adoption outcome is not final.)*
- *Technical reasons:* there was no app ecosystem of its own, so it depended on Linux/Android compatibility
  (Starnix) to run existing software [S1106]. Hardware requirements limited which existing devices it could
  replace [S1107].
- *Non-technical reasons:* in 2023, 16–20%+ of a roughly 400-person team was laid off and the speaker effort was
  dropped [S1107]. Google already owns Android and ChromeOS, so a replacement must justify its cost against
  incumbents controlled by the same company. Commentators read the cuts as a scaling-back of ambition
  [S1108] (opinion).
- *Status 2026:* Fuchsia is still developed and has a published 2025 roadmap page. It runs on Nest Hub devices.
  "Microfuchsia" (2024) runs Fuchsia as a guest under pKVM/QEMU on Android, possibly for protected workloads
  [S1108]. No phone or PC deployment has been announced [S1108].

**Lessons for our OS.**
- A capability kernel plus a userspace Linux-ABI personality is a proven shape (Starnix). Its hard parts are the
  ones the RFC lists in advance: signals, futex semantics, file-change notification, and filesystem
  performance [S1106].
- Even with Google's resources, a clean kernel needed an ABI-compatibility layer to get software. Our POSIX layer
  for a C compiler is far smaller, and keeping it that size is a scope decision.
- Technical success (shipping OTA on real devices) did not secure the project. Corporate priorities did.

**Sources.** [S1103], [S1104], [S1105], [S1106] (primary); [S1107], [S1108], [S1109] (secondary).

## Genode

**What it tried to fix.** Genode (TU Dresden origins, later Genode Labs, GPLv2 from 2008 [S1112]) is an OS
framework, not a kernel. It runs on several microkernels as well as its own base-hw kernel [S1114]. Its targets are
TCBs of millions of lines, the forced choice between rigid separation kernels and unassurable general-purpose
OSes, and nondeterministic memory overcommit that can kill arbitrary processes. Its answer is a recursive tree of
components. Each parent supplies its children's resources and environment, authority comes from capabilities, and
memory and CPU budgets are explicit and can be traded along the tree [S1110].

**What worked.**
- Each application can have its own tailored TCB, and resource accounting avoids global OOM-style behaviour by
  design [S1110].
- The project has been maintained continuously, with releases every year and the Foundations book revised every
  year since 2015. Sculpt OS is a usable desktop distribution, and the 2026 roadmap aims at headless VM hosts and
  servers [S1114].
- Reuse is pragmatic: Linux drivers through DDE-Linux, plus VirtualBox, Chromium and Qt ports [S1113][S1114].

**What did not work.**
- Drivers are the bottleneck. Genode admits it would be far less useful without drivers ported from Linux (Intel
  WiFi, USB host controllers, Intel graphics). Each port is a separate project. One i.MX8 graphics port took two
  months, and each Linux version bump repeats the cost, which grows with every driver added. The old approach would
  have been "unbearable" for ARM SoCs, which led to a redesign of DDE-Linux in 2021 [S1113].
- Configuration format churn: the migration from XML to the new HID format was "the most delicate challenge" of
  2025 [S1114].

**Why it did not achieve wide adoption.** *(Genode is still active; its adoption outcome is not final.)*
- *Technical reasons:* it depends on Linux drivers for hardware coverage, so it trails Linux on new hardware
  [S1113]. Its native component model and API are not POSIX, so existing software reaches it through ports and
  emulation layers [S1110][S1114].
- *Non-technical reasons:* a small company plus volunteers, with progress "depending on community support"
  [S1114]. It is licensed AGPLv3 with a linking clause, and anyone wanting more permissive terms needs a commercial
  licence [S1111][S1112]. That model funds Genode Labs but makes Genode hard to embed in commercial products
  without a deal.
- *Status 2026:* Release 26.05 is current. Sculpt 26.10 plans to make base-hw the default kernel and drop XML
  [S1114].

**Lessons for our OS.**
- Explicit resource budgets passed down a parent tree are a worked-out alternative to overcommit and an OOM killer.
  Its cost is that every component must handle running out of quota.
- Without a driver strategy, a capability OS stays limited to a few machines. Porting Linux drivers is a
  continuing maintenance cost, not a one-time one [S1113].
- The licence affects who can adopt the system, independently of its technical merit.

**Sources.** [S1110], [S1111], [S1112], [S1113], [S1114] (all primary; no independent secondary source found).

## Redox

**What it tried to fix.** Redox (2015 onward, Rust) aims to be a Unix-like general-purpose OS. It uses a
microkernel, runs drivers as unprivileged userspace daemons, and builds on a "scheme" resource model drawn from
Plan 9 and MINIX influences. It began in 2015 as a unikernel and was later redesigned [S1115]. POSIX-like calls are
mostly implemented in the relibc library rather than as kernel syscalls [S1116].

**What worked.**
- It shows a Rust microkernel with a kernel of about 50k lines and isolated driver daemons, and it runs real ported
  software, including several COSMIC apps [S1116][S1117].
- The compatibility shim lives in userspace (relibc), so the kernel syscall interface is free to change and is
  declared unstable [S1116].
- Performance work in 2026 includes an io_uring-like ring-buffer API ("Redox Rings"), which improved NVMe I/O about
  14–15x in a benchmark that bypassed the filesystem [S1118].

**What did not work.**
- Moving code into userspace does not remove it. Redox's own book says monolithic-kernel code is "just moved to
  user-space daemons" [S1116].
- Hardware coverage is thin. As of 2025 there was no WiFi, USB covered only HID, storage and hubs, there was no
  I2C driver, and IOMMU support was not implemented. The project proposes running a stripped-down Linux under QEMU
  to get drivers for rarer devices [S1117].
- Self-hosting is still incomplete after about ten years: Cargo and rustc are unreliable and ptrace needs rework
  [S1117]. A kernel memory leak made a GCC-based test build take about 10 hours until it was fixed in 2026 [S1118].
- The 14x gain from switching to rings suggests that the earlier scheme/message I/O path left a lot of performance
  unused (inference).

**Why it did not achieve wide adoption.** *(Redox is still active; its adoption outcome is not final.)*
- *Technical reasons:* driver gaps, no Wayland (which blocks the full COSMIC desktop), partial POSIX (full POSIX
  compliance is explicitly not a goal), and no CPython [S1117].
- *Non-technical reasons:* funded by donations, Patreon, merchandise and NLnet/NGI grants, with a few part-time
  roles, and in 2025 it was still looking for an experienced kernel developer [S1117]. Its last tagged release is
  0.9.0 (2024); it now ships daily images [S1118].

**Lessons for our OS.**
- Redox is the closest peer to our profile (Rust, microkernel, Unix-like userspace via a libc shim), and its
  10-year path to self-hosting is a schedule warning.
- Design high-throughput I/O as shared-memory rings early. Adding them later was a large win, which implies the
  default path was slow.
- Drivers on aarch64 are the biggest gap. Using "recommended hardware" plus a Linux-in-VM driver host are the
  fallbacks Redox itself considers [S1117].

**Sources.** [S1115], [S1116], [S1117], [S1118] (all primary project documents; no independent secondary).

## Haiku / BeOS

**What it tried to fix.** BeOS (Be Inc., 1990s) was a clean-slate desktop OS designed for media and multiprocessor
machines. Haiku (founded as OpenBeOS on 2001-08-18, renamed in 2004) reimplements it as open source [S1120].
Its stated design is a kernel tuned for responsiveness, a "fully threaded" design, a database-like filesystem (BFS)
with indexed metadata, a rich object-oriented API, and one team that writes everything from kernel to desktop apps
for consistency [S1119].

**What worked.**
- A single integrated team and API produced a coherent, responsive system that is still developed after 25 years
  [S1119][S1121].
- Indexed file attributes and queries influenced later desktop search thinking. This report did not verify a
  documented lineage into specific systems [unverified].

**What did not work.**
- Progress has been slow. Alpha 1 came in 2009, eight years after the project started. Beta 1 came in 2018 [S1120],
  and beta 6 in August 2026, two years after beta 5 [S1121]. There is still no R1 final [S1121].
- Re-creating a complete desktop OS, including drivers and toolkit, with volunteers took decades rather than years
  [S1120][S1121].

**Why it did not achieve wide adoption.** *(Haiku is still active; its adoption outcome is not final.)*
- *Technical reasons (BeOS):* this report did not find a primary source for BeOS's technical gaps at the time, such
  as networking, hardware support or applications [unverified]. *Haiku:* it targets a model whose native
  application ecosystem disappeared with Be, and it still has no stable R1 [S1121].
- *Non-technical reasons (BeOS):* the main factors were not technical. In 1996 Apple bought NeXT for about $400M
  after talks with Be failed over price. That account comes from secondary reporting, and the figures are rumoured
  [S1125]. Be then pursued PC OEM preinstallation. It said a Hitachi dual-boot deal collapsed because of
  Hitachi's Windows licence terms, and it sued Microsoft for exclusive OEM dealing [S1123]. Be sold its assets to
  Palm in 2001 [S1123]. Microsoft settled in 2003 for about $23.5M without admitting wrongdoing [S1124].
  *Haiku:* volunteer project with one paid contract developer in 2024, funded by record but small donations
  [S1122].

**Lessons for our OS.**
- BeOS shows that distribution channels (OEM preinstallation, platform-vendor deals) can decide an OS's fate
  regardless of technical merit [S1123][S1125].
- A small team owning the whole stack gives coherence, but that same scope stretched Haiku's schedule over
  decades. Limiting what we build ourselves is a choice we have to make, not something that follows from the
  design.
- Indexed metadata in the filesystem is an idea to evaluate on its own merits. It is not tied to BeOS's fate.

**Sources.** [S1119], [S1120], [S1121], [S1122] (primary); [S1123], [S1124] (secondary); [S1125] (weak).

## Theseus

**What it tried to fix.** Theseus (Rice/Yale, OSDI 2020) targets *state spill*: one component holding state on
behalf of another, which blocks live update and fault recovery. The OS is many tiny "cells" (Rust crates)
loaded at runtime. Everything runs in a single address space at a single privilege level (SAS/SPL). Isolation
comes from Rust's type and ownership system instead of hardware. "Intralingual" design pushes OS invariants,
such as the ownership of mapped pages, into the compiler's checks [S1126].

**What worked.**
- Live evolution and fault recovery of core components. Out of 13 deterministic faults injected into IPC/ITC,
  Theseus recovered from all but two, where the tasks hung without crashing. The MINIX 3 comparison covered
  faults in its IPC layer, which MINIX 3's own recovery literature had not evaluated [S1126].
- `MappedPages`-style APIs let the compiler prevent use-after-unmap and similar errors. The authors report no
  significant performance drawbacks from the intralingual designs they measured [S1126].
- It was achieved with roughly 38k lines of Rust and about four person-years [S1126].

**What did not work.**
- In random hardware-fault injection, Theseus failed to recover from 31% of the faults that manifested, mainly
  because Rust/LLVM only emit synchronous unwinding tables [S1126].
- Unsafe code is unavoidable. Some of it is "infectious", meaning it can break isolation. Theseus must trust rustc
  and core/alloc [S1126].
- Legacy code is the main limit. Unsafe languages such as C would need hardware protection or SFI to be isolated.
  Mixing in unsafe components breaks the chain of compiler knowledge. A spill-free filesystem would break POSIX,
  so Theseus accepts spill in its FS cells [S1126].

**Why it did not achieve wide adoption.**
- *Technical reasons:* SAS/SPL rules out running untrusted native C binaries without extra mechanisms, and
  POSIX-style abstractions conflict with spill freedom [S1126]. The README calls it experimental and "not yet
  mature" [S1127].
- *Non-technical reasons:* it is a small academic project. The last commit to the main branch was on 2024-09-22,
  so it has been dormant for about two years [S1128].

**Lessons for our OS.**
- Language-based isolation removes context-switch costs, but it cannot host a C compiler's output safely. A POSIX
  layer for C needs hardware isolation, so the system would run a hybrid of both models.
- Rust panic unwinding is not a fault-recovery mechanism for hardware-level faults. Recovery coverage depends on
  compiler support for asynchronous unwinding [S1126].
- Some cell-boundary invariants (MappedPages-style ownership types) can be adopted in a conventional
  address-space kernel without adopting SAS/SPL.

**Sources.** [S1126] (primary, peer-reviewed), [S1127], [S1128] (primary, project repository).

## Hubris

**What it tried to fix.** Hubris is Oxide Computer's microcontroller OS (decision taken in March 2020), built for
the housekeeping cores in its servers. The available options lacked memory protection or had "serious problems"
[S1130]. The task set is fixed at build time and cannot change at runtime. Drivers are unprivileged tasks isolated by
the MPU. IPC is synchronous rendezvous with no kernel queues, and *leases* lend memory for large transfers. A
supervisor task restarts faulted tasks in place. Generation numbers in task IDs let peers detect a restart
[S1129][S1130].

**What worked.**
- It ships in Oxide's product as the service-processor (BMC-equivalent) firmware [S1130], and it is still actively
  developed in October 2026 [S1131].
- Static allocation lets peak memory be checked at build time and keeps the kernel out of resource allocation
  [S1129].
- It was co-designed with the Humility debugger, which reads task state and coredumps over JTAG/SWD, so the
  firmware needs no console [S1130].

**What did not work / acknowledged costs.**
- Synchronous IPC trades throughput for latency and size. Each task can have only one outstanding message, which
  limits concurrency [S1130]. A server cannot learn that a reply failed [S1129].
- There are no capabilities yet: any task can message any task, and mandatory access control is only planned
  [S1129].
- Tasks do not share code, which costs flash. Firmware ships as a whole image and field updates of parts are not
  supported [S1129].
- Unsafe Rust can still corrupt memory inside a task [S1130].

**Why it did not achieve wide adoption.** *(Hubris is still active; its adoption outcome is not final.)*
- *Technical reasons:* it deliberately targets a narrow niche (MPU microcontrollers, a static system image) and is
  not a general-purpose OS [S1129].
- *Non-technical reasons:* it was built for one company's hardware alongside its first product, without aiming
  at generality [S1130]. Wide adoption is not a stated goal [S1129][S1130].

**Lessons for our OS.**
- Restart-in-place supervision plus generation-tagged IDs is a simple, proven way to make restarts visible to
  clients.
- Rejecting asynchronous kernel messaging and requiring build-time configuration works because Hubris is
  single-purpose firmware. A general-purpose OS cannot copy those choices directly.
- Designing the debugger with the kernel from day one paid off.

**Sources.** [S1129], [S1130], [S1131] (all primary; Oxide is the system's author).

## MINIX 3

**What it tried to fix.** Monolithic kernels run all drivers in kernel mode without fault isolation, so a single
driver bug can take down the system. MINIX 3 (Vrije Universiteit) runs almost the whole OS as tightly restricted
user-mode servers and drivers on top of a minimal kernel. A *reincarnation server* detects failed components and
restarts them, transparently to applications where possible. It aims to stay POSIX-conformant [S1132].

**What worked.**
- Driver recovery works. After a block-driver crash, the file server retries the I/O. For Ethernet drivers, TCP
  retransmission hides the failure. In one measurement, recovery took a mean of 0.36 s, which was dominated by
  the TCP retransmission timeout [S1132].
- The reported overhead was 5–10% compared with the same system using in-kernel drivers, measured before any
  optimisation [S1132].
- MINIX became one of the most widely deployed OSes through an unexpected channel. Intel runs a version of it in
  the Management Engine of most recent PCs, attracted by the BSD licence [S1135].

**What did not work.**
- Recovery covers drivers, not the core. If the process manager or file server crashes (the two servers that
  together implement POSIX), the system cannot recover [S1132]. Isolation alone does not make the core
  restartable.
- The design assumes transient faults. Logical and Byzantine failures, such as a driver that reports success but
  produces garbage, are out of scope [S1132].
- Theseus later showed experimentally that faults injected into MINIX 3's own IPC layer were outside what its
  recovery handled [S1126].

**Why it did not achieve wide adoption.**
- *Technical reasons:* as a POSIX-like system it competed with Linux and BSD on their own terms (applications,
  drivers) while carrying 5–10% overhead [S1132].
- *Non-technical reasons:* it was academic and grant-funded. The public repository's last commits are from
  November 2018, and the download page still offers 3.3.0 [S1133][S1134]. Its largest deployment (Intel ME)
  came from the permissive licence. It gave the project neither feedback nor contributions, and Tanenbaum learned
  of it from the press [S1135]. Why the funding ended was not verified [unverified].

**Lessons for our OS.**
- User-mode drivers with supervised restart are cheap and effective for *drivers*. Recovering stateful core
  services (process table, VFS) needs explicit state design: checkpointing, or Theseus-style avoidance of state
  spill.
- A permissive licence can produce huge invisible deployment and nothing flowing back to the project. Whether
  that is acceptable is a values decision.

**Sources.** [S1132], [S1133], [S1134], [S1135] (primary); [S1126] (primary, independent evaluation).

## Amoeba

**What it tried to fix.** Amoeba (VU/CWI, about 1981–1996) aimed to make a network of machines appear as one
time-sharing system. Users had cheap terminals, and computation ran on a shared *processor pool*. All objects were
named and protected by sparse, cryptographically protected capabilities carrying rights bits, which servers could
validate without kernel involvement. Communication used fast RPC [S1136].

**What worked.**
- RPC performance was a "major success" [S1136]. The immutable "bullet" file server, with a separate directory
  server, made replication simple and fast, and the authors called it the most innovative part [S1136].
- Capabilities were a single mechanism for naming and protection across the network [S1136].
- An unexpected output: Python was created to administer Amoeba, because its system calls were awkward to reach
  from the Bourne shell [S1138].

**What did not work.**
- The authors called run-to-completion (non-preemptive) threads "probably the worst mistake" of Amoeba 3.0.
  Adding a print statement inside a critical region could trigger an RPC, block the thread, and break mutual
  exclusion. Run-to-completion also blocked multiprocessor parallelism. Amoeba 4.0 replaced it with explicit
  mutexes [S1136].
- Point-to-point, master/slave RPC fit Unix pipelines and multicast-style parallel workloads poorly, so multicast
  was added later [S1136].
- The first Unix emulation reused the MINIX file server as "a quick and dirty solution", and emulated programs
  could not use the bullet server [S1136].

**Why it did not achieve wide adoption.**
- *Technical reasons:* its model assumed a dedicated processor pool and LAN, unlike personal workstations and
  then PCs. It reached software through emulation of about 100 common Unix utilities, not native applications
  [S1136].
- *Non-technical reasons:* it was a research project. The project page was last updated in 1998 with release 5.3
  [S1137]. Licensing history (early commercial distribution) was not verified [unverified].

**Lessons for our OS.**
- Sparse (password-style) capabilities validated by servers avoid kernel bookkeeping, but they can be copied like
  data. That trade-off differs from kernel-held handles (Zircon, seL4) [S1136][S1104].
- Concurrency semantics that rely on programmers knowing when code blocks are fragile. Hidden RPCs make this worse
  [S1136].
- A Unix emulation layer that bypasses the native storage model creates a second-class world inside the system
  [S1136].

**Sources.** [S1136], [S1137], [S1138] (primary).

## Sprite

**What it tried to fix.** Sprite (UC Berkeley, 1984–about 1994) aimed to "do networking right" by designing the
network into a new kernel, so that a cluster of workstations behaved like one time-sharing system [S1139].

**What worked.**
- It had a network file system with aggressive client caching, which the retrospective describes as the fastest
  in the world into the 1990s [S1139].
- Transparent process migration ran in daily use: about 31% of all processing ran migrated [S1140]. Compiles sped
  up 3–6x, limited by the file servers [S1140]. Idle-host tracking evicted migrated processes when the owner
  returned [S1139].
- It presented a true single system image (one root, one password file) [S1139].
- The log-structured filesystem was its most durable idea. It reached NetApp products and flash controllers.
  Process migration came back as VM live migration, implemented at VMware by a former Sprite member [S1139].

**What did not work.**
- Process migration was complicated to build and hard to keep working as the kernel changed [S1139].
- The kernel became harder to maintain with age and staff turnover [S1139].
- Porting Sprite as a Mach 3.0 user-level server (1992) performed poorly [S1139].

**Why it did not achieve wide adoption.**
- *Technical reasons:* adoption was all or nothing. The file system could not be taken without the whole OS.
  Installation was never easy, and applications were hard to keep portable [S1139].
- *Non-technical reasons:* Unix vendors added features faster than a team of about four students could match.
  Shared libraries and SunOS/Ultrix binary compatibility consumed research time. Only about ten outside sites
  used it, NFS and AFS won, and the team decided in late 1991 to wind down. Ousterhout suggests that building it
  as an extension of an existing OS might have eased technology transfer, though it would probably have prevented
  the single-system-image work [S1139].

**Lessons for our OS.**
- Ideas that can be separated from the system (LFS) survived. Ideas that needed the whole system (single system
  image) did not. That bears on which of our mechanisms are worth making extractable [S1139].
- Features that cut across the whole kernel, like migration, need continuous use and testing or they rot as the
  kernel evolves [S1139].
- A small team chasing compatibility with moving commercial targets loses. Our "no compatibility obligation" only
  holds if the POSIX layer's scope stays fixed.

**Sources.** [S1139], [S1140] (primary; no independent secondary source fetched).

## Exokernel (MIT Aegis / XOK)

**What it tried to fix.** Fixed kernel abstractions such as VM, IPC and filesystems deny applications
domain-specific optimisation, and any new abstraction has to be emulated on top of them. The exokernel separates
*protection* from *management*. A small kernel securely multiplexes physical resources through *secure bindings*,
a *visible revocation* protocol, and an *abort protocol*. Untrusted *library OSes* implement the abstractions
[S1141].

**What worked.**
- Aegis primitives ran 10–100x faster than Ultrix [S1141]. On Xok, unmodified Unix programs on the ExOS libOS
  ran as fast as or faster than on FreeBSD/OpenBSD. The Cheetah web server, which served from the file cache with
  precomputed checksums and co-located files on disk, ran about 8x faster than the best OpenBSD server on small
  documents [S1142].
- Debugging libraries ("relinking instead of rebooting") was easier than debugging kernels. Downloaded code was
  valuable for letting the kernel safely delegate decisions, more than for raw speed [S1142].
- The idea of the kernel as a control plane, with applications on a direct data plane, returned in Arrakis:
  2–5x lower latency and about 9x throughput over Linux for a NoSQL store [S1143]. Linking Arrakis to exokernel
  lineage is this report's characterisation.

**What did not work.**
- Most of the measured gains came from the C-FFS filesystem, not from the exokernel structure. Against OpenBSD
  running the same C-FFS, Xok was comparable on 8 of 11 steps [S1142].
- Sharing state between mutually distrusting libOSes was the hardest problem. ExOS left the process map, fd
  tables, sockets, TTYs and the mount table in writable shared memory [S1142].
- The protected disk system (XN) took four designs before settling on downloaded deterministic metadata functions
  (UDFs). Higher-level invariants still needed code downloaded into the kernel [S1142].
- fork took about 6 ms against under 1 ms on OpenBSD because page tables could not be shared. Moving abstractions
  into libraries hid information the kernel needed (cache pages versus VM pages), and exposing kernel structures
  made the libOS API unstable [S1142].

**Why it did not achieve wide adoption.**
- *Technical reasons:* to get POSIX semantics such as a shared file namespace, fds across fork and signals, a
  libOS must protect shared state. That pulls much of a conventional kernel back in [S1142]. The open problems
  listed in 1997 (paging, swapping, a windowing system, fair global policy) were never closed in a deployed
  system [S1142].
- *Non-technical reasons:* it was an academic prototype. Its ideas were absorbed piecemeal as kernel-bypass and
  control/data-plane split designs rather than as an OS [S1143]. Broader industry uptake (DPDK/SPDK, unikernels)
  was not sourced here [unverified].

**Lessons for our OS.**
- Separating protection from abstraction is powerful for specialised data paths (network, storage). The cost
  appears as soon as abstractions are *shared*: namespaces, fds and pipes [S1142].
- Our POSIX compatibility layer is a library-OS problem. ExOS's list of unprotected shared tables is a concrete
  checklist of what such a layer must protect [S1142].
- Measure where the speed-up actually comes from. In 1997 it was mostly the filesystem design, not the kernel
  boundary [S1142].

**Sources.** [S1141], [S1142], [S1143] (primary, peer-reviewed).

## Oberon

**What it tried to fix.** Project Oberon (Wirth and Gutknecht, ETH, 1986–89, revised 2013) aimed to build an
entire workstation system — OS, compiler, and later the CPU in Verilog — that one person could understand as a
whole [S1144][S1145]. The core OS is about 4,600 lines and the compiler about 2,900 [S1144].

**What worked.**
- Extreme leanness and the module system. The book itself is the proof that a complete system can be explained in
  full [S1144].
- The text-as-command user interface, with tool texts as menus, directly inspired Plan 9's Help and Acme. Pike
  liked its simplicity and use of the mouse buttons [S1146].
- Its garbage-collected, type-safe single-language design anticipated the language-based OSes (Singularity,
  Theseus) [S1144][S1126].

**What did not work.**
- It runs a single process with a central polling loop. Commands run to completion, cannot interact with the user,
  and there are deliberately "no protection mechanisms" because tasks cannot interleave [S1144].
- Pike called the single-process design with weak networking "an architectural throwback" [S1146].
- Long jobs block the machine. Wirth suggests delegating them to compute servers [S1144].

**Why it did not achieve wide adoption.**
- *Technical reasons:* it is tied to a single language (Oberon), has no hardware isolation, no preemption and no
  multi-user support, and is incompatible with existing software [S1144][S1146].
- *Non-technical reasons:* it was designed for teaching and understanding, not for market. Wirth himself wrote in
  2013 that proposing a system competing with worldwide "standards" would be preposterous, because very few people
  would use it [S1144].

**Lessons for our OS.**
- Whole-system comprehensibility is achievable only by refusing features (multiprocessing, protection,
  compatibility). Every one of those our OS keeps adds size.
- Cooperative single-thread designs remove whole bug classes but cannot host untrusted or long-running programs,
  which a C compiler workload needs.
- A UI idea can outlive its OS when it can be ported independently (Oberon to Acme) [S1146].

**Sources.** [S1144], [S1145], [S1146] (primary).

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
