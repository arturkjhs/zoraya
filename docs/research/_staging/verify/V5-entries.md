### DM-061: Priority inversion; priority inheritance as an afterthought (Mars Pathfinder)

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** VxWorks, POSIX threads (all implementations), Linux (rt-mutex, PI futexes), most general-purpose kernels
- **Classification:** contested
- **Confidence:** high

**What the design is.** Mutexes ignore priority by default. A high-priority thread waiting on a lock held by a low-priority thread can be delayed without bound while medium-priority threads preempt the holder. In POSIX the default mutex protocol is `PTHREAD_PRIO_NONE`. Inheritance (`PRIO_INHERIT`) and ceiling (`PRIO_PROTECT`) are optional features [S363].

**Why it was chosen.** Plain mutexes are cheaper and simpler. Priority inheritance requires tracking the owner, walking PI chains and boosting priorities in the scheduler [S359]. Most workloads are not hard real-time. The issue itself was understood early: Lampson and Redell described it for monitors in 1980 [S361], and Sha, Rajkumar and Lehoczky formalised inheritance and priority ceiling in 1990 [S358].

**Why it is a problem.** Mars Pathfinder (1997) reset repeatedly. A high-priority bus task waited on a mutex held by the low-priority meteorological task, which medium-priority tasks kept preempting. Priority inheritance existed in VxWorks but Wind River had left it out of the default `select()` mutex "for optimum performance". JPL reproduced the fault on a ground replica in under 18 hours and fixed it by patching a global configuration variable on the spacecraft; Reeves: "We just missed it" [S357]. Linux added rt-mutexes and PI futexes so user-space pthread mutexes can avoid unbounded inversion [S360]. Even so, the maximum PI chain depth depends on all lock nesting and is very hard to work out [S359].

**Why it was not fixed.** It is a contested design point, not just inertia. Yodaiken argues PI is unreliable or intrusive. He holds that the original paper's basic algorithm is wrong, and that PI papers over designs that ignore how priority and mutual exclusion interact. RTLinux omitted it on purpose [S361]. Locke answers that the technical points are not new and generally correct but the conclusions are a strawman, and that PI is a legitimate tool, as Pathfinder showed [S362]. Both authors wrote as executives of competing real-time Linux vendors (FSMLabs and TimeSys) [S361][S362].

**Alternatives that exist.** Priority ceiling, which bounds blocking to one critical section and prevents deadlock [S358]. Yodaiken's preferred options: short atomic critical sections, removing contention (for example flip-buffers), or a server thread that handles requests in priority order [S361]. IPC-level priority propagation in synchronous-message systems such as QNX (see GR-018). In seL4's MCS extensions, a passive server runs on the scheduling context that the caller donates with `seL4_Call` [S2401].

**Cost of the alternatives.** Ceiling requires static analysis to know the highest-priority user of every lock in advance [S361][S362]. Ceiling emulation also changes the thread's priority on every acquisition, even without contention [S362]. PI adds kernel complexity and per-lock overhead [S359], [S360]. Ignoring the problem leaves latent field failures, as with Pathfinder.

**Relevance to our OS.** medium — the compositor and input/audio paths are latency-sensitive, so the choice of default lock protocol and of priority propagation across IPC is a real design decision.

**Sources.** [S357], [S358], [S359], [S360], [S361], [S362], [S363], [S2401]

### DM-062: Kernel memory model specified after the fact (LKMM)

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** Linux; C/C++ before C11 (pthreads era); any kernel targeting multiple weakly ordered ISAs
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** For most of its life, Linux defined the meaning of concurrent memory accesses only through prose (`memory-barriers.txt`) plus per-architecture barrier implementations. A formal, executable Linux kernel memory model (cat/herd) arrived in 2018 [S364]. The prose document still says it is "not a specification" and is incomplete [S365].

**Why it was chosen.** When SMP Linux appeared, no language-level memory model existed. C had none until C11. Kernel developers needed working barriers across many architectures immediately, and formal tools for weak memory were research prototypes. Torvalds argued that a 500-page spec is hard to apply to a 15M-line project that knowingly works outside specs, and that C11's model cannot simply be adopted by the kernel [S364].

**Why it is a problem.** Linux targets more than 30 architectures with differing weak models (Alpha, ARM, Power, Itanium, MIPS…). That generated long LKML debates about what code may assume [S364]. Prose leaves room for ambiguity. The formal model resolved ambiguities in RCU documentation and `memory-barriers.txt`, and informed fixes to code wrongly relying on fully ordered lock-unlock pairs and to ARM64 lock/fence combinations [S364]. Alpha's dependency-ordering quirk forced a barrier inside `READ_ONCE()` (since v4.15) [S365]. More generally, threads implemented as a library on a compiler with no model cannot guarantee correctness, because ordinary optimisations can introduce races [S366].

**Why it was not fixed.** The kernel's primitives (`READ_ONCE`, `smp_mb`, RCU) predate C11 and do not map cleanly onto it. The kernel chose its own model rather than converge [S364].

**Alternatives that exist.** Start from a language memory model: C11/C++11, and Rust, whose atomics follow C++20's rules [S2400]. Formalise the kernel's own primitives up front with tools such as herd [S364]. Support fewer architectures, or only those with stronger guarantees.

**Cost of the alternatives.** Language models may not express what kernels need, such as dependency ordering and barriers C11 lacks [S364]. Rust's model omits C++'s "consume" ordering, so dependency ordering is not covered either [S2400]. Formal models need specialist maintainers, and even the kernel's model is described as its maintainers' collective opinion, not an infallible oracle [S365].

**Relevance to our OS.** high — a Rust aarch64 kernel inherits Rust's atomics model for free, but device memory, DMA, page-table updates and any custom primitives (RCU-like, seqlocks) still need written ordering rules from day one.

**Sources.** [S364], [S365], [S366], [S2400]

### DM-063: 32-bit `time_t` and the year 2038

- **Category:** Time, clocks and scheduling
- **Systems affected:** Unix/POSIX ABIs on 32-bit platforms, Linux kernel and C libraries, filesystems with 32-bit on-disk timestamps
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Unix time is a count of seconds since 1970. On 32-bit ABIs it was stored as a signed 32-bit `time_t`, which overflows in January 2038. The width is baked into structs (`timeval`, `timespec`, `input_event`), ioctls, syscalls and on-disk formats [S531].

**Why it was chosen.** On 1970s hardware, a machine word was cheap and the range (to 2038) looked far beyond the system's expected lifetime. Signed values allowed dates before 1970.

**Why it is a problem.** The fix is a whole-ABI transition, not a type change:
- **Kernel.** Linux 5.6 (2020) was the first kernel usable as a base for 32-bit systems that must run past 2038 [S531].
- **Filesystems.** ext4 with small inodes, ext2, XFS (at the time) and UFS keep signed 32-bit timestamps on disk [S531].
- **User space.** User space must be rebuilt [S531]. Runtimes that make syscalls directly (futex, seccomp) must move to new time64 syscalls [S531]. Some interfaces, most importantly `struct input_event`, cannot be fixed compatibly and fall back to CLOCK_MONOTONIC or unsigned 32-bit times, which only pushes the limit to 2106 [S531].
- **glibc.** glibc 2.34 (2021) made 64-bit `time_t` opt-in through `_TIME_BITS=64`, which also requires `_FILE_OFFSET_BITS=64`, and left the default at 32 bits [S532].

Any library whose ABI includes `time_t` changes ABI when this is switched, which is why distributions treat it as a mass transition. Debian's 2023–2024 "t64" transition found about 495 library packages changing ABI and roughly 5,000–6,000 dependent packages needing rebuilds [S2403].

**Why it was not fixed.** ABI stability. Every struct and syscall containing time needed a duplicate, and distributions had to coordinate library rebuilds [S531][S532][S2403].

**Alternatives that exist.** 64-bit `time_t` on all 64-bit ABIs, and the time64 syscall set on 32-bit Linux [S531]. Windows FILETIME is a 64-bit count of 100 ns units since 1601 [S2402].

**Cost of the alternatives.** Larger structs. On 32-bit Linux, a decade of duplicated interfaces and coordinated rebuilds [S531][S532][S2403].

**Relevance to our OS.** medium — the OS is 64-bit-only, so `time_t` width is easy. The lesson is to keep time types (width, epoch, resolution) out of fixed-layout ABIs and on-disk formats unless they are versioned.

**Sources.** [S531], [S532], [S2402], [S2403]

### DM-064: Leap seconds in POSIX time

- **Category:** Time, clocks and scheduling
- **Systems affected:** POSIX (specification), Linux, all NTP-synchronised systems
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** POSIX defines "seconds since the Epoch" so that every day has exactly 86400 seconds. It leaves the relationship to actual UTC unspecified [S533]. UTC, however, inserts leap seconds to track Earth's rotation, so a POSIX clock must repeat or skip a second, or be smeared [S534][S535].

**Why it was chosen.** It makes calendar arithmetic simple and table-free: converting a timestamp to a date needs no leap-second table and gives the same result on every machine.

**Why it is a problem.** Every leap second exercises a code path that is used almost never:
- **2012.** The Linux timekeeping code did not notify hrtimers. CLOCK_REALTIME timers fired a second early and caused futex load spikes in MySQL, Java and Firefox [S536].
- **2017.** A time difference that should never have been negative went negative at the leap second and panicked Cloudflare's DNS server [S537].

Workarounds diverge. Google has smeared since 2008, using different smear shapes over time. Others smear over 1,000 s (UTC-SLS), 2,000 s (Bloomberg) or a configurable cosine (Meinberg). A smeared clock is up to about 0.5 s from UTC, and Google asks other smearers to adopt one common smear [S534]. The CGPM cited the risk of serious malfunctions from these discontinuities, the uncoordinated handling methods, and a possible negative leap second that has never been tested [S535].

**Why it was not fixed.** POSIX cannot represent 23:59:60 without breaking the 86400-second invariant that existing code relies on [S533]. Linux offers CLOCK_TAI, but it depends on NTP to set the offset [S538].

**Alternatives that exist.**
- Leap smearing [S534].
- Separate TAI and monotonic clocks [S538].
- The policy fix: the CGPM resolved in 2022 to raise the UT1-UTC tolerance by 2035, effectively ending leap seconds [S535].

**Cost of the alternatives.** A smear makes clocks wrong by up to half a second relative to UTC for a day, and different smear policies do not match each other [S534]. TAI-based system time needs a leap table to display civil time.

**Relevance to our OS.** medium — leap seconds are being phased out, but the general lesson stands. Define which time scale the kernel exposes, and never let civil-time discontinuities reach interval timers.

**Sources.** [S533], [S534], [S535], [S536], [S537], [S538]

### DM-065: Wall clock vs monotonic clock misuse; monotonic clocks added late

- **Category:** Time, clocks and scheduling
- **Systems affected:** POSIX/Unix (`gettimeofday`, `time`), Linux, language runtimes (Go before 1.9); contrast Windows QPC
- **Classification:** mistake
- **Confidence:** high

**What the design is.** For decades the default Unix time APIs returned only wall-clock time, which can be stepped by an administrator, NTP or leap seconds. `clock_gettime` dates from POSIX.1-1996, but CLOCK_MONOTONIC stayed an optional feature until POSIX.1-2024 made it mandatory. Linux added MONOTONIC_RAW in 2.6.28 and BOOTTIME (which includes suspend) only in 2.6.39 [S538].

**Why it was chosen.** Early hardware had a single time-of-day clock and no stable high-rate counter. On Windows, early QPC was unreliable on some multi-core systems with non-invariant or unsynchronised TSCs, for example [S541]. One clock looked simpler.

**Why it is a problem.** Programs measure intervals by subtracting wall-clock readings, and those intervals go negative or jump.
- **Prevalence.** A survey of Go code estimated that about 29% of `time.Now` uses were subtractions or comparisons exposed to wall-clock jumps [S539].
- **Incident.** Cloudflare's 2017 DNS outage was a negative duration from `time.Now()` [S537].
- **Too many clocks.** Linux now has several monotonic clocks with different suspend semantics. In 2018 an attempt to make CLOCK_MONOTONIC include suspend time was reverted: systemd watchdogs killed daemons after resume, journald declared journals corrupt, and NetworkManager and MATE misbehaved [S540]. Once programs depend on a clock's documented semantics, the clock cannot be fixed.

**Why it was not fixed.** Existing APIs could not change meaning [S540]. The fixes were new clocks [S538] and runtime-level changes. Go embedded a monotonic reading inside `time.Time` (shipped in Go 1.9 [S2404]) precisely so that existing code would be fixed without edits. Its designers argued that two APIs that behave the same 99% of the time invite rare failures, which a leap second triggers on every machine at once [S539].

**Alternatives that exist.**
- Windows QPC: monotonic, unaffected by DST, leap seconds or time setting, kept separate from the UTC API [S541].
- Go's dual-reading `Time` [S539][S2404].
- Linux's explicit clock IDs [S538].

**Cost of the alternatives.** Dual readings cost extra clock reads and a larger struct [S539]. Multiple clocks force every API with a timeout to say which clock it means. The suspend question (count it or not) has no single right answer [S540].

**Relevance to our OS.** high — the timeout and deadline types in the native syscall ABI can be monotonic-only by construction, with suspend semantics specified up front. On Arm, Windows already bases QPC on the Arm Generic Timer's system counter, which we can use the same way [S541].

**Sources.** [S537], [S538], [S539], [S540], [S541], [S2404]

### DM-066: Time zones via `TZ` and process-global state

- **Category:** Time, clocks and scheduling
- **Systems affected:** POSIX/C library (`tzset`, `localtime`, `setenv`), glibc, Rust ecosystem on Unix
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** The C library's local time depends on the `TZ` environment variable (or `/etc/localtime`), read by `tzset`. `tzset` is called implicitly by conversion functions and stores its results in the globals `tzname`, `timezone` and `daylight` [S542]. There is one time zone per process, held in global mutable state.

**Why it was chosen.** It suited single-threaded processes with one user at one location, and passing configuration through the environment was idiomatic Unix.

**Why it is a problem.**
- **Globals are unsafe.** Reading the globals can be undefined behaviour in multithreaded programs [S542].
- **Hidden environment reads.** Time conversion reads the environment, while `setenv`/`unsetenv` are MT-Unsafe [S234]. If any thread changes any environment variable while another formats a local time, the result can be a crash. Rust's chrono had a segfault advisory (RUSTSEC-2020-0159, related CVE-2020-26235): a dangling-pointer dereference triggered when an environment variable is set in another thread, possibly by a third-party library [S544].
- **Language workarounds.** Rust eventually made `std::env::set_var` `unsafe` in the 2024 edition [S236].
- **Single zone.** Setting `TZ` changes the behaviour of the whole program, so using several zones at once needs a separate API [S546].

**Why it was not fixed.** The interface is in ISO C and POSIX, and the globals are part of the ABI [S542]. Zone-object variants exist only outside the standards: NetBSD's page lists only `tzset` as POSIX-conforming, not `tzalloc` or the `_rz`/`_z` functions [S546].

**Alternatives that exist.** NetBSD `tzalloc`/`localtime_rz` pass an explicit `timezone_t` object, so many zones can be used without touching `TZ` [S546]. Languages with explicit zone objects do the same [unverified for specific libraries].

**Cost of the alternatives.** Every call site has to pass a zone, and the system zone needs some discovery mechanism that is not ambient environment.

**Relevance to our OS.** medium — time zones are user-space policy. The lesson for the OS is to avoid process-global mutable configuration (environment) that library code reads implicitly. The POSIX layer must still emulate `TZ`.

**Sources.** [S542], [S234], [S544], [S236], [S546]

### DM-067: Fixed periodic timer tick (HZ)

- **Category:** Time, clocks and scheduling
- **Systems affected:** Linux (pre-NO_HZ and still in some configurations), Windows (global timer resolution), classic Unix
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** The kernel takes a periodic interrupt at a fixed rate (HZ) on every CPU, whether or not there is work. It uses the interrupt for timekeeping, timers and time-slice preemption [S547]. Time is often counted in ticks (jiffies) [S549].

**Why it was chosen.** It is simple. One periodic hardware timer drives everything. Linux versions from the 1990s and early 2000s could not omit ticks at all. It is still the right choice for heavily loaded systems whose idle gaps are short [S547].

**Why it is a problem.**
- **Energy.** A periodic tick wastes energy on idle systems. The kernel documentation says a battery device can drain two to three times faster [S547].
- **Virtualization.** The same document says a mainframe running 1,500 guests might lose about half its CPU time to unneeded ticks [S547].
- **Jitter.** The tick adds jitter to real-time and HPC work [S547].
- **Wraparound bugs.** Tick counters wrap, so Linux deliberately starts jiffies 5 minutes before the 32-bit wrap to make wrap bugs show up early [S549].
- **Windows.** A related mistake: before Windows 10 2004, `timeBeginPeriod` set a global timer resolution. Any one process asking for 1 ms raised interrupt frequency for every process and could prevent power-saving states [S548].

**Why it was not fixed.** It has been retrofitted, at a cost. Tickless idle slows idle entry and exit slightly. NO_HZ_FULL needs at least one housekeeping CPU and RCU callback offloading, works only with exactly one runnable task, keeps a residual tick about once per second, and needs a reboot to reconfigure [S547]. Windows changed `timeBeginPeriod` semantics only in 2020, and again for hidden windows in Windows 11 [S548].

**Alternatives that exist.** One-shot, deadline-programmed timers (dynticks) [S547], timer coalescing, and per-process timer resolution [S548].

**Cost of the alternatives.** More clock reprogramming. Higher latency for real-time on some hardware. Complex interactions with RCU, load balancing and accounting [S547].

**Relevance to our OS.** high — a new aarch64 kernel can be tickless from the start using the Generic Timer's compare registers, and can account time in nanoseconds rather than ticks. Retrofitting this later is costly, as Linux shows.

**Sources.** [S547], [S548], [S549]

### DM-068: General-purpose scheduler design churn (O(1) → CFS → EEVDF; sched_ext)

- **Category:** Time, clocks and scheduling
- **Systems affected:** Linux; implicitly any OS with a single general-purpose scheduler
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Linux has one in-kernel general-purpose fair scheduler for all workloads, from phones to servers. Ingo Molnar's O(1) scheduler was merged during 2.5 development to fix the 2.4 scheduler's scalability problems [S2405]. CFS, merged in 2.6.23 (2007), replaced the O(1) scheduler's interactivity code [S553][S2407]. EEVDF replaced CFS starting in 6.6, and the transition was completed in 6.12 [S551][S2406]. sched_ext, merged in 6.12 (November 2024), adds a scheduler class whose policy is supplied at runtime as BPF programs [S550][S2406].

**Why it was chosen.** One scheduler means one place to fix bugs, and no fragmentation of tuning across vendors. CFS was conceptually simple, a weighted fair queue on a single CPU [S552].

**Why it is a problem.**
- **Heuristics that kept breaking.** The O(1) scheduler's interactivity estimator was tuned repeatedly, and fixing one corner case opened others. This led to Kolivas's fairness-based RSDL and then to CFS [S2405].
- **Multicore bugs.** Multicore and NUMA workarounds made CFS very complex. Lozi et al. found four bugs that left cores idle while threads waited: 13–24% losses on typical workloads, 138x in a corner case. The bugs evaded conventional tools [S552].
- **No way to express latency.** CFS could not express latency needs. It relied on fragile heuristics, and latency-sensitive tasks were pushed toward privileged real-time classes [S553].
- **Churn.** EEVDF replaces heuristics with deadlines and per-task slice requests via `sched_setattr` [S551][S553]. That is the third general-purpose algorithm in about two decades (O(1), CFS, EEVDF), and the second full replacement 16 years after CFS.

**Why it was not fixed.** This is a governance question as much as a technical one.
- **Out-of-tree pressure.** Out-of-tree desktop schedulers such as Kolivas's BFS claimed better interactivity and openly gave up NUMA and large-SMP scaling. Kolivas doubted BFS would ever be accepted into mainline [S555] (weak). His earlier RSDL lost to CFS in a selection that was itself controversial [S2405].
- **Contested merge.** Zijlstra objected to the first sched_ext posting, noting that Torvalds had rejected loadable schedulers several times before [S2408]. In 2024 Torvalds said he planned to merge it for 6.11, and Gleixner asked him to wait three months to integrate it collaboratively [S554]. It landed in 6.12 [S2406].

**Alternatives that exist.**
- **Pluggable policy.** Pluggable scheduling policy with a safe fallback: sched_ext reverts to the fair class on error, stall or SysRq-S [S550].
- **Clearer policies.** Better-defined algorithms (EEVDF) [S551].
- **Per-workload schedulers.** Per-workload or user-level schedulers, which is the argument for sched_ext [S550][S2406].

**Cost of the alternatives.** The two positions:
- **Pro-pluggable.** Experimentation and workload-specific policies without kernel forks [S550][S2406].
- **Anti-pluggable.** Fragmentation, and improvements that never reach the default scheduler. Zijlstra's objection placed sched_ext in the line of loadable schedulers that Torvalds had long refused, made worse by depending on BPF [S2408]. Gleixner's complaint was about integration quality more than the idea itself [S554].
- **Shared risk.** A single scheduler concentrates risk, as the wasted-cores bugs show [S552].

**Relevance to our OS.** medium — the choice between one built-in policy and a minimal kernel mechanism with policy elsewhere (in user space or loadable) should be explicit early, including how a desktop compositor expresses latency needs.

**Sources.** [S550], [S551], [S552], [S553], [S554], [S555], [S2405], [S2406], [S2407], [S2408]

### DM-069: Kernel TTY layer and line disciplines

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Unix/POSIX (termios), Linux, BSDs, macOS/XNU
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** The kernel has a TTY subsystem between a character device (UART, virtual console, pty) and processes. A pluggable *line discipline* (default N_TTY) does canonical-mode line editing, echo, CR/LF translation, flow control (^S/^Q), and turns control characters into signals. Programs configure it through termios ioctls [S800]. Other line disciplines (for example n_gsm for GSM 07.10 multiplexing) can be attached with an ioctl [S819].

**Why it was chosen.** Terminals were teletypes and serial terminals. Handling characters as they arrived, echoing them, and editing lines in the kernel meant every program, even `cat`, got usable interactive input without its own editing code. Pseudo-terminals later reused the same machinery so terminal emulators and remote logins looked like serial lines [S800].

**Why it is a problem.** (1) *Complexity and bugs in privileged code.* Åkesson calls the subsystem "a twisty little mess of special cases" [S800]. CVE-2014-0196 was a race in `n_tty_write` that corrupted memory and could escalate privileges, and it is in CISA's Known Exploited Vulnerabilities catalog [S817]. (2) *Rarely used disciplines are reachable by unprivileged users.* The kernel autoloaded any requested ldisc module for any user. In 2019 Greg Kroah-Hartman posted a switch to stop this but kept the default on [S818]; it exists as `CONFIG_LDISC_AUTOLOAD`, still `default y` [S803]. CVE-2023-6546, a use-after-free in n_gsm that an unprivileged user can reach, shows the risk [S819]. (3) *Locking.* Removing the Big Kernel Lock from the TTY layer took more than one attempt, and the second (2010) series touched 54 files [S820]. In 2009 the maintainer resigned after a TTY regression broke kdesu and Linus Torvalds objected to breaking user space [S821]. (4) *The editing is too weak to be used.* Serious interactive programs switch to raw mode and use readline-style libraries [S800]. Microsoft's console team makes the same point: advanced editing is only possible when the application does it itself [S815].

**Why it was not fixed.** termios and pty semantics are POSIX ABI used by shells, editors, ssh and every terminal emulator. Even the ldisc-autoload restriction defaults to the old behaviour "to not break working systems" [S818].

**Alternatives that exist.** Plan 9 kept only minimal erase/kill editing in `/dev/cons`. Raw mode is switched by writing `rawon` to a `consctl` file instead of using ioctls [S822]. The window system `rio`, a user-space file server, provides each window's `cons`/`consctl` [S823]. Plan 9 is a research system with a small user base, so this design has far less field experience than termios. Windows hosts the console in user mode: it moved from CSRSS to conhost.exe in Windows 7 "for security and reliability reasons" [S816], and ConPTY added a pseudo-console [S814].

**Cost of the alternatives.** A user-space terminal service has to be on the path of every interactive byte. The Windows design carried its own large problems (see DM-073). Plan 9's model does not provide POSIX termios, so ported software needs an emulation layer.

**Relevance to our OS.** high — the POSIX layer must satisfy `isatty`/termios for compilers and shells, and we choose whether that is a kernel object or a sandboxed user-space service.

**Sources.** [S800], [S803], [S814], [S815], [S816], [S817], [S818], [S819], [S820], [S821], [S822], [S823]

### DM-070: Process groups, sessions, controlling terminals, job control, `SIGHUP`

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS/XNU
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Processes form a strict two-level hierarchy: sessions contain process groups (jobs). A session has at most one controlling terminal, and that terminal has one foreground group. Background reads raise SIGTTIN, and background writes raise SIGTTOU when TOSTOP is set. Keyboard signals go to the foreground group [S801]. On terminal hangup the kernel sends SIGHUP to the session leader. When the session leader (controlling process) exits, SIGHUP goes to the foreground group. A newly orphaned group with stopped members gets SIGHUP followed by SIGCONT [S154][S825].

**Why it was chosen.** A user had one physical terminal and needed to run, suspend and resume several jobs on it. When a dial-up line hung up, the user's processes had to be cleaned up. Process groups and SIGHUP did both with little kernel state. POSIX standardized a merge of System V (SIGHUP when the controlling process ends) and 4.2BSD (daemon plus `vhangup()`) behaviour [S825].

**Why it is a problem.** The lifetime of a process is tied to the lifetime of a terminal, and detaching takes ritual. The ritual is: fork, let the parent `_exit`, call `setsid` in the child (because `setsid` fails with EPERM for a group leader), and avoid getting a controlling TTY again by using `O_NOCTTY` [S801][S154]. Keeping a job alive after logout depended on ignoring SIGHUP or using `nohup`. When systemd 230 switched to cgroup-based logout cleanup with SIGTERM/SIGKILL, those idioms stopped working. screen and tmux broke, and distributions disabled the feature at build time [S824]. The opposite failure is a security one. Processes that keep the user's controlling terminal across a privilege change can inject input into it (DM-072). The fix for the SELinux sandbox escape was to create a new session [S807], but util-linux could not apply `setsid` to `su` without breaking it [S808]. The orphaned-group rules exist only to rescue stopped jobs that would otherwise "languish forever" [S825]. That is a sign of how much special-casing the model needs.

**Why it was not fixed.** The semantics are in POSIX and every job-control shell relies on them. Systemd's replacement is layered on top and did not remove them [S824].

**Alternatives that exist.** In Plan 9, windows replace job control. rio turns DEL and hangup into *notes* delivered to a process whose pid is given explicitly when the window is created. Its manual describes no foreground/background terminal arbitration [S823]. On Linux, systemd tracks sessions with cgroups instead of terminals. Poettering's argument was that leaving processes behind after logout is a security problem as well as untidy [S824]. Windows console control events (`SetConsoleCtrlHandler`/`GenerateConsoleCtrlEvent`) are a separate signalling channel [S815].

**Cost of the alternatives.** Cgroup-based cleanup broke long-standing user workflows and needed an explicit "linger" opt-in [S824]. Plan 9 gives up ^Z-style suspension on a single terminal. Each alternative still needs some "this work belongs to this login" grouping.

**Relevance to our OS.** medium — the POSIX layer must emulate job control for shells, but native process lifetime should probably not depend on a terminal.

**Sources.** [S801], [S154], [S807], [S808], [S815], [S823], [S824], [S825]

### DM-071: In-band escape sequences for terminal control; escape-sequence injection

- **Category:** Terminals, sessions and consoles
- **Systems affected:** all VT/ECMA-48 terminals and emulators (xterm, iTerm2, mintty, rxvt, PuTTY, Windows Terminal, …); every OS that uses them
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Terminal control is done by control functions "embedded in character-coded data" (ECMA-48) [S813]. The terminal parses the program's output stream for ESC/CSI/OSC sequences that move the cursor, set colours and titles, and so on. Some sequences make the terminal *reply* by writing data into the input stream.

**Why it was chosen.** One byte stream carries everything. It works over serial lines, pipes, sockets and ssh without any RPC layer. Microsoft names this remoting property as the main reason it now recommends VT over its own console API [S815].

**Why it is a problem.** Any program that prints untrusted data, such as logs, HTTP responses, filenames or git metadata, passes commands to the terminal. In 2003 H D Moore showed that title-reporting sequences placed attacker text on the command line in xterm, dtterm, rxvt, Eterm and PuTTY, and that Eterm/rxvt "screen dump" could write arbitrary files. The delivery paths were syslog, Apache logs, and wget/curl/ftp output [S810]. Twenty years later Leadbeater reported about ten more CVEs of the same "echoback" kind, several of them allowing code execution [S811]. Examples: DECRQSS replies in iTerm2, mintty and SwiftTerm, where the bug was copied from erroneous xterm documentation; ConEmu title reporting; Windows Terminal OSC 9;9; urxvt input reaching Perl `eval`. CVE-2022-45063 is a concrete case: an xterm OSC 50 reply containing ^G ran commands in zsh vi mode [S812]. The root causes are structural. Parsers deviate from the VT state machine, risky queries are on by default, and the defence ("escape ESC in untrusted output") falls on every program that writes to a terminal [S811].

**Why it was not fixed.** The in-band channel *is* the protocol. Removing replies or sequences breaks terminal-aware software. Fixes are per emulator and per feature, and the same bugs come back [S811].

**Alternatives that exist.** The classic Windows Console API is out-of-band, with typed calls to a console host [S815][S816]. Other mitigations: mosh keeps the terminal state on the server side, tmux keeps `allow-passthrough` off, and emulators can disable reporting by default [S811].

**Cost of the alternatives.** An out-of-band API cannot be carried over a plain byte channel. Windows had to build ConPTY to translate its API back into VT, and the translation is imperfect [S814][S815]. Filtering at the emulator removes features that real applications use.

**Relevance to our OS.** high — our terminal and POSIX layer must speak VT for ported tools, but a native terminal protocol could separate data from control and make "reply into input" a distinct, deniable capability.

**Sources.** [S810], [S811], [S812], [S813], [S814], [S815], [S816]

### DM-072: `TIOCSTI` input injection

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Linux (until opt-out in 6.2), historically all BSD-derived Unix; OpenBSD (disabled 2017)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `TIOCSTI` is an ioctl that pushes a byte into a terminal's input queue as if the user had typed it. Any process with a file descriptor for the terminal can call it on its controlling TTY [S803].

**Why it was chosen.** It made simple tricks cheap: a shell or mail program "typing ahead" for the user. OpenBSD removed such uses from csh, mailx and ksh before disabling the call [S806]. The threat model of the time assumed every process on a terminal belonged to the same user.

**Why it is a problem.** Holding the terminal becomes authority to run commands as whoever owns the shell reading it. A process that keeps the terminal after a privilege change, or after its parent shell takes over again, can type commands into that shell. Examples: sandbox escapes in SELinux `sandbox` (CVE-2016-7545) [S807] and bubblewrap (CVE-2017-5226) [S805], and "TTY pushback" against `su`/`sudo` when they do not allocate a new pty [S804][S808]. The kernel's own Kconfig help now calls it a malicious privilege-escalation mechanism with no meaningful real-world use [S803].

**Why it was not fixed.** Linux kernel developers rejected removal for years. Alan Cox NAKed Matt Brown's 2017 patches. He argued that privilege-changing programs should allocate a new pty anyway, that TIOCSTI is not the only attack path, and that the affected applications were already fixed. Solar Designer disputed the last point [S808]. util-linux shipped CVE-2016-2779 unfixed because `setsid()` would break `su`/`runuser` [S808]. Proposals for general ioctl-filtering LSMs stalled [S803]. Linux 6.2 (patch committed in 2022, released in 2023) finally added `CONFIG_LEGACY_TIOCSTI` and the sysctl `dev.tty.legacy_tiocsti`, but the default is still Y [S803][S804].

**Alternatives that exist.** OpenBSD made TIOCSTI return EIO in 2017 [S806]. Android filtered it [S803]. Sandboxes block it with seccomp or create a new session (setsid) [S807][S809]. `su`/`sudo` can be configured to allocate a new pty [S804].

**Cost of the alternatives.** Some legacy tools lose a feature. Cox's warning was partly borne out: Flatpak's TIOCSTI filter was bypassed through `TIOCLINUX` copy/paste on virtual consoles (CVE-2023-28100) [S809]. Removing one ioctl does not fix a design in which sharing a terminal means sharing authority.

**Relevance to our OS.** high — a clear case for separating rights (write output, read input, inject input) on terminal handles instead of granting all of them through one file descriptor.

**Sources.** [S803], [S804], [S805], [S806], [S807], [S808], [S809]

### DM-073: Windows console API vs VT sequences (ConPTY retrofit)

- **Category:** Terminals, sessions and consoles
- **Systems affected:** Windows NT family
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Windows console programs call a rich Win32 Console API (cursor, buffer, input records, history, aliases). Calls are serialized as IOCTLs through the ConDrv driver to a console host process (CSRSS before Windows 7, conhost.exe afterwards) [S814][S816]. Windows had no PTY. Until 2018 the console host was the only interface to a command-line application [S814].

**Why it was chosen.** NT follows an "everything is an object" philosophy. A typed API is easier to verify than escape sequences, and shared services such as history and aliases did not have to be reimplemented by every shell [S816]. The design dates from about 1989 and assumed a local GUI [S816].

**Why it is a problem.** Third-party terminals and sshd had to create a hidden console, simulate input and scrape its buffer. Microsoft says this caused instability, crashes, data corruption, extra power use and lost formatting [S814]. The API cannot be carried over a byte channel, so remoting needs a whole RPC layer [S815]. Some calls run in the "wrong" direction, injecting input or reading back output. Microsoft names these as a way to cross security and privilege levels [S815]. This is the same class as TIOCSTI (DM-072), although Microsoft's page says VT-based platforms do not allow such operations. Giving console applications raw window handles became fragile as window security tightened [S815]. The console was built around UCS-2, which covers only the Basic Multilingual Plane [S816].

**Why it was not fixed.** "Hundreds of millions" of applications and scripts depend on it [S816]. Microsoft states there are no plans to remove the API [S815].

**Alternatives that exist.** ConPTY (`CreatePseudoConsole`, 2018): conhost translates legacy API calls into VT text for any terminal over pipes, and Microsoft now recommends that new applications emit VT [S814][S815]. Unix-style ptys with in-band VT are the obvious alternative.

**Cost of the alternatives.** ConPTY keeps a simulated screen buffer and projects only its effects, so the translation is "not perfect" [S815]. It first shipped in the autumn 2018 release of Windows 10 and is not available on Windows 7, so applications supporting older Windows keep both code paths [S814]. Moving to VT brings in the escape-injection class (DM-071). This entry and DM-071 are two sides of the same in-band vs out-of-band tradeoff.

**Relevance to our OS.** medium — shows the cost of a terminal model that is not a byte stream when remote access and ported Unix tools matter; a typed protocol needs a planned byte-stream translation.

**Sources.** [S814], [S815], [S816]


### DM-074: BSD sockets API design (`sockaddr` casts, protocol-specific structs)

- **Category:** Networking APIs
- **Systems affected:** BSD, Linux, macOS, POSIX, Windows (Winsock); contrast Plan 9, IETF TAPS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** A socket is a descriptor created with a (family, type, protocol) triple. Addresses are passed as a generic `struct sockaddr *` plus a length, and the caller casts protocol-specific structs such as `sockaddr_in` and `sockaddr_in6` to it. The application chooses the address family, resolves names, picks an address and connects. Protocol options go through `setsockopt` with numeric levels [S436][S437].

**Why it was chosen.** 4.2BSD (1983) had to support several protocol families at once, in C with no generics. A tagged struct with a length was the cheapest form of polymorphism. Making sockets descriptors let `read`/`write` work on connections, and the design has lasted more than forty years.

**Why it is a problem.**
- The casts are unchecked. A wrong size or family compiles fine, and returned addresses are truncated when the buffer is too small; the caller finds out only by checking the returned length [S437].
- IPv6 did not fit `sockaddr_in`. That forced `sockaddr_in6`, `sockaddr_storage` (sized and aligned so that casts work) and replacements for `gethostbyname`, and every application had to be ported [S436].
- The length type itself changed from `int` to a proposed `size_t` and then to `socklen_t` [S437].
- The IETF's TAPS architecture lists the structural problems. TCP and TLS-over-TCP need different calls. Sockets bind to one address and one interface. TCP is an unstructured byte stream, so every application parses its own message boundaries. Asynchrony is "try and fail". Mechanisms like Happy Eyeballs racing must be implemented separately in each application [S434].

**Why it was not fixed.** Every networked program depends on it, and RFC 3493 explicitly kept IPv6 changes "as small as possible" for that reason [S436]. TAPS is meant as an enhanced replacement, but it is designed to run alongside sockets rather than displace them, for incremental deployment [S434].

**Alternatives that exist.**
- Plan 9 exposes networks as `/net/<proto>/clone|ctl|data` files. `dial("tcp!host!service")` takes a textual address, and a user-level connection server does resolution. The paper says only name resolution differs between networks [S435].
- TAPS defines a protocol-agnostic connection/message API with path and protocol racing built in [S434].
- `getaddrinfo` already lets applications avoid writing IPv4/IPv6-specific code [S438].

**Cost of the alternatives.** Textual addresses and file-based control move parsing and error reporting into strings, without type checking either. Higher-level APIs like TAPS hide policy, such as which address or path is used. The RFC itself notes that some applications need fine-grained control of protocol details, so such APIs must still expose specialised features [S434]. TAPS is recent (RFC 9621, 2025), so its costs are less tested than those of sockets [S434]. An API that resolves names for you inherits the resolver problems in DM-075.

**Relevance to our OS.** medium. Our native API can take typed endpoint values or names and include resolution and racing. The POSIX layer must still provide `sockaddr` exactly as C code expects for the compiler toolchain.

**Sources.** [S434], [S435], [S436], [S437], [S438]

### DM-075: Name resolution in libc: blocking, no async, NSS

- **Category:** Networking APIs
- **Systems affected:** glibc/Linux (NSS), POSIX `getaddrinfo`; musl; language runtimes (Go); contrast Windows `GetAddrInfoEx`, Plan 9 `cs`
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Name lookup is a synchronous libc call, `getaddrinfo`. It may consult `/etc/hosts`, DNS, LDAP and so on, as configured by `nsswitch.conf`. glibc implements each source as a shared library, `libnss_SERVICE.so`, loaded into the calling process [S438][S441]. The only async variant on Linux is the GNU extension `getaddrinfo_a` [S439].

**Why it was chosen.** Host lookup started as reading `/etc/hosts`, which was local and fast. NSS made the set of sources configurable per site without relinking programs [S441]. Loading sources in-process also avoids depending on a resolver daemon being up. That benefit is still real, which is why this is classed as a tradeoff rather than a decision whose constraints have gone away.

**Why it is a problem.**
- A lookup can take a long time because of the network, but the call blocks a thread [S438]. Event-loop programs need a thread per lookup or a separate library. Go's docs explain that a blocked C lookup costs an OS thread. Go therefore prefers its own DNS resolver, falls back to libc when `nsswitch.conf` or `resolv.conf` uses features it does not implement, and caps concurrent cgo lookups at 500 [S440].
- `getaddrinfo_a` is GNU-only and notifies by signal or by a new thread. A request already being processed cannot be cancelled [S439].
- Loading NSS modules into every process means that resolver code, and its dependencies, run in each caller's address space. Static binaries that call `getaddrinfo` still `dlopen` NSS modules. For static setuid binaries this enabled CVE-2025-4802, a local attack that loaded a library from `LD_LIBRARY_PATH` (bug introduced in glibc 2.27, fixed in 2.39) [S443].
- Resolver behaviour differs between libcs. musl, for example, queries nameservers in parallel and lacked DNS-over-TCP until 1.2.4 [S442]. The same configuration can give different results.

**Why it was not fixed.** The API is in POSIX and NSS is glibc's ABI. Async lookup has stayed an extension, and runtimes work around it individually [S439][S440].

**Alternatives that exist.**
- Plan 9 puts resolution in a user-level connection server reached through files, so it is not linked into each program [S435].
- Windows `GetAddrInfoExW` supports overlapped/async completion and cancellation, but only since Windows 8 [S444].
- macOS does not let programs make direct DNS requests, so Go uses the system (cgo) resolver there [S440].

**Cost of the alternatives.** A resolver service adds an IPC hop and a daemon that must be up, and it becomes a privileged single point of failure. Bundled resolvers such as Go's diverge from system policy and must detect when to defer to it [S440].

**Relevance to our OS.** medium. Making name resolution an async service behind a capability fits our model. The POSIX layer must still offer blocking `getaddrinfo` for C programs.

**Sources.** [S435], [S438], [S439], [S440], [S441], [S442], [S443], [S444]


