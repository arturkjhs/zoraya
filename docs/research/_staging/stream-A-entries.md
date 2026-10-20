# Stream A entries (categories 1 and 15)

## 1. Process model and program execution

### DM-001: `fork()` as the primary process-creation primitive

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS/XNU; indirectly every OS that has to emulate it (Windows ports, K42, Fuchsia/Starnix)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** A new process is made by duplicating the caller: address space, descriptor table, signal state, credentials and so on. The child then optionally calls `exec` to replace its image. All other process-creation customization (redirection, privilege dropping) happens as ordinary system calls run by the child between `fork` and `exec`.

**Why it was chosen.** Ritchie described the PDP-7 `fork` as roughly 27 lines of assembly, chosen because it was easy to implement without changing much else; a combined fork-exec would have been harder because `exec` did not yet exist as such (as quoted in [S100]). Programs and memory were tiny, and `fork` needs no arguments: "inherit everything" is a default for every piece of state, and the fork-to-exec gap reuses existing syscalls rather than needing a parameter for each. Before threads, fork-without-exec was also the concurrency mechanism [S100].

**Why it is a problem.** Baumann et al. list concrete failure modes [S100]. (1) *Not thread-safe:* the child gets only the calling thread, so locks held by other threads (e.g. the malloc lock) stay locked forever; only async-signal-safe functions are allowed before `exec`. (2) *Insecure by default:* everything is inherited, which violates least privilege, and fork-without-exec gives every worker the same layout, which defeats ASLR (Blind ROP). (3) *Slow:* the cost grows with parent size; Chrome sees up to 100 ms fork delays, while `posix_spawn` stays around 0.5 ms. (4) *Encourages overcommit:* honest accounting would require reserving double the parent's virtual size, so Linux overcommits and relies on the OOM killer (cross-ref DM-030). (5) *Doesn't compose:* user-mode buffered I/O gets duplicated, and POSIX lists 25 special cases for what a child inherits. (6) *Constrains kernel design:* fork pushes all process state into one cheaply-copyable monolithic place. K42's authors report that supporting fork burdened core objects [S100].

**Why it was not fixed.** Decades of software (shells, servers, Zygote-style preforkers, Redis snapshots) depend on fork semantics [S100]. The point is disputed: Drepper agrees that fork+exec is a bad way to start programs, but defends fork for multi-process designs with a shared layout and prefers lower-level `clone()` over replacing it [S101].

**Alternatives that exist.** `posix_spawn` with file actions and attributes [S102]; Windows `CreateProcess` with explicit parameters [S104]; Fuchsia, where a launcher service builds the process from handles the caller supplies, and direct construction is restricted by job policy [S103]; cross-process syscalls that operate on an empty child before it starts (ExOS-style, per [S100]).

**Cost of the alternatives.** A spawn API needs a knob for every piece of process state. `posix_spawn` cannot run arbitrary code in the child, reports some failures only as exit status 127, and is an optional POSIX feature [S102]. CreateProcess has 10 parameters plus an attribute list [S104]. Copy-on-write snapshotting (Redis, fuzzers, Zygote) needs a separate snapshot API if fork goes away [S100].

**Relevance to our OS.** high — the native process-creation primitive shapes the kernel's object model, and the POSIX layer will still need some fork emulation for the C toolchain.

**Sources.** [S100], [S101], [S102], [S103], [S104]

### DM-002: File descriptors inherited across `exec` by default (`O_CLOEXEC` retrofit and its races)

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows has the analogous `bInheritHandles` but opt-in per handle
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Every open descriptor survives `fork` and `exec` unless it has `FD_CLOEXEC` set. The flag was historically set by a separate `fcntl()` call after the descriptor was created.

**Why it was chosen.** Inheritance by default is what makes shell plumbing work: the shell sets up fds 0–2 (or more) and execs the program, which needs to know nothing. In a single-threaded process, set-then-exec has no race.

**Why it is a problem.** In multithreaded programs, another thread can fork+exec between `open()` and `fcntl(F_SETFD)`, leaking the descriptor into an unrelated program; the Linux man page calls `O_CLOEXEC` essential for this reason [S106]. POSIX Issue 8 says the atomic flags are "necessary to avoid a data race in multi-threaded applications" and adds `O_CLOFORK` for the matching fork-time leak [S107]. Python's PEP 446 catalogues consequences: hangs because a child holds a pipe end open, listening ports kept busy after restart, files that cannot be deleted on Windows, and security leaks (cited cases involve Apache/PHP, OpenSSH, and CWE's fd-exposure entry), affecting projects from Firefox to qemu [S105]. Libraries cannot know which fds a future child will need, so every fd-creating call needs a flag (`SOCK_CLOEXEC`, `pipe2`, `accept4`, `dup3`, ...), and a single forgotten one is a leak. Closing "everything else" before exec needed brute-force loops or `/proc/self/fd` scanning until `close_range()` arrived (Linux 5.9, `CLOSE_RANGE_CLOEXEC` in 5.11) [S108].

**Why it was not fixed.** Flipping the kernel default would break every program that relies on implicit inheritance. Fixes therefore landed as opt-in flags call by call (Linux 2.6.23 for `O_CLOEXEC` [S106]), and runtimes changed their own defaults instead (Python 3.4 made new fds non-inheritable [S105]).

**Alternatives that exist.** Windows: a handle is inherited only if it is marked inheritable *and* `bInheritHandles` is TRUE, and `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` lists exactly which handles a given child gets, which fixes the concurrent-spawn problem [S104]. `posix_spawn` file actions let the parent describe the child's fd table explicitly [S102]. Fuchsia gives a new process only the handles the launcher is handed [S103].

**Cost of the alternatives.** An explicit handle list makes simple redirection more verbose and needs a convention for passing well-known slots (stdin/out/err). Even Windows' design still carries the global `bInheritHandles` switch, whose problems the docs themselves acknowledge [S104].

**Relevance to our OS.** high — with capability handles, "what does a child get" is a security decision; the default should be decided deliberately, not inherited.

**Sources.** [S102], [S103], [S104], [S105], [S106], [S107], [S108]

### DM-003: PID as a reusable integer name; PID-reuse races; `pidfd` retrofit

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux (pidfd since 5.1–5.3), BSDs (FreeBSD process descriptors [unverified]), macOS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Processes are named by small integers from a global namespace. `kill`, `waitpid`, `/proc/<pid>` and most tools take a PID. Once a process is reaped its PID can be reassigned.

**Why it was chosen.** It was cheap and simple with small process tables. A parent owns its children's PIDs until it reaps them, so the parent–child case is race-free if the parent is disciplined.

**Why it is a problem.** Anyone who is *not* the parent (supervisors, `pkill`, PID files, debuggers, monitoring agents) has no way to hold a reference. Between looking up a PID and acting on it, the process can exit and the PID can be reused, so the signal hits an unrelated process. The kernel man page states this failure directly [S111], and LWN calls PID reuse arguably the biggest problem with PIDs [S109]. Even for children, the guarantee breaks if `SIGCHLD` is ignored, `SA_NOCLDWAIT` is set, or something else reaps the zombie [S110].

**Why it was not fixed.** PIDs are part of every API and tool. Linux added a parallel handle-based API instead: `pidfd_send_signal` (5.1) [S111], `CLONE_PIDFD` and `pidfd_open` (5.3), pollable exit notification, and `waitid(P_PIDFD)` [S109][S110]. Adding `CLONE_PIDFD` used up the last free `clone()` flag bit, which forced the new `clone3()` with an extensible struct (cross-ref DM-055) [S112].

**Alternatives that exist.** Windows: `CreateProcess` returns a process *handle*, which stays valid until closed even after the process ends, alongside a PID that is valid only while the process lives [S113]. Zircon/Fuchsia: processes are kernel objects reached only through handles [S103]. Linux pidfds now also support `setns`, `pidfd_getfd` and `process_madvise` [S110], which makes them close to a process capability.

**Cost of the alternatives.** Handles must be closed, and a leaked handle pins kernel state (a "zombie" process object). Humans and shells still want printable short names, so a handle system needs a separate, explicitly racy lookup path. Retrofitting doubles the API surface: Linux now has both PID and pidfd variants of many calls.

**Relevance to our OS.** high — if processes are capability objects from day one, the race disappears by construction; the POSIX layer only has to emulate PIDs.

**Sources.** [S103], [S109], [S110], [S111], [S112], [S113]

### DM-004: Threads added late to the Unix process model

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux (LinuxThreads → NPTL), BSDs; contrast Windows NT and Mach, which had threads from the start
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Unix defined the process as the single unit of both resource ownership and execution. POSIX threads were added later. POSIX requires many attributes (PIDs, credentials, signal dispositions, cwd, umask, rlimits, record locks) to be process-wide, while the signal mask and errno are per-thread [S118]. Linux implements threads as tasks that share resources, so some of these "process-wide" attributes are in fact per-task in the kernel.

**Why it was chosen.** On uniprocessors, cheap `fork` plus pipes gave enough concurrency. Threads arrived when SMP and GUIs demanded them, and had to be layered over existing semantics without breaking single-threaded programs.

**Why it is a problem.** Every older process-level feature needed a retrofitted rule for threads, and the rules disagree. (1) *Credentials:* in Linux, uid/gid are per-thread, so to meet POSIX glibc sends a hidden real-time signal to every thread to make each one repeat `setuid()` [S114]. A runtime that skips libc gets this wrong: in Go, `syscall.Setuid` changed only one OS thread, leaving other goroutines running as root. That issue was open from 2011 until the fix in Go 1.16 [S115]. (2) *Signals:* a process-directed signal goes to an arbitrary thread that has not blocked it, while dispositions are process-wide [S116]. So a library cannot own a signal (cross-ref DM-017). (3) *fork in threads:* see DM-001. (4) *Conformance churn:* LinuxThreads gave each thread its own PID, did not share credentials, and could not deliver process-directed signals. NPTL (glibc 2.3.2, Linux 2.6) still needed fixes up to 2.6.16 and still does not share the nice value [S118].

**Why it was not fixed.** Changing the process/thread split would break the ABI. Each mismatch was patched separately in kernel or libc.

**Alternatives that exist.** Windows NT treats a process as a resource container (address space, handles, security context) and a thread as the schedulable entity, with an optional per-thread impersonation token [S117]. Mach split tasks from threads in the same way [unverified here; see stream J]. Fuchsia also has distinct process and thread objects [S103].

**Cost of the alternatives.** Per-thread security contexts (NT impersonation) bring their own confusion about which identity applies to a given access. A clean split still has to answer, for each resource, whether it is per-process or per-thread, but it can do so once and consistently.

**Relevance to our OS.** high — we must decide from the start which state belongs to the address-space container and which to the thread, especially authority/credentials.

**Sources.** [S103], [S114], [S115], [S116], [S117], [S118]

### DM-005: Zombies, `wait` semantics and reparenting to init

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; containers (PID 1 problem)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A terminated child stays a zombie (holding its PID, exit status and rusage) until its parent calls `wait`. If the parent dies first, the orphans are reparented to init (PID 1), which must reap them. Only the parent can wait for a process.

**Why it was chosen.** The exit status has to live somewhere until it is collected. Keeping the PID reserved until reaping is also what makes PIDs safe for the parent to use (DM-003). With integer names and no handles, a parent–child relation is the only natural owner.

**Why it is a problem.** Each zombie holds a process-table slot, and if the table fills no new processes can be created [S120]. Exit status is tied to *parentage*, not to whoever cares: supervisors cannot wait for double-forked daemons, which reparent to init (the `setsid` double-fork idiom exists precisely to detach [S154]). Linux therefore added `PR_SET_CHILD_SUBREAPER` (3.4) so that session and service managers can adopt orphaned descendants [S119]. In containers, the program running as PID 1 often does not reap at all and gets no default signal handling, which is why tini exists and is bundled as `docker run --init` [S121]. The behaviour of `SIGCHLD = SIG_IGN` was left unspecified in early POSIX and differed in Linux 2.4 [S120]. And the reap-or-leak contract creates the PID-reuse window for non-parents.

**Why it was not fixed.** It is core POSIX semantics. Fixes were additive: subreapers [S119], and pidfds that let non-parents poll for exit (but `waitid` on a pidfd still only works for one's own children) [S109][S110].

**Alternatives that exist.** Windows: whoever holds a process handle can wait for it and read its exit code, and the object is freed when the last handle closes [S113]. Fuchsia: process objects are handles as well [S103]. Job objects/cgroups give tree-wide supervision (DM-100).

**Cost of the alternatives.** Handle-based lifetime moves the leak from "unreaped zombie" to "unclosed handle", which is just as invisible. The resource cost still exists; only ownership becomes explicit and transferable.

**Relevance to our OS.** medium — falls out of the DM-003 decision; if exit status hangs off a process capability, reparenting and zombies largely disappear, but the POSIX layer must emulate them.

**Sources.** [S103], [S109], [S110], [S113], [S119], [S120], [S121], [S154]

### DM-006: Process-wide mutable state: cwd, umask, signal dispositions

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows (per-process current directory, plus per-drive cwds in the environment)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Path resolution and file creation depend on implicit per-process globals: the current directory, root directory, umask, and signal dispositions. POSIX requires these to be shared by all threads [S118]. (Environment, locale and TZ are similar; see DM-085, DM-097, DM-066.)

**Why it was chosen.** For single-threaded command-line programs, implicit context keeps calls short (`open("foo")`) and lets the parent configure the child through inheritance across `fork`/`exec`.

**Why it is a problem.** Shared mutable state in a multithreaded process is a race. Any thread calling `chdir()` changes how every other thread's relative paths resolve, so libraries cannot safely use `chdir`. There was no way to *read* the umask without setting it, which forced a set-and-restore pair; another thread creating a file in between gets the wrong mode. Linux 4.7 added a read path via `/proc/self/status` [S123]. Signal dispositions are process-wide, so two libraries cannot independently own `SIGCHLD` or `SIGPIPE` [S116]. Implicit cwd also feeds path-based TOCTOU (DM-039) and Windows DLL planting from the current directory (DM-007). The kernel's answer for paths was `openat()` and friends: explicit directory fds, documented as both a race fix and a way to emulate a per-thread cwd [S122]. Linux also lets a thread `unshare(CLONE_FS)` to get a private cwd/root/umask [S124], which is non-portable and itself violates POSIX's process-wide rule.

**Why it was not fixed.** The implicit forms are in every API and every program. The `*at()` family was added alongside rather than replacing them.

**Alternatives that exist.** Directory-handle-relative APIs (`openat`, Capsicum-style) [S122]. Fuchsia has no global root; each component receives a namespace table of handles, and `..` and leading `/` are emulated in client code (fdio) [S125], so "cwd" is library state, not kernel state.

**Cost of the alternatives.** Every call needs an explicit base handle, which is more verbose, and the POSIX layer must still emulate `chdir`/umask with libc-level state, re-creating the race for ported code.

**Relevance to our OS.** high — whether path resolution has any implicit base in the native ABI is a foundational choice; the cost lands on the POSIX emulation layer.

**Sources.** [S116], [S118], [S122], [S123], [S124], [S125]

### DM-007: Shared-library resolution: DLL search order / DLL hell, `LD_LIBRARY_PATH`, soname versioning

- **Category:** Process model and program execution
- **Systems affected:** Windows (DLL search order, SxS), Linux/ELF (`ld.so`, RPATH/RUNPATH, `LD_*`), macOS (dyld, not researched here)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Executables name dependencies by short module name, and a loader resolves them at run time by searching a list of locations made up of environment, embedded paths, caches and system directories. Libraries are shared and upgraded independently of the programs that use them.

**Why it was chosen.** Shared libraries save memory and disk, and let one security fix update every consumer. A name-based search lets software be installed anywhere without relinking.

**Why it is a problem.** *Resolution is implicit and context-dependent.* Windows searches up to a dozen places, including the application folder, the current folder and `PATH`; if an attacker controls any of them, a planted DLL is loaded, and the docs warn about exactly this [S126]. Microsoft's 2010 advisory on "binary planting" led to a long run of bulletins (2010–2014) fixing Office, Windows components and .NET one at a time, because the root cause was applications passing insufficiently qualified paths. Microsoft could ship only an opt-in mitigation, not a global fix [S128]. Safe search mode, KnownDLLs, `SetDefaultDllDirectories` and `LOAD_LIBRARY_SEARCH_*` are all retrofits [S126]. On ELF, the search order mixes `DT_RPATH` (applies to the whole tree), `LD_LIBRARY_PATH`, `DT_RUNPATH` (direct deps only), `ld.so.cache` and default paths. The loader has to switch into secure-execution mode for setuid binaries and ignore `LD_LIBRARY_PATH`/`LD_PRELOAD`, so environment-controlled code loading is an ever-present privilege hazard (cross-ref DM-011, DM-085) [S127]. *Versioning:* when independently-upgraded shared components overwrite each other ("DLL hell"), installed programs break. Dolstra's thesis identifies interference and undeclared dependencies as root causes of deployment failures [S129].

**Why it was not fixed.** Every search location is relied on by some installed software. Microsoft explicitly made the safer modes opt-in [S128][S126].

**Alternatives that exist.** Nix/NixOS: each dependency is a store path that includes a hash of all build inputs, so components never overwrite one another and references are exact [S129][S130]. Windows packaged apps resolve through a declared package dependency graph [S126]. ELF `$ORIGIN` lets an app find bundled libraries relative to itself [S127]. Static linking avoids run-time resolution entirely.

**Cost of the alternatives.** Exact references and static linking lose the "patch one library, fix everyone" property; a security update then requires rebuilding or redeploying every consumer. Nix-style stores need tooling for garbage collection and break programs that hard-code FHS paths.

**Relevance to our OS.** medium — we choose the loader and the binary format, and a capability-based loader could resolve dependencies from handles given at spawn rather than by searching names.

**Sources.** [S126], [S127], [S128], [S129], [S130]

### DM-008: `argv` as an unstructured string vector; quoting delegated to every shell and program

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX (string vector), Windows (single command-line string), every language runtime's process API
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** On Unix, `execve` passes an array of NUL-terminated byte strings. The program itself decides which are options and which are operands. On Windows, `CreateProcess` passes one string, and *each program* splits it, typically with the C runtime's rules [S104][S132].

**Why it was chosen.** A string vector is the most general and language-neutral interface. It costs nothing in the kernel, and leaves parsing policy to programs and shells. Windows inherited a single command line from DOS.

**Why it is a problem.** *Windows:* splitting is the callee's business. The MSVC rules involve backslash-parity handling before quotes [S132], and `cmd.exe` uses different rules. Rust's standard library promised arguments would not be shell-evaluated, but its escaping for `.bat`/`.cmd` targets could be bypassed to run arbitrary commands (CVE-2024-24576, critical). The Rust team concluded that no escaping can be correct for every `cmd.exe` case, and now refuses some arguments [S131]. The single string also makes the executable path itself ambiguous: an unquoted `C:\Program Files\MyApp` can run `C:\Program.exe`, which Microsoft documents as a security risk [S104]. *Unix:* the vector avoids splitting bugs, but anything typed is erased. An operand that starts with `-` becomes an option. For example, git passed an `ssh://-oProxyCommand=...` hostname to ssh as an option, which allowed command execution from a malicious `.gitmodules` (CVE-2017-1000117) [S133]. Building a command as a string for `system()`/`sh -c` reintroduces shell quoting. The total size of arguments plus environment is also capped by `ARG_MAX` [S102].

**Why it was not fixed.** The interface is the contract between all programs. Structure would have to be agreed upon by every program, not imposed by the kernel.

**Alternatives that exist.** The Unix vector is itself the better half of this comparison. Language-level APIs that never go through a shell (Rust `Command`, Python `subprocess` without `shell=True`) help. The `--` end-of-options convention and the "reject leading dash" fixes (git) are partial measures [S133]. Typed or structured invocation (e.g. passing handles plus a typed message at spawn, as Fuchsia's launcher does for handles [S103]) is an alternative [unverified as a general argv replacement].

**Cost of the alternatives.** A typed argument schema needs a schema language and a way to evolve it, and would make the POSIX toolchain (compilers, make, shells) depend on an emulation of plain argv anyway.

**Relevance to our OS.** medium — the native spawn ABI should at least pass a vector (never a single string); whether to add structure is open, and the C compiler only needs classic argv.

**Sources.** [S102], [S103], [S104], [S131], [S132], [S133]

### DM-100: No reliable, inescapable process-tree containment (process groups vs job objects vs cgroups)

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux (cgroups retrofit, `cgroup.kill` in 5.14), BSDs; Windows (job objects, nesting only since Windows 8)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Classic Unix offers process groups and sessions (designed for terminal job control) as its only way to address a group of processes. Any process can leave its group with `setsid()`/`setpgid()`, and double-forking is the standard way to detach [S154]. There is no kernel object meaning "this service and everything it spawned".

**Why it was chosen.** Process groups were built for interactive shells: send ^C to the foreground job. Escaping them is a *feature* for daemons. Supervision of long-running services was not in scope.

**Why it is a problem.** A supervisor cannot reliably find, account for or kill everything a service started: daemons double-fork away, PIDs race (DM-003), and orphans reparent to init (DM-005). Linux added subreapers so that supervisors at least get notified [S119]. It then made cgroups the de-facto tracking mechanism, but killing a cgroup still meant walking the tree recursively while processes kept forking. Container managers had no single PID that covered everything to kill. `cgroup.kill` (Brauner, 2021) adds a race-free kill that catches children forked during the kill [S153][S152]. Each step is a retrofit onto a model where containment was optional.

**Why it was not fixed.** Process groups are part of the job-control ABI (DM-070). Containment came in as separate subsystems (cgroups), whose v1 design then had to be redone as v2 [S152].

**Alternatives that exist.** Windows job objects: by default children join their parent's job, and the association cannot be broken. Jobs support limits, accounting, `TerminateJobObject`, and kill-on-last-handle-close [S151]. But until Windows 8 a process could be in only one job, which broke tools that nest; breakaway flags complicate guarantees [S151]. Fuchsia's process creation is scoped by jobs with policy, such as forbidding direct process construction [S103].

**Cost of the alternatives.** Mandatory containment needs rules for nesting and for deliberate breakaway (e.g. a user session launching an independent app). Windows shows that getting nesting wrong at first is costly [S151]. Accounting adds per-fork kernel work.

**Relevance to our OS.** high — if every process is created inside a job/container capability, supervision, resource limits and the "kill this app" operation come for free; this is cheap to design in now and expensive later.

**Sources.** [S103], [S119], [S151], [S152], [S153], [S154]

## 15. Boot, update and recovery

### DM-090: In-place, non-atomic package updates vs image-based A/B updates

- **Category:** Boot, update and recovery
- **Systems affected:** Traditional Linux distributions (dpkg/rpm), Windows Update (servicing stack), most BSD package flows; alternatives in Android, ChromeOS, OSTree-based systems, NixOS
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** The running system is updated by unpacking new files over old ones, one package at a time, with maintainer scripts doing configuration. The system remains partly old and partly new during the update.

**Why it was chosen.** It is flexible (any subset of packages, local modification), uses little extra disk, and suits mutable, hand-administered machines. Before cheap storage, keeping a second full copy of the OS was unaffordable.

**Why it is a problem.** An update interrupted by a crash, power loss or failing script leaves an inconsistent system. dpkg has explicit "half-installed" and "half-configured" states and recovery commands (`--configure -a`, `--audit`) for exactly this [S137]. Updating a live system creates conflicts between the libraries and services already running and those on disk. systemd recommends rebooting into a special offline-update mode to avoid them, which gives up the "no downtime" benefit [S136] (cross-ref DM-091). The resulting state depends on the update path and on local edits, so machines drift apart and rollback is not first-class. The NixOS paper names destructive, imperative updating as the cause of difficult rollback and unreliable upgrades [S130].

**Why it was not fixed.** Package managers, maintainer scripts and FHS layouts form a huge ecosystem, and users value mutability. Image-based schemes need a read-only `/usr` and a clear split between OS and state.

**Alternatives that exist.** Android A/B: the update is written to the inactive slot in the background. The bootloader falls back to the old slot if the new one does not get marked successful after several boot attempts. The only downtime is a reboot, and Virtual A/B (Android 10) reduces the storage cost [S134]. OSTree: complete, content-addressed trees, deployed as hardlinks and switched atomically; `/etc` gets a 3-way merge and `/var` is shared [S135]. NixOS: immutable store paths and atomic switching between system generations [S129][S130].

**Cost of the alternatives.** Extra storage (two slots, or several deployments); local changes must be confined to `/etc` and `/var` [S135]; a boot is still needed to activate the update; and package-level flexibility is lost or has to be layered back on. OSTree itself notes that pure block-image replication is rigid [S135].

**Relevance to our OS.** high — we can define the system image as immutable and content-addressed from the start, with state separated, which is very hard to retrofit.

**Sources.** [S129], [S130], [S134], [S135], [S136], [S137]

### DM-091: Replacing in-use files requires reboot (Windows) / stale libraries in running processes (Unix)

- **Category:** Boot, update and recovery
- **Systems affected:** Windows (share modes, delayed rename), Linux/Unix (unlink-while-mapped), all systems for the kernel itself
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Windows typically refuses to replace a file another process has open or mapped (cross-ref DM-044). Installers schedule renames for the next boot via `MOVEFILE_DELAY_UNTIL_REBOOT`, recorded in the `PendingFileRenameOperations` registry value [S138]. Unix lets the new file replace the name immediately, while running processes keep the old inode mapped.

**Why it was chosen.** Windows' locking stops a program from seeing a file change under it mid-run. Unix's unlink semantics let updates proceed without coordination and never block on a busy file.

**Why it is a problem.** *Windows:* Microsoft says the primary reason installs require a restart is that files being updated are in use [S139]. Delayed operations run only at boot, can fail silently (the API only reports whether the registry entry was written), and require admin rights [S138]. Restart Manager (Windows Installer 4.0+) shuts down and restarts apps to free files, but critical services still force a reboot [S139]. *Unix:* there is no forced reboot, but after a library security update, running daemons keep executing the *old, vulnerable* code with no indication. Tools like needrestart scan processes for deleted mappings (via `/proc/<pid>/maps`) and guess which services to restart. For interpreted languages, where the source files are read at run time, they admit the heuristic is imperfect [S140]. A process that loads a plugin after an update can mix old and new library versions, which motivates offline updates [S136]. Either way, "is the update actually in effect?" has no reliable answer.

**Why it was not fixed.** These are deep filesystem semantics (open-file locking vs inode lifetime) and process models. Live patching only covers the kernel, and only narrowly: just traceable functions can be patched, and transitions can stall [S141].

**Alternatives that exist.** Restart coordination (Restart Manager) [S139]; offline update boot [S136]; A/B images, where the switch happens at a planned reboot [S134]; kernel livepatch for critical fixes [S141]. Hot code loading (e.g. Erlang) [unverified, not researched here].

**Cost of the alternatives.** Each one trades downtime against consistency: an offline or A/B reboot is predictable but disruptive; restart coordination needs apps to save and restore state [S139]; livepatching is limited and needs reliable stack traces [S141].

**Relevance to our OS.** medium — with immutable images (DM-090), we need an explicit model for which running components are on which version and how services are restarted; a microkernel-style restartable service design could make this cheaper.

**Sources.** [S134], [S136], [S138], [S139], [S140], [S141]

### DM-092: Firmware and boot-chain complexity (UEFI, BootHole, LogoFAIL)

- **Category:** Boot, update and recovery
- **Systems affected:** UEFI PCs and servers (x86 and some Arm), GRUB2/shim-based Linux, Windows Boot Manager
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** Pre-OS firmware (UEFI) is a large runtime with drivers, filesystems, network stacks and image parsers. It runs with the highest privilege, before any OS protection exists. Secure Boot verifies signatures of executables in the chain (shim → GRUB → kernel), with a revocation list (dbx) kept in firmware variables.

**Why it was chosen.** It provides a vendor-neutral, extensible boot environment for an ecosystem of many OEMs, IBVs and OSes, plus a third-party CA so that non-Microsoft OSes can boot under Secure Boot.

**Why it is a problem.** *Signed code parsing unsigned input:* BootHole (CVE-2020-10713) was a buffer overflow in GRUB2's parsing of `grub.cfg`, a plain-text file that is not signed. It gave attacker code execution before the OS, despite Secure Boot, on essentially every Linux distribution, with more than 80 shims affected [S142]. *Privileged parsers of attacker-controlled media:* LogoFAIL is a set of flaws in the image parsers firmware uses for boot logos. They span the major IBVs (AMI, Insyde, Phoenix; three CVEs) and let a crafted image change privileged UEFI settings [S143]. *Revocation is the weak point:* fixing BootHole required coordinated GRUB, shim, distro and dbx updates. Applying dbx before fixed bootloaders made machines unbootable. An earlier revocation had bricked devices, and some BootHole fixes caused failed reboots [S142]. For BlackLotus (CVE-2023-24932), Microsoft staged revocation over years (2023 → enforcement not before 2026). Once applied it cannot be undone, it breaks old recovery media, and some firmware fails to accept the DB/DBX updates [S144].

**Why it was not fixed.** Firmware is supplied by many separate parties: chip vendor, IBV, OEM, and OS. Shared IBV code spreads bugs across OEMs [S143], and revocation must never brick installed devices [S142][S144].

**Alternatives that exist.** Minimal, verifying boot ROM stubs: ChromeOS's read-only firmware verifies read-write firmware and the kernel with keys held in write-protected storage [S146]. Oxide's `phbl` runs from the reset vector, puts the CPU into long mode, and directly loads an embedded host OS package [S145]. Unified signed boot images (kernel, initrd and cmdline signed together) remove unsigned config parsing [S149].

**Cost of the alternatives.** Minimal firmware gives up the generic "boot any OS from any device" property and needs platform-specific bring-up. The Oxide and ChromeOS approaches depend on the vendor controlling hardware and software together.

**Relevance to our OS.** medium — on QEMU `virt` aarch64 we can choose a minimal path (direct kernel load, or a tiny verified stub); for real hardware we will inherit whatever firmware exists, so a small trusted boot surface is the controllable part.

**Sources.** [S142], [S143], [S144], [S145], [S146], [S149]

### DM-093: Verified/measured boot absent (or partial) by default

- **Category:** Boot, update and recovery
- **Systems affected:** General-purpose Linux distributions (Secure Boot covers only bootloader/kernel), BSDs; contrast ChromeOS, Android, (Windows/macOS not researched here)
- **Classification:** contested
- **Confidence:** medium

**What the design is.** On most general-purpose Unix-like systems, nothing verifies the OS beyond the kernel. Where Secure Boot is used, the locally generated initrd and kernel command line are unsigned, and the root filesystem is not verified at all [S149].

**Why it was chosen.** The systems are mutable and locally administered: the initrd is generated on the machine, the owner edits anything, and the root filesystem changes during package updates (DM-090). Verifying a mutable root filesystem is not meaningful.

**Why it is a problem.** An attacker with brief offline or root access can modify the initrd (which unlocks disk encryption) or system binaries, and that persists across reboots without detection [S149]. In contrast, ChromeOS verifies from a read-only root of trust through firmware and kernel, then verifies each root-filesystem block on demand with a hash tree, so tampering is detected at the next reboot [S146]. dm-verity's design states that the root hash is the sole trust anchor and must be authenticated separately [S147]. Android has enforced verified boot strictly since 7.0, with rollback protection so that an exploit cannot persist by downgrading [S148].

**Why it was not fixed.** Verified boot needs an immutable, vendor-built (or owner-signed) image and a key-management story. These conflict with the traditional mutable distro model and with owner control. Work such as signed unified kernel images and TPM-sealed secrets is recent [S149].

**Alternatives that exist.** ChromeOS (read-only firmware stub, signed kernel, dm-verity, recovery to a known-good image) [S146]; Android Verified Boot / AVB (chain from hardware root of trust, rollback indexes) [S148]; UKIs plus TPM measured boot on Linux [S149].

**Cost of the alternatives.** *Position A (security):* verified boot should be the default; owners can be supported via developer mode or user-enrolled keys, as ChromeOS allows [S146]. *Position B (owner control):* the FSF argues that boot restrictions controlled by manufacturers are "restricted boot" and demands that owners be able to disable them or install free OSes [S150]. Technically, verified boot also costs rollback-protection complexity (ChromeOS admits replaying old hash partitions is hard to detect [S146]), a recovery path, and immutability of the system image. ChromeOS also explicitly does not protect against runtime compromise or malicious stateful data [S146].

**Relevance to our OS.** medium — pairing an immutable image (DM-090) with dm-verity-style verification is cheap if designed in; who holds the keys is a policy decision for the architect.

**Sources.** [S146], [S147], [S148], [S149], [S150]

## 2. Security, authority and identity (added by stream A at coordinator request)

### DM-101: Containers as an emergent assembly of retrofitted primitives (no kernel container object)

- **Category:** Security, authority and identity (chosen over "Configuration, observability and system management" because containers are relied on as a security boundary the kernel never defines as a unit, and because the alternatives compared — jails, zones, Zircon jobs — are security/authority constructs)
- **Systems affected:** Linux (namespaces 2002–2013, cgroups, seccomp, LSMs) and the container runtimes built on them; contrast FreeBSD jails, Solaris zones, Zircon jobs, Plan 9
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Linux has no kernel "container" object. User-space runtimes assemble a container from independent mechanisms: about eight per-resource namespace types (mount, UTS, IPC, PID, network, user, cgroup, time), cgroups for resource control, and seccomp plus LSMs for further restriction [S162][S163]. The namespaces were retrofitted one at a time over roughly a decade, from mount namespaces in 2.4.19 (2002) to user namespaces completed in 3.8 [S162].

**Why it was chosen.** *Steelman:* separate, composable primitives let one kernel serve many policies with no single definition of "container". These include full OS containers, per-application sandboxes (browsers, build tools, systemd service hardening) and checkpoint/migration, where PID namespaces keep PIDs stable [S162]. Proposals to make the container a kernel object were rejected. Namespaces maintainer Eric Biederman argued it "muddies the waters and makes things more brittle" [S163]. Unprivileged user namespaces let ordinary users build sandboxes without setuid helpers [S155].

**Why it is a problem.** *(1) The security boundary exists only as a user-space convention.* A container is secure only if every namespace, cgroup, seccomp filter and LSM profile is configured consistently by the runtime. The kernel has no object whose invariants it can enforce or reason about. *(2) No unit of identity.* The kernel does not know what a container is, so audit cannot attribute events to one, which needed a separate container-ID retrofit with its own debates about nesting and permissions [S163]. *(3) No unit of control.* Killing "a container" had no single handle until `cgroup.kill`. A container sharing a PID namespace or cgroup with outsiders had no single PID that covered everything to kill [S153] (cross-ref DM-100). *(4) Each retrofit has its own failure modes.* Unprivileged user namespaces expose root-only kernel code to ordinary users (see DM-110). The cgroup v1 multi-hierarchy design had to be replaced by v2 (see DM-111).

**Why it was not fixed.** Every namespace type is ABI, and runtimes depend on the exact behaviour of each one. Kernel-level container objects lacked consensus [S163].

**Alternatives that exist.** *FreeBSD jails* (2000): one construct that confines "omnipotent root" while keeping the simple Unix model. Delegated administrators are limited to their jail [S158], and jails extend chroot to cover users, processes and networking [S164]. *Solaris zones* (2004): designed from the start to combine namespace isolation, security isolation and resource management. They are "double-hulled": zone processes can never acquire all privileges, so even a namespace escape does not grant e.g. `/dev/kmem` access [S159]. *Zircon jobs*: a tree of jobs; every process belongs to exactly one job, which carries policy, privilege tracking and resource limits [S160]. *Plan 9*: per-process name spaces built with `bind`/`mount` and shared or copied selectively via `rfork` are the ordinary way programs see resources [S161]. Isolation is then a property of naming, not a container add-on.

**Cost of the alternatives.** A jail or zone is a fixed bundle, so finer sandboxes such as per-tab browser isolation need other tools, and zones' fixed privilege set was a stated limitation [S159]. Kernel container objects raise the nesting and ownership questions Biederman objected to [S163]. Plan 9 namespaces alone give neither resource limits nor a privilege model [S161].

**Relevance to our OS.** high — if a process's namespace is simply the set of capabilities it was given (Zircon/Plan 9 style), "container" and "sandbox" need no extra kernel surface, but a job-like unit for limits, accounting, identity and kill is still needed (DM-100).

**Sources.** [S153], [S155], [S158], [S159], [S160], [S161], [S162], [S163], [S164]

## Stream notes

**Searched.** HotOS 2019 fork paper (full PDF text extracted locally) and the Drepper/Krieger follow-up; POSIX Issue 8 `posix_spawn` and `open`; Linux man pages (open, close_range, pidfd_open, pidfd_send_signal, nptl, pthreads, signal, wait, PR_SET_CHILD_SUBREAPER, openat, umask, unshare, setsid, ld.so, dpkg); LWN on pidfd, clone3, and the cgroup.kill patch; Microsoft Learn (CreateProcessW, process handles, processes and threads, job objects, DLL search order, advisory 2269637, MoveFileEx, Restart Manager, MSVC argv parsing, KB5025885); Fuchsia process creation and namespaces docs; PEP 446; Go issue 1435; Rust CVE-2024-24576; Ubuntu CVE-2017-1000117; Android A/B and Verified Boot; ChromiumOS verified boot; kernel dm-verity, livepatch and cgroup-v2 docs; OSTree; Nix thesis abstract and NixOS JFP abstract; systemd offline updates; needrestart; tini; Eclypsium BootHole; CERT/CC VU#811862 (LogoFAIL); Oxide phbl and RFD 284; Poettering's trusted-boot post; FSF statement.

**Dropped/merged.** None dropped. All 12 index rows survived.

**New entries.** DM-100 (no inescapable process-tree containment: process groups vs job objects vs cgroups). Note for coordinator: the seed-list item "Linux namespaces and cgroups retrofitted for containers; cgroups v1 → v2" has **no row in the index**. DM-100 covers only the process-containment slice; namespaces and the cgroup v1→v2 redesign remain uncovered (category 2 or 7?).

**Cross-references / overlap.** DM-001↔DM-030 (overcommit); DM-002/DM-003↔GR-005 and DM-055 (clone3); DM-004↔DM-017, DM-059; DM-006↔DM-039, DM-066, DM-085, DM-097; DM-007↔DM-011, DM-085 (`LD_PRELOAD`); DM-008↔DM-095 (leading `-`); DM-091↔DM-044 (share modes); DM-090/DM-093↔GR-017 (stream I should share or cite S134, S146, S148).

**Open questions.** (1) Could not fetch Ritchie's "Evolution of the Unix Time-sharing System" (bell-labs URL returns 410, Nokia 403). The 27-line fork claim is cited via S100's quotation only. (2) FreeBSD process descriptors (`pdfork`) and Mach task/thread split are marked [unverified]; streams J/K may have sources. (3) Erlang-style hot code loading as a DM-091 alternative is unverified. (4) The NixOS JFP paper and Nix thesis: only the abstracts were read, so rollback mechanics are attributed only at abstract level. (5) Oxide RFD 284 does not state the rationale for avoiding UEFI (it defers to RFD 75, which was not fetched), so S145 supports only what phbl does, not why. (6) No macOS/dyld or Windows-side verified-boot material was researched.

**Weak or caveated sources.** S121 (tini README) is used only as an anecdotal/practice pointer. S149 is a blog post, by the systemd lead developer, so it is authoritative on systemd design but still a blog. S150 is an advocacy statement, used only to state the FSF position. S101 is an informal blog conversation. S140 (needrestart README) describes tool behaviour, not kernel semantics. NVD and cve.org pages for CVE-2020-10713 did not render, so BootHole rests on the discoverer's advisory (S142).

**Addendum (DM-101, coordinator request).** Added DM-101 from the reserve block under category 2 (Security, authority and identity); reasoning is in its Category field. Narrowed at coordinator request to the umbrella question (containers as an assembly of retrofitted primitives vs first-class designs); user-namespace attack-surface detail and cgroup v1/v2 hierarchy detail moved to stream B's DM-110 and DM-111. Fetched sources S156 (Canonical userns post) and S157 (CVE-2022-0185 report), plus the S152 'Issues with v1' section, are no longer cited by DM-101 but remain verified and available for DM-110/DM-111. Searched: user_namespaces(7), LWN namespaces overview and container-ID articles, CVE-2022-0185 oss-sec report, Canonical's Ubuntu 23.10 userns post, Kamp/Watson jails paper, FreeBSD handbook, Price/Tucker zones paper (full text), Zircon job docs, Plan 9 namespaces paper. Open: Google's June 2023 kCTF post (the '44% of exploits need unprivileged userns' figure) did not render and is known only via S156. Weak/caveated: S162/S163 are LWN (secondary).
