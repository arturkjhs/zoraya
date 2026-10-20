# Catalog of operating system design mistakes and tradeoffs

Part of [TASK-01](TASK-01-design-mistakes-research.md). Evidence base only: no design decisions are made here.
IDs are stable and are referenced by later ADRs; see [index.md](index.md) for the ID allocation. References
`[S<N>]` resolve in [sources.md](sources.md). Verification status of every entry is recorded in
[research-log.md](research-log.md).

## 1. Process model and program execution

### DM-001: `fork()` as the primary process-creation primitive

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS/XNU; indirectly every OS that has to emulate it (Windows ports, K42, Fuchsia/Starnix)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** A new process is made by duplicating the caller: address space, descriptor table, signal state, credentials and so on. The child then optionally calls `exec` to replace its image. All other process-creation customization (redirection, privilege dropping) happens as ordinary system calls run by the child between `fork` and `exec`.

**Why it was chosen.** Ritchie described the PDP-7 `fork` as roughly 27 lines of assembly, chosen because it was easy to implement without changing much else; a combined fork-exec would have been harder because `exec` did not yet exist as such (as quoted in [S100]). Programs and memory were tiny, and `fork` needs no arguments: "inherit everything" is a default for every piece of state, and the fork-to-exec gap reuses existing syscalls rather than needing a parameter for each. Before threads, fork-without-exec was also the concurrency mechanism [S100].

**Why it is a problem.** Baumann et al. list concrete failure modes [S100]. (1) *Not thread-safe:* the child gets only the calling thread, so locks held by other threads (e.g. the malloc lock) stay locked forever; only async-signal-safe functions are allowed before `exec`. (2) *Insecure by default:* everything is inherited, which violates least privilege, and fork-without-exec gives every worker the same layout, which defeats ASLR (Blind ROP). (3) *Slow:* the cost grows with parent size; Chrome sees up to 100 ms fork delays, while `posix_spawn` stays around 0.5 ms. (4) *Encourages overcommit:* honest accounting would require reserving double the parent's virtual size, so Linux overcommits and relies on the OOM killer (cross-ref DM-030). (5) *Doesn't compose:* user-mode buffered I/O gets duplicated, and POSIX lists 25 special cases for what a child inherits. (6) *Constrains kernel design:* fork pushes all process state into one cheaply-copyable monolithic place. K42's authors report that supporting fork burdened core objects [S100].

**Why it was not fixed.** Decades of software (shells, servers, Zygote-style preforkers, Redis snapshots) depend on fork semantics [S100]. The point is disputed: Drepper agrees that fork+exec is a bad way to start programs, but defends fork for multi-process designs with a shared layout and prefers lower-level `clone()` over replacing it. He also argues that current security models (setuid, SELinux domain transitions) hang off `exec`, and that if `exec` stays so does `fork` [S101].

**Alternatives that exist.** `posix_spawn` with file actions and attributes [S102]; Windows `CreateProcess` with explicit parameters [S104]; Fuchsia, where a launcher service builds the process from handles the caller supplies, and direct construction is restricted by job policy [S103]; cross-process syscalls that operate on an empty child before it starts (ExOS-style, per [S100]).

**Cost of the alternatives.** A spawn API needs a knob for every piece of process state. `posix_spawn` cannot run arbitrary code in the child, reports some failures only as exit status 127, and is an optional POSIX feature [S102]. CreateProcess has 10 parameters plus an attribute list [S104]. Copy-on-write snapshotting (Redis, fuzzers, Zygote) needs a separate snapshot API if fork goes away [S100]. Drepper adds that spawning workers instead of forking them gives each a different ASLR layout, which removes the shared-pointer layout that multi-process designs rely on and makes page de-duplication less effective [S101].

**Relevance to our OS.** high — the native process-creation primitive shapes the kernel's object model, and the POSIX layer will still need some fork emulation for the C toolchain.

**Sources.** [S100], [S101], [S102], [S103], [S104]

### DM-002: File descriptors inherited across `exec` by default (`O_CLOEXEC` retrofit and its races)

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows has the analogous `bInheritHandles` but opt-in per handle
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Every open descriptor survives `fork` and `exec` unless it has `FD_CLOEXEC` set. The flag was historically set by a separate `fcntl()` call after the descriptor was created.

**Why it was chosen.** Inheritance by default is what makes shell plumbing work: the shell sets up fds 0–2 (or more) and execs the program, which needs to know nothing. In a single-threaded process, set-then-exec has no race.

**Why it is a problem.** In multithreaded programs, another thread can fork+exec between `open()` and `fcntl(F_SETFD)`, leaking the descriptor into an unrelated program; the Linux man page calls `O_CLOEXEC` essential for this reason [S106]. POSIX Issue 8 says the atomic flags are "necessary to avoid a data race in multi-threaded applications" and adds `O_CLOFORK` for the matching fork-time leak [S107]. Python's PEP 446 catalogues consequences: listening ports kept busy after a server restart, files and directories that cannot be deleted on Windows, and security leaks (cited cases involve Apache/PHP, OpenSSH, and CWE's fd-exposure entry), affecting projects from Firefox to qemu [S105]. Libraries cannot know which fds a future child will need, so every fd-creating call needs a flag (`SOCK_CLOEXEC`, `pipe2`, `accept4`, `dup3`, ...), and a single forgotten one is a leak. Closing "everything else" before exec needed brute-force loops or `/proc/self/fd` scanning until `close_range()` arrived (Linux 5.9, `CLOSE_RANGE_CLOEXEC` in 5.11) [S108].

**Why it was not fixed.** Flipping the kernel default would break every program that relies on implicit inheritance. Fixes therefore landed as opt-in flags call by call (Linux 2.6.23 for `O_CLOEXEC` [S106]), and runtimes changed their own defaults instead (Python 3.4 made new fds non-inheritable [S105]).

**Alternatives that exist.** Windows: a handle is inherited only if it is marked inheritable *and* `bInheritHandles` is TRUE, and `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` lists exactly which handles a given child gets, which fixes the concurrent-spawn problem [S104]. `posix_spawn` file actions let the parent describe the child's fd table explicitly [S102]. Fuchsia gives a new process only the handles the launcher is handed [S103].

**Cost of the alternatives.** An explicit handle list makes simple redirection more verbose and needs a convention for passing well-known slots (stdin/out/err). Even Windows' design still carries the global `bInheritHandles` switch, whose problems the docs themselves acknowledge [S104].

**Relevance to our OS.** high — with capability handles, "what does a child get" is a security decision; the default should be decided deliberately, not inherited.

**Sources.** [S102], [S103], [S104], [S105], [S106], [S107], [S108]

### DM-003: PID as a reusable integer name; PID-reuse races; `pidfd` retrofit

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux (pidfd since 5.1–5.3), BSDs (FreeBSD process descriptors since 9.0), macOS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Processes are named by small integers from a global namespace. `kill`, `waitpid`, `/proc/<pid>` and most tools take a PID. Once a process is reaped its PID can be reassigned.

**Why it was chosen.** It was cheap and simple with small process tables. A parent owns its children's PIDs until it reaps them, so the parent–child case is race-free if the parent is disciplined.

**Why it is a problem.** Anyone who is *not* the parent (supervisors, `pkill`, PID files, debuggers, monitoring agents) has no way to hold a reference. Between looking up a PID and acting on it, the process can exit and the PID can be reused, so the signal hits an unrelated process. The kernel man page states this failure directly [S111], and LWN calls PID reuse arguably the biggest problem with PIDs [S109]. Even for children, the guarantee breaks if `SIGCHLD` is ignored, `SA_NOCLDWAIT` is set, or something else reaps the zombie [S110].

**Why it was not fixed.** PIDs are part of every API and tool. Linux added a parallel handle-based API instead: `pidfd_send_signal` (5.1) [S111], `CLONE_PIDFD` (5.2) [S112], `pidfd_open` and pollable exit notification (5.3), and `waitid(P_PIDFD)` [S109][S110]. Adding `CLONE_PIDFD` used up the last free `clone()` flag bit, which forced the new `clone3()` with an extensible struct (cross-ref DM-055) [S112].

**Alternatives that exist.** Windows: `CreateProcess` returns a process *handle*, which stays valid until closed even after the process ends, alongside a PID that is valid only while the process lives [S113]. Zircon/Fuchsia: processes are kernel objects owned by jobs and created and manipulated through handles [S103][S2000]. FreeBSD has had process descriptors (`pdfork`, `pdkill`) since 9.0, years before Linux pidfds; by default closing the last descriptor terminates the process [S2002]. Linux pidfds now also support `setns`, `pidfd_getfd` and `process_madvise` [S110], which makes them close to a process capability.

**Cost of the alternatives.** Handles must be closed, and a leaked handle pins kernel state (a "zombie" process object). Humans and shells still want printable short names, so a handle system needs a separate, explicitly racy lookup path. Retrofitting doubles the API surface: Linux now has both PID and pidfd variants of many calls.

**Relevance to our OS.** high — if processes are capability objects from day one, the race disappears by construction; the POSIX layer only has to emulate PIDs.

**Sources.** [S103], [S109], [S110], [S111], [S112], [S113], [S2000], [S2002]

### DM-004: Threads added late to the Unix process model

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux (LinuxThreads → NPTL), BSDs; contrast Windows NT and Mach, which had threads from the start
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Unix defined the process as the single unit of both resource ownership and execution. POSIX threads were added later. POSIX requires many attributes (PIDs, credentials, signal dispositions, cwd, umask, rlimits, record locks) to be process-wide, while the signal mask and errno are per-thread [S118]. Linux implements threads as tasks that share resources, so some of these "process-wide" attributes are in fact per-task in the kernel.

**Why it was chosen.** On uniprocessors, cheap `fork` plus pipes gave enough concurrency; before threads, fork without exec was the practical concurrency mechanism [S100]. Threads arrived when SMP and GUIs demanded them, and had to be layered over existing semantics without breaking single-threaded programs.

**Why it is a problem.** Every older process-level feature needed a retrofitted rule for threads, and the rules disagree. (1) *Credentials:* in Linux, uid/gid are per-thread, so to meet POSIX glibc sends a hidden real-time signal to every thread to make each one repeat `setuid()` [S114]. A runtime that skips libc gets this wrong: in Go, `syscall.Setuid` changed only one OS thread, leaving other goroutines running as root. That issue was open from 2011 until the fix in Go 1.16 [S115]. (2) *Signals:* a process-directed signal goes to an arbitrary thread that has not blocked it, while dispositions are process-wide [S116]. So a library cannot own a signal (cross-ref DM-017). (3) *fork in threads:* see DM-001. (4) *Conformance churn:* LinuxThreads gave each thread its own PID, did not share credentials, and could not deliver process-directed signals. NPTL (glibc 2.3.2, Linux 2.6) still needed fixes up to 2.6.16 and still does not share the nice value [S118].

**Why it was not fixed.** Changing the process/thread split would break the ABI. Each mismatch was patched separately in kernel or libc.

**Alternatives that exist.** Windows NT treats a process as a resource container (address space, handles, security context) and a thread as the schedulable entity, with an optional per-thread impersonation token [S117]. Mach split tasks from threads in the same way [unverified here; see stream J]. Fuchsia also has distinct process and thread objects: a Zircon process is a container of handles, address-space regions and threads [S2000].

**Cost of the alternatives.** Per-thread security contexts (NT impersonation) bring their own confusion about which identity applies to a given access. A clean split still has to answer, for each resource, whether it is per-process or per-thread, but it can do so once and consistently.

**Relevance to our OS.** high — we must decide from the start which state belongs to the address-space container and which to the thread, especially authority/credentials.

**Sources.** [S100], [S114], [S115], [S116], [S117], [S118], [S2000]

### DM-005: Zombies, `wait` semantics and reparenting to init

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; containers (PID 1 problem)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A terminated child stays a zombie (holding its PID, exit status and rusage) until its parent calls `wait`. If the parent dies first, the orphans are reparented to init (PID 1), which must reap them. Only the parent can wait for a process.

**Why it was chosen.** The exit status has to live somewhere until it is collected. Keeping the PID reserved until reaping is also what makes PIDs safe for the parent to use (DM-003). With integer names and no handles, a parent–child relation is the only natural owner.

**Why it is a problem.** Each zombie holds a process-table slot, and if the table fills no new processes can be created [S120]. Exit status is tied to *parentage*, not to whoever cares: supervisors cannot wait for double-forked daemons, which reparent to init [S119]; the fork-then-`setsid` idiom moves the child into a new session and process group [S154]. Linux therefore added `PR_SET_CHILD_SUBREAPER` (3.4) so that session and service managers can adopt orphaned descendants [S119]. In containers, the program running as PID 1 often does not reap at all and gets no default signal handling, which is why tini exists and is bundled as `docker run --init` [S121]. The behaviour of `SIGCHLD = SIG_IGN` was left unspecified in early POSIX and differed in Linux 2.4 [S120]. And the reap-or-leak contract creates the PID-reuse window for non-parents.

**Why it was not fixed.** It is core POSIX semantics. Fixes were additive: subreapers [S119], and pidfds that let non-parents poll for exit (but `waitid` on a pidfd still only works for one's own children) [S109][S110].

**Alternatives that exist.** Windows: a process handle stays valid after the process ends [S113]; on termination the exit code is set and the process object becomes signaled, so any handle holder can wait on it, and the object persists until its last handle is closed [S2001]. Fuchsia: processes are kernel objects owned by jobs and reached through handles [S2000]. Job objects/cgroups give tree-wide supervision (DM-100).

**Cost of the alternatives.** Handle-based lifetime moves the leak from "unreaped zombie" to "unclosed handle", which is just as invisible. The resource cost still exists; only ownership becomes explicit and transferable.

**Relevance to our OS.** medium — falls out of the DM-003 decision; if exit status hangs off a process capability, reparenting and zombies largely disappear, but the POSIX layer must emulate them.

**Sources.** [S109], [S110], [S113], [S119], [S120], [S121], [S154], [S2000], [S2001]

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

**Why it is a problem.** *Resolution is implicit and context-dependent.* Windows searches up to a dozen places, including the application folder, the current folder and `PATH`; if an attacker controls any of them, a planted DLL is loaded, and the docs warn about exactly this [S126]. Microsoft's 2010 advisory on "binary planting" led to a long run of bulletins (2010–2014) fixing Office, Windows components and .NET one at a time, because the root cause was applications passing insufficiently qualified paths. Microsoft could ship only an opt-in mitigation, not a global fix [S128]. Safe search mode, KnownDLLs, `SetDefaultDllDirectories` and `LOAD_LIBRARY_SEARCH_*` are all retrofits [S126]. On ELF, the search order mixes `DT_RPATH` (applies to the whole tree), `LD_LIBRARY_PATH`, `DT_RUNPATH` (direct deps only), `ld.so.cache` and default paths. The loader has to switch into secure-execution mode for setuid binaries and ignore `LD_LIBRARY_PATH`/`LD_PRELOAD`, so environment-controlled code loading is an ever-present privilege hazard (cross-ref DM-011, DM-085) [S127]. *Versioning:* when independently-upgraded shared components overwrite each other ("DLL hell"), installed programs break. Dolstra's thesis names lack of isolation between components, hard-to-identify dependencies, and version incompatibilities as the root causes of such deployment failures [S129].

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

**Why it was chosen.** A string vector is the most general and language-neutral interface. It costs nothing in the kernel, and leaves parsing policy to programs and shells. Windows inherited a single command line from DOS [unverified].

**Why it is a problem.** *Windows:* splitting is the callee's business. The MSVC rules involve backslash-parity handling before quotes [S132], and `cmd.exe` uses different rules. Rust's standard library promised arguments would not be shell-evaluated, but its escaping for `.bat`/`.cmd` targets could be bypassed to run arbitrary commands (CVE-2024-24576, critical). The Rust team concluded that no escaping can be correct for every `cmd.exe` case, and now refuses some arguments [S131]. The single string also makes the executable path itself ambiguous: an unquoted `C:\Program Files\MyApp` can run `C:\Program.exe`, which Microsoft documents as a security risk [S104]. *Unix:* the vector avoids splitting bugs, but anything typed is erased. An operand that starts with `-` becomes an option. For example, git built an `ssh` command line from an `ssh://` URL whose hostname began with `-`, so ssh treated the hostname as an option; a malicious `.gitmodules` could thereby run arbitrary programs (CVE-2017-1000117) [S133][S2003]. Building a command as a string for `system()`/`sh -c` reintroduces shell quoting. The total size of arguments plus environment is also capped by `ARG_MAX` [S102].

**Why it was not fixed.** The interface is the contract between all programs. Structure would have to be agreed upon by every program, not imposed by the kernel.

**Alternatives that exist.** The Unix vector is itself the better half of this comparison. Language-level APIs that never go through a shell (Rust `Command`, Python `subprocess` without `shell=True`) help. The `--` end-of-options convention and git's fix of forbidding hostnames, ports and repository names that begin with `-` are partial measures [S2003]. Typed or structured invocation (e.g. passing handles plus a typed message at spawn, as Fuchsia's launcher does for handles [S103]) is an alternative [unverified as a general argv replacement].

**Cost of the alternatives.** A typed argument schema needs a schema language and a way to evolve it, and would make the POSIX toolchain (compilers, make, shells) depend on an emulation of plain argv anyway.

**Relevance to our OS.** medium — the native spawn ABI should at least pass a vector (never a single string); whether to add structure is open, and the C compiler only needs classic argv.

**Sources.** [S102], [S103], [S104], [S131], [S132], [S133], [S2003]

### DM-100: No reliable, inescapable process-tree containment (process groups vs job objects vs cgroups)

- **Category:** Process model and program execution
- **Systems affected:** Unix/POSIX, Linux (cgroups retrofit, `cgroup.kill` in 5.14), BSDs; Windows (job objects, nesting only since Windows 8)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Classic Unix offers process groups and sessions (designed for terminal job control) as its only way to address a group of processes. Any process can leave its group with `setsid()`/`setpgid()`, and double-forking is the standard way to detach [S154][S233]. There is no kernel object meaning "this service and everything it spawned".

**Why it was chosen.** Process groups were built for interactive shells: send ^C to the foreground job. Escaping them is a *feature* for daemons. Supervision of long-running services was not in scope.

**Why it is a problem.** A supervisor cannot reliably find, account for or kill everything a service started: daemons double-fork away from their supervisor [S233], PIDs race (DM-003), and orphans reparent to init (DM-005). Linux added subreapers so that supervisors at least inherit orphaned descendants [S119]. It then made cgroups the de-facto tracking mechanism [S233], but killing a cgroup still meant a recursive user-space walk (systemd's approach) while processes kept forking. For containers that share a cgroup or PID namespace with other processes, killing the container's init PID was not enough. `cgroup.kill` (Brauner, 2021) kills a cgroup tree with one write and handles concurrent forks: a child forked into a cgroup that is being killed receives SIGKILL [S153][S152]. Each step is a retrofit onto a model where containment was optional.

**Why it was not fixed.** Process groups are part of the job-control ABI (DM-070). Containment came in as separate subsystems (cgroups), whose v1 design then had to be redone as v2 (DM-111) [S152].

**Alternatives that exist.** Windows job objects: by default children created with `CreateProcess` join their parent's job, and the association cannot be broken. Jobs support limits, accounting, `TerminateJobObject`, and kill-on-last-handle-close [S151]. But until Windows 8 a process could be in only one job, which broke tools that nest; breakaway flags complicate guarantees [S151]. Fuchsia's process creation is scoped by jobs with policy, such as forbidding direct process construction in a job tree [S103].

**Cost of the alternatives.** Mandatory containment needs rules for nesting and for deliberate breakaway (e.g. a user session launching an independent app). Windows shows that getting nesting wrong at first is costly [S151]. Accounting adds per-fork kernel work.

**Relevance to our OS.** high — if every process is created inside a job/container capability, supervision, resource limits and the "kill this app" operation come for free; this is cheap to design in now and expensive later.

**Sources.** [S103], [S119], [S151], [S152], [S153], [S154], [S233]

## 2. Security, authority and identity

### DM-009: Ambient authority: a process may name and open anything its user can

- **Category:** Security, authority and identity
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS, Windows NT (any ACL/identity system with global namespaces)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** A process names resources through global namespaces (file paths, PIDs, port numbers, IPC keys) and the kernel decides access by checking the process's identity against the object's permissions. The process does not present or choose an authority; any request it makes is evaluated with all the authority it holds. Miller, Yee and Shapiro call the absence of selection "ambient authority" [S201].

**Why it was chosen.** Time-sharing systems needed to protect users from one another, and a per-user identity plus a per-object list is cheap to check and easy to administer. Programs were mostly trusted tools run by their user, so "program authority = user authority" was a reasonable approximation. Global names make composition easy: any program can open any path the user types.

**Why it is a problem.** Designation and authority are separated, so a program acting for several principals cannot say which authority a request uses. Hardy's Tymshare compiler had a license to write its own directory; a user passed the name of the billing file as the debug output path and the compiler overwrote it [S200]. Hardy also reports that the rules for when a program may open a file kept growing (the last version needed fourteen boolean operators), and each clause added to enable a case introduced security problems in programs that had been secure [S200]. The failure was observed in the 1970s, so it is not purely a consequence of later threat models. The same pattern underlies path-based TOCTOU, setuid exploitation (DM-011), and sandbox escapes: a sandbox has to close every global namespace, and Capsicum found even `fexecve` reaches the global namespace via the ELF interpreter path [S202].

**Why it was not fixed.** Every POSIX API takes global names. Capsicum therefore keeps the global-name APIs and adds an opt-in capability mode, plus a sandbox-launch library and a capability-aware run-time linker, because even descriptor-based exec reaches the global namespace through the linker path embedded in the binary [S202].

**Alternatives that exist.** Object-capability systems (KeyKOS, EROS; later seL4) combine designation with authority so that holding a reference is the permission [S201][S200]. Fuchsia starts a process with nothing; it gets only the handles and namespace entries its creator gives it [S204]. Capsicum retrofits capability mode and per-descriptor rights onto FreeBSD (shipping since 9.0) [S203].

**Cost of the alternatives.** Every resource must be passed explicitly, so launchers, shells and libraries need a delegation protocol; Capsicum needed a new runtime (libcapsicum) and a rethought process model [S202]. Arbitrary user-typed paths need a powerbox or a trusted file chooser. Existing Unix software is not structured for this, which limits retrofit adoption. On the other side, Hardy reports that Key Logic's capability systems came out smaller than equivalent access-list systems because capabilities replaced several ad hoc naming and protection mechanisms [S200]; the cost is mostly in porting and delegation plumbing, not in the kernel.

**Relevance to our OS.** high — the native ABI is being designed now, and whether syscalls take global names or handles is the decision that determines all later sandboxing cost.

**Sources.** [S200], [S201], [S202], [S203], [S204]

### DM-010: Access control keyed on user identity, not on program or request (uid/gid DAC)

- **Category:** Security, authority and identity
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows NT (SID-based, same principle)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Every process carries a user ID (plus groups); files carry owner, group and mode bits; the kernel allows an operation if the process's credentials match. All programs a user runs get the same rights.

**Why it was chosen.** On multi-user time-sharing machines the threat was other users. Watson et al. note DAC "was designed to protect users from each other" [S202]. It is cheap (a few bits per inode), understandable, and owners can manage it without an administrator.

**Why it is a problem.** The modern threat is a compromised or malicious program running as the single user who owns all the interesting data. DAC cannot express "this PDF parser may read this one file", so a browser renderer or image decoder exploit gets everything the user can touch [S202]. Neither DAC nor MAC was designed for one application handling many kinds of data for one user [S202]. Hardy's confused deputy is the same mismatch: the decision depends on who the subject is, not on what the request is for [S200].

**Why it was not fixed.** The uid model is wired into file systems, NFS, `ls -l`, shells and POSIX. Systems that wanted per-app isolation reused it rather than replacing it: Android assigns each app its own uid, "using DAC identities to directly represent applications rather than users", which the SEAndroid authors say mitigates but does not remove DAC weaknesses [S222].

**Alternatives that exist.** Capabilities attached to descriptors (Capsicum) [S202]; MAC policies keyed on program labels (SELinux/SEAndroid) [S222]; per-application identity (Android uids, Windows AppContainer SIDs — see DM-016).

**Cost of the alternatives.** Per-app uids multiply identities and need custom kernel patches for things DAC never controlled (Android needed custom kernel changes so that DAC identities could control creation of network and Bluetooth sockets) [S222]. MAC needs large policies (DM-015). Capabilities need explicit delegation (DM-009). Users still need a "the human is the principal" notion for sharing between people.

**Relevance to our OS.** high — choosing who the principal is (user, app, component, or the held handle) is a foundational ABI decision.

**Sources.** [S200], [S202], [S222]

### DM-011: `setuid`/`setgid` binaries

- **Category:** Security, authority and identity
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** mistake
- **Confidence:** high

**What the design is.** An executable file with the set-user-ID bit runs with the file owner's effective uid rather than the invoker's. This is the standard way to let unprivileged users perform specific privileged actions (`passwd`, `su`, `sudo`, `pkexec`, `mount`). Bell Labs filed the patent on Ritchie's invention of the bit in 1973 [S205].

**Why it was chosen.** It is a tiny mechanism (one mode bit) that delegates privilege without a daemon or IPC, which mattered on 1970s machines. It lets a program, not a person, hold a privilege, which was a real advance over shared root passwords.

**Why it is a problem.** The privileged program starts with an execution context entirely chosen by the attacker: argv, environment, fds, rlimits, signal dispositions, cwd, umask. Every one is an input. PwnKit (CVE-2021-4034): with `argc == 0`, pkexec's `argv[1]` overlapped `envp[0]`, and its out-of-bounds write reintroduced `GCONV_PATH`, which made glibc load an attacker library as root; the bug existed since pkexec's creation in 2009 [S206][S207]. Sudo's heap overflow CVE-2021-3156 is another root-from-any-user bug in a setuid program [S208]. The dynamic loader needs a special "secure-execution mode" that strips a long list of variables (`LD_*`, `GCONV_PATH`, `LOCALDOMAIN`, `TMPDIR` and others) [S127]. The uid-changing calls themselves are badly designed: Chen, Wagner and Dean found semantics differ between Linux, Solaris and FreeBSD, POSIX leaves "appropriate privilege" open, and misuse caused real sendmail vulnerabilities [S205].

**Why it was not fixed.** `su`, `passwd`, `sudo` and many distribution tools depend on it, and POSIX specifies it. Replacements need a privileged broker that every distribution has to adopt.

**Alternatives that exist.** Android removed all setuid/setgid programs in 4.3 and mounts /system nosuid for app processes [S210]. systemd's `run0` elevates through the service manager and polkit with no setuid bit and the command inherits no execution or security-context credentials from the caller, because it runs in a fresh service forked by the service manager [S211]. Under OpenBSD pledge a process cannot set setuid/setgid bits, and with execpromises set an exec of a setuid/setgid target fails with `EACCES` [S212]; OpenBSD's kernel refuses `argc == 0`, which blocked PwnKit [S206]. Plan 9 changes a process's user id only by presenting a one-shot capability that the host owner's authentication agent (factotum) registers with a kernel device, rather than through a file mode bit [S214].

**Cost of the alternatives.** A broker daemon is a long-running privileged process with its own IPC attack surface (polkit itself is such a broker). Policy moves into configuration files that must be audited.

**Relevance to our OS.** high — a capability OS can exclude privilege-on-exec entirely; the POSIX layer must decide whether to emulate it.

**Sources.** [S205], [S206], [S207], [S208], [S127], [S210], [S211], [S212], [S214]

### DM-012: Root as an all-powerful superuser

- **Category:** Security, authority and identity
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS (before SIP); Windows Administrator/SYSTEM is analogous
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** User ID 0 bypasses permission checks and "can access all resources" [S205]. Every administrative operation, from binding port 80 to loading kernel modules, is gated on this one identity.

**Why it was chosen.** A single administrator on a small machine needed a simple escape hatch; one check (`uid == 0`) is trivial to implement and reason about. Fine-grained privilege would have needed policy machinery that early Unix deliberately avoided.

**Why it is a problem.** Any compromise of any root process is total, so every daemon that needs one privileged operation becomes a full-system risk. Linux introduced capabilities precisely because a compromised program with only some privileges can do less damage than one running as root [S216]. Root also cannot be meaningfully constrained by the system it administers: Apple says that before System Integrity Protection (OS X El Capitan) root had no permission restrictions, so any installer given an administrator password could overwrite system files; SIP now limits what root can do to protected locations [S215]. Root is the payoff for every setuid bug (DM-011).

**Why it was not fixed.** Decades of tools, scripts, packaging and documentation assume "run as root". Splitting root into capabilities happened, but the split became coarse again (DM-013).

**Alternatives that exist.** Plan 9 has no all-powerful account; the host owner is described as the closest analogue to root but "far weaker" [S214]. Capability kernels (seL4, Zircon) have no identity that bypasses checks; power is whatever handles a process holds. macOS SIP layers a non-root-bypassable integrity policy [S215].

**Cost of the alternatives.** Recovery and administration need a defined path (for SIP, restarting into Recovery mode to turn it off with `csrutil` [S2004]); with no superuser, a misconfigured system can lock out its owner. Plan 9's model depends on a separate authentication server design [S214].

**Relevance to our OS.** high — deciding whether any principal bypasses checks, and how a human recovers a broken system without one, is a core security-model decision.

**Sources.** [S205], [S214], [S215], [S216], [S2004]

### DM-013: Linux "POSIX capabilities" vs real capability systems (`CAP_SYS_ADMIN`)

- **Category:** Security, authority and identity
- **Systems affected:** Linux (based on the withdrawn POSIX.1e draft [S216])
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Linux splits root's power into per-thread privilege bits (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, ...) held in permitted/effective/inheritable/bounding/ambient sets, with file capabilities in an xattr since 2.6.24 and an ambient set since 4.3 [S216]. These are privileges attached to a subject, not unforgeable references to objects.

**Why it was chosen.** It gives a compatible, incremental way to stop running daemons as full root without changing any API: the same `uid == 0` checks become `capable(X)` checks.

**Why it is a problem.** Granularity eroded. In Linux 3.2, 451 of 1,167 capability checks were `CAP_SYS_ADMIN` (16% in Linux 2.2), developers pick it when unsure because their feature will be administered by system administrators and the kernel gives little guidance on which capability to use, and holding it often lets an attacker escalate to full root [S217]. The man page now tells kernel developers "Don't choose CAP_SYS_ADMIN if you can possibly avoid it" [S216]. The bits are still ambient: a process with `CAP_DAC_OVERRIDE` overrides checks on every file, so confused-deputy problems remain. The name also collides with real capabilities; LWN notes Capsicum's capabilities are "entirely different from Linux capabilities" [S219]. The set semantics across `execve` were complex enough to need a later ambient set [S216].

**Why it was not fixed.** Splitting a capability breaks programs that hold only the old one, so the proposed remedy is to accept either the old or new capability, which keeps the old bit powerful [S217].

**Alternatives that exist.** Object capabilities (KeyKOS/EROS, seL4, Zircon handles, Capsicum rights on descriptors [S203]) bind authority to a specific object and are passed explicitly.

**Cost of the alternatives.** Object capabilities require restructuring how programs acquire resources (DM-009) and cannot be applied to existing binaries by flipping a bit.

**Relevance to our OS.** medium — mainly a naming and design warning: a "privilege bit" set is not a capability system, and coarse administrative privileges tend to accrete.

**Sources.** [S216], [S217], [S219], [S203]

### DM-014: Sandboxing retrofitted as syscall filters (seccomp-bpf) vs pledge/unveil vs Capsicum

- **Category:** Security, authority and identity
- **Systems affected:** Linux (seccomp-bpf), OpenBSD (pledge/unveil), FreeBSD (Capsicum), macOS (Seatbelt), Windows (restricted tokens)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Three retrofits to restrict an already-running Unix process. seccomp-bpf: a BPF program inspects each syscall number and register arguments [S218]. pledge: the program declares named subsystem "promises"; unveil: it declares the visible file paths [S212][S213]. Capsicum: the process enters capability mode, losing global namespaces, and descriptors carry rights [S202][S203].

**Why it was chosen.** None of these systems could remove ambient authority (DM-009), so each adds a one-way restriction on top. seccomp needs no change to the rest of the kernel; pledge optimizes for programmer effort; Capsicum aims for a principled capability model while staying Unix-compatible.

**Why it is a problem.** Each has a characteristic failure. seccomp: the kernel docs say "System call filtering isn't a sandbox"; filters cannot dereference pointers, so paths and structs cannot be checked, and syscall numbers differ by ABI so filters must check the architecture [S218]. Policies keyed to syscall numbers are also widely reported to break when libc changes which syscalls it uses [unverified]. Chromium's pre-Capsicum sandboxes took from a few hundred lines to about 22,000 (Windows) and 11,301 (Linux seccomp plus a userspace syscall wrapper), and Watson et al. found vulnerabilities in all of them; the Linux suid sandbox was also porous and needed OS privilege to set up [S202]. pledge's promises are coarse, a violation kills the process [S212], and unveil warns that hiding files from arbitrary programs is dangerous and is cleared by `execve` [S213]. Capsicum needs real program restructuring and a new runtime [S202]; its Linux port was described in 2014 as invasive and a "hard sell" [S219]; I found no evidence it was merged [unverified].

**Why it was not fixed.** These are already the fix; the underlying ambient-authority ABI cannot change.

**Alternatives that exist.** Native capability systems where a fresh process has nothing (Fuchsia [S204], seL4). In practice seccomp is used widely as attack-surface reduction, e.g. Android blocking io_uring from apps [S220].

**Cost of the alternatives.** Capsicum showed low code cost when the model fits (tcpdump about 10 lines, Chromium about 100) [S202]; pledge's simplicity drove broad adoption across OpenBSD's base system [unverified]. A native capability design pays the delegation cost everywhere instead.

**Relevance to our OS.** high — the POSIX compatibility layer hosting a C compiler will need exactly this kind of confinement, and the native layer can avoid needing it.

**Sources.** [S202], [S203], [S212], [S213], [S218], [S219], [S220], [S204]

### DM-015: Mandatory access control retrofitted via LSM (SELinux/AppArmor)

- **Category:** Security, authority and identity
- **Systems affected:** Linux (SELinux, AppArmor, Smack), Android (SEAndroid); similar: TrustedBSD MAC, macOS Seatbelt
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** The Linux Security Modules framework puts hooks at kernel object operations plus opaque `void*` security blobs on kernel structures; a module such as SELinux uses them to enforce a system-wide policy on top of DAC [S221].

**Why it was chosen.** At the March 2001 Kernel Summit the NSA presented SELinux while several other MAC projects maintained their own patches; Torvalds asked for a generic hook framework rather than one built-in model, and LSM was merged in December 2003 [S221]. This let the kernel avoid choosing a security model.

**Why it is a problem.** Policies are big and hard to get right: Xu et al. cite an SELinux policy of over 30,000 statements, and misconfigured policies make access control ineffective [S223]. MAC was designed for administrator-defined system policy, not for applications compartmentalising themselves, so app developers cannot use it for least privilege [S202]. The hook framework let only one "major" module own the blobs; LWN called stacking a "perennial" problem in 2012 [S224], and in 2014 it was not expected soon [S219]; the kernel docs now describe "a close approximation" of stacking [S221]. Disabling SELinux to make software work is a widely reported admin practice [unverified — anecdotal only].

**Why it was not fixed.** The hook-based design is now the kernel ABI for every security module; the policy complexity reflects the complexity of the ambient-authority system being constrained.

**Alternatives that exist.** Android shows MAC can work when one vendor writes the whole policy: SEAndroid mitigated multiple published exploits [S222]. Capability systems make most policy implicit in handle distribution [S202].

**Cost of the alternatives.** SEAndroid needed kernel work such as security labeling for the yaffs2 file system, special handling for apps that zygote forks without an `exec` (SELinux normally changes context at exec), and separate middleware MAC extensions for Android's framework-level permissions [S222]. Without MAC, system-wide invariants such as "no app may ever touch this device" have to be ensured by construction instead of by a declarative policy.

**Relevance to our OS.** medium — we could need system-wide policy for invariants, but should decide whether it is a layer over ambient authority or derived from capability distribution.

**Sources.** [S221], [S222], [S223], [S224], [S219], [S202]

### DM-016: Windows security descriptors/ACL complexity and token model

- **Category:** Security, authority and identity
- **Systems affected:** Windows NT and successors
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Every securable object has a security descriptor with an owner, a DACL of prioritized allow/deny entries, and a SACL [S226], [S227]; every process has a primary access token listing user, group and logon SIDs, privileges, optional restricting SIDs, and whether it is a primary or impersonation token, and a thread can additionally hold an impersonation token [S225]. Vista added Mandatory Integrity Control: integrity labels, stored in the SACL, are checked before the DACL, and objects get no-write-up by default [S226]; AppContainer adds a further isolation layer that MIC also respects [S226].

**Why it was chosen.** NT was designed for enterprise domains and formal security evaluation [unverified], so it made access control uniform across all kernel object types and expressive enough for any policy. Tokens give a clean per-thread security context and support impersonation for servers.

**Why it is a problem.** Expressiveness exceeds what developers can reason about. Govindavajhala and Appel count up to about 30 rights on about 15 object kinds, and their scanner found several user-to-administrator escalations caused by commercial vendors' ACL misconfigurations on registry keys, files and services (CERT VU#953860) [S227]. Impersonation tokens make servers act with the client's authority, a confused-deputy surface [unverified as a measured trend]. Before Vista, applications typically ran as administrators, so Windows needed registry (and file) virtualization, which Microsoft calls an "interim" compatibility technology [S228], and integrity levels as a second, mandatory layer [S226].

**Why it was not fixed.** The model is consistent and is the basis of Active Directory; complexity lives in policy rather than in the mechanism, so it is retrofitted with layers (MIC, AppContainer) rather than replaced.

**Alternatives that exist.** Unix's simpler mode bits: three privileges for user, group and other on a few kinds of object [S227] (less expressive; that this yields fewer misconfigurations is plausible but not measured in the cited source); capability handles (NT handles themselves carry granted access masks after open — see GR-003).

**Cost of the alternatives.** Simpler models cannot express delegation to specific groups or deny entries, and push complexity into setuid helpers (DM-011).

**Relevance to our OS.** medium — positive lesson: one uniform object/security model across all kernel objects; negative lesson: expressive ACL languages produce misconfiguration CVEs.

**Sources.** [S225], [S226], [S227], [S228]

### DM-101: Containers as an emergent assembly of retrofitted primitives (no kernel container object)

- **Category:** Security, authority and identity
- **Systems affected:** Linux (namespaces 2002–2020, cgroups, seccomp, LSMs) and the container runtimes built on them; contrast FreeBSD jails, Solaris zones, Zircon jobs, Plan 9
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Linux has no kernel "container" object; the kernel is unaware of how user space composes containers [S163]. User-space runtimes assemble a container from independent mechanisms: eight per-resource namespace types (cgroup, IPC, network, mount, PID, time, user, UTS) [S2602], cgroups for resource control, and seccomp plus LSMs for further restriction. The namespaces were retrofitted one at a time: mount namespaces in 2.4.19 (2002), user namespaces completed in 3.8 (2013) [S162], cgroup namespaces in 4.6 and time namespaces in 5.6 [S2602]. (Category chosen over "Configuration, observability and system management" because containers are relied on as a security boundary the kernel never defines as a unit.)

**Why it was chosen.** *Steelman:* separate, composable primitives let one kernel serve many policies with no single definition of "container". These include full OS containers, per-application sandboxes (browsers, build tools, systemd service hardening) and migration between hosts, where PID namespaces keep PIDs stable [S162]. A proposal to make containers kernel objects was rejected: namespaces maintainer Eric Biederman said it "muddies the waters and makes things more brittle" [S163]. Unprivileged user namespaces let ordinary users build sandboxes, replacing many uses of setuid programs [S230].

**Why it is a problem.** *(1) The security boundary exists only as a user-space convention.* A container is secure only if every namespace, cgroup, seccomp filter and LSM profile is configured consistently by the runtime. The kernel has no object whose invariants it can enforce or reason about. *(2) No unit of identity.* Because the kernel does not know what a container is, audit cannot attribute events to one, which needed a separate container-ID retrofit with its own debates about nesting and permissions [S163]. *(3) No unit of control.* Killing "a container" had no single kernel handle until `cgroup.kill` (see DM-100 for details) [S153]. *(4) Each retrofit has its own failure modes.* Unprivileged user namespaces expose root-only kernel code to ordinary users (see DM-110). The cgroup v1 multi-hierarchy design had to be replaced by v2 (see DM-111).

**Why it was not fixed.** Every namespace type is ABI, and runtimes depend on the exact behaviour of each one. Kernel-level container objects lacked consensus [S163].

**Alternatives that exist.** *FreeBSD jails* (2000): one construct that confines "omnipotent root" while keeping the simplicity of the Unix root model. Privileged users find their requests limited to the jail, so administration can be delegated per jail [S158], and jails build on the chroot concept [S164]. *Solaris zones* (2004): designed from the start to combine namespace isolation, security isolation and resource management. They are "double-hulled": zone processes can never acquire all privileges, so even a namespace escape does not grant e.g. `/dev/kmem` access [S159]. *Zircon jobs*: a tree of jobs; every process belongs to exactly one job, which tracks privileges and limits resources [S160]. *Plan 9*: per-process name spaces built with `bind`/`mount` and shared or copied selectively via `rfork` are the ordinary way programs see resources [S161]. Isolation is then a property of naming, not a container add-on.

**Cost of the alternatives.** A jail or zone is a fixed bundle, so finer sandboxes such as per-tab browser isolation need other tools, and zones' fixed privilege set was a stated limitation [S159]. Kernel container objects raise the nesting and ownership questions Biederman objected to [S163]. Plan 9 namespaces alone give neither resource limits nor a privilege model [S161].

**Relevance to our OS.** high — if a process's namespace is simply the set of capabilities it was given (Zircon/Plan 9 style), "container" and "sandbox" need no extra kernel surface, but a job-like unit for limits, accounting, identity and kill is still needed (DM-100).

**Sources.** [S153], [S158], [S159], [S160], [S161], [S162], [S163], [S164], [S230], [S2602]

### DM-110: Unprivileged user namespaces expose root-only kernel attack surface

- **Category:** Security, authority and identity
- **Systems affected:** Linux (container and sandbox retrofit via namespaces)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Since Linux 3.8 any unprivileged process can create a user namespace in which it holds a full capability set; that lets it create other namespaces and use operations normally limited to root, scoped to the resources the namespace governs [S155]. It is the basis for rootless containers and for browser sandboxes [S230].

**Why it was chosen.** Containers were built from separate namespaces retrofitted onto a kernel with a global root (DM-012, DM-101). User namespaces can replace many uses of setuid programs (DM-011), which were themselves a risk, and let applications build stronger sandboxes [S230].

**Why it is a problem.** Every kernel subsystem must be correct about which capability checks are namespace-relative, and code once reachable only by real root becomes reachable by anyone [S230]. The man page records a series of security fixes, e.g. the `/proc/PID/setgroups` file (3.19) and requiring `CAP_SETFCAP` to map uid 0 (5.12) [S155]. At Pwn2Own 2023, a 9-year-old nf_tables bug (CVE-2023-35001) was reachable by an unprivileged user on Ubuntu because user namespaces opened that surface [S231]. Canonical reports that unprivileged user namespaces are widely used in privilege-escalation chains and states that a Google report found 44% of the exploits Google saw required them [S230]. This figure is second-hand: Canonical's link points to Google's kCTF post [S220], which (as fetched on 2026-10-20) gives a 60% io_uring figure but does not state the 44% figure; no Google original for 44% was found [unverified].

**Why it was not fixed.** The sysctl switch is all-or-nothing and breaks applications that rely on it, so Ubuntu 23.10 added per-application AppArmor gating instead [S230]. Rootless containers and browsers depend on it.

**Alternatives that exist.** Per-app gating (Ubuntu) [S230]; capability systems where creating an isolated environment needs no privilege because a new process starts with nothing and gets only the handles it is given (Fuchsia) [S204].

**Cost of the alternatives.** Disabling user namespaces forces sandboxes back to setuid helpers; gating requires profiles for each app, and Canonical admits its profile coverage may never be complete [S230].

**Relevance to our OS.** high — shows the cost of building isolation by making privileged kernel code "safe for unprivileged use" instead of making isolation the default.

**Sources.** [S155], [S230], [S231], [S220], [S204]

## 3. Inter-process communication

### DM-017: Signals as a notification/IPC mechanism; async-signal safety

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** mistake
- **Confidence:** high

**What the design is.** A signal is a small integer delivered asynchronously to a process (later, to a thread). It either runs a default action (terminate, stop, ignore) or interrupts whatever code is running to call a user handler on the same stack. The same mechanism carries hardware faults (SIGSEGV), job-control events, timers (SIGALRM), child exit (SIGCHLD) and plain IPC notifications (SIGUSR1, SIGHUP for "reload").

**Why it was chosen.** Early Unix ran single-threaded processes on small machines. A software interrupt that reused the hardware-interrupt model was the cheapest way to report faults, let the terminal kill a process, and wake a process out of a blocking call. There was no event loop or descriptor-multiplexing primitive to deliver events to instead. Brown, looking back, calls the original design simple and elegant but "fatally flawed": the one-shot handlers raced from the start, so the IPC/notification use was problematic even before threads arrived [S303]. Synchronous fault delivery (SIGSEGV, SIGFPE), by contrast, he describes as handled well by the original design [S303].

**Why it is a problem.** A handler can run in the middle of any instruction sequence, including inside `malloc` or stdio, so it may only call *async-signal-safe* functions. That list is short, differs between standards revisions, and implementations deviate (glibc `execl` was unsafe before 2.24; `aio_suspend` is unsafe) [S300]. Calling an unsafe function from a handler can corrupt memory or deadlock. A concrete case: OpenSSH's SIGALRM handler called `syslog()` and could corrupt the heap mid-`malloc`. This was CVE-2006-5051. It was fixed, then re-introduced in 2020 when a guard `#ifdef` was removed, giving unauthenticated remote root on glibc Linux (CVE-2024-6387, "regreSSHion") [S301]. The rest of the history is a series of patches, each exposing the next problem [S303]: one-shot handlers that had to re-arm themselves and could lose signals; `EINTR` from interrupted system calls; `sigaction` flags; `pselect`/`ppoll` to close wait races; real-time signals whose limited queues can still lose signals; and SIGCHLD/SIGCLD semantic differences. Threads made it worse, because delivery is now split between process-directed and thread-directed pending sets [S302].

**Why it was not fixed.** POSIX and every shell, daemon and runtime rely on signal semantics, including SIGKILL, SIGPIPE defaults and SIGHUP-to-reload conventions. Linux added `signalfd` (2.6.22, 2007) to read signals as descriptor events [S302], [S303]. But the signals must be blocked first. The blocked mask stays in force across `exec`, so helper programs (including ones spawned behind the scenes by libraries) do not see the signals they expect unless the parent unblocks them first, and synchronously generated signals such as SIGSEGV still need real handlers [S302], [S303]. The old path cannot be removed.

**Alternatives that exist.** Descriptor-based event delivery: `signalfd`, plus `pidfd` for child exit. Windows has no general asynchronous-signal mechanism for IPC [unverified as stated; see S339 for the console case]. Console control events run on a fresh thread the system creates, not by interrupting a running one [S339]. Microkernels (seL4) deliver asynchronous events as notification objects that a thread explicitly waits on [S305]. Faults then become IPC to a fault handler.

**Cost of the alternatives.** Something still has to kill an unresponsive process unconditionally. Every design keeps a non-catchable termination path. Synchronous faults (page fault, divide by zero) need some form of upcall or fault IPC, whose handler runs in a constrained context too. A per-thread fresh handler, as on Windows, moves the problem to ordinary thread synchronisation [S339].

**Relevance to our OS.** high — the POSIX layer must emulate signals for the C toolchain, but the native ABI can choose whether asynchronous interruption of user code exists at all.

**Sources.** [S300], [S301], [S302], [S303], [S305], [S339]

### DM-018: Mach IPC performance and the L4 response

- **Category:** Inter-process communication
- **Systems affected:** Mach 3.0, OSF/1 MK, MkLinux, GNU Hurd, XNU (Mach heritage), L4 family, seL4
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Mach 3.0 moved Unix out of the kernel into a user-level server, so every system service became an IPC through ports. Mach ports are kernel-buffered message queues, unlike later L4 endpoints [S305]; port rights act like capabilities and messages can carry out-of-line memory [unverified in cited sources]. Liedtke's L3/L4 redesigned IPC as minimal synchronous rendezvous between threads, tuned per processor [S304], [S305].

**Why it was chosen.** Mach aimed for multiprocessor and distributed systems, plus Unix compatibility. Asynchronous buffered ports with rich, network-transparent semantics fit that goal, and the generality looked worth the cost at the time.

**Why it is a problem.** By the early 1990s a one-way microkernel message cost about 100 µs. This led vendors to move services back into the kernel [S305]. Chen and Bershad measured Mach 3.0 plus its Unix server executing substantially more system instructions, at more memory cycles per instruction, than monolithic Ultrix [S307]. Some read this as inherent to microkernels. Liedtke reanalysed the same data. A best-case Mach kernel call took 18 µs on a 50 MHz 486 (about 900 cycles), of which roughly 800 cycles were pure kernel overhead, against a minimal 15 cycles in L3. He argued the cache misses came from Mach's large working set, not from the structure itself [S304]. With a minimal, processor-specific design, L4 IPC was 10–20× faster [S305]. L4Linux came within about 5% of native Linux on AIM, against roughly 5–7× larger penalties for MkLinux [S306].

**Why it was not fixed.** Mach's API was baked into its OS servers and into XNU, which kept Mach messaging but runs BSD in the same kernel address space [unverified in cited sources]; vendors generally responded to slow IPC by moving services back into the kernel [S305]. The L4 community also did not keep Liedtke's full design. Long IPC, IPC timeouts, and clans-and-chiefs were abandoned as complex or unusable. Thread-ID destinations were replaced by endpoint capabilities [S305].

**Alternatives that exist.** seL4/Fiasco.OC: synchronous endpoints with no kernel buffering, badged capabilities, and virtual message registers [S305]. Notification objects for asynchronous signalling. Shared memory for bulk data.

**Cost of the alternatives.** Purely synchronous IPC forces multithreaded servers. Before notifications, a server could not wait for both clients and interrupts. Synchronous rendezvous is "clearly the wrong way" to synchronise across cores [S305]. Processor-specific fast paths reduce portability [S304]. Classified as a tradeoff rather than contested: the cited primary sources agree that Mach's slowness came from its implementation and generality, not from user-level services as such [S304], [S305], and L4 paid for its speed with lost features that later had to be re-added (notifications) [S305]. Whether the remaining overhead of about 5% and the extra engineering are worth the isolation is the separate, genuinely contested monolithic-vs-microkernel question (DM-046).

**Relevance to our OS.** high — if services live in user space, IPC cost and semantics set the performance ceiling of everything else.

**Sources.** [S304], [S305], [S306], [S307]. [S308] unverified, not counted.

### DM-019: System V IPC: separate key namespace, objects outside the fd model

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX (XSI), Linux, BSDs, macOS, Solaris
- **Classification:** mistake
- **Confidence:** high

**What the design is.** System V message queues, semaphores and shared memory are named by integer `key_t` values [S310]. `*get` calls turn a key into an identifier used by the other calls [S309]. They are not file descriptors. They have their own `ipc_perm` permission bits, and they persist until explicitly removed [S311].

**Why it was chosen.** In the late 1970s and early 1980s the file system offered no shared memory and no counting semaphores. A separate subsystem let unrelated processes rendezvous without a common parent or a filesystem object, and integer keys were cheap.

**Why it is a problem.**
- Keys are a flat global namespace with no collision protection. `ftok` builds keys from inode and device bits and explicitly gives no uniqueness guarantee [S310].
- Objects are not reference-counted by process lifetime. Removal must be explicit, and a crashed program leaks segments whose pages stay in memory or swap [S311].
- Because they are not fds, they do not work with `poll`, descriptor passing, `close-on-exec`, or capability-style delegation.
- Limits are system-wide tunables (`SHMMAX`, `SEMMNI`, …) whose defaults and setting methods vary per OS; some BSD defaults are "unworkably small" and must be raised by hand [S313].
- Containers needed a whole new namespace type just to isolate these identifiers [S312].
- The lifetime model fights other policy. systemd's `RemoveIPC` deletes a user's objects at logout, which can make PostgreSQL "very unreliable" [S313].

**Why it was not fixed.** The interfaces are in the XSI part of POSIX and in long-lived software. They are kept for compatibility, while applications migrate away one by one. PostgreSQL 9.3 moved most of its shared memory to `mmap` specifically to avoid kernel-parameter tuning [S314].

**Alternatives that exist.** POSIX shared memory, message queues and semaphores, offered as alternative APIs for the same jobs [S309] (`shm_open` uses path-like names and returns an fd [unverified in cited sources]). Linux `memfd_create`: an anonymous, fd-backed, reference-counted memory object with seals so it can be shared safely with untrusted peers [S315]. Capability-system VMOs and handles (Zircon, seL4 frames), where there is no global name at all.

**Cost of the alternatives.** Without a global name, two unrelated processes need some broker or inherited channel to find each other. That is a bootstrapping and naming problem the design must solve elsewhere. Seals and fd passing add API surface [S315].

**Relevance to our OS.** medium — the native design should not repeat it, but the POSIX layer may need to emulate SysV shm/sem for ported software.

**Sources.** [S309], [S310], [S311], [S312], [S313], [S314], [S315]

### DM-020: Untyped byte streams as the only composition medium

- **Category:** Inter-process communication
- **Systems affected:** Unix/POSIX shells and tools, Linux, BSDs, macOS; contrast PowerShell (Windows), Nushell
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Unix pipes, files and terminals carry undifferentiated bytes. Programs compose by agreeing informally on text formats such as lines and whitespace-separated fields. The kernel imposes no schema, and every consumer parses.

**Why it was chosen.** The original authors made this a deliberate style rule. Write programs whose output can become input to "another, as yet unknown, program", and avoid columnar or binary input formats [S316]. A byte stream is the lowest common denominator. It needs no shared type system across languages, it can be inspected by a human, and it keeps the kernel simple. Even Snover, the main critic, grants that the traditional model reflected the technology available when it emerged [S317].

**Why it is a problem.** Structure is reconstructed by parsing, which is lossy and fragile. Snover's critique: tools bind getting objects, processing them and rendering text together, so downstream tools do "prayer-based parsing" to recover the objects [S317]. Concrete failure modes: filenames containing newlines, spaces, leading dashes or control characters break line-oriented loops, `xargs`, and globs. Some of these are security bugs, such as CVE-2013-7085 in Debian's `uscan`, where a whitespace character in a filename let remote attackers delete arbitrary files (as listed by Wheeler [S318], a weak source). Output formats become de facto ABIs that cannot change without breaking scripts.

**Why it was not fixed.** Every tool reads and writes text, and the conventions are in POSIX. Partial fixes such as `-print0`/`-0` and NUL separators are spread unevenly [S318].

**Alternatives that exist.** PowerShell pipes .NET objects and uses reflection for generic `Where`/`Sort`/`Format` operations [S317]. Nushell passes structured values and tables between built-ins and handles JSON natively [S319].

**Cost of the alternatives.** Object pipelines bind to one runtime and type system (.NET for PowerShell). Once rendered to text, the objects are gone, so legacy commands take part only as text streams [S317]. Nushell gets structure largely by reimplementing tools as built-ins [S319]. The pro-text position, rooted in the original style rules [S316], values being language-agnostic, human-inspectable, and debuggable with ordinary tools (this characterisation is ours, not a quotation). The pro-structure position [S317] values precision and robustness. Both are held by experienced practitioners.

**Relevance to our OS.** medium — this choice shapes the native shell and tool ABI, and a typed IPC layer could carry schemas, but the hosted C toolchain will expect byte streams.

**Sources.** [S316], [S317], [S318], [S319]

### DM-021: Userspace message bus retrofitted (D-Bus; kdbus/bus1 rejected)

- **Category:** Inter-process communication
- **Systems affected:** Linux desktop and systemd stack (D-Bus); contrast Android (binder), macOS (Mach/XPC) [unverified for XPC]
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Linux has no kernel service-bus primitive. D-Bus is a user-space daemon that accepts connections over Unix sockets and routes method calls and signals between them. It adds well-known names, activation, a policy layer, fd passing (`UNIX_FD`), and peer-credential queries [S328].

**Why it was chosen.** Kernel developers prefer mechanism in the kernel and policy in user space. Unix sockets plus `SCM_RIGHTS` and `SO_PEERCRED` already provided the transport. A daemon can ship and evolve without kernel changes.

**Why it is a problem.** Each call crosses the daemon: two hops, extra copies, extra context switches. Proponents said reliable sender metadata cannot be obtained race-free from user space. The kdbus pull request (quoted in [S322]) claimed to fix "almost unfixable races" in D-Bus implementations. The credentials API kept growing (e.g. a ProcessFD field), a sign of retrofitting [S328].

**Why it was not fixed.** kdbus (2015) was never merged. Lutomirski NACKed it. His arguments: memfd over `SCM_RIGHTS` already handles bulk data; metadata could be a small Unix-socket extension; the metadata design broke CRIU; and pool buffering escaped memory-cgroup accounting [S320], [S321], [S324]. Biederman said the metadata-transfer code was wrong (quoted in [S322]). Torvalds was inclined to trust his maintainer but was "not AT ALL impressed" by the performance argument, which he thought pointed at user-space fixes [S324]. The merge was delayed for another cycle [S323] and never happened; LWN later called it "the sad story of kdbus" [S326]. bus1 (2016) offered capability-like handles, per-user quotas, global (causal) message ordering and multicast [S325]. It stalled, and resurfaced in 2026 stripped down to the basics and reimplemented in Rust [S326]. Meanwhile, the bus1 project produced dbus-broker, a user-space D-Bus broker aimed at high performance and reliability [S327].

**Alternatives that exist.** Android binder is in-tree, kernel-mediated IPC with object references, and it was accepted [S329], [S325]. Capability-based IPC with kernel-routed handles (seL4 endpoints, Zircon channels).

**Cost of the alternatives.** A kernel bus puts policy, quotas, ordering and accounting in the kernel, which is exactly where the kdbus review found problems [S321], [S324]. In LWN reader comments, a bus1 developer argues binder does not account resource usage and has no ordered multicast, and another commenter answers that binder is proven on billions of devices [S326] (comments, weak evidence; S325 confirms binder lacks multicast). The disagreement is about where a bus belongs, not only about implementation quality.

**Relevance to our OS.** high — we must decide early whether service discovery, naming and broadcast are kernel objects, a user-space broker, or capability-passing conventions.

**Sources.** [S320], [S321], [S322], [S323], [S324], [S325], [S326], [S327], [S328], [S329]

### DM-022: Descriptor passing over Unix sockets (`SCM_RIGHTS`): API shape and in-flight GC bugs

- **Category:** Inter-process communication
- **Systems affected:** Linux, BSDs, macOS, Unix/POSIX
- **Classification:** mistake
- **Confidence:** high

**What the design is.** A process sends open file descriptors as ancillary data (`cmsg`) on an `AF_UNIX` socket. The kernel keeps a reference while the message is queued, and the receiver gets new descriptor numbers [S334].

**Why it was chosen.** It retrofitted capability transfer (BSD-era [unverified for exact release]) onto the existing socket API without a new syscall family. That made privilege separation possible: a privileged process opens a resource and passes it to a sandboxed one.

**Why it is a problem.** The API is awkward and full of edge cases:
- On Linux stream sockets the fds must ride along with at least one byte of real data.
- There is at most one `SCM_RIGHTS` item per message, capped at 253 fds (`SCM_MAX_FD`; 255 before 2.6.38).
- A too-small control buffer silently truncates the fd list, signalled only by `MSG_CTRUNC`.
- Fds over the receiver's `RLIMIT_NOFILE` are automatically closed.
- Until Linux 4.5 the number of in-flight fds was unlimited [S334].

The deeper flaw is semantic. A socket can be sent over itself or over a peer, which creates reference cycles held only by in-flight messages, so Linux needs a garbage collector [S330]. That collector is a recurring source of races. CVE-2021-0920 is a use-after-free from a `MSG_PEEK` vs GC race. It was found and discussed on a public thread in 2016, but the suggested patch was not applied until it was re-reported in 2021; it was exploited in the wild from at least November 2020 to root Samsung devices, and CISA lists it as known-exploited [S330], [S331], [S333]. CVE-2024-26923 is a GC race against `connect()` [S332]. In 2024 the whole algorithm was replaced: each inflight AF_UNIX socket became a vertex in a directed graph, and the collector now finds dead strongly connected components [S335], [S336].

**Why it was not fixed.** Cycles are allowed by the API, so the kernel cannot remove the GC without breaking semantics. The ancillary-data format is POSIX and used by D-Bus, systemd, Wayland and others.

**Alternatives that exist.** Zircon channels: a message is bytes plus handles, transferred atomically. Handles are moved (discarded from the sender on success or failure) and need `ZX_RIGHT_TRANSFER`. A channel handle cannot be written into its own channel. Channels cannot be duplicated, and when the last handle to an endpoint closes, queued messages and the handles inside them are destroyed, possibly recursively [S337], [S338]. seL4 transfers capabilities in IPC.

**Cost of the alternatives.** Move semantics make sharing explicit (duplicate first), and rights checks add API surface [S337]. Whether move-only handles fully remove cycles (e.g. two channels each queued inside the other) is not addressed in the Zircon docs [S338] [unverified]. Any handle-in-message design must still decide this.

**Relevance to our OS.** high — handle transfer in IPC is central to a capability-leaning design, and cycle and lifetime rules must be designed in, not patched later.

**Sources.** [S330], [S331], [S332], [S333], [S334], [S335], [S336], [S337], [S338]

## 4. I/O, asynchrony and event handling

### DM-023: `select` → `poll` → `epoll` evolution; epoll's fd-vs-file-description semantics

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX (`select`, `poll`), Linux (`epoll`); contrast FreeBSD/macOS (`kqueue`), Solaris (`/dev/poll`, event ports)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `select` and `poll` are stateless: each call passes the whole interest set, the kernel scans it, and the result says which descriptors are ready. Linux `epoll` makes the interest set a kernel object. You add entries once with `epoll_ctl` and get ready events with `epoll_wait`. The key of an epoll registration is the pair of fd number and *open file description*, not the fd alone [S403].

**Why it was chosen.** `select` dates from early-1980s BSD, when a process had a few dozen descriptors and scanning a bitmap cost almost nothing. The stateless design adds no kernel state and is easy to reason about. When servers began holding thousands of idle connections, Banga, Mogul and Druschel measured `select` at nearly 25% of CPU in a proxy, even after optimising it. They proposed an explicit stateful interest-registration API [S400]. epoll (Linux 2.5.44, 2002) took that direction [S403]. Tying registrations to the file object is cheap to implement, because the kernel hooks the wait queue of the underlying file.

**Why it is a problem.** The `select`/`poll` design was reasonable for its time, but its cost grows with the number of watched descriptors, not with the number of events [S400][S401]. epoll's semantics have specific failure modes:
- Closing an fd does not remove the registration while another descriptor shares the description, whether from `dup`, `fork` or `SCM_RIGHTS`. The program then gets events for an fd number it has already closed, or that has been reused. After the close, `EPOLL_CTL_DEL` cannot remove the stale entry, and the only fix is to rebuild the epoll set [S403][S405].
- Libraries cannot fully defend against this, because their callers may close fds directly (Majkowski's blog [S405], weak; S403 makes the same point about fds duplicated behind the scenes by library functions).
- Epoll was not originally designed for balancing load across threads, as Majkowski argues [S404] (weak). Level-triggered mode wakes every waiting thread (thundering herd). Edge-triggered mode needs a drain-to-`EAGAIN` discipline, which can starve other descriptors [S403]. Later fixes were bolted on: `EPOLLONESHOT`, and `EPOLLEXCLUSIVE` in Linux 4.5 [S403][S404].

**Why it was not fixed.** Linux's no-regressions rule freezes the semantics, so every fix arrived as an opt-in flag. The man page now just documents the hazards [S403].

**Alternatives that exist.** kqueue (FreeBSD 4.1, 2000) applies registration changes and waits in a single call. It deletes knotes when the descriptor is closed, and it covers many event types besides fd readiness [S401][S402]. Lemon criticised the alternatives of the time. RT-signal queues overflow and deliver stale events for closed or reused fds. `/dev/poll` keeps returning `POLLNVAL` for closed fds and cannot itself be polled [S401]. Completion-based designs avoid readiness entirely; see DM-026.

**Cost of the alternatives.** Keying registrations to the descriptor means the close path must find and tear down registrations, which kqueue does through a per-descriptor knote array [S401]; that is extra bookkeeping on `close`. Stateful interest sets of any kind bring kernel memory, accounting and lifetime questions that `poll` never had.

**Relevance to our OS.** high. If our handles are capabilities that can be duplicated or transferred, we must decide explicitly whether a wait registration belongs to the handle or to the object. That is the exact choice epoll got wrong.

**Sources.** [S400], [S401], [S402], [S403], [S404], [S405]

### DM-024: POSIX AIO and Linux native AIO limitations

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** POSIX (`aio_*`), glibc, Linux native AIO (`io_setup`/`io_submit`/`io_getevents`); contrast Windows overlapped I/O
- **Classification:** mistake
- **Confidence:** high

**What the design is.** POSIX defines `aio_read`/`aio_write` with signal or thread notification. glibc implements them in user space with a pool of threads [S408]. Linux added a separate kernel AIO interface for asynchronous submission. glibc never wrapped it, and programs reach it through `libaio`, whose wrappers have different prototypes [S409].

**Why it was chosen.** Kernel AIO was built for databases doing `O_DIRECT` I/O to raw block devices. For that narrow case, an asynchronous path straight to the block layer was what users needed. A user-space POSIX AIO built on threads is portable, and it needs no kernel changes.

**Why it is a problem.**
- Native AIO is asynchronous only for `O_DIRECT`. Buffered I/O runs synchronously, so most applications cannot use it [S410][S406].
- Even `O_DIRECT` submissions block when metadata must be read or the device's request slots are full. Applications therefore offload submission to threads anyway [S410].
- It supports few operations beyond read and write, and asynchronous `fsync` did not work at all in 2016 kernels (no filesystem implemented the method) [S406].
- Each I/O copies about 104 bytes of descriptors and needs at least two syscalls [S410].
- glibc's threaded POSIX AIO "scales poorly" [S408].

The result was a niche interface with "long term undiscovered bugs" [S410]. Applications and libraries kept private I/O offload thread pools instead [S410] (see DM-029). Torvalds called AIO "a horrible ad-hoc design" and refused to extend it one syscall at a time [S407].

**Why it was not fixed.** Extending AIO was tried several times: Axboe in 2010, then LaHaise's 2016 thread-based patches. Generic async-syscall proposals (fibrils, syslets, threadlets, acall) were never merged [S406][S410]. Axboe found that retrofitting AIO made the API "even more complicated" and fixed only one deficiency, so he started over with io_uring [S410]. The old ABI remains for compatibility.

**Alternatives that exist.** Windows overlapped I/O treats files, sockets, named pipes and mail slots alike: any handle that supports overlapped I/O can complete through the same mechanism, IOCP [S417] (that NT had it from its first release is not stated in the cited source). Linux io_uring (5.1, 2019) supports buffered I/O and arbitrary operations through shared rings (DM-025).

**Cost of the alternatives.** Making every operation asynchronous means every kernel path must handle cancellation and continuation. Linux's history suggests this is either slow (punting to kernel threads) or invasive. Torvalds, quoted in Axboe's slides, predicted io_uring would have "subtle and nasty bugs", including security bugs from racing with fd close [S411]. io_uring's security record (DM-025) bears that out.

**Relevance to our OS.** high. Native async I/O has to be part of the design from day one and cover all operations. Bolting it onto a synchronous VFS produces exactly this outcome.

**Sources.** [S406], [S407], [S408], [S409], [S410], [S411], [S417]

### DM-025: `io_uring`: performance win vs security attack surface

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Linux (5.1+); downstream: ChromeOS, Android, Docker/containerd, GKE
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** io_uring is a pair of submission and completion rings shared through `mmap` between the application and the kernel. It supports pre-registered files and buffers, kernel-side polling of the submission queue, and an extensible opcode field designed to carry many request types [S410]; the opcode set has since grown into something like a second syscall table (our characterisation). It was first posted on 8 January 2019 and merged on 8 March 2019 for 5.1-rc1 [S411].

**Why it was chosen.** Native AIO could not be fixed (DM-024). Fast NVMe devices made syscall overhead and per-I/O copies dominant, and Spectre/Meltdown mitigations made syscalls more expensive. Shared rings batch submissions and can avoid syscalls entirely [S410].

**Why it is a problem.** io_uring runs operations on behalf of the process in kernel worker contexts. That path bypasses syscall-level policy and touches large amounts of kernel code. In Google's kCTF VRP, 60% of exploit submissions targeted io_uring. Google disabled it on ChromeOS and its production servers, blocked it for Android apps with seccomp, and considered disabling it by default on GKE Autopilot [S220][S413]. seccomp has no visibility into operations requested through io_uring, so letting a seccomp-sandboxed program use io_uring "almost certainly" lets it bypass the sandbox [S416]. Before 2023 the only system-wide off switch was compiling io_uring out of the kernel. The patch adding a sysctl cited many unprivileged-exploitable bugs [S414]. The resulting `kernel.io_uring_disabled`/`io_uring_group` knobs [S415] are coarse.

**Why it was not fixed.** The fix is in progress, not refused. Per-ring restrictions (`IORING_REGISTER_RESTRICTIONS`) exist, but they cannot inspect arguments. Task-level restrictions and cBPF filters were still being reworked in 2026 [S416]. The deeper issue is structural: a second, asynchronous path to most of the kernel must replicate every check that the synchronous path enforces.

**Alternatives that exist.** Windows IOCP limits completion delivery to a fixed set of I/O operations, not arbitrary syscalls [S417]. A capability system could make the ring itself a capability that names exactly which objects and operations it may touch. Per-ring restrictions are a weak form of this [S416].

**Cost of the alternatives.** Disabling io_uring gives up large gains in IOPS and syscall reduction [S410]. A restricted op set means the "everything async" goal (DM-024) is only partly met. Filtering ring entries is hard to use: Axboe said the switch to classic BPF filters comes with "a bit of pain on the usability front" [S416].

**Relevance to our OS.** high. If our native ABI is ring-based, policy enforcement (capabilities or a seccomp equivalent) has to apply to each ring entry, not to the ring-entry syscall. That needs to be designed in, not added later.

**Sources.** [S410], [S411], [S220], [S413], [S414], [S415], [S416], [S417]

### DM-026: Readiness-based vs completion-based I/O models

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix family (`select`/`poll`/`epoll`/`kqueue`: readiness), Windows NT (overlapped I/O and IOCP: completion), Linux io_uring (completion); runtimes such as libuv, Rust async
- **Classification:** contested
- **Confidence:** medium

**What the design is.** A readiness API tells you that an operation would not block now, and you then perform it with an ordinary call. A completion API takes the operation and buffer up front and later reports that it has finished [S417][S410].

**Why it was chosen.** Readiness grew out of Unix's synchronous-call model. It needs no change to `read`/`write`, and the kernel never holds user buffers. NT built asynchronous I/O into its I/O manager from the start, with IOCP adding a scheduler-aware thread pool. IOCP releases waiting threads in LIFO order and caps the number running at a set concurrency value [S417].

**Why it is a problem.** The two camps disagree about which model is the problem.
- *Pro-completion position.* Readiness cannot express disk I/O (DM-029). It needs at least two syscalls per operation, and portable runtimes end up with one design per OS: libuv runs epoll, kqueue, event ports and IOCP behind one interface [S418]. Completion covers files and sockets uniformly [S417][S410].
- *Pro-readiness position.* Classic completion commits a buffer to every pending read, which costs memory with many idle connections (the problem io_uring's provided buffers were built to address [S420]). It also transfers buffer ownership to the kernel. That makes cancellation hard: dropping a request does not stop the kernel writing into the buffer, and in Rust this clashes with borrowed-buffer APIs like `AsyncRead` [S419]. Software written for readiness can find IOCP impractical when it is designed to be cross-platform; wepoll exists to emulate epoll on Windows for that reason [S421] (README, weak).

**Why it was not fixed.** Each ecosystem's ABI froze its model, so portable runtimes absorb the mismatch [S418][S421].

**Alternatives that exist.** io_uring narrows the gap from the completion side. Provided buffer rings let the kernel pick a buffer only when data arrives, so more reads can be in flight than there are buffers [S420]. io_uring also offers poll-style readiness operations [S410]. kqueue is readiness-based but more general [S402].

**Cost of the alternatives.** Completion models need explicit cancellation semantics and kernel-owned or pinned buffers [S419], and their per-operation kernel paths add attack surface (DM-025). Readiness models leave file I/O to thread pools [S418].

**Relevance to our OS.** high. This is a foundational ABI choice. A capability-style kernel with message passing leans toward completion, while the POSIX compatibility layer needs readiness semantics, so whichever model is native must be able to emulate the other.

**Sources.** [S402], [S410], [S417], [S418], [S419], [S420], [S421]

### DM-027: `EINTR` and restartable system calls ("PC loser-ing")

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; language runtimes on top (Go, Python, Rust std)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** When a signal arrives during a blocking "slow" syscall, Unix runs the handler and then either restarts the call (`SA_RESTART`) or fails it with `EINTR`, leaving the caller to retry. Which calls restart depends on the interface. `poll`, `epoll_wait`, `select`, `nanosleep`, System V IPC and sockets with timeouts never restart, whatever `SA_RESTART` says [S116].

**Why it was chosen.** Gabriel's essay records this as the archetype of "worse is better." Unix did not back the PC out so the interrupted call could be transparently re-entered, as MIT's ITS did. It returned an error and let the user program retry, because "implementation simplicity was more important than interface simplicity" [S422]. On a small PDP-11 kernel, unwinding to the syscall boundary was the simplest way to deliver a signal promptly. It was a conscious tradeoff that the MIT side disputed at the time [S422], and the cost of the alternative (clean unwinding or re-entry on every blocking path [S427]) still applies today, so this is a tradeoff rather than a decision whose constraints have since disappeared.

**Why it is a problem.** The cost moves to every call site, forever.
- The restart rules form a per-call table that few programmers know [S116].
- Linux adds a non-POSIX case: some calls return `EINTR` after `SIGSTOP`/`SIGCONT` even with no handler installed [S116].
- Retrying requires accounting for partial transfers.
- A runtime change can expose latent bugs everywhere. Go 1.14's signal-based preemption meant that programs using the low-level `syscall` packages would "see more slow system calls fail with `EINTR`" [S424]. Writes to cgroupfs began failing even though Go installs handlers with `SA_RESTART` [S425], and the Go team judged the fix "much too invasive and risky" to backport [S426].

**Why it was not fixed.** The semantics are part of POSIX and of every application's error handling. Changing them would break programs that rely on `EINTR` to interrupt a blocking wait, the main legitimate use.

**Alternatives that exist.** With `SA_RESTART`, the kernel already restarts many calls transparently, which is close to the MIT approach, but only for some calls [S116]. Linux's internal restart-block mechanism, which resumes sleeps with the remaining time, goes further [unverified]. In Windows NT, a user-mode APC runs only when its thread enters an alertable wait (`SleepEx`, `WaitForSingleObjectEx` and similar), and the wait then returns with a status saying APCs ran [S2100]. Cancelling I/O is an explicit operation on the request: IRPs are cancelled through `IoCancelIrp` or when the issuing thread exits [S430]. Signal-free designs remove the problem entirely by delivering events as messages (DM-130).

**Cost of the alternatives.** Transparent restart needs every blocking kernel path to be safely re-entrant or to unwind cleanly. Linux kernel developers have noted that interruptible sleeps require cleanup code on every early-return path [S427]. Explicit cancellation needs cancel routines in every driver that may queue requests indefinitely [S430].

**Relevance to our OS.** high. If we have no asynchronous signal upcalls, `EINTR` disappears natively and lives only in the POSIX layer, but we still need a uniform cancellation mechanism for blocking operations.

**Sources.** [S422], [S116], [S424], [S425], [S426], [S427], [S430], [S2100]

### DM-028: Uninterruptible sleep (D state); hung NFS

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Linux (`TASK_UNINTERRUPTIBLE`), other Unix kernels with the same sleep model; NFS clients
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A kernel thread waiting in `TASK_UNINTERRUPTIBLE` ignores all signals, including `SIGKILL`, until an explicit wakeup [S427]. NFS "hard" mounts, the default, retry indefinitely, so an unreachable server leaves processes unkillable in "D" state [S428].

**Why it was chosen.** Many kernel paths, such as page-cache I/O, metadata locks and disk writes, cannot easily unwind halfway. Applications also assume file writes cannot be interrupted, as Alan Cox argued against making those sleeps interruptible [S427]. For local disks, the wait was assumed to be short and bounded.

**Why it is a problem.** The assumption fails for network filesystems, failing devices and buggy drivers. The process cannot be killed, the hung-task detector fires, and often only a reboot recovers. NFS's `intr` option was a partial escape. It has been ignored since 2.6.25, and the `soft` alternative risks silent data corruption [S428]. TASK_KILLABLE (2.6.25) lets fatal signals break the sleep, since a dying process never returns to user space to mishandle the error. Adoption was slow: at first only NFS used it, and no drivers did [S427]. Killable waits also spread hangs: a VFS path can take a mutex in uninterruptible sleep behind a killable NFS waiter, so innocent processes enter D state too [S429].

**Why it was not fixed.** Converting each sleep site requires a correct cleanup path. This is done one site at a time across thousands of call sites, and a sleep holding a lock can block others no matter how the lock holder sleeps [S427][S429].

**Alternatives that exist.** NT requires drivers that may queue IRPs indefinitely to support cancellation. The I/O manager cancels IRPs when the issuing thread exits, and after five minutes it disassociates IRPs whose cancellation has not completed [S430]. In microkernels with user-space file servers, a client blocked on IPC can in principle be torn down independently of the server [unverified for specific systems].

**Cost of the alternatives.** Every driver that may hold requests indefinitely needs cancellation logic [S430]; Microsoft now steers drivers to cancel-safe queues or KMDF to abstract it away, which suggests hand-written cancel routines were hard to get right [S430] (inference). Abandoning an in-flight request may leave a device or remote server in an unknown state. That is why Linux NFS prefers "hard" and warns about "soft" [S428].

**Relevance to our OS.** medium. With drivers and filesystems in user space, a process killer can reclaim a client without asking the server, but we must define what happens to its in-flight requests.

**Sources.** [S427], [S428], [S429], [S430]

### DM-029: Regular files always "ready"; no non-blocking disk I/O in the readiness model

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** POSIX, Linux, BSDs, macOS; event-loop runtimes (libuv/Node.js, Samba)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** POSIX requires regular files to "always select true" for reading, writing and error [S431]. `O_NONBLOCK` has no effect on buffered file reads. A read of uncached data blocks the thread until the page cache is filled [S432].

**Why it was chosen.** Disk access was treated as "fast": bounded and not dependent on a remote party, unlike terminals, pipes and sockets (the same assumption behind uninterruptible disk waits [S427]; the original rationale for `select` is not documented in the cited sources). "Ready" for a file has no clear meaning, because any byte may or may not be cached and readiness cannot be tracked per byte range. Making file reads always ready kept `select` simple.

**Why it is a problem.** A single-threaded event loop stalls whenever it touches an uncached file, a slow disk or a network filesystem. Every major runtime therefore keeps a thread pool for file I/O. libuv states plainly that no platform file I/O primitives were usable, so it uses a limited-size pool [S418]. Samba's developers wanted to try a non-blocking read first and fall back to threads only on a cache miss [S432], because thread offload adds communication overhead even when the data is already cached [S432]. Retrofits are hard:
- `O_NONBLOCK` could not be given meaning for files, because programs like Samba and squid set it and rely on it being ignored [S432].
- Checking cache residency first with a proposed `fincore()` is indirect and racy, since the cache can change between the check and the read [S432].
- The eventual `RWF_NOWAIT` (Linux 4.14) is meaningful only for `preadv2` reads, and Linux 5.9 and 5.10 had a bug where it could return 0 before EOF [S433].

**Why it was not fixed.** The POSIX text and decades of applications fix the semantics [S431][S432]. Linux took the route of new syscalls and flags, and then of io_uring.

**Alternatives that exist.** Windows overlapped I/O treats files and sockets alike [S417]. io_uring does buffered file I/O asynchronously [S410]. FreeBSD kqueue's vnode read filter reports the distance to EOF instead of firing unconditionally as `poll` does [S402], although that does not make the reads themselves non-blocking.

**Cost of the alternatives.** Completion-based file I/O needs the whole storage stack to be asynchronous or to punt to kernel workers (DM-024, DM-025). A readiness signal for files is ill-defined, because readiness depends on the offset and on cache state.

**Relevance to our OS.** high. Our filesystem will be a service from the start, so file and network I/O can share one asynchronous model. The POSIX layer then only has to emulate "always ready".

**Sources.** [S402], [S410], [S417], [S418], [S427], [S431], [S432], [S433]

### DM-130: Waitable events outside the descriptor model (signals, timers, process exit) and their fd retrofits

- **Category:** I/O, asynchrony and event handling
- **Systems affected:** Unix/POSIX, Linux (`signalfd`, `timerfd`, `eventfd`, `pidfd`), BSDs/macOS (kqueue filters); contrast Windows NT (waitable objects), Fuchsia/Zircon (ports)
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** In classic Unix, only descriptors can be passed to `select`/`poll`. Process exit is reported by `SIGCHLD` and `wait`, timers by `SIGALRM`, and signals by asynchronous handlers. An event loop that needs all of these must convert them into fd events, either with the self-pipe trick or with later Linux fds: `signalfd`, `timerfd`, `eventfd`, and `pidfd` in 5.3 [S110][S302]. Lemon's kqueue paper names exactly this gap: signals, file system changes and process exit cannot be collected with `select`/`poll` [S401].

**Why it was chosen.** Signals predate multiplexed I/O, and `select` was added in BSD for terminals and sockets [unverified: history not sourced here]. Each kind of event got its own mechanism when it was needed, and no single "waitable object" concept existed to unify them.

**Why it is a problem.** This entry covers the lack of one waitable model; the signal-semantics problems themselves are DM-017 and the PID races DM-003.
- Lemon notes that using signals as a notification mechanism precludes multiple event handlers, making it "unsuitable for use in library code" [S401].
- Each retrofit carries over the old mechanism's process-global semantics. `signalfd` requires blocking the signal with `sigprocmask`, and the mask and pending signals survive `execve` (detail in DM-017) [S302]. `signalfd` also cannot receive synchronously generated signals such as SIGSEGV [S302].
- `pidfd_open` on a child is race-free only if `SIGCHLD` is not ignored and nothing else reaps the child; otherwise the child must be created with `CLONE_PIDFD` [S110] (see DM-003).

**Why it was not fixed.** Each retrofit was added in its own Linux release while the original mechanisms stayed for POSIX compatibility. kqueue (FreeBSD 4.1, 2000) brought process, signal and vnode events into the same queue as descriptor events [S401][S402]; its timer and user filters exist in today's man page [S402]. Linux took a different direction.

**Alternatives that exist.**
- NT made processes, threads, events, mutexes, semaphores, waitable timers and change notifications waitable handles with one wait call [S463].
- kqueue has filters for processes, signals, timers, vnodes and user events in one queue [S402].
- Zircon's `zx_object_wait_async` delivers signal changes on any waitable object as packets to a port, which also carries asynchronous messages from IPC transports [S466][S467].

**Cost of the alternatives.**
- `WaitForMultipleObjects` is limited to `MAXIMUM_WAIT_OBJECTS` handles. Above that, the docs recommend extra waiting threads or the thread pool [S463].
- Zircon's async waits fire once per registration, and edge mode can permanently miss a transition that happens before the call completes [S467]. Both systems still need a second API (IOCP, or Zircon channel reads) for data I/O.
- A uniform waitable model still has to define what "signalled" means for each object type.

**Relevance to our OS.** high. If every kernel object exposes signal state that can be waited on through one port or queue, this whole class of retrofits disappears. The POSIX layer can then emulate signals on top of it rather than the reverse.

**Sources.** [S401], [S402], [S463], [S110], [S302], [S466], [S467]

## 5. Memory management

### DM-030: Memory overcommit and the OOM killer

- **Category:** Memory management
- **Systems affected:** Linux (default heuristic overcommit), other Unix-likes to varying degrees; Android (userspace killers); contrast Windows NT (strict commit accounting)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Linux by default lets processes reserve more virtual memory than RAM plus swap can back. Only "obviously" excessive requests are refused (mode 0). Mode 1 always overcommits, and mode 2 enforces a commit limit of swap plus a share of RAM [S500]. When memory actually runs out at page-fault time, there is no allocation call left to fail, so the kernel's OOM killer picks a victim process by a heuristic "badness" score and kills it [S501].

**Why it was chosen.** Much allocated memory is never touched: sparse arrays, large reserved-but-unused heaps, and copy-on-write pages after `fork()`. Overcommitting uses RAM more efficiently and needs less swap [S500][S501]. Fork-heavy Unix workloads make this pressure worse. Baumann et al. argue that fork encourages overcommit: a conservative implementation must account for a worst case of double the process's virtual size, and failing fork for that reason is undesirable, so Linux overcommits by default [S100].

**Why it is a problem.** It moves allocation failure from a call site, where the program can handle it, to an arbitrary later memory access, where it cannot. Victim selection is a heuristic based mainly on memory size, adjusted for run time, niceness and privilege, and the kernel grew years of knobs and proposals to steer it (oom_adj, a cgroup OOM controller, name-based policies), several of which drew objections [S501]. LWN's 2009 review concluded that killing by kernel heuristics is not optimal [S501]. Forking servers are prime victims because half of each non-sharing child's memory is added to the parent's score [S501]. Even `MAP_NORESERVE` and similar promises carry no guarantee: the mmap man page's BUGS section says any process can be killed when memory runs out [S509]. Android moved the kill decision from an in-kernel low-memory killer to the userspace lmkd daemon driven by PSI pressure monitors [S506], and PSI was designed so userspace can see pressure before latency spikes and OOM kills [S504].

**Why it was not fixed.** Strict mode 2 exists [S500], but it is not the default. Software written against overcommit (huge virtual reservations, fork of large processes) would fail under strict mode. The fix that actually happened was incremental: better heuristics, then userspace killers using PSI [S504][S506].

**Alternatives that exist.** Windows NT charges every committed page against a system commit limit equal to RAM plus page files. When the charge reaches the limit, allocations fail, and system-managed page files can grow [S502]. Linux mode 2 does the same [S500].

**Cost of the alternatives.** Strict commit needs backing store for memory that may never be used. Microsoft warns that hitting the commit limit causes freezes, crashes and malfunctions, so commit exhaustion is still a failure mode, just a different one [S502]. Programs must handle allocation failure everywhere, and many do not.

**Relevance to our OS.** high — the commit policy determines whether "allocation failed" is a value returned to the caller or a kill delivered asynchronously, and that shapes every API above it, including the POSIX layer's `fork`.

**Sources.** [S100], [S500], [S501], [S502], [S504], [S506], [S509]

### DM-031: `mmap`'d files: errors surface as `SIGBUS`; truncation hazards

- **Category:** Memory management
- **Systems affected:** POSIX/Unix generally, Linux; Wayland compositors (shared-memory buffers); databases using mmap
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** `mmap` makes file contents appear as ordinary memory. I/O happens through page faults, so there is no call that can return an error. Touching a page past end-of-file raises SIGBUS. The effect of the file changing size under a mapping is unspecified [S509].

**Why it was chosen.** It is a uniform, zero-copy way to read files and share memory. The page cache is reused directly, with no system call per access and no duplicate buffer in user space [S510].

**Why it is a problem.** Errors that `read` would return become signals that can arrive at any load or store:
- **I/O errors.** Any code that touches mapped memory can receive SIGBUS, and a DBMS has to handle it with cumbersome signal handlers. Pages corrupted in memory (for example by pointer bugs) are written back silently, with no chance to check them first [S510].
- **Hidden stalls.** Any access may block on I/O, and there is no asynchronous read path [S510].
- **Scalability.** Page eviction and TLB shootdowns limit scaling on fast storage [S510].
- **Abandonment.** MongoDB (MMAPv1) and InfluxDB moved off mmap after significant engineering cost [S510]. (MongoDB later reintroduced mmap in a limited role [S510]; the paper is a position paper by buffer-pool advocates.)
- **Hostile peers.** A process sharing a file can truncate it and crash the other side with SIGBUS, or change the data after it has been validated [S315][S512]. The 2014 sealing patchset listed the compositor-receives-client-buffer case as its first use case [S513].

**Why it was not fixed.** Sealing (`memfd_create` plus `F_SEAL_SHRINK`, Linux 3.17) gives a fix that both sides must opt into [S315]. Seven years later no Wayland compositor enforced it, because old clients and portable clients use `shm_open`. Compositors kept relying on SIGBUS handlers instead [S513]. Simon Ser's 2021 proposal for an OpenBSD-style `__MAP_NOFAULT` (out-of-bounds reads return zeros) shows the problem was still open [S513].

**Alternatives that exist.**
- File sealing, so the receiver can verify that the buffer cannot shrink [S315][S512].
- No-fault mappings, as in OpenBSD [S513].
- Explicit buffer-pool I/O (`pread`, O_DIRECT, io_uring), where errors come back as return values [S510].

**Cost of the alternatives.** Explicit I/O needs a user-space cache and copies, which is more code. Sealing needs cooperation from every client [S513]. Zero-fill-on-fault hides truncation instead of reporting it.

**Relevance to our OS.** high — the compositor will receive client buffers from untrusted processes, so the shared-memory object type must make "cannot shrink or change under you" checkable, and must not require signal handlers.

**Sources.** [S509], [S510], [S315], [S512], [S513]

### DM-032: Executable stacks/heaps by default; W^X retrofitted

- **Category:** Memory management
- **Systems affected:** Unix/Linux, Windows (pre-DEP), OpenBSD, macOS
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Historically, any readable page of a process could be executed, including the stack, the heap and data. W^X ("write xor execute") is the policy that a page is writable or executable but not both. It was added to existing systems in the early 2000s.

**Why it was chosen.** Much hardware had no per-page execute permission. OpenBSD 3.3 (2003) could enforce W^X only on sparc, sparc64, alpha and hppa, because those MMUs had a real execute bit. i386 and powerpc came later [S514]. Trampolines, thunks and JITs placed code in data pages, and nothing in the threat model of the time forbade it. The constraint was soft rather than absolute: PaX emulated non-executable pages on x86 from 2000, before OpenBSD shipped W^X [S517] (weak source).

**Why it is a problem.** Executable data pages are what makes classic buffer-overflow code injection work. DEP and W^X exist to stop it [S516][S518]. Retrofitting shows how much of the cost is compatibility:
- **Windows DEP.** It broke JITs that did not mark their code executable, and ATL 7.1-era thunks [S516].
- **Linux.** In 2020, x86 binaries without a `PT_GNU_STACK` marking still got every readable mapping made executable [S519].
- **glibc 2.41 (2025).** `dlopen` stopped making the stack executable on behalf of libraries [S520]. That broke Steam games, Discord, Julia and MATLAB, and forced a tunable that turns executable stacks back on [S521].
- **OpenBSD.** Strict enforcement came only in 6.0 (2016), and still needs a `wxallowed` escape hatch because ports violate it [S515].

W^X alone does not stop return-to-existing-code attacks [S518]. Plain W^X also allows a mapping to switch from W to X later, unlike PaX MPROTECT or iOS [S517] (weak source).

**Why it was not fixed.** Earlier, hardware support was missing [S514]. Later, binaries and toolchain defaults made the stack executable through missing markings [S519][S520][S521].

**Alternatives that exist.**
- OpenBSD mandatory W^X; violating binaries must be marked `PT_OPENBSD_WXNEEDED` and live on a `wxallowed` file system [S515].
- Windows DEP with explicit `PAGE_EXECUTE*` allocation and a write-then-protect pattern [S516].
- Apple silicon enforces W^X for all apps. JITs use a `MAP_JIT` region with per-thread write/execute toggling; under the Hardened Runtime this needs an entitlement and is limited to one region [S522].

**Cost of the alternatives.** JITs need explicit permission flips, entitlements and instruction-cache maintenance [S522]. Stack trampolines and unmarked assembly still need an executable stack [S519]. Every enforcement step has broken shipped software [S516][S521].

**Relevance to our OS.** high — on aarch64 with no legacy binaries, W^X can be an invariant from day one. The open question is how the POSIX layer and a hosted C compiler or JIT get executable memory: as a capability, or through a W-to-X transition API.

**Sources.** [S514], [S515], [S516], [S517], [S518], [S519], [S520], [S521], [S522]

### DM-033: Kernel mapped into every user address space; Meltdown/KPTI

- **Category:** Memory management
- **Systems affected:** Linux, Windows, macOS, BSDs on affected x86 and some Arm cores
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Mainstream kernels mapped their own memory into the top of every process's address space and protected it only with supervisor-only page permissions. A system call or interrupt then needs no page-table switch.

**Why it was chosen.** Performance. Keeping the kernel mapped avoids flushing the TLB on every entry to and exit from the kernel [S526]. Page-permission checks were assumed to be enforced architecturally, including during speculation (inferred; the Meltdown paper describes memory isolation as the foundation that this broke [S523]).

**Why it is a problem.** Meltdown showed that out-of-order execution on affected CPUs reads kernel memory from user space before the permission fault takes effect. It needs no software bug and breaks address-space isolation, including in cloud settings [S523]. The fix was KAISER, later KPTI: two page-table sets, with only an entry stub mapped while user code runs [S525]. KAISER had been proposed months earlier against KASLR side channels [S524], and the Meltdown authors said it must be deployed immediately [S523].
- **Per-entry cost.** A CR3 write of about a hundred cycles on every kernel entry and exit, plus TLB flushes without PCID [S525].
- **Workload cost.** Typical regressions were single-digit percentages (about 5%), with about 30% on a loopback networking test as the worst case reported [S526].
- **Memory.** Extra memory per process [S525].
- **arm64.** KPTI was also expensive there and incompatible with the Statistical Profiling Extension (SPE) [S527].

**Why it was not fixed.** It was fixed, but as an emergency retrofit [S523][S525]. PTI can be disabled at boot (`nopti`) [S525]; that it is enabled by default only on affected CPUs is [unverified] from the cited sources. The underlying hazard (shared mappings plus speculation) is a property of hardware, so the right choice depends on the CPU.

**Alternatives that exist.**
- Separate kernel address space (KPTI-style) [S525].
- ARMv8.5 E0PD, which makes EL0 accesses to the kernel half fault in constant time without unmapping. It is enabled only if every CPU supports it, and the initial patch states the aim of not using KPTI on such CPUs, leaving that integration to follow-up patches [S527].
- PCID or ASIDs to reduce flush cost [S526].

**Cost of the alternatives.** Full separation costs time on every syscall and interrupt [S525][S526]. E0PD needs ARMv8.5 or later on every CPU in the system [S527]. Whether E0PD fully covers speculative access is something I could not settle from primary sources [unverified].

**Relevance to our OS.** medium — aarch64's split TTBR0/TTBR1 design and E0PD make the shared mapping cheap to protect on new cores, but QEMU `virt` on Apple Silicon and future CPUs need a policy for when hardware isolation is not trustworthy.

**Sources.** [S523], [S524], [S525], [S526], [S527]

### DM-034: No first-class memory-pressure signalling to applications

- **Category:** Memory management
- **Systems affected:** Linux (until PSI, 2018), traditional Unix; contrast Windows, iOS/macOS, Android
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Classic Unix gives a process no standard way to learn that the system is short of memory. Pressure shows up as swapping, slowdowns, and finally an OOM kill. Linux added Pressure Stall Information (PSI) in 2018: per-system and per-cgroup stall-time metrics with pollable thresholds [S504].

**Why it was chosen.** On swap-backed time-sharing systems, memory pressure was meant to be invisible. The kernel paged things out, and applications were not expected to cooperate. Load average looked like enough of a signal (inferred from [S505]). There is no direct evidence that this was a deliberate, constraint-driven choice, and Windows has offered waitable memory notifications since XP [S507], so the entry treats it as a tradeoff (transparent paging versus application cooperation) rather than as right-at-the-time.

**Why it is a problem.** Without a signal, caches cannot shrink before the system is hurting, and killers act too late.
- **Linux metrics.** Load average does not measure time lost to waiting [S505]. Before PSI there was no accurate measure of lost productivity, so operators either under-used hardware or accepted disruptions [S504].
- **Android.** The in-kernel low-memory killer used fixed free-memory thresholds, thrashed on low-RAM devices without killing anything, slowed reclaim, and was customized by device partners. It was removed in kernel 4.12. Its replacement, userspace lmkd, first ran on vmpressure events full of false positives, and moved to PSI in Android 10 [S506].
- **iOS.** Even with signals, delivery is best-effort. When demand rises faster than apps respond, the system "jettisons" apps [S508].

**Why it was not fixed.** For decades there was no agreed metric. PSI came from Facebook's container fleet [S505]. It now has to be adopted by userspace managers, one at a time [S506].

**Alternatives that exist.**
- Windows has `CreateMemoryResourceNotification` since XP: waitable low and high memory objects [S507].
- iOS has app-delegate callbacks, notifications and a dispatch memory-pressure source, plus kernel-discardable `NSPurgeableData` [S508].
- Linux has PSI triggers that can be polled through file descriptors, including by unprivileged users with coarse windows [S504].

**Cost of the alternatives.** Notifications are advisory and system-wide [S507]. Responding wrongly, for example by walking the whole object graph, can make pressure worse by pulling pages out of compression [S508]. PSI still needs a policy daemon to act on it [S506].

**Relevance to our OS.** high — a compositor-based desktop with caching clients benefits from a pressure event channel designed with the memory model (for example, purgeable or discardable memory objects) instead of retrofitted.

**Sources.** [S504], [S505], [S506], [S507], [S508]

### DM-035: `brk`/`sbrk` legacy heap interface

- **Category:** Memory management
- **Systems affected:** Unix/POSIX historically, Linux/glibc (main arena), BSDs
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** `brk`/`sbrk` move a single "program break", the end of one contiguous data segment, to grow or shrink the heap. They date back at least to Seventh Edition Unix [S2200]; the Linux man page lists them under 4.3BSD and SUSv1, marked LEGACY in SUSv2 and removed in POSIX.1-2001 [S528].

**Why it was chosen.** With a simple linear address-space layout and no general `mmap` (inferred), one growable segment was the obvious and cheap heap primitive. The V7 manual shows the hardware model: the break was rounded to 64 bytes on the PDP-11 and could fail if too many segmentation registers would be needed [S2200].

**Why it is a problem.**
- **One shared break.** The break is a single per-process cursor, and memory can only be returned to the kernel from the top. The old BSD malloc could return memory "only in rare circumstances" [S529]. glibc trims only when the free space at the top of the heap exceeds a threshold, and sends large allocations to `mmap` instead [S530].
- **Predictability.** A contiguous heap is predictable, which helps exploitation. OpenBSD's mmap-based malloc gets randomization, out-of-band metadata and guard behaviour, and early speed tests showed about a 30% speedup over the old malloc, though less in the final version [S529].
- **Inconsistent API.** The argument type of `sbrk` varies across systems, and the man page advises avoiding both calls [S528].

**Why it was not fixed.** It does not need fixing. Modern allocators already avoid or minimise it [S529][S530]. It survives for binary compatibility, and OpenBSD notes it still reduces the address space available to applications on 32-bit systems [S529].

**Alternatives that exist.** Allocators built only on `mmap` (OpenBSD malloc [S529]), and hybrid allocators (glibc [S530]).

**Cost of the alternatives.** More system calls and page-granular waste for small heaps, and the kernel must zero fresh pages [S530].

**Relevance to our OS.** low — a new OS should simply not provide a break. The POSIX layer can emulate `sbrk` over an address-space-reservation primitive if a ported program needs it.

**Sources.** [S528], [S529], [S530], [S2200]

## 6. Filesystems, namespaces and storage semantics

### DM-036: POSIX advisory record locks are owned by the process and dropped on any close()

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** POSIX (specified behaviour), Linux, BSDs, macOS, any POSIX-conformant system
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `fcntl(F_SETLK/F_SETLKW)` byte-range locks belong to a (process, inode) pair, not to the file descriptor or open file description. POSIX says that when a process closes *any* descriptor for a file, all of that process's locks on the file go away, even locks taken through a different open file description [S603]. The locks are not inherited across `fork()`, and threads in one process never conflict with each other [S600].

**Why it was chosen.** When the interface was designed, processes were single-threaded and a process was the natural unit of ownership. Tying locks to the process made cleanup on exit simple (inferred; locks are released when the process terminates [S600]), and the interface gave byte-range granularity, which BSD `flock()` (whole-file only) does not provide [S601]. Nobody was thinking about libraries that open files behind the caller's back, or about threads (inferred). Layton points to Jeremy Allison's account that the close semantics were codified from an early implementation rather than designed [S601], which weakens the steelman.

**Why it is a problem.** Lock state is effectively global to the process, so code that does not know about the lock can destroy it. A library routine that opens, reads and closes the same file, without the caller knowing, silently drops the caller's lock; opening two hard links to one file and closing one triggers it too. Layton says this can lead to silent data corruption [S601][S600]. Because threads share ownership, the locks cannot coordinate threads; Layton's demo shows threads overwriting each other's appends [S601]. SQLite's source calls POSIX locks "broken by design". It has to keep a reference-counted `unixInodeInfo` per inode, do its own lock bookkeeping, and defer `close()` of descriptors (the `pUnused` list) until the last lock is released [S602]. Developers of Samba, NFS Ganesha, SQLite and OpenJDK expressed interest in OFD locks to simplify their code and avoid corruption when files are closed [S601].

**Why it was not fixed.** The semantics are in the standard and existing programs depend on them, so changing `F_SETLK` was not an option. Linux added a parallel lock type instead.

**Alternatives that exist.** Open file description (OFD) locks (Linux 3.15, originally "file-private" locks) are byte-range locks owned by the open file description. They are inherited across `fork()`/`dup()`, released only on the last close, and conflict between separate opens even within one process [S600][S601]. They are now standardized as `F_OFD_*` in POSIX.1-2024 [S603]. BSD `flock()` has the right ownership but only whole-file granularity [S601].

**Cost of the alternatives.** OFD locks do no deadlock detection [S600]. Conflicting locks from the two families conflict with each other even within one process and on the same descriptor, so mixed use in one program is subtle [S600][S601]. Retrofitting took a decade-old API plus a new set of commands, and it left two lock models in the standard.

**Relevance to our OS.** medium. Any lock primitive should be owned by a handle or capability with explicit lifetime. The POSIX layer still has to emulate the broken semantics if hosted C programs rely on them.

**Sources.** [S600], [S601], [S602], [S603]

### DM-037: fsync() error semantics: errors reported once, dirty data dropped ("fsyncgate")

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Linux (ext4, XFS, Btrfs), NetBSD, macOS, OpenBSD (behaviour since changed so later calls keep failing while the file stays open [S608]); POSIX (underspecified); PostgreSQL, SQLite, LevelDB, LMDB, Redis as victims
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Buffered writes land in the page cache and are written back asynchronously. If writeback fails, Linux file systems mark the affected pages clean (discarding or keeping stale contents) and report the error once to a later `fsync()` [S604][S606]. POSIX says nothing about the file's state after `fsync()` fails [S605].

**Why it was chosen.** Kernel developers' rationale: the most common write error is a removed USB stick. Keeping pages dirty indefinitely for a device that is gone would exhaust memory, so the data is dropped [S604]. Error state lived in the page cache's mapping and was historically cleared when reported. Before `errseq_t` (4.13/4.16) only the first caller after an error could see it [S607].

**Why it is a problem.** PostgreSQL retried `fsync()` after EIO on the assumption that the dirty data was still pending. On Linux the retry *succeeds*, because the error flag was cleared, and the checkpoint advances past writes that never reached disk. Ringer reported real corruption on XFS [S605]. PostgreSQL's checkpointer often opened files just before `fsync()`, so it never saw errors from writeback that happened before its `open()` [S604]. Rebello et al. found that ext4, XFS and Btrfs all mark pages clean after failure, while page contents and failure reporting vary between them, and that none of five widely used applications (PostgreSQL, LMDB, LevelDB, SQLite, Redis) handle fsync failure sufficiently, with data loss and corruption as outcomes [S606]. The meaning of "successful fsync" also differs by platform. On Darwin, `fsync()` does not flush the drive's cache, and a separate `F_FULLFSYNC` is needed [S609].

**Why it was not fixed.** Keeping pages dirty was rejected over memory exhaustion. A sticky per-inode error was considered unlikely because it would diverge further from POSIX [S604]. `errseq_t` improved reporting to "once per open file description" but is explicitly "not perfect" [S607]. Kernel developers told PostgreSQL to use direct I/O, which Andres Freund called "a metric ton of work" [S604]. PostgreSQL's response was to PANIC and recover from WAL; this landed in v12 and was backported to 9.4 [S608]. The classification as a mistake applies mainly to the reporting side (errors lost, retries falsely succeeding); dropping the data itself is a tradeoff against memory exhaustion that FreeBSD resolved the other way [S604][S608].

**Alternatives that exist.** FreeBSD keeps buffers dirty after a write error, so later `fsync()` calls keep failing (with a policy exception when the device disappears) [S608]. Applications can use direct I/O and manage durability themselves [S604]. Google had an out-of-tree netlink error-notification channel [S604].

**Cost of the alternatives.** Keeping dirty data can pin unbounded memory against a dead device [S604]. Direct I/O moves caching, readahead and alignment into every application [S604]. Crash-and-replay needs a WAL design.

**Relevance to our OS.** high. Durability and error-reporting semantics of the storage API are a core contract that has to be specified (who sees an error, whether data is retained, what a retry means) rather than inherited.

**Sources.** [S604], [S605], [S606], [S607], [S608], [S609]

### DM-038: No ordering or atomicity contract for file updates; crash consistency left to folklore (ext4 delalloc, 2009)

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** POSIX (unspecified crash semantics), Linux ext3/ext4/Btrfs/XFS/ReiserFS, applications (Git, Mercurial, LevelDB, SQLite, PostgreSQL, HDFS, ZooKeeper, desktop config stores)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** POSIX defines `write`, `rename`, `fsync`, etc. but leaves what survives an unclean shutdown undefined [S611]. Applications build "atomic" updates from idioms such as write-temp-then-rename, and correctness depends on unstated file-system behaviour ("persistence properties") [S612].

**Why it was chosen.** Leaving ordering unspecified lets file systems delay allocation, batch and reorder writes, which is a big performance win. ext4's delayed allocation could hold data for up to a minute versus about five seconds on ext3 [S610]. `fsync()` on every update was expensive; Ts'o cites the fsync performance problems of Firefox 3.0 [S611]. Ts'o's position was that apps needing durability must call `fsync()`/`fdatasync()` [S611].

**Why it is a problem.** After ext4 shipped, users found zero-length configuration files after crashes. Desktop apps used write-then-rename without `fsync`, and ext4 committed the rename before the data [S610]. The pattern had "worked" only because ext3's `data=ordered` mode, designed as a security measure against exposing stale blocks, flushed data before metadata as a side effect [S610]. Pillai et al. tested six Linux file systems and found persistence properties vary widely. For example, appends to one file are persisted before a later rename in ext3's ordered mode, but not in ext4's same mode unless a special option is enabled. They found 60 crash vulnerabilities in 11 mature applications, including a Git case where a crash after rename breaks all later commits [S612]. Correct code needs knowledge the API never states, such as fsyncing the parent directory after create or rename.

**Why it was not fixed.** The kernel changed behaviour rather than the contract. 2.6.30 added heuristics that force allocation on rename-over-existing and on truncate, with a mount option to turn them off [S610][S611]. The standard still leaves crash semantics open.

**Alternatives that exist.** Explicit atomic-replace primitives (Windows `ReplaceFile` is Microsoft's recommended replacement for TxF single-file use [S639]). Transactional system calls (TxOS) [S617]. Embedded transactional stores (Ts'o recommended SQLite-like stores) [S611]. Pillai et al. propose ext3-fast to keep ordering cheaply [S612].

**Cost of the alternatives.** Strong ordering costs throughput; that is why delayed allocation exists [S610]. Transactional APIs proved hard to adopt (see DM-045). A database for every config file adds a dependency and its own failure modes.

**Relevance to our OS.** high. A new FS API can state ordering and atomic-replace guarantees explicitly. Whether to pay their cost is a central filesystem design decision.

**Sources.** [S610], [S611], [S612], [S617], [S639]

### DM-039: Path-based TOCTOU and symlink races

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** POSIX/Unix (all), Linux (container runtimes: runc, Docker), BSDs, macOS; Windows (reparse points) [unverified for Windows specifics]
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Most file system calls take a pathname that the kernel resolves anew on every call, following symlinks and `..` through a mutable shared tree. A program that checks a path (`stat`, `access`) and then uses it (`open`) is operating on two independent lookups.

**Why it was chosen.** String paths are simple, human-readable and composable with the shell. Symlinks were added for flexibility across file systems. In a single-user or trusted multi-user setting with no adversarial co-tenants, races were a minor concern.

**Why it is a problem.** An attacker who can modify any directory along the path can swap a component between check and use. The kernel's own docs call symlink TOCTOU "a long-standing class of security issues" [S619]. In 2009 a search of the NVD for "symlink attack" returned over 600 hits [S617]. In CVE-2018-15664, `docker cp` was vulnerable to a symlink-exchange race giving root read-write access to the host [S615]. runc has had 14 path-safety CVEs since 2017, including 2025 bugs that combine mounts and symlinks to reach `/proc/sys/kernel/core_pattern` [S614]. Symlinks also make `..` ambiguous: `getwd` and `cd ..` disagree [S618]. Porter et al., who propose system transactions as that change, argue that the counter-measure arms race suggests TOCTOU can be eliminated "only by changing the API". They note Linux had to add close-on-exec flags to 15 syscalls to close one race [S617].

**Why it was not fixed.** Every path-taking syscall and every program depends on the current semantics. Linux kept adding narrow fixes: `O_NOFOLLOW`, `*at()` calls, `protected_symlinks` [S619], and finally `openat2()` with `RESOLVE_BENEATH`, `RESOLVE_IN_ROOT`, `RESOLVE_NO_SYMLINKS`, `RESOLVE_NO_MAGICLINKS` and `RESOLVE_NO_XDEV` (5.6) [S613]. Adoption is limited: seccomp policies often block `openat2` because it takes a pointer argument, and procfs magic links need extra handling [S614].

**Alternatives that exist.** Plan 9 has no symlinks and defines `..` lexically, with kernel-tracked names and `fd2path` [S618]. Fuchsia does not implement `..` on file servers, so a directory handle confines its holder to that subtree [S616]. Handle-relative resolution with explicit restrictions (`openat2`, libpathrs) [S613][S614]. Transactional system calls serialize the attacker's change [S617].

**Cost of the alternatives.** Removing `..` and symlinks breaks POSIX expectations. Fuchsia's client-side canonicalization works only because it has no symlinks [S616]. `RESOLVE_*` flags break existing layouts if applied indiscriminately, and resolution can fail with `EAGAIN` under races [S613]. Manual component-by-component traversal is "quite finicky" [S614].

**Relevance to our OS.** high. Whether names are resolved relative to handles (capability-style) or against a global mutable tree largely decides whether this bug class exists at all.

**Sources.** [S613], [S614], [S615], [S616], [S617], [S618], [S619]

### DM-040: Hard links to files the user cannot access

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Unix/POSIX, Linux (before protected_hardlinks), BSDs, macOS
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A hard link is a second directory entry for the same inode. Traditionally any user could create one to any file on the same file system, including files they cannot read or write and setuid binaries, because link creation checks only write permission on the target directory [S620].

**Why it was chosen.** In the inode/directory-entry model, names are just references, and a link count lets files have several names cheaply (used by tools and early `mv`/`rename` implementations [unverified]). Checking only the directory was consistent with treating directories as the authority over names.

**Why it is a problem.** A root process following a hard link planted in a shared directory such as `/tmp` crosses a privilege boundary, which is the hardlink TOCTOU race [S620][S619]. A user can "pin" a vulnerable setuid/setgid binary by linking to it, so an administrator's upgrade leaves the old exploitable copy reachable [S620][S621]. Users are surprised they can link files they cannot access [S620]. Link-restriction fixes (chiefly for symlinks) were discussed from 1996 onward, and the patch history lists repeated objections [S621].

**Why it was not fixed.** Objections recorded against the symlink half of the patch included POSIX conformance, breaking applications, "just use `mkstemp()`/`O_EXCL`", and whether the fix belonged in the VFS or an LSM [S621]. Linux adopted the Openwall/grsecurity restriction as `protected_hardlinks` (and `protected_symlinks`); the patches were posted in 2012 [S620][S621]. Linking then requires owning the file or having read and write access [S621][S619]. Cook argued POSIX already permits this check [S620]. Known breakage was small: some `at` daemons [S620].

**Alternatives that exist.** Restricting link creation to owners or holders of read and write access (Linux, Ubuntu for about 1.5 years before mainline) [S620]. Plan 9 builds aliasing through per-process bind/mount rather than persistent links [S161]; whether Plan 9 file servers support hard links at all could not be verified [unverified].

**Cost of the alternatives.** Some legitimate tools relying on unrestricted links break [S620]. Removing hard links altogether also removes a cheap snapshot/dedup idiom used by backup tools [unverified]. The restriction is a sysctl, so the old semantics persist where it is disabled [S619].

**Relevance to our OS.** medium. If names are references to objects, the authority to create a new name for an object should be tied to authority over the object, not just the directory.

**Sources.** [S619], [S620], [S621], [S161]

### DM-041: A single global mount namespace, with mounting reserved for root

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Traditional Unix, Linux (before 2.4.19 / 3.8), BSDs, macOS; Windows drive letters are a related global namespace
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Classic Unix has one file tree shared by all processes. `mount` alters that shared tree and therefore requires superuser privilege. Per-process views (chroot aside) did not exist.

**Why it was chosen.** One tree is simple to administer and explain. Because the tree is shared and setuid programs trust paths like `/etc/passwd`, letting an ordinary user mount over a directory would let them subvert privileged programs. Linux's docs say many features were restricted to privileged users because they could confuse set-user-ID-root programs [S155]. The constraint is partly self-inflicted (setuid trust in a shared tree), it has not gone away, and Plan 9 (described c. 1995) already had per-process namespaces and no super-user [S625], so this is treated as a tradeoff rather than right-at-the-time.

**Why it is a problem.** Users and applications cannot compose their own view of resources, such as sandboxes, per-application `/tmp`, or private device views, without privilege. That pushed sandboxing into setuid helpers and root daemons. Linux retrofitted mount namespaces (`CLONE_NEWNS`, 2.4.19) [S622]. Full isolation proved too strict, so shared-subtree propagation followed in 2.6.15, adding a notoriously complex state machine [S622]. Unprivileged mounting arrived only via user namespaces (3.8+), limited to specific filesystem types; block-device file systems still need init-namespace privilege [S155]. Making root-only kernel code reachable to unprivileged users widened the attack surface. Canonical cites a Google figure that 44% of observed exploits required unprivileged user namespaces, and Ubuntu now gates them with AppArmor [S156].

**Why it was not fixed.** Setuid binaries and global path assumptions are everywhere, so namespaces had to be opt-in and bolted on. Each relaxation needed new safety rules, such as locked mounts and flags that cannot be changed in less-privileged namespaces [S622].

**Alternatives that exist.** Plan 9 makes the name space a per-process attribute with no global name space. Processes build it with `mount`, `bind` and union directories, and each window gets its own `/dev/cons` [S161]. Plan 9 "has no super-user" and user processes construct their namespaces themselves [S625].

**Cost of the alternatives.** Plan 9 programs depend on agreed conventions for where things live, because nothing is global [S161]. On Linux, retrofitted unprivileged namespaces became an exploit enabler [S156]. Per-process namespaces complicate debugging and administration ("which tree is this process seeing?") [unverified as a measured cost].

**Relevance to our OS.** high. Whether the namespace is per-process and composable by unprivileged code is a foundational decision that interacts directly with the capability model and the absence of setuid.

**Sources.** [S622], [S155], [S161], [S625], [S156]

### DM-042: atime updated on every read

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Unix/POSIX, Linux (default changed 2.6.30), Windows NTFS (last-access time), Btrfs and other CoW file systems
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Every file records a last-access time. POSIX's model marks `st_atime` for update when data is read. The actual write may be deferred, but it must happen by the time the file is closed or stat'ed [S533].

**Why it was chosen.** On small, shared systems atime supports useful policies: detecting unread mail (mail tools such as mutt), cleaning unused temp files (tmpwatch), and backup decisions [S627][S628]. Disk writes for metadata were a modest cost relative to workloads of the time (inferred). The cost was not new, though: in 2007, on ext3, Molnar already rated it Linux's worst I/O deficiency [S627], so this is a tradeoff between a cheap usage signal and write amplification, made worse (not created) by copy-on-write and flash.

**Why it is a problem.** It turns every cached read into a disk write. Ingo Molnar called it "by far the biggest IO performance deficiency" in Linux in 2007 [S627]. On copy-on-write file systems the cost multiplies. On Btrfs, a recursive grep after a snapshot rewrites inode metadata and unshares it from the snapshot, so reading files consumes disk space, and relatime may not help much [S631]. Read-only workloads therefore also wear flash and fill snapshots.

**Why it was not fixed.** Programs depend on atime, so distributions avoided `noatime` as a default [S627]. Linux compromised with `relatime` (2.6.20): it updates atime only if it is older than mtime/ctime or more than a day old, and has been the default since 2.6.30 [S627][S628]. `lazytime` keeps timestamps in memory for up to 24 hours [S628]. NTFS similarly defers last-access writes by up to an hour and can disable them [S630].

**Alternatives that exist.** `noatime`/`relatime`/`lazytime` [S628]. NTFS's deferred and optional last-access [S630]. Commenters on LWN proposed opt-in per-file atime or a separate atime store [S631] (proposal only).

**Cost of the alternatives.** relatime still breaks "accessed within the last N" queries [S627]. Disabling last-access affects backup and remote-storage tools [S630]. Dropping atime removes a cheap usage signal that tiering and cleanup tools use.

**Relevance to our OS.** low. Easy to avoid from scratch (make access tracking opt-in), but the POSIX layer must decide what `st_atime` reports.

**Sources.** [S627], [S628], [S533], [S630], [S631]

### DM-043: Windows MAX_PATH and DOS-reserved device names

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows (Win32 API layer; NTFS itself is less restricted), applications ported to Windows (Git, Python)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Win32 path APIs historically limit paths to `MAX_PATH` (260 characters). Longer paths need the `\\?\` prefix (about 32,767 characters, absolute paths only), or, since Windows 10 1607, an opt-in that requires *both* a machine-wide registry setting (or the equivalent Group Policy [S635]) *and* a `longPathAware` manifest in each application [S632]. Names such as CON, PRN, AUX, NUL, COM1–9 and LPT1–9 are reserved in every directory, even with an extension (`NUL.txt` is NUL), and Windows also treats superscript digits as part of COM/LPT names [S633].

**Why it was chosen.** DOS 1.0 had no subdirectories. Magic device names let users redirect output to the printer or discard it without temporary files. Because tools appended extensions automatically, the names had to work with any extension [S634]. A fixed small path buffer suited 1980s memory budgets (inferred).

**Why it is a problem.** Fixed limits leak into every application. Cloning a Git repository with long names into a deep folder can fail [S632]. Relative paths cannot use the `\\?\` prefix and so stay limited to MAX_PATH [S632]. The shell and the file system disagree, so the API can create paths that Explorer cannot handle [S632][S633]. Names that are legal on NTFS through `\\?\` or other OSes become inaccessible or dangerous through Win32 (reserved names, trailing dots and spaces) [S633]. Windows may still generate 8.3 short-name aliases unless disabled, adding lookup cost and a second name for long-named files [S630][S633].

**Why it was not fixed.** Compatibility. Raymond Chen notes batch files and documentation still rely on the reserved names [S634]. Lifting MAX_PATH globally would break programs with fixed 260-character buffers, hence the dual opt-in [S632]. Python enabled long-path support in 3.6 but still needs an administrator to set the policy [S635].

**Alternatives that exist.** NT's native namespace sits below these Win32 conventions (inferred: without the 260-character and reserved-name rules). Win32 is a translation layer on top of it, and `\\.\` and `\\?\` expose it [S633]. Unix places devices in `/dev` rather than reserving names everywhere.

**Cost of the alternatives.** Unbounded paths need dynamic buffers in every API and still need *some* limit to bound kernel work. Raw namespace access bypasses normalization that some code relies on [S633].

**Relevance to our OS.** medium. A clear lesson for API design: avoid magic names and fixed buffer limits in a path ABI, and keep any compatibility translation layer from becoming the de-facto native API.

**Sources.** [S630], [S632], [S633], [S634], [S635]

### DM-044: Windows share modes: open files block deletion, rename and update

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows NT family (Win32 `CreateFile`); contrast Linux (mandatory locking removed)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Every Win32 open specifies a share mode. A handle opened without `FILE_SHARE_DELETE` prevents others from deleting *or renaming* the file, and without `FILE_SHARE_WRITE` or `FILE_SHARE_READ` it blocks those opens. A conflicting open fails with `ERROR_SHARING_VIOLATION` [S636]. Traditional deletion only takes effect when all handles close [S638].

**Why it was chosen.** It gives enforced, kernel-checked mutual exclusion that works for naive programs and for network file sharing. An open file cannot change underneath an editor or a running executable. This is a real integrity property, not merely an accident.

**Why it is a problem.** Updates of in-use files are blocked. Microsoft's own docs say the primary reason installs and updates require a restart is that files being updated are in use [S139]. Any third-party handle, such as an antivirus scanner, indexer or backup agent, can make deletes and renames fail transiently, which makes atomic replace-by-rename unreliable [unverified as a measured frequency]. Correctness depends on every opener choosing the right share flags.

**Why it was not fixed.** Applications rely on exclusivity for integrity. Microsoft added mitigations instead. Restart Manager shuts down and restarts applications holding files [S139]. POSIX-style delete (`FILE_DISPOSITION_POSIX_SEMANTICS`) removes the name immediately while existing handles keep the data, though a mapped view still blocks deletion [S638].

**Alternatives that exist.** Unix decouples names from open files: unlink and rename succeed while the file is open, and old handles keep the old inode. Linux tried mandatory locking but found it racy with `write()` and `mmap()` ("unreliable"), made it optional in 4.5 and removed it in 5.15 [S600].

**Cost of the alternatives.** Under Unix semantics, running processes keep using old binaries and libraries after an update, so "updated" does not mean "in effect". Advisory-only coordination means nothing protects a file from a careless writer. Mandatory locking failed in practice on Linux [S600].

**Relevance to our OS.** medium. The interaction between names, open handles and updates determines how live system updates work, and both extremes have documented failure modes.

**Sources.** [S600], [S636], [S139], [S638]

### DM-045: No usable transactional filesystem API (Windows TxF deprecated)

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows (TxF/TxR, Vista+), Linux Btrfs (user transaction ioctls), research TxOS; by absence: POSIX, Linux, BSDs, macOS
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Mainstream file APIs offer single-call atomicity at best (`rename`) and no way to group several file operations atomically. Attempts to add transactions exist but have not stuck.

**Why it was chosen.** Transactions across arbitrary kernel objects are hard: isolation, conflict handling, resource limits and interaction with non-transactional actors. The pragmatic position is that applications needing ACID use a database, and Microsoft's own guidance points to ESE or SQL Server [S639].

**Why it is a problem.** Without it, applications hand-roll crash consistency and get it wrong (DM-038) [S612]. Multi-file updates such as installers and package managers can leave systems half-upgraded. TxOS cites a Vista SP1 prerequisite patch that caused endless reboot cycles [S617]. TOCTOU races persist because check and use cannot be grouped [S617].

**Why it was not fixed.** Microsoft shipped TxF in Vista, but reports "extremely limited developer interest" because of "complexity and various nuances". It says the API "may not be available in future versions" and recommends `ReplaceFile`, Windows Installer, ESE or SQL FILESTREAM instead [S639]. `fsutil` documents TxF as deprecated but still present [S630]. Btrfs added userspace transaction ioctls for Ceph; they were deprecated in 2017 (slated for removal in 4.17) because Btrfs had become unsuitable as a Ceph backend, leaving no widespread legitimate use [S640]. That abandonment says more about Ceph's needs than about transactions in general.

**Alternatives that exist.** TxOS (SOSP 2009) put ACID system transactions over many kernel resources in Linux 2.6.22. It reported about 10% overhead for a transactional software install, negligible overhead for non-transactional work, and a transactional ext3 built in under a month [S617]. It remained a research prototype.

**Cost of the alternatives.** *Position A (transactions are the right abstraction):* TxOS shows that overheads are modest and that one mechanism fixes both crash-consistency and TOCTOU classes [S617]. *Position B (they don't pay for themselves):* two production attempts (TxF and Btrfs) were abandoned for complexity and lack of users [S639][S640], and simpler primitives such as atomic replace cover most needs [S639]. Unresolved.

**Relevance to our OS.** high. Whether the storage layer offers grouped atomic operations, and how small that interface can be, is open, and the record shows a broad API alone does not guarantee adoption.

**Sources.** [S612], [S617], [S630], [S639], [S640]

## 7. Kernel structure, drivers and fault isolation

### DM-046: Monolithic kernel vs microkernel

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, BSDs, Windows NT, macOS/XNU (hybrid); alternatives MINIX 3, L4/seL4, QNX, Zircon
- **Classification:** contested
- **Confidence:** medium

**What the design is.** In a monolithic kernel, drivers, filesystems, network stack and memory management all run in one privileged address space and call each other directly. A microkernel keeps only address spaces, threads, IPC and interrupt delivery in privileged mode, and runs everything else as isolated user-mode servers that talk by messages.

**Why it was chosen.** Linux in 1991 was a one-person project on a 386, and it needed to be usable right away. Torvalds' answer to Tanenbaum was that Linux "wins heavily on points of being available now". He also attacked MINIX for its problems with a multithreaded filesystem and with multitasking inside the kernel [S705]. In 2006 he argued that separate address spaces mean "you can't share data structures". Locking and coherency then turn into distributed algorithms, which are harder to write than shared-memory code, and he called "hybrid kernel" a marketing term [S706]. Mach-era microkernels also had a real IPC cost. Biggs et al. point out that the cost was thought to be inherent until L4 [S704].

**Why it is a problem.** All privileged code is in the TCB, and a bug anywhere in it can compromise everything. Biggs et al. reviewed all 115 critical Linux CVEs up to late 2017 and could classify 112 of them. They estimate that a microkernel design alone would have removed 29% of them, and 96% would no longer have been critical [S704]. Tanenbaum et al. argue that MINIX 3 keeps foreign code out of kernel mode completely, for a user-mode-driver overhead below 10% [S707]. Driver faults (DM-047) are the main argument here.

**Why it was not fixed.** Every mainstream kernel would need a rewrite and all its drivers re-hosted. Torvalds, Linux's lead maintainer, rejects the premise that a microkernel is simpler or more reliable [S706].

**Alternatives that exist.** MINIX 3 runs drivers and servers as user processes and has a reincarnation server [S707]. L4 and seL4 are small, fast and, for seL4, formally verified [S704]. QNX and Zircon are production microkernels [unverified — no source fetched for either]. Nooks keeps the monolithic kernel and isolates drivers inside it [S702].

**Cost of the alternatives.** Both sides have evidence. Pro-microkernel: Biggs et al. say IPC cost is now small [S704]. However, their CVE study is an analysis of what would have happened, not a measurement, and it carries about ±9 percentage points of uncertainty by the authors' own estimate. Their 96% figure also includes 38% of CVEs rated only "weakly mitigated", which could still violate confidentiality or integrity [S704]. The authors belong to the seL4 group, so they are not neutral. Anti-microkernel: Torvalds' 2006 points on shared state and distributed coordination [S706] apply to any design that splits the FS, VM and block layers across servers. Restartability is also limited for stateful servers (DM-051) [S707]. The MINIX 3 performance figure comes from the system's own authors [S707].

**Relevance to our OS.** high. This is the top structural decision for a new Rust kernel, and Rust weakens part of the isolation argument (DM-048) without removing the TCB-size argument.

**Sources.** [S704], [S705], [S706], [S707], [S702]

### DM-047: Driver bugs as a dominant cause of OS crashes; fault-isolation approaches

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, OpenBSD, Windows (XP era onward); any OS with in-kernel drivers
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Device drivers are written by many vendors and are the largest and fastest-changing part of the kernel. They run with full kernel privilege, with no isolation from the core.

**Why it was chosen.** Running in the kernel gives direct hardware access, cheap interrupt handling and zero-copy calls into kernel subsystems. When there are few devices and the drivers are written by the kernel team, the risk is small.

**Why it is a problem.** Chou et al. ran static analysis over 21 Linux snapshots and one OpenBSD snapshot. For certain error types, driver error rates were up to 3–7x higher than in the rest of the kernel, and bugs survived about 1.8 years on average [S700]. Swift et al. report that extensions make up over 70% of Linux code, citing Chou et al. [S702]. They also state that drivers caused 85% of recently reported Windows XP failures [S702]. **Provenance caveat:** the only source given for that 85% figure is a 2003 private communication from Rob Short, a Microsoft vice president [S702]. It was never published as a study, and its method and sample are unknown. The SDV paper repeats it by citing Nooks, and changes "failures" to "crashes" [S703]. So the most-quoted number in this field has one unpublished origin, and it should be treated as an anecdote from Microsoft, not an independently established measurement. The bug classes are concrete: misuse of complex driver APIs (SDV encodes more than 60 WDM usage rules) [S703], lock and interrupt misuse, use-after-free and null dereference [S700]. Double fetches from user memory are also concentrated in drivers, with 57 of 90 found in Linux [S746] (see DM-160). The picture changed later. Palix et al. found that over Linux 2.6 the driver fault rate per line fell below that of arch/ and fs/ [S701]. So "drivers are uniquely bad" was strongly true in 2001 and is weaker now. Drivers still have the largest absolute number of faults, and they make up about 57% of the code [S701].

**Why it was not fixed.** Isolating drivers means either a new driver model or overhead (see the alternatives). Vendors write against the existing models.

**Alternatives that exist.** Static verification: SDV, shipped with Windows driver development [S703]. In-kernel isolation: Nooks recovered from 99% of crash-causing injected faults [S702]. User-mode driver frameworks: Windows UMDF, with a host process and a kernel reflector [S720]. Full user-mode drivers: MINIX 3 [S707]. Recovery that hides the failure from applications: shadow drivers [S708].

**Cost of the alternatives.** Nooks cut kHTTPd throughput by 60% and network streams by about 10% [S702]. It explicitly does not defend against malicious drivers [S702]. UMDF excludes filesystem, display and print drivers [S720]. Static checkers find only violations of rules someone has written down [S703]. Recovery is only as good as fault detection: in the shadow-driver tests, 35% of visible failures were not detected automatically [S708]. User-mode drivers isolate nothing if the device can still DMA anywhere (DM-161).

**Relevance to our OS.** high. We will write virtio drivers first, but any driver model we pick fixes where hardware bugs and vendor code end up.

**Sources.** [S700], [S701], [S702], [S703], [S707], [S708], [S720], [S746]

### DM-048: Memory-unsafe implementation language (C) for kernels

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, BSDs, Windows NT, XNU; nearly every production kernel
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Kernels are written in C (and some C++). The language does not enforce bounds, lifetimes or initialization, so spatial and temporal memory safety rests on programmer discipline, review and tooling.

**Why it was chosen.** C was designed to write Unix. It gives predictable code generation, direct control over layout, no runtime, and easy linking with assembly. For decades no memory-safe language had the same control without a garbage collector.

**Why it is a problem.** Large C/C++ codebases show the same pattern again and again. About 70% of the CVEs Microsoft assigns each year are memory-safety issues [S710]. About 70% of 912 high/critical Chromium security bugs since 2015 are too [S709]. Android's share was 76% in 2019 [S711]. These are whole-product figures, not kernel figures. A Linux-kernel-specific breakdown by bug class could not be verified [unverified]. Concrete kernel bug classes include use-after-free and null dereference [S700]. They also include the out-of-bounds read in CrowdStrike's CSagent.sys that crashed about 8.5 million Windows machines [S716], [S717], [S715]. Privileged C code keeps growing; Linux was about 26 MSLOC in 2018 [S704].

**Why it was not fixed.** Rewriting is not practical. The working approach is to write new code in a safe language. Android's memory-safety vulnerabilities fell from 223 to 85 a year (2019–2022) as most new code moved to memory-safe languages. Google calls this a correlation, not proven causation [S711]. Linux now accepts Rust drivers: Binder, Nova, Tyr and null-block are in mainline [S713]. Leaf drivers must go through safe abstractions, not raw C bindings [S712]. Adoption is socially contested. In January 2025 DMA-mapping maintainer Christoph Hellwig refused Rust abstractions and said a multi-language kernel would be "impossible to maintain" [S741].

**Alternatives that exist.** Rust kernels such as Redox [S1115] (also Theseus and Hubris [unverified — not fetched]) and Rust drivers in Linux [S713]. Language-based OSes such as Singularity [S707]. A small verified C core instead of a safe language (seL4) [S704].

**Cost of the alternatives.** Rust still needs `unsafe` for MMIO, DMA, page tables and FFI. Safety then depends on those abstractions being sound [S712]. Mixed-language kernels carry maintenance and review costs that maintainers have rejected publicly [S741]. Memory safety does nothing about logic errors such as a 21-vs-20 field mismatch, except that a checked index would have panicked rather than read wild memory. In kernel mode a panic still takes down the system unless it can be contained (DM-051) [S717].

**Relevance to our OS.** high. We have already chosen Rust. The open questions are how much `unsafe` to allow, where it may live, and what a panic does.

**Sources.** [S709], [S710], [S711], [S712], [S713], [S715], [S716], [S717], [S741], [S700], [S704], [S707], [S1115]

### DM-049: No stable in-kernel driver API/ABI (Linux)

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux (contrast: Windows WDM/KMDF/UMDF, macOS DriverKit)
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Linux deliberately offers no stable binary or source interface to drivers. Kernel internals can change in any release, and whoever changes an interface fixes every in-tree user. Only the syscall interface toward user space is promised to stay stable [S714].

**Why it was chosen.** The kernel documentation gives three reasons [S714]. A binary ABI cannot work because compiler version, config options and architecture change structure layout. A stable source API would freeze mistakes: the USB stack was reworked at least three times, and some security fixes required interface changes. Keeping old interfaces would add unpaid work. The prescribed fix is to merge the driver upstream, where it gets maintained for free.

**Why it is a problem.** Out-of-tree drivers break with every kernel update, so vendors who don't upstream ship kernels that are frozen or patched. The scale of this (Android vendor kernels, proprietary GPU drivers) could not be verified from fetched sources [unverified]. Upstreaming also concentrates all drivers in one TCB (DM-046, DM-047).

**Why it was not fixed.** This is policy, not oversight. Maintainers see the instability as what keeps the kernel improvable [S714].

**Alternatives that exist.** Windows gives vendors stable driver frameworks, including UMDF with a host process model [S720] and the WDM/KMDF APIs that SDV checks [S703]. A WDF driver built against an older framework version keeps loading on later framework versions as long as the major version does not change [S2300]. Microkernels move drivers behind an IPC protocol, so the stable contract is a message protocol, not an in-kernel API.

**Cost of the alternatives.** The two positions:
- *For instability:* a frozen API keeps bad designs alive. The Windows driver API is complex enough that Microsoft built a model checker with more than 60 usage rules to catch misuse [S703]. Linux's in-tree drivers also improved: their fault rate per line fell below other directories over Linux 2.6 [S701]. Linux's fault study does not show that the policy caused this.
- *For a stable interface:* vendors can ship drivers independently of OS releases. WDF binaries keep working across framework minor versions, and new features are reached through explicit runtime availability checks [S2300]. In return, the OS supports old interfaces indefinitely, and a driver that skips those checks can bug-check the machine [S2300].

Neither side's evidence is clean. Note that a microkernel's IPC protocol is also an ABI with exactly this versioning problem (DM-055).

**Relevance to our OS.** medium. With no third-party drivers this costs us nothing now. Whether driver↔kernel or driver↔server protocols are versioned is a decision we make once.

**Sources.** [S714], [S720], [S703], [S701], [S2300]

### DM-050: Third-party kernel code with full privilege (CrowdStrike, July 2024)

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Windows (security products, filter drivers); equally any OS with loadable modules (Linux LKMs, former macOS kexts)
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** The OS lets signed third-party modules (security agents, filesystem filters, anti-cheat, drivers) load into the kernel with full privilege. These modules may also interpret data that is updated often.

**Why it was chosen.** Microsoft lists what vendors get from kernel mode [S716]: system-wide visibility, the ability to block file and process operations, resistance to tampering even by attackers with admin-level privileges, and early-boot start via ELAM. Kernel placement can also help high-throughput workloads such as network inspection [S716].

**Why it is a problem.** On 19 July 2024 a CrowdStrike content update (Channel File 291) crashed about 8.5 million Windows devices, under 1% of the installed base [S715]. CrowdStrike's root-cause analysis [S717] describes the mechanism. An IPC template type declared 21 input fields, but the sensor supplied 20. Testing used wildcards for the 21st field, so the mismatch went unnoticed. The first non-wildcard instance made the in-kernel Content Interpreter read past its input array. The interpreter had no runtime bounds check, and content was not deployed in stages [S717]. Microsoft's own conclusion is that kernel drivers give these properties "at the cost of resilience" [S716]. Code signing says who wrote a module, not that it is correct. In-kernel isolation such as Nooks targets exactly this kind of buggy extension, but it explicitly assumes extensions are not malicious, and malicious kernel code can get around it [S702].

**Why it was not fixed.** Microsoft describes user-mode parity for performance and visibility as still a work in progress, so vendors kept kernel components [S716]. EU interoperability commitments also come up in this context [unverified].

**Alternatives that exist.** Microsoft's guidance after the incident: keep kernel sensors minimal, and do content parsing and updates in user mode, where a failure can be recovered from. For tamper resistance it points to user-mode protections such as VBS enclaves [S716]. In November 2024 Microsoft announced platform support for antivirus running in user mode, with a private preview planned for July 2025. It also made deployment rings mandatory for security-vendor updates and announced Quick Machine Recovery for machines that cannot boot [S718]. Other routes, not from Microsoft's guidance, are verified in-kernel bytecode (eBPF-style) [unverified] and microkernel user-mode servers (DM-046).

**Cost of the alternatives.** User-mode security agents give up some visibility and tamper resistance, which are the properties vendors wanted in the first place [S716]. Staged rollout slows the response to fast-moving threats. Recovery tools such as Quick Machine Recovery treat the symptom, not the cause [S718].

**Relevance to our OS.** high. We should decide early whether any code other than the core kernel can ever run privileged, and how a "monitor everything" service would get its visibility without that.

**Sources.** [S715], [S716], [S717], [S718], [S702]

### DM-051: Kernel crash = system crash; no component restart

- **Category:** 7. Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, BSDs, Windows NT, XNU (partial exceptions: Windows TDR, UMDF)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** When kernel code hits a fatal error, the whole machine stops (panic or bug check) or limps on in an unknown state (Linux oops). No component can be restarted alone, because all components share one address space and one set of locks.

**Why it was chosen.** If a fault can corrupt any kernel memory, stopping is the only safe response. Restart is also not free. Torvalds argues that because OS components depend on each other and share coherency state, restarting one failed service often does not save the system [S706].

**Why it is a problem.** One driver bug becomes total loss of availability (DM-047, DM-050). Linux shows the strain. Torvalds "absolutely DETESTS" BUG_ON() because it kills the machine with locks held, leaving it undebuggable and unreported [S721]. His rule became "absolutely no new BUG_ON() calls" in favour of WARN_ON_ONCE() and real error handling [S722]. In other words, a monolithic kernel copes by trying not to die, because it cannot die partially. Microsoft says kernel code cannot fail and restart the way a normal user application can [S716].

**Why it was not fixed.** Restart needs isolation (DM-046) plus a way to rebuild the restarted component's state, and both are expensive to retrofit.

**Alternatives that exist.** MINIX 3's reincarnation server restarts drivers and servers that crash or stop answering pings [S707]. Shadow drivers (on Nooks, in Linux) replay logged configuration and resubmit or cancel requests, so the failure is hidden from applications. With injected transient faults that Nooks could detect, all ten test applications kept running [S708]. Windows TDR resets the GPU and reinitializes the display driver without a reboot [S719]. UMDF puts drivers in host processes [S720].

**Cost of the alternatives.** Restart helps with transient, fail-stop faults. Deterministic bugs come back as soon as the component restarts [S708]. Irreversible side effects (writes already sent to disk, printed output) cannot be undone [S708]. Recovery took about 4–5 seconds for disk and network drivers [S708]. MINIX 3 can only restart servers that hold little internal state [S707]. In the L4-based driver-VM approach described in the same paper, a restarted driver comes back in its initial state, for example with the default audio volume [S707]. TDR bug-checks on the sixth GPU hang if five or more occur within a minute, and some legacy applications may render black after a reset [S719]. Recovery is only as good as fault detection: 35% of visible failures were not detected automatically [S708].

**Relevance to our OS.** high. In a Rust kernel the panic strategy (abort vs unwind vs restart a server) sets how much of this we can have.

**Sources.** [S706], [S707], [S708], [S716], [S719], [S720], [S721], [S722]

### DM-161: Driver isolation without DMA isolation (IOMMU off or porous)

- **Category:** Kernel structure, drivers and fault isolation
- **Systems affected:** Linux, macOS, FreeBSD, Windows; any user-mode-driver design
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Bus-mastering devices can DMA to physical memory. Unless an IOMMU restricts each device to its own buffers, a device (or a driver that programs it) can read and write all of RAM, whatever the CPU-side privilege separation says.

**Why it was chosen.** Early PCs had no IOMMUs. Using one costs mapping and invalidation overhead on every I/O, and internal devices were trusted hardware.

**Why it is a problem.** Hot-pluggable DMA peripherals (Thunderbolt, USB-C) broke the "internal, trusted" assumption [S748]. In 2018 Linux did not enable the IOMMU automatically on Thunderbolt systems, so connected devices could read and write system memory, while systems shipping with Windows 10 1803 or newer could already use the IOMMU against Thunderbolt DMA attacks [S749]. An enabled IOMMU is still not enough. Thunderclap showed that macOS, FreeBSD and Linux could be compromised despite using IOMMUs. The cause was the complex shared memory that devices could legitimately reach, combined with drivers trusting device-controlled data. They got kernel control-flow hijack (a root shell) within seconds. Windows used the IOMMU only in limited cases [S748]. For OS structure, this means a user-mode driver (DM-047, DM-051) is isolated only on the CPU side. If its device can DMA anywhere, the driver is still effectively in the TCB.

**Why it was not fixed.** Performance cost, platform firmware variability, and many drivers that assume physically contiguous, long-lived mappings. Linux's 2018 patch still identity-maps internal devices and fully maps only external ones, and it disables PCIe ATS on external devices because ATS can bypass the IOMMU [S749].

**Alternatives that exist.** IOMMU on by default with per-device domains, and short-lived, minimal mappings. Bounce buffers for untrusted devices. Treating device-written memory as untrusted input in drivers [S748].

**Cost of the alternatives.** I/O throughput and latency overhead, more complex DMA APIs, and residual exposure through shared buffers [S748].

**Relevance to our OS.** medium. QEMU `virt` with virtio-iommu (or SMMUv3 on aarch64) lets us decide early whether every driver gets its own DMA domain; virtio devices are trusted today, so the cost shows up later.

**Sources.** [S748], [S749]

## 8. Kernel–user interfaces and ABI design

### DM-052: `errno` and integer error codes

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS (contrast: NT NTSTATUS, Zircon zx_status_t)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** A failing call returns a sentinel (usually -1 or NULL) and puts a small integer in `errno`. The integer comes from a flat, global list of names (EINVAL, ENOENT, …) whose numeric values vary by system.

**Why it was chosen.** Early Unix returned an error indicator plus a code from the kernel, and a single global variable was the cheapest way to expose it to C [unverified — PDP-11 history not checked against a fetched source]. That held until threads arrived. Some architectures still signal failure through a separate flag register, such as a3 on MIPS/Alpha or the carry bit on SPARC [S724].

**Why it is a problem.** errno(3) itself lists the pitfalls [S355]:
- The value means something only after a failure, and success may overwrite it.
- Nothing ever resets it to zero, so for calls where -1 is a valid result the caller must clear it first.
- It had to become thread-local once threads existed [S355], and pthreads functions do not use it at all: they return the error number directly [S118].
- Numeric values differ across Unix systems and even between Linux architectures (EDEADLOCK on MIPS, PowerPC and SPARC).
- Names alias (EAGAIN/EWOULDBLOCK, ENOTSUP/EOPNOTSUPP).

A flat namespace pushes errors toward a few overloaded codes. The kernel docs note that subsystems return ENOSYS or EINVAL for unknown ioctls where ENOTTY is correct [S726]. Once shipped, error values are ABI. Torvalds rejected an ioctl returning ENOENT because userspace depended on the old code [S743]. Codes carry no detail: which argument, which object, which path component.

**Why it was not fixed.** Every C program and libc depends on it; POSIX specifies it.

**Alternatives that exist.** Zircon returns `zx_status_t` directly from every call. Codes are grouped (parameter, state, permission, I/O) and given precise meanings, for example INVALID_ARGUMENT (never valid) vs OUT_OF_RANGE (might become valid), WRONG_TYPE for the wrong handle type, and PEER_CLOSED [S725]. Rust's `Result<T, E>` makes the error part of the return type.

**Cost of the alternatives.** Direct return codes still have no room for detail. Zircon documents conventions, not a way to attach context [S725]. Richer error objects need allocation or variable-size returns across the syscall boundary. A POSIX compatibility layer will have to map native errors back onto errno, and that mapping loses information.

**Relevance to our OS.** high. The error vocabulary of our native ABI becomes ABI the day it ships, and it has to translate cleanly to errno for the C compiler sandbox.

**Sources.** [S355], [S118], [S724], [S725], [S726], [S743]

### DM-053: `ioctl` as an untyped escape hatch

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows DeviceIoControl is analogous
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** `ioctl(fd, request, arg)` passes a device-specific command number and an untyped pointer or integer to whatever driver backs the descriptor. The kernel knows neither the payload's layout nor its direction, and the same is true of the multiplexers fcntl and prctl.

**Why it was chosen.** With one uniform file abstraction, device-specific operations (terminal modes, disk geometry) needed some outlet that did not add a syscall per device. A single escape hatch kept the syscall table small and let drivers grow independently.

**Why it is a problem.** The kernel's own ioctl guide lists the failure modes [S726]:
- Every handler needs a matching `compat_ioctl` path for 32-bit processes.
- Struct layout differs by architecture: 64-bit fields are 4-byte aligned on x86-32 and only 2-byte aligned on m68k. Pointers have to be passed as `__u64`.
- `struct timespec`/`timeval` in payloads broke with the move to 64-bit time_t (DM-056).
- Implicit padding leaks uninitialized kernel memory to user space.
- Versioned commands break existing applications.
- Unknown commands get ENOSYS or EINVAL instead of ENOTTY.

The adding-syscalls guide warns that ioctl can lead to "a somewhat opaque API". It says fcntl "hides a lot of complexity" and calls prctl "a complicated multiplexor" [S727]. Payloads are not described in any machine-readable form, so there is no generic tracing, no way to generate bindings, and no type check at the boundary. The handler reads raw user memory directly, which is where double fetches come from (DM-160). Even error codes become de facto ABI [S743].

**Why it was not fixed.** Thousands of drivers and every user-space tool that talks to devices depend on it. New interfaces keep being added as ioctls because it is the cheapest path, for example PROCMAP_QUERY in 2024 [S732].

**Alternatives that exist.** Dedicated, reviewed syscalls with extensible structs (DM-055) [S727]. Typed IPC protocols with an IDL, such as Fuchsia FIDL [unverified — not fetched]. Plan 9's text control files.

**Cost of the alternatives.** Syscalls need central review and an entry in the syscall tables (shared or per-architecture) [S727]. An IDL needs a compiler, versioning rules and a runtime. Text control files bring back parsing (DM-054). Some escape hatch always survives. The real question is whether it is typed and self-describing.

**Relevance to our OS.** high. A capability/handle-based ABI makes it tempting to have one "invoke(handle, op, bytes)" call, which is ioctl again unless the bytes are typed.

**Sources.** [S726], [S727], [S732], [S743]

### DM-054: Text-based `/proc` and `/sys` interfaces

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux (procfs, sysfs); Plan 9 heritage; other Unixes to lesser degrees
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** The kernel exports state and controls as synthetic files containing human-readable text. Consumers open, read and parse them, and they write text to change settings.

**Why it was chosen.** Text works with `cat`, `grep` and shell scripts, needs no special tooling, can be inspected by people, and is independent of architecture (no struct layout or endianness). sysfs tries to keep this manageable with a rule: ASCII, "preferably with only one value per file". It allows arrays of one type and frowns on mixed types and multi-line formatting [S729].

**Why it is a problem.** procfs predates that rule and breaks it. `/proc/pid/mountinfo` has a variable number of optional fields ended by a lone "-". Parsers must scan for the separator, ignore unknown tags, and cope with reused mount IDs [S730]. Every consumer writes its own parser, and every format change risks breaking one. Text is also slow when the data is large. Profilers that symbolize addresses had to read and parse all of `/proc/pid/maps`. A 2024 binary ioctl (PROCMAP_QUERY) cut an end-to-end address normalization from about 50 µs to about 10 µs, with 35–40x in a microbenchmark [S732]. Reading a file is not atomic across multiple reads, so consumers can see torn snapshots [unverified]. The outcome is a growing set of binary replacements: statmount() returns a struct with a mask of filled fields [S731], and PROCMAP_QUERY is an ioctl on the text file [S732]. Linux now ships two interfaces for the same data.

**Why it was not fixed.** The text formats are ABI. Scripts and tools parse them, so they cannot be changed, only supplemented.

**Alternatives that exist.** Binary syscalls with masks (statmount [S731], statx [S734]). One-value-per-file discipline (sysfs [S729]). Structured self-describing formats such as netlink attributes, or typed IPC to a system service.

**Cost of the alternatives.** Binary interfaces need headers, versioning and tooling to inspect, and they lose shell scriptability. One-value-per-file trades parsing for many open/read/close calls and still gives no atomic multi-value read. Every structured format needs a schema and an evolution story.

**Relevance to our OS.** medium. Observability interfaces get decided early and are hard to remove. Whether they are typed records with a text renderer, or text, is a choice we can still make.

**Sources.** [S729], [S730], [S731], [S732], [S734]

### DM-055: Unchecked flags and fixed structs: syscall extensibility

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux (open, clone, many *2/*3 successors), Unix/POSIX generally
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Many original syscalls take a fixed argument list and a flags word whose unknown bits are silently ignored. Extending them means either a new flag bit, which is unsafe if old kernels ignore it, or a whole new syscall.

**Why it was chosen.** Early syscalls were few, designed by a small team, and changed together with the libc. Checking flags looked like overhead, and nobody expected decades of extensions running on kernels of many ages.

**Why it is a problem.** If unknown bits are ignored, user space cannot tell whether a feature is supported. A program asking for safe behaviour on an old kernel silently gets unsafe behaviour. openat2(2) states it directly: openat ignores unknown flag bits, and openat2 rejects them [S613]. Bits run out and get reused. In clone(), the low byte of flags is the exit signal. CLONE_PID was silently ignored after 2.5.16 and its bit reused for CLONE_PIDFD, and CLONE_STOPPED's bit was reused for CLONE_NEWCGROUP [S733]. A reused bit means an old binary can trigger new semantics. The other route is syscall proliferation: eventfd/eventfd2, dup2/dup3, inotify_init/inotify_init1, pipe/pipe2, renameat/renameat2 [S727].

**Why it was not fixed.** Old calls cannot start rejecting bits that existing binaries pass. The fix applies only to new calls.

**Alternatives that exist.** Linux's current rules [S727]: reject unknown flags with EINVAL, and require zero when no flags exist yet. For complex calls, pass a struct with a size field. Newer fields must default to zero; the kernel zero-extends smaller structs and requires unknown trailing bytes to be zero. openat2 and clone3 follow this. Size acts as a version, and nonzero unsupported fields give E2BIG [S613], [S733]. statx and statmount use request/response masks, so the kernel reports which fields it actually filled [S734], [S731].

**Cost of the alternatives.** Strict rejection makes probing for a feature cost a failed syscall. Size-versioned structs need zero-fill discipline in callers [S613] and a copy-in helper in the kernel. Masks need callers to check what came back. statx accepts unsupported requested bits and just reports what it filled; it rejects only the reserved bit with EINVAL. So masks serve a different purpose than strict flags [S734].

**Relevance to our OS.** high. This is cheap to get right on day one and impossible to fix later: every native syscall and IPC message should reject unknown bits and carry a size or version.

**Sources.** [S727], [S613], [S731], [S733], [S734]

### DM-056: Fixed-width types baked into the ABI (`off_t`, `time_t`, `stat64` proliferation)

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux (esp. 32-bit), glibc, Unix/POSIX; Debian armhf/armel
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** File offsets, sizes, inode numbers, IDs and timestamps were defined as C types whose width matched the machine word (often 32 bits) and were embedded in syscall structs. When the widths turned out too small, each type grew through parallel interfaces instead of in place.

**Why it was chosen.** Memory and register width were scarce. 32 bits covered every disk and timestamp of the era, and fixing the width to `long` matched the hardware.

**Why it is a problem.** On 32-bit Linux, stat exists in three kernel generations: `__old_kernel_stat` with narrow fields, `stat` with a larger st_ino, and `stat64` (Linux 2.4) with larger st_ino, 32-bit uid/gid and later nanosecond timestamps. There are also fstatat64 and newfstatat depending on architecture. glibc hides this by repacking [S735]. statx (Linux 4.11) is a fourth design, built to be extensible from the start [S734]. In user space, glibc kept a parallel `*64` API (`off64_t`, `fseeko64`) under `_LARGEFILE64_SOURCE`, plus `_FILE_OFFSET_BITS=64` to redirect the old names. The default is still 32 bits, and `_TIME_BITS=64` works only together with `_FILE_OFFSET_BITS=64` [S737]. Every library whose public structs contain these types inherits the problem. Debian's 2024 64-bit time_t transition changed the ABI of about 495 library packages and needed 5,000–6,000 rebuilds. Some of the ABI changes (58 in one analysis) came from finally enabling large-file support. i386 was left out to keep legacy binaries running [S736]. Timestamps inside ioctl structs broke too [S726]. Compat layers have to handle explicit 64-bit arguments for 32-bit callers [S727]. The 2038 problem itself is DM-063.

**Why it was not fixed.** Changing a type's width breaks every binary that embeds it. Distributions have to coordinate a flag-day rename [S736].

**Alternatives that exist.** 64-bit-only ABIs: on 64-bit Linux there is a single stat syscall [S735]. Explicit fixed-width fields (`__u64`) in all UAPI structs [S726]. Extensible structs with masks, like statx [S734].

**Cost of the alternatives.** 64-bit fields everywhere cost a little space. That is irrelevant on aarch64, but it is an ABI decision that 32-bit targets would inherit.

**Relevance to our OS.** medium. As a 64-bit-only aarch64 OS we avoid most of this by default. The remaining risk is any "obviously big enough" width we choose now, such as a 32-bit handle, PID or nanosecond count.

**Sources.** [S734], [S735], [S736], [S737], [S726], [S727]

### DM-057: Where the stable ABI lives: syscalls (Linux) vs libc/ntdll (BSD, Windows, macOS)

- **Category:** 8. Kernel–user interfaces and ABI design
- **Systems affected:** Linux; macOS/XNU; OpenBSD; Windows NT
- **Classification:** tradeoff
- **Confidence:** high

**What the design is.** Linux promises that the raw syscall interface will not break [S714], [S743]. Programs, including static binaries and runtimes like Go, may trap into the kernel directly. Other systems promise stability only at a user-space library and treat syscall numbers as private.

**Why it was chosen.** Linux's kernel and C library come from different projects, so the kernel boundary is the only contract the kernel developers own. Torvalds' rule is that if a kernel change breaks a user program, that is a kernel bug [S743]. Apple, the BSDs and Microsoft ship kernel and libc together, so the library is the natural contract.

**Why it is a problem.** Each choice has a cost:
- *Syscall-stable (Linux):* every mistake in the syscall layer is permanent. The result is stat generations (DM-056), ignored flags (DM-055) and error codes frozen in place [S743]. Raw syscall conventions also vary by architecture [S724].
- *Library-stable:* programs that bypass libc break. Apple says a static binary "assumes binary compatibility at the kernel system call interface", which Apple does not guarantee, so static binaries are unsupported [S745]. Windows NT syscall numbers shift between releases, for example NtCreateFile is 0x52 through Windows 7, then 0x53, 0x54, and 0x55 from Windows 10 [S744]. Go had to give up most direct syscalls: from Go 1.11 the runtime calls libSystem on macOS so binaries stay compatible with future releases [S738], and Go 1.16 calls libc on OpenBSD because OpenBSD 6.9+ requires it for non-static binaries [S739].

**Why it was not fixed.** Neither model is broken; each fits its system's organisation. OpenBSD went further in the other direction. Since 7.5, pinsyscalls(2) pins each syscall number to its call site in libc, and a mismatch is killed with SIGABRT [S740]. That turns "libc is the ABI" into a security mechanism against injected syscall instructions.

**Alternatives that exist.** Linux model: stable syscalls plus a vDSO. Library model: macOS libSystem, Windows ntdll/kernel32, and OpenBSD libc with pinning [S740]. Capability systems can version the IPC protocol instead.

**Cost of the alternatives.** Library-stable ties every language runtime to the system C library (or a system-provided non-C shim) and rules out fully static binaries [S745], [S739]. Syscall-stable freezes kernel mistakes forever and makes the kernel boundary the attack surface for every program [S714].

**Relevance to our OS.** high. We must choose whether the native ABI is the raw trap interface or a Rust system library (with the POSIX layer above it). That choice decides whether static binaries are possible and how freely the syscall layer can change.

**Sources.** [S714], [S724], [S738], [S739], [S740], [S743], [S744], [S745]

### DM-160: Raw user pointers in syscall arguments and double-fetch races

- **Category:** Kernel–user interfaces and ABI design
- **Systems affected:** Linux, FreeBSD, Android, Windows NT
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** System calls take pointers to user memory, and kernel code reads from them whenever it needs a value (copy_from_user, get_user, probing user buffers). User memory can be changed by another thread at any time, so the same user field may be read twice with different results.

**Why it was chosen.** Passing pointers is cheap and flexible. Variable-length structures (ioctl payloads, a length header followed by a body) are naturally read in two steps: header first, then body.

**Why it is a problem.** In a double fetch, the kernel reads a size or type field once to validate it and again to use it. A racing thread changes it in between, which leads to overflows or confused types. Wang et al. found 90 double fetches in Linux by static pattern analysis, 57 of them in drivers. They found six previously unknown double-fetch bugs across Linux, FreeBSD and Android, three of them exploitable [S746]. On Windows, Jurczyk and Coldwind's Bochspwn used CPU-level instrumentation to find races in syscall handlers fetching user-mode input, and exploited three of them [S747]. The bug is invisible in single-threaded review because each read looks correct on its own.

**Why it was not fixed.** The pointer-passing ABI is set by every existing syscall and ioctl (DM-053). The fixes are per call site: read once into a kernel copy, then validate.

**Alternatives that exist.** Copy-in/copy-out at the boundary, where the kernel copies the whole argument once and works only on its private copy. Message-passing kernels do this naturally: for example, a Zircon `zx_channel_write` atomically writes the message data into the channel, and the receiver reads its own copy, so the handler never touches sender memory [S2601]. Typed message formats (DM-053) make the copy size known in advance.

**Cost of the alternatives.** Copying every argument costs memory bandwidth for large buffers, and Zircon caps channel messages at 64 KiB [S2601]. Zero-copy paths such as shared rings (io_uring, see DM-025) bring back shared mutable memory on purpose and need the same discipline.

**Relevance to our OS.** high. A new syscall ABI can make "read once, into kernel memory" a structural rule enforced by the Rust types at the boundary, not a convention.

**Sources.** [S746], [S747], [S2601]

## 9. Concurrency and synchronization primitives

### DM-058: Big Kernel Lock; SMP retrofitted onto uniprocessor kernels

- **Category:** 9. Concurrency and synchronization primitives
- **Systems affected:** Linux (BKL, 1.3.x–2.6.39), FreeBSD (Giant, SMPng), other Unix kernels [unverified for others]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** A kernel written for one CPU gets multiprocessor support through a single global lock: only one CPU may execute (most) kernel code at a time. Concurrency is then added by moving subsystems out from under the lock one by one.

**Why it was chosen.** It was the only feasible first step. In Linux 1.3.x the "lock" was a bit set on kernel entry, so CPUs spun at the door and no driver had to change [S341]. Generic spinlocks did not exist on most architectures, so the BKL was the only SMP mutex available. The 2.1.23 change that spread it (9 to 761 call sites) is also what first allowed two tasks in the kernel at once [S341]. FreeBSD's Giant played the same role [S343].

**Why it is a problem.** The lock was recursive, so there was no risk of deadlocking with oneself, and code took it "just to be safe" at no visible cost [S341]. Its many uses became entangled: unrelated subsystems, such as a webcam driver list and the NFS daemon, excluded each other for no reason [S341]. Nobody could say what a given `lock_kernel()` protected. Each removal required reverse-engineering implicit invariants. When the BKL was pushed out of the VFS, every driver `open` got its own copy, and many driver owners never took the time to check whether it was needed [S341]. The cost: poor scalability for a decade, and latency problems; the realtime tree's low tolerance for latency motivated part of the late removal work [S342].

**Why it was not fixed.** The fix was spread across hundreds of call sites in code with absent or uninterested owners. It took roughly 1995–2011. The end game removed or staged modules nobody would fix [S342] and finished with "BKL: That's all, folks" (January 2011, targeted at 2.6.39) [S340], [S342]. FreeBSD similarly kept code not marked MPSAFE running under Giant [S343].

**Alternatives that exist.** Design for SMP from day one with explicit, documented lock scopes, or avoid shared kernel state (multikernel-style per-core state). Notably, seL4 deliberately keeps a big lock. Because microkernel system calls are short, a big lock performs comparably to fine-grained locking on shared-cache cores, and it keeps the kernel verifiable [S344], [S345].

**Cost of the alternatives.** Fine-grained locking from the start costs complexity, deadlock risk, and verification difficulty [S345]. The big-lock approach only works when the protected code paths are short. It breaks down for a monolithic kernel with long syscalls, which is exactly the BKL lesson [S344].

**Relevance to our OS.** high — the lesson is less "never use a big lock" than "keep what the lock covers small and documented"; a small kernel may legitimately start with one.

**Sources.** [S340], [S341], [S342], [S343], [S344], [S345]

### DM-059: Thread cancellation (`pthread_cancel`)

- **Category:** 9. Concurrency and synchronization primitives
- **Systems affected:** POSIX threads (glibc, musl, BSD libcs); analogous: Windows `TerminateThread`, Java `Thread.stop`
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** One thread can request another's termination. In deferred mode the target dies at the next cancellation point, a long list of functions including `open`, `read`, `close`, `accept` and `pthread_cond_wait`. In asynchronous mode it can die at any instruction. Cleanup handlers run in reverse push order, then TSD destructors [S346], [S118].

**Why it was chosen.** Threads blocked in system calls need some way to be stopped, for example to abort a search or shut down a server, without each blocking call taking a "stop" argument. POSIX tried to make this safe with cancellation points and cleanup handlers instead of raw asynchronous kill.

**Why it is a problem.**
- Asynchronous mode is unusable in practice: only `pthread_cancel`, `pthread_setcancelstate` and `pthread_setcanceltype` are async-cancel-safe [S118].
- Deferred mode is subtle too, although this part was an implementation defect, not a flaw in the POSIX design. glibc implemented each cancellable syscall by briefly enabling *asynchronous* cancellation around it. A cancel arriving after the kernel had acted, but before user space saw the result, could leak a descriptor from `open`/`accept`. For `close` it left the program unable to know whether the fd was released, risking closing a different thread's fd later [S348]. Signal handlers that interrupted such syscalls ran with async cancellation on [S349].
- Reported in 2011 (BZ #12683), the race was fixed only in glibc 2.41, and a follow-up crash fix landed in 2.42 [S350], [S349].
- Every library on a cancellable path must be written cancellation-safe with push/pop handlers, and most are not.
- On Linux the mechanism rides on a hidden real-time signal [S346], inheriting signal complexity (DM-017).
- The forced-kill cousins are worse. `TerminateThread` can leave critical sections and the heap lock held [S351]. Java removed `Thread.stop` because releasing monitors leaves "damaged" objects [S352].

**Why it was not fixed.** It is part of POSIX, and C code depends on it, for example to break out of blocking reads. Fixing the race required arch-specific syscall wrappers [S349].

**Alternatives that exist.** Cooperative cancellation: a flag plus an interrupt that makes blocking calls return, as in Java's recommended pattern [S352]. Cancellation tokens passed explicitly. In capability or handle systems, closing or revoking the handle a thread waits on makes the wait return an error. musl decides precisely whether a syscall completed by checking the saved program counter against assembly labels around it [S348]. That shows deferred cancellation can be implemented without the glibc race.

**Cost of the alternatives.** Cooperative schemes need every blocking primitive to be interruptible, plus disciplined callers. A thread stuck in uncooperative code cannot be stopped, short of killing the whole process.

**Relevance to our OS.** high — the native ABI should define how blocking waits are interrupted (handle close, timeout, cancel token); the POSIX layer must still emulate `pthread_cancel` for ported C code.

**Sources.** [S346], [S118], [S348], [S349], [S350], [S351], [S352]

### DM-060: Non-reentrant libc interfaces with hidden global state (`strtok`, `localtime`, `errno` before TLS)

- **Category:** 9. Concurrency and synchronization primitives
- **Systems affected:** C standard library / POSIX on all Unix-likes; Windows CRT [unverified]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Many C and POSIX functions keep state in static storage. `strtok` stores its position internally [S353]. `localtime`, `gmtime`, `asctime` and `ctime` return pointers to static buffers that any later call overwrites [S354]. `errno` was originally a plain global `int` [S355]. The environment (`getenv`/`setenv`) is an unsynchronised global array.

**Why it was chosen.** These functions predate threads. In a single-threaded process, static buffers save callers from allocating memory and keep signatures short. On 1970s machines both mattered.

**Why it is a problem.** With threads, every such function is a data race. POSIX exempts roughly 90 functions from its thread-safety requirement, including `strtok`, `localtime`, `asctime`, `getenv` and `strerror` [S118]. glibc labels them, for example `MT-Unsafe race:tmbuf env locale` [S354]. The fix doubled the API with `_r` variants (SUSv2), and those differ subtly: `localtime_r` need not call `tzset`, so portable code must call it explicitly [S354]. `errno` had to become a per-thread macro. Old code that declares `extern int errno` is now harmful [S355]. Environment mutation is still unsound with threads. Rust 2024 made `std::env::set_var`/`remove_var` `unsafe` for exactly this reason, after shipping them as safe [S236]. Boehm's broader point: threads bolted onto a language and library designed without them cannot be made fully correct by the library alone [S366].

**Why it was not fixed.** The originals are in ISO C and POSIX and cannot change signature. Removing them breaks source compatibility, so both sets coexist.

**Alternatives that exist.** APIs designed after threads, with caller-supplied buffers or returned owned values (all `_r` functions, Rust `std`). Thread-local error values or error returns instead of `errno`. Immutable process environment snapshots.

**Cost of the alternatives.** More parameters, explicit buffer management, and divergence from code written against the old names.

**Relevance to our OS.** medium — native interfaces can avoid hidden global state entirely, but the POSIX compatibility libc must provide the legacy functions and decide how unsafe `setenv` is allowed to be.

**Sources.** [S118], [S353], [S354], [S355], [S236], [S366]

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

### DM-120: Futex interface: one multiplexed syscall with accreted operations

- **Category:** Concurrency and synchronization primitives
- **Systems affected:** Linux (futex, PI futexes, futex2); user-space libcs that build on it
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Linux exposes user-space synchronisation through one `futex` syscall whose `op` argument selects among many operations (wait, wake, requeue, cmp-requeue, wake-op, PI lock/unlock, requeue-PI…). The futex word is an aligned `int` in user memory. Drepper describes the kernel as identifying futexes by the underlying physical memory, so processes sharing a mapping share the futex [S367]; for private futexes the kernel uses the virtual address [S369]. The uncontended path stays in user space (GR-011 covers the core idea, which held up).

**Why it was chosen.** Fast uncontended locking without syscalls was the goal [S367]. A single multiplexing entry point let new operations be added as glibc needed condition variables, robust mutexes and PI [S367], [S360] (the "added quickly" motive is an inference, not stated by the sources).

**Why it is a problem.** The semantics are hard to use correctly. Drepper notes that the inventors' own published user-level code was incorrect [S367]. Operations are added and then found broken: `FUTEX_REQUEUE` "proved to be broken and unusable" and survives only for compatibility [S367]. PI futexes add kernel-side `pi_state` and rt-mutex machinery [S360]. One requeue bug, which failed to ensure two distinct futex addresses, became a local privilege escalation, CVE-2014-3153, later listed in CISA's known-exploited catalog (2022) [S368]. The interface could not wait on multiple futexes, and its word is fixed at 32 bits. That led to a separate futex2 interface "designed to overcome limitations of the original": `futex_waitv` waits on up to 128 futexes, and the API reserves sizes of 8–64 bits, though only 32-bit futexes are currently supported [S369].

**Why it was not fixed.** Every libc and runtime depends on the existing op codes, so they can be added to but not removed [S367], [S369].

**Alternatives that exist.** Separate, typed syscalls per operation: the futex2 direction [S369], and Zircon, which has distinct `zx_futex_wait`, `zx_futex_wake`, `zx_futex_requeue` and owner-tracking calls, with the owner used for priority inheritance [S2600]. Kernel-provided higher-level objects (Windows events/keyed events [unverified]). Notification objects in capability kernels (seL4) [S305].

**Cost of the alternatives.** More syscalls and ABI surface. Higher-level kernel objects cost a syscall even when uncontended unless paired with a user-space fast path. Removing multiplexing loses the ease of extension that made futexes evolve quickly.

**Relevance to our OS.** high — our own wait/wake primitive will underpin the POSIX layer's pthreads and the native runtime, and its extensibility shape (typed calls vs op codes) is an early ABI choice.

**Sources.** [S305], [S360], [S367], [S368], [S369], [S2600]

## 10. Time, clocks and scheduling

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

## 11. Terminals, sessions and consoles

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

## 12. Networking APIs

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

### DM-076: Privileged ports below 1024

- **Category:** Networking APIs
- **Systems affected:** Unix/BSD, Linux, macOS (wildcard binds unrestricted since 10.14; binds to a specific address still restricted); contrast iOS, Windows
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Binding a TCP/UDP port below 1024 requires root, or on Linux `CAP_NET_BIND_SERVICE`. Linux makes the threshold configurable per network namespace with `net.ipv4.ip_unprivileged_port_start` (default 1024; 0 disables it) [S445].

**Why it was chosen.** On early multi-user hosts run by trusted administrators, a connection from a source port below 1024 proved that root on the remote host had sent it. BSD `rcmd`/`rlogin` built authentication on this: `rresvport` obtains a privileged port, and servers then trust `.rhosts`/`hosts.equiv` [S446]. The low-port rule also stopped ordinary users from impersonating well-known services on shared machines.

**Why it is a problem.** The proof only holds if every host on the network is administered by someone trustworthy, which stopped being true once anyone could run their own machine. Meanwhile the rule forces daemons that serve port 80 or 443 to start as root or carry a capability. That is ambient privilege for a trivial need, and it encouraged setuid daemons and root-run servers. It is also global per namespace and tied to the port number, not to any notion of which program should own the service.

**Why it was not fixed.** The default stays for compatibility, but it is now widely disabled. Linux added the per-namespace sysctl in 2017 so container managers could lift the restriction without giving up control of the network namespace [S447]. Docker has set it to 0 in containers by default since 2020, calling privileged-port binding a safe operation [S449]. Apple lifted the restriction in macOS Mojave without documenting it, and iOS never had it. The macOS change covers only wildcard binds: as of 2025, binding a low port on a specific local address still needs root [S448].

**Alternatives that exist.** Grant a specific port to a specific program by policy. One example is socket activation: the service manager creates the listening socket and passes the fd to the daemon at `exec` [S233]. Or drop the restriction entirely, as iOS, macOS (for wildcard binds) and Docker containers do [S448][S449].

**Cost of the alternatives.** With no restriction on a shared host, any local user can grab a well-known port before the real daemon starts. Review of the Docker change agreed that it must not be applied in the host network namespace, and a later runc change rejects that case [S449]. Per-port authority needs a policy store and an authority to manage it.

**Relevance to our OS.** low. With no compatibility obligations, we can make "bind port N" a capability like any other and not encode trust in port numbers.

**Sources.** [S445], [S446], [S447], [S448], [S449], [S233]

### DM-077: `SIGPIPE` as default on writing to a closed socket/pipe

- **Category:** Networking APIs
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; language runtimes (Python, Rust)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Writing to a pipe with no readers, or to a stream socket whose peer has gone away, sends `SIGPIPE`, and the default action kills the process. If the process ignores (or handles) the signal, `write` instead fails with `EPIPE` [S450][S452].

**Why it was chosen.** For shell pipelines this is useful: in `producer | head`, the producer dies as soon as `head` exits, even if it never checks write errors. Simple filters therefore need no error handling for this case [unverified as a historical rationale].

**Why it is a problem.** The same rule applies to sockets, where a peer closing a connection is a routine event, not a reason to kill a server.
- Every network program must disable the signal: process-wide with `SIG_IGN`, per call with `MSG_NOSIGNAL` (Linux 2.2; POSIX only from 2008) [S451], or per socket with BSD's `SO_NOSIGPIPE` [S453].
- Ignoring the signal is a process attribute that affects all threads [S451], so a library cannot change it without changing behaviour for the whole program.
- Runtimes override it, which creates inconsistency. Python ignores `SIGPIPE` at startup and turns the error into `BrokenPipeError`, so a plain `script | head` produces a traceback [S454]. Rust's startup code has ignored `SIGPIPE` since 2014. When the issue was filed this was undocumented process-wide state, Rust libraries called from a non-Rust `main` behave differently from Rust applications, and the default is still debated [S455].

**Why it was not fixed.** POSIX mandates the behaviour for both pipes and sockets [S452], and shell pipelines depend on it.

**Alternatives that exist.** Per-call and per-socket opt-outs [S451][S453]. Runtimes that make `EPIPE` an ordinary error [S454]. A design could treat a closed reader as an error return everywhere and let pipeline tools exit on it.

**Cost of the alternatives.** Without the signal, every producer in a pipeline must check write errors, or it keeps running after its reader has gone. Python's `BrokenPipeError` noise and Rust's debate show that both defaults hurt someone [S454][S455].

**Relevance to our OS.** low. A native API without asynchronous signals returns an error. Only the POSIX layer needs to emulate `SIGPIPE`, for pipelines of ported tools.

**Sources.** [S450], [S451], [S452], [S453], [S454], [S455]

### DM-078: Network stack in the kernel vs user-space stacks

- **Category:** Networking APIs
- **Systems affected:** Linux, BSDs, Windows (in-kernel stacks); DPDK, Arrakis, IX, Google Snap, Fuchsia (netstack as a component); Linux XDP/AF_XDP as a middle ground
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Mainstream OSes run TCP/IP inside the kernel, behind the sockets API. The alternatives are:
- Kernel bypass: the application drives the NIC directly with poll-mode drivers, as in DPDK [S459].
- Control-plane/data-plane splits: the kernel sets up protection and hardware or virtualization enforces it, as in Arrakis and IX [S456][S457].
- A user-space networking service: Google's Snap [S458], or Fuchsia's netstack component [S462].

**Why it was chosen.** An in-kernel stack shares the NIC safely among many processes. It enforces isolation and firewalling, and it works with standard tools such as tcpdump and netstat [S460][S461]. When the sockets API was designed, NICs offered no hardware virtualization or multiple queues, so a shared, protected stack had to sit in the kernel [unverified as a sourced historical claim].

**Why it is a problem.** The two positions disagree.
- *Pro-bypass / user space.* Kernel crossings, copies and generic code paths limit performance. Arrakis reports 2 to 5x lower latency and 9x higher throughput than tuned Linux for a NoSQL store [S456]. IX keeps protection and reports up to 3.6x the throughput of a key-value store and more than 2x lower tail latency [S457]. Snap runs on over half of Google's fleet, gets more than 3x the Gbps per core of the kernel stack for RPC workloads, and can be upgraded without moving applications off the machine [S458]. Fuchsia treats the stack as a replaceable component [S462].
- *Pro-kernel.* Bypass gives up the OS's isolation and security mechanisms, breaks standard tooling, dedicates cores, and makes applications reimplement routing tables and higher-level protocols [S460]. Cloudflare estimated bypass would save only about 5 to 10% CPU for its proxy workload, while losing tcpdump, netstat statistics, iptables-based DDoS defences, and the ability to share the NIC with SSH [S461]. Cloudflare does use a partial bypass on a single RX queue for DDoS filtering, so even this operator uses both [S461].

**Why it was not fixed.** Not a defect to fix but an ongoing split. Linux's answer is XDP: programmable fast paths inside the kernel's driver path, with redirection to user space through AF_XDP sockets. In the 2018 XDP paper, true zero-copy AF_XDP delivery was still work in progress [S460].

**Alternatives that exist.** All of the above. XDP keeps the kernel in charge but, by its authors' own measurement, does not quite match DPDK's peak performance [S460].

**Cost of the alternatives.** Bypass and dataplane OSes need virtualized NICs or dedicated hardware queues and dedicated cores [S456][S457][S460], plus per-application stacks [S460]. A user-space service adds a hop between application and stack and depends on scheduling: Snap needed a kernel/user-space CPU scheduler co-design [S458]. A rewrite is a big job. Fuchsia replaced its Go netstack because of language and ownership concerns, and listed reaching performance parity as a risk [S462].

**Relevance to our OS.** high. As a capability-leaning, possibly microkernel-style OS, we will probably host the stack as a service. Snap, IX and Arrakis show this can perform well, but only with careful IPC and scheduling design.

**Sources.** [S456], [S457], [S458], [S459], [S460], [S461], [S462]

## 13. Graphics, input and window systems

### DM-079: X11 security model: any client can read input and contents of others

- **Category:** Graphics, input and window systems
- **Systems affected:** X Window System (X11, Xorg); Linux/BSD desktops on X
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** X checks access only when a client connects (host address or auth cookie). After that, a client can manipulate any resource on the server. Scheifler and Gettys judged per-resource access control not worth its performance cost, because accidental interference between clients seemed very unlikely [S835]. Any connected client can therefore read the keyboard, capture other windows and send synthetic events.

**Why it was chosen.** The 1986 X paper (which describes X10) addressed accidental interference, not hostile clients, and access control was by host address [S835]. Openness let window managers and other desktop managers be ordinary clients, keeping policy outside the server [S835].

**Why it is a problem.** Rutkowska showed that an unprivileged `xinput test` logs every keystroke, including a root password typed into `su` in another terminal. It works across user accounts and SELinux domains, with no exploit needed, because it is how X is designed [S831]. Screen capture and keystroke injection work the same way. OpenSSH warns that trusted X11 forwarding lets a remote host perform keystroke monitoring [S830]. The EWS authors argue that X11 and Windows inherited Xerox Alto-era trust assumptions [S858].

**Why it was not fixed.** The SECURITY extension (1996) added "untrusted" clients that cannot grab the keyboard, read trusted windows or use others' resources. It also breaks ICCCM focus mechanisms and leaves property access implementation-dependent [S826]. ssh uses it for untrusted forwarding, with a 20-minute default timeout [S830]. XACE (NSA, 2006–2009) added LSM-style hooks, but its own spec admits incomplete event-delivery coverage and unsecured GLX/direct rendering [S827]. In a comment on her post, Rutkowska reported that XACE was apparently unused, and that SELinux's sandbox used a nested X server instead [S831]. Any real fix breaks window managers, screenshot tools and input methods that depend on the open model.

**Alternatives that exist.** Under Wayland the compositor is the display server. It routes input itself from its scenegraph [S829], so clients do not see other clients' input. Qubes isolates GUI per VM domain [S831]. EROS EWS was a capability-based trusted window system in under 4,500 lines, about 10x smaller than Trusted X [S858].

**Cost of the alternatives.** Screenshots, screencasting, global shortcuts, input capture and remote desktop each need a mediated interface. On Linux these became xdg-desktop-portal D-Bus APIs with desktop-specific backends [S832], which feeds the fragmentation in DM-084. EWS stayed a research system [S858].

**Relevance to our OS.** high — the compositor is our input trust boundary; deciding who may observe or inject input (accessibility, automation, capture) has to be a capability design up front.

**Sources.** [S826], [S827], [S829], [S830], [S831], [S832], [S835], [S858]

### DM-080: X11 network transparency and server-side rendering

- **Category:** Graphics, input and window systems
- **Systems affected:** X Window System
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** X is a protocol over "any reliable byte stream" [S834]. Clients ask the server to draw lines, text and images into server-side resources (windows, pixmaps, GCs, fonts). The server holds the device-specific code [S834][S835].

**Why it was chosen.** The goals were network transparency (an application on one machine displays on another of any architecture), device independence, and mechanism rather than policy [S835]. High-level drawing requests are compact on the wire. The protocol is asynchronous and was designed for 5–50 ms round trips. Applications need no rewriting or relinking for new display hardware [S835]. For time-shared machines and 1980s workstations this was a strong design.

**Why it is a problem.** Rendering moved to the client. Toolkits draw locally with GPUs and hand over finished buffers, which left the X server, in Daniel Stone's words, a "terrible, terrible, terrible" IPC bus between client and compositor [S833]. The rendering paths every application uses (MIT-SHM, DRI2) do not work over the network, and Stone said X's middleman role hits its worst case over the Internet [S833]. In the composited X architecture every frame goes through the server twice, from client to server and from compositor back through the server [S829].

**Why it was not fixed.** Backward compatibility froze the core protocol. Features moved to extensions and clients instead. Stone counted four input stacks, five display-management extensions and four buffer models [S833].

**Alternatives that exist.** Wayland leaves remote rendering out of scope and recommends a separate remoting server or a compositor with remoting [S828]. Clients render into their own buffers [S829]. waypipe proxies Wayland like `ssh -X`, compressing buffers with lz4/zstd or video codecs [S836]. Stone proposed VNC-like compositor-to-compositor transport [S833].

**Cost of the alternatives.** Per-application remoting is no longer built in. waypipe is an add-on: it does not filter protocols, is not hardened against malicious peers, and contains "plenty of unsafe Rust" [S836]. Users who relied on `ssh -X` lost a convenience, and commenters on the Stone report argued that per-application remote display is still a legitimate use case that X serves [S833]. Classed right-at-the-time-wrong-now rather than mistake because the 1986 constraints (no client GPUs, slow networks, shared hosts) were real [S835].

**Relevance to our OS.** medium — we target local QEMU, but whether remote display is a protocol property or a proxy affects the compositor protocol design.

**Sources.** [S828], [S829], [S833], [S834], [S835], [S836]

### DM-081: Windows win32k: window manager and GDI in kernel mode

- **Category:** Graphics, input and window systems
- **Systems affected:** Windows NT 4.0 and later
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Since NT 4.0 (1996), the Window Manager (USER), GDI and graphics drivers run in kernel mode as executive services (win32k), reached by system call from every GUI thread [S837].

**Why it was chosen.** In NT 3.51 these components ran in CSRSS and were called through Fast LPC, paired threads and shared buffers. Microsoft's white paper gives the motivation. A client/server transition cost 60–70 µs against 4–5 µs for a kernel transition on a Pentium 100. Moving into the kernel saved 256 KB–1 MB of working set and fixed visible slowness, such as Pinball being "virtually unplayable" on NT 3.51 [S837]. Microsoft argued reliability was unchanged because a CSRSS crash already brought down the system, and claimed security was unaffected [S837].

**Why it is a problem.** The security claim did not hold up. Win32k exposes over a thousand system calls to every process. Forshaw says NT system call bugs are "nothing compared to Win32k", and win32k is a standard sandbox escape. He traces it to the NT 4 performance decision, made before Microsoft's security push. The leaked Hacking Team Chrome chain used a win32k bug to break out [S840]. Win32k elevation bugs keep being exploited in the wild, for example CVE-2021-1732, which is in the CISA KEV catalog [S841]. The white paper's own reasoning ("if application code can crash the system, NT has a bug") puts a very large parser in the kernel's trusted computing base [S837].

**Why it was not fixed.** The Win32 GUI API and its drivers are the Windows compatibility surface. Microsoft instead added opt-in mitigations: `DisallowWin32kSystemCalls` from Windows 8 [S838], and a Win32k syscall filter used by Edge in Windows 10 Anniversary Update [S840].

**Alternatives that exist.** The NT 3.51 user-mode design itself [S837]. User-space display servers and compositors (X servers, Wayland compositors) [S829]. Chrome renderers run with win32k locked down, which "significantly reduces the kernel attack surface" [S839].

**Cost of the alternatives.** User-mode GUI paid real context-switch and memory costs on 1990s hardware [S837]. Retrofitting lockdown is expensive. Chrome's renderer lockdown took about two years, including replacing GDI font code with DirectWrite. Extending it to the Flash (PPAPI) process took almost 10 more months, including HDCP rework. That lockdown shipped first only on Windows 10, because of unexplained instability on 8.1 likely caused by injected third-party code [S840].

**Relevance to our OS.** high — the compositor and graphics drivers should be user-space services that sandboxed processes (including the POSIX layer) can reach only through granted handles; per-frame IPC costs on modern hardware are the open question.

**Sources.** [S829], [S837], [S838], [S839], [S840], [S841]

### DM-082: Window messages across privilege boundaries (shatter attacks, UIPI)

- **Category:** Graphics, input and window systems
- **Systems affected:** Windows NT family (pre-Vista fully; Vista+ partially mitigated)
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** Any process on a desktop can get a window handle and post messages to any window on that desktop. Some messages carry pointers or callback addresses. Before Vista, services ran in Session 0 alongside the interactive user's applications [S845].

**Why it was chosen.** Window messages were the GUI's general event and IPC mechanism, and the Win32 API dates to NT 3.1 (1993) [S844]. All windows on a desktop were assumed to share one trust domain. The EWS authors make the general point that window systems also act as IPC mechanisms, which makes them part of security policy [S858].

**Why it is a problem.** Messages carry no authenticated sender. Paget's 2002 "shatter" paper described the class and called it unfixable by a patch [S844]. In his worked example, a guest user found an edit box in a virus scanner's UI running as LocalSystem. He lifted its length limit with `EM_SETLIMITTEXT`, pasted shellcode in, then sent `WM_TIMER` with the shellcode's address as the callback [S853]. CVE-2002-1230 is a concrete case: a local user sent WM_COPYDATA and then WM_TIMER to the NetDDE agent and ran code as LocalSystem (MS02-071) [S854].

**Why it was not fixed.** The message model is the Win32 API. Microsoft fixed its own services one at a time [S854]. In Vista it added two structural changes: services were isolated in a non-interactive Session 0 [S845], and User Interface Privilege Isolation (UIPI) blocks messages from lower-integrity senders. UIPI is leaky by design. Certain messages below `WM_USER` always pass [S842][S843]. Applications can open holes with `ChangeWindowMessageFilter(Ex)` [S842], and Microsoft shipped compatibility shims that re-enable custom messages [S843]. MSRC classes UAC as defence-in-depth rather than a security boundary, and integrity levels are not listed as boundaries [S846].

**Alternatives that exist.** Microsoft's own advice is to use a secured IPC mechanism instead of window messages [S843]. In Wayland, input routing is decided by the compositor rather than by clients posting events to each other [S829]. EWS proposed a capability-mediated window system [S858].

**Cost of the alternatives.** Session 0 isolation broke interactive services [S845]. UIPI needed per-application filter exceptions and shims [S843], so it is weaker than a boundary [S846].

**Classification note.** Reclassified from right-at-the-time-wrong-now to mistake. The flaw did not come from a later change in constraints. NT 3.1 already ran privileged services and user applications on one desktop, and Paget dates the vulnerability class to 1993 [S844][S845].

**Relevance to our OS.** high — the GUI protocol must not become an ambient, unauthenticated IPC channel between clients of different trust.

**Sources.** [S829], [S842], [S843], [S844], [S845], [S846], [S853], [S854], [S858]

### DM-083: No frame synchronization in X (tearing); "every frame is perfect"

- **Category:** Graphics, input and window systems
- **Systems affected:** X Window System; Wayland (as the alternative)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** In core X, clients issue separate drawing requests (rectangles, pixmaps, text) that the server applies to the visible window as they arrive. There is no notion of a complete frame and no vblank-synchronized presentation [S833]. The server is not required to keep window contents, and clients redraw on Expose events [S835].

**Why it was chosen.** Memory was scarce. The X paper notes that one 1024×1024 8-plane image takes a megabyte. It rejects both server display lists and off-screen backing as too slow or too costly, and argues that applications can redraw from their own data [S835]. Double-buffering every window was not affordable.

**Why it is a problem.** Users see partly drawn frames and tearing. Synchronization was retrofitted in layers: compositing managers, DRI2, then the Present extension (2013) for VBLANK-synchronized updates [S848]. Even Present deliberately accepts "an occasional tearing artifact" so that one application's queued blit does not block another's [S847]. Stone describes DRI2 as limited by the core protocol [S833].

**Why it was not fixed.** The core protocol's immediate-mode semantics could not change. Each fix had to be an opt-in extension that toolkits, drivers and compositors all adopted [S833][S848].

**Alternatives that exist.** In Wayland, surface state is double-buffered. attach, damage and region changes stay pending until `wl_surface.commit` applies them atomically, and frame callbacks pace redraws [S849]. The compositor schedules page flips [S829]. A Mutter developer cites Wayland's motto "every frame is perfect" for this [S850].

**Cost of the alternatives.** Atomicity adds latency and new stall modes. If a client's GPU work is late, the compositor can miss a refresh. Making Mutter wait for client buffers before applying transactions took almost two years of on-and-off work [S850]. Games and drawing tablets want tearing for lower latency, so Wayland added `tearing-control-v1` to opt back in, which compositors may ignore [S851].

**Relevance to our OS.** high — the compositor protocol needs atomic commits, explicit fences and a defined latency opt-out from the start.

**Sources.** [S829], [S833], [S835], [S847], [S848], [S849], [S850], [S851]

### DM-084: Wayland's minimal core and protocol-extension fragmentation

- **Category:** Graphics, input and window systems
- **Systems affected:** Wayland ecosystem (GNOME/Mutter, KDE/KWin, wlroots compositors, Weston, toolkits)
- **Classification:** contested
- **Confidence:** medium

**What the design is.** The Wayland core only moves buffers, input and surfaces. One process combines display server, window manager and compositor, and rendering is left to clients ("whatever you want it to be") [S828]. Desktop features come from extension protocols governed by wayland-protocols, whose members are projects, not individuals [S852][S2500]. Under the current rules, `xdg`/`wp` protocols need 3 member ACKs and 3 open-source implementations, and a single NACK blocks them. `ext` protocols need 2 ACKs (1 in the earlier rules) plus one open-source client and one server. Since late 2024 an experimental `xx` tier is merged after a two-week review unless NACKed, and a NACK must be grounded in technical reasoning [S2500].

**Why it was chosen.** X showed the cost of a protocol frozen in a central server, and it accumulated four input stacks and several overlapping buffer and display models anyway [S833]. Leaving rendering out keeps Wayland small and feasible [S828]. A small core lets desktops differ in policy. The governance treats a NACK as the last resort against protocols that would harm the ecosystem [S2500].

**Why it is a problem.** The two positions read the same facts differently.
- *Position B (it under-delivers).* Behaviour that applications rely on varies by compositor. Server-side decorations (`xdg-decoration`) are still an unstable protocol, the compositor decides the mode, and Mutter and Weston do not implement it, so applications on GNOME must draw their own frames [S857]. Tearing control is likewise absent from Mutter [S851]. Screenshots, screencasts, global shortcuts and remote desktop moved to xdg-desktop-portal, with separate GNOME, KDE and wlroots backends [S832]. waypipe notes that compositor-specific screenshot protocols pass through unfiltered [S836]. In September 2024 Valve's Joshua Ashton started "frog-protocols" as a parallel repository, saying basic protocols wait "months, to years" (citing FIFO presentation, needed for VSync) [S855][S856].
- *Position A (minimal core plus governance is right).* Differences between compositors are partly deliberate policy choices that a small core allows [S828][S852]. X fragmented into overlapping extensions despite having one server [S833]. Portals give a de facto cross-desktop layer [S832]. The process also responded. The upstream `fifo-v1` protocol was merged in October 2024 and shipped in wayland-protocols 1.38 [S2501], and the experimental tier was adopted that same month [S2500].

**Why it was not fixed.** The process is consensus by design, and the veto that protects diversity also slows convergence. When frog-protocols launched, Mike Blumenkrantz's proposal for an "experimental" stage drew mixed reactions [S856]. It was adopted in October–November 2024 together with limits on NACK use [S2500]. Whether this fixes the speed problem is not yet established.

**Alternatives that exist.** *Position A:* the FAQ and governance docs [S828][S852][S2500], and the evidence that X fragmented despite one server [S833]. *Position B:* frog-protocols, and an Ashton merge request to support `frog-fifo-v1` in Mesa's Wayland WSI [S855][S856]. Some Wayland developers objected that adding support without upstream consensus is undesirable. Others said an iterative model gives useful feedback [S856].

**Cost of the alternatives.** A rich mandated core (X, win32k) freezes policy and grows a monolith (DM-080, DM-081). Fast-moving side repositories risk forks and duplicate protocols that clients must support during transition [S855].

**Relevance to our OS.** high — with one compositor and no ecosystem we can define a richer core, but must decide what is protocol versus policy before third-party clients depend on it.

**Sources.** [S828], [S832], [S833], [S836], [S851], [S852], [S855], [S856], [S857], [S2500], [S2501]

## 14. Configuration, observability and system management

### DM-085: Environment variables as implicit global configuration

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows (per-process environment, but with a thread-safe API)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Each process has a flat list of `NAME=value` strings inherited across `fork`/`exec`. Any library may read any variable at any time with `getenv`, and many do (locale, time zone, loader paths, DNS, proxy settings, character conversion).

**Why it was chosen.** It is the simplest possible way to pass optional configuration through a chain of programs without changing argument conventions, and it costs nothing on a PDP-11. Inheritance is useful: setting `PATH` or `TERM` once configures a whole session.

**Why it is a problem.** (1) Thread safety: glibc marks `setenv`/`unsetenv` MT-Unsafe and POSIX does not require reentrancy [S234]. Rust made `std::env::set_var` `unsafe` in the 2024 edition, because libc `getenv` is unsynchronized and libraries do not document which functions read the environment (DNS lookups do), so no program can guarantee soundness once threads exist [S235][S236]. glibc's attempt to make `getenv` thread-safe "in more cases" broke applications that managed `environ` themselves and needed a compatibility fix [S237]. (2) Security: the environment is attacker-chosen input to privileged programs; the loader must strip a long list of variables in secure-execution mode [S127], and PwnKit worked by smuggling `GCONV_PATH` back in [S206]. `LD_PRELOAD` silently injects code into every child [S127]. (3) Hidden coupling: Poettering cites heavy dependence on environment variables as one reason shell-based init scripts are unpredictable [S233]; `run0` sessions take the service manager's environment rather than the caller's, copying only a few variables such as `TERM` [S211].

**Why it was not fixed.** `environ` is a public global array in POSIX and C programs write it directly, so any locking scheme breaks someone [S237].

**Alternatives that exist.** Explicit configuration passed at spawn (Rust's recommended `Command::env` for children [S235]); service managers that construct the environment from configuration rather than inheritance [S211]; on Windows, Rust documents `set_var` as always sound, even with multiple threads [S235].

**Cost of the alternatives.** Losing ambient inheritance means every launcher must plumb configuration explicitly; the POSIX layer will still need an `environ` for the C compiler and its tools.

**Relevance to our OS.** high — the native process-start ABI decides whether an inherited mutable string table exists at all.

**Sources.** [S234], [S235], [S236], [S237], [S127], [S206], [S211], [S233]

### DM-086: Windows registry

- **Category:** Configuration, observability and system management
- **Systems affected:** Windows NT and successors
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A system-defined hierarchical database of keys and typed values, used by the OS and applications for configuration [S238][S239], stored in binary hive files and parsed in the kernel [S241].

**Why it was chosen.** Raymond Chen lists the problems with INI files: no Unicode, per-file rather than per-key security, lost updates from concurrent writers, whole-file parsing on each access, a 32 KB limit, only two levels, string-only values, a default location in the admin-only Windows directory, and hard central administration. He says the registry "tried to address these concerns" [S240]. These are real problems, and a central store also enables central administration.

**Why it is a problem.** (1) A shared mutable namespace for all programs: apps that assumed they could write `HKLM\Software` broke under non-admin users, so Vista added registry virtualization, redirecting writes to a per-user VirtualStore, which Microsoft calls interim and which breaks registry-as-IPC between a service and an app [S228]. (2) Kernel attack surface: a 20-month Project Zero audit (May 2022–December 2023) led to 50 CVEs. Hives are a binary format parsed entirely in kernel by 30-year-old C, and since Vista any user can load an arbitrary hive via `RegLoadAppKey` [S241]. (3) Opaque state: data is not owned by an application. Microsoft notes that uninstallers traditionally cannot clean `HKCU` for logged-out users [S2502], and warns not to edit data that is not yours [S238].

**Why it was not fixed.** Every Win32 application and Group Policy depends on it. The fix has been isolation layers: virtualization [S228], and for packaged (MSIX) apps a per-package `registry.dat` merged into `HKLM\Software`, with `HKCU` writes copied to a per-app location that is deleted on uninstall [S2502].

**Alternatives that exist.** Per-application text files (DM-087), which Chen says reopen INI problems once programs write them [S240]; declarative, immutable system configuration (NixOS, DM-087).

**Cost of the alternatives.** File-based configuration loses typed values, per-key ACLs and safe concurrent writes [S240].

**Relevance to our OS.** medium — argues for typed, per-component configuration owned by the component, and against a kernel-resident shared database.

**Sources.** [S238], [S239], [S240], [S241], [S228], [S2502]

### DM-087: Per-program text configuration syntax in `/etc`

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Each program defines its own configuration file format, usually line-based text in `/etc`, edited by hand or by ad-hoc scripts. There is no common schema, type system, validation, or transaction.

**Why it was chosen.** Text works with every editor and with `diff`, `grep` and version control, needs no shared infrastructure, and each program can evolve its own format. Chen notes that text configuration files that are only read sidestep many INI problems [S240].

**Why it is a problem.** Errors are syntactic and semantic and found late. Yin et al. studied 546 real misconfigurations from a commercial storage system and from CentOS, MySQL, Apache and OpenLDAP. In 70–85.5% of cases a parameter was set wrongly. Of those, 38–54% were illegal values a checker could catch, and 12–30% were inconsistencies between parameters [S242]. The study covers configuration in general, not text formats in particular. It argues for schemas and validation rather than against text as such. Automated management needs a parser per format; Augeas exists to turn many native formats into a tree because editing them with sed/awk is unsafe [S243]. Updates are destructive in-place edits, so rollback and reproducibility are hard. Putting `/etc` under version control does not fix this, because configuration is coupled to the installed package versions [S244].

**Why it was not fixed.** No central authority owns the format of third-party programs; every attempt at a common store (GConf/dconf, Elektra) covers only some programs [unverified].

**Alternatives that exist.** NixOS builds the whole system configuration from a purely functional specification, making it immutable, reproducible and rollback-able [S244]. Registry-style typed stores (DM-086). Config-editing layers (Augeas) [S243].

**Cost of the alternatives.** NixOS stores every package under hashed store paths with no `/usr` or `/lib`, so hard-coded paths must be patched per package. Mutable state such as `/etc/passwd`, `/etc/resolv.conf` and `/var` stays outside the functional model [S244]. Typed stores lose the universality of text and need tools to inspect [S240].

**Relevance to our OS.** medium — we control every native component, so a single schema-checked configuration model is feasible; the POSIX layer will still meet text configs.

**Sources.** [S242], [S243], [S244], [S240]

### DM-088: Init systems: shell-script SysV init vs systemd

- **Category:** Configuration, observability and system management
- **Systems affected:** Linux distributions; comparable: Solaris SMF, macOS launchd, s6/runit
- **Classification:** contested
- **Confidence:** medium

**What the design is.** SysV init runs ordered shell scripts per runlevel; each script starts a daemon that daemonizes and is tracked by a PID file. systemd replaces this with declarative unit files, dependency-based parallel start, socket activation and cgroup-based tracking, and has grown into a suite covering logging, login and other services.

**Why it was chosen.** SysV init was simple, transparent and editable in a language every admin knew. Poettering made the case for systemd in 2010: startup was serialized, the shell was slow (`grep` invoked at least 77 times in `/etc/init.d`), double-forked daemons escaped supervision, and shell scripts were fragile because they depend on environment variables. With socket activation, the manager creates listening sockets before starting daemons, which removes most ordering [S233].

**Why it is a problem.** The two positions disagree, and both are held by experienced practitioners.
- *Position A (systemd proponents).* Scripts cannot supervise, restart or reliably track services, and they serialize boot [S233]. Solaris SMF had already shown a declarative service model with dependencies and instances [S250].
- *Position B (critics).* systemd integrates too much into one project around PID 1, pushes other software to depend on its interfaces, and ignores portability to BSD and other Unixes. The s6 author calls it "technically unsustainable" [S249] and offers small supervision tools in the daemontools tradition instead [S251]. He also concedes that low-level userspace needed improving, and that s6 shares that goal [S249]. Note that this critic maintains a competing project. In Debian's 2014 dispute, Ian Jackson pushed for "loose coupling", so that no package could depend on a specific PID 1 [S246].

**Why it was not fixed.** It is a governance dispute as much as a technical one. Debian's technical committee chose systemd as the default for jessie [S245], after a 4:4 tie broken by the chair's casting vote [S246]. A 2014 GR on coupling resolved that no GR was required [S247]. A 2019 GR with seven proposals plus "further discussion" chose "systemd but we support exploring alternatives" [S248].

**Alternatives that exist.** SMF [S250], launchd (whose socket-activation model systemd copies [S233]; launchd's own documentation was not fetched), s6/runit supervision [S251], and systemd itself.

**Cost of the alternatives.** Minimal supervisors leave dependency ordering to separate tools (s6-rc in the s6 case) and offer fd-holding rather than full socket activation [S251]. An integrated manager concentrates complexity and policy in one project [S249].

**Relevance to our OS.** high — we will need a service manager; the contested question is how much belongs in it and which component tracks service lifetime.

**Sources.** [S233], [S245], [S246], [S247], [S248], [S249], [S250], [S251]

### DM-089: Unstructured text logging (syslog)

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, network devices
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Programs send free-form text lines tagged with facility and severity to a syslog daemon over a local socket or UDP; the daemon appends them to text files. RFC 3164 (2001) only documented existing practice [S252].

**Why it was chosen.** Text is human-readable and tool-independent, and a fire-and-forget UDP sender never blocks the program that is logging [S252]. That early systems had little to log and one administrator to read it is plausible but [unverified].

**Why it is a problem.** BSD syslog timestamps have no year or time zone, and messages were limited to 1024 bytes over UDP. The RFC's security section lists problems with authentication, message forgery and sequenced delivery [S252]. RFC 5424 notes that implementations shared little beyond the PRI field [S253]. The journal design document lists the consequences. Client-supplied metadata such as identity and PID is not verified. Analysis depends on fragile regular expressions that turn log strings into de facto ABI. Lookups are O(n), and log manipulation after an intrusion is usually found only by chance [S254]. The original announcement cited the 2011 kernel.org break-in as motivation for hash-chaining [S256].

**Why it was not fixed.** RFC 5424 added structured data, MSGID and RFC 3339 timestamps [S253], but the message body stays free text and old formats persist. The journald replacement was contested. Its initial FAQ said the binary format would not be standardized [S256]. Today's documentation says the code is the only authoritative description of the format and asks readers to use the C API rather than parse files directly [S255].

**Alternatives that exist.** systemd-journald: indexed, structured key/value entries, trusted fields derived from kernel credentials, Forward Secure Sealing, corruption handling by rotation [S254][S255]. Windows ETW: kernel-level structured tracing with providers and consumers, enabled dynamically in production [S257].

**Cost of the alternatives.** Binary logs need specific tools to read. Early critics objected to losing standard Unix text tools [S256, comments: weak]. Structured schemas need governance. Text logs keep real advantages (universal tooling, readability without special software), which is why this is classed as a tradeoff.

**Classification note.** Changed from right-at-the-time-wrong-now to tradeoff. The entry gave no sourced evidence about the constraints at the time, and the alternative (binary structured logs) has costs that are still disputed [S255][S256].

**Relevance to our OS.** medium — a new OS can make logs structured, with kernel-attested sender identity, from day one.

**Sources.** [S252], [S253], [S254], [S255], [S256], [S257]

### DM-111: cgroups v1 multiple hierarchies and the v1→v2 redesign

- **Category:** Configuration, observability and system management
- **Systems affected:** Linux
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Control groups group processes for resource control. v1 allowed many independent hierarchies, each with any set of controllers, and threads of one process could sit in different groups; v2 has a single unified hierarchy, processes only in leaves, and a defined delegation model [S152].

**Why it was chosen.** Flexibility: each controller could be organized independently [S152]. cgroups were also adopted by init systems for process tracking because double-forked daemons escape the PID tree [S233] (see DM-100).

**Why it is a problem.** The kernel's own v2 documentation lists the v1 failures: multiple hierarchies were impractical (a controller could be in only one, could not be moved once populated, and userland ended up managing several near-duplicate trees); per-thread placement blurred app API and system management; processes in a parent competed with child groups with controller-specific, sometimes undefined, results; delegation was loose, racy and exposed kernel internals; controllers used inconsistent names, units and defaults [S152].

**Why it was not fixed.** It was fixed only by a parallel second interface; v1 and v2 hierarchies coexist, and moving controllers between them is discouraged [S152], so tools had to support both for years.

**Alternatives that exist.** cgroup v2's single hierarchy, no-internal-process rule, and namespace-based delegation [S152].

**Cost of the alternatives.** v2 constrains layout (no processes in inner nodes, with exceptions for the root and threaded subtrees) [S152].

**Relevance to our OS.** medium — resource accounting and process grouping should be designed together with the process model rather than bolted on.

**Sources.** [S152], [S233]

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

**Alternatives that exist.** Android A/B: the update is written to the inactive slot while the system runs. The only downtime is the reboot. If the new slot fails to boot, or is not marked successful after several attempts, the bootloader falls back to the old one. Android 10 replaced legacy A/B with Virtual A/B [S134]. OSTree: complete, content-addressed trees, deployed as hardlinks and switched atomically; `/etc` gets a 3-way merge and `/var` is shared [S135]. NixOS: immutable store paths and atomic switching between system generations [S129][S130].

**Cost of the alternatives.** Extra storage (two slots, or several deployments); local changes must be confined to `/etc` and `/var` [S135]; a boot is still needed to activate the update; and package-level flexibility is lost or has to be layered back on. OSTree contrasts itself with plain image replication, which is reliable and predictable but often keeps no persistent writable state [S135].

**Relevance to our OS.** high — we can define the system image as immutable and content-addressed from the start, with state separated, which is very hard to retrofit.

**Sources.** [S129], [S130], [S134], [S135], [S136], [S137]

### DM-091: Replacing in-use files requires reboot (Windows) / stale libraries in running processes (Unix)

- **Category:** Boot, update and recovery
- **Systems affected:** Windows (share modes, delayed rename), Linux/Unix (unlink-while-mapped), all systems for the kernel itself
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Windows typically refuses to replace a file another process has open or mapped (cross-ref DM-044). Installers schedule renames for the next boot via `MOVEFILE_DELAY_UNTIL_REBOOT`, recorded in the `PendingFileRenameOperations` registry value [S138]. Unix lets the new file replace the name immediately, while running processes keep the old inode mapped.

**Why it was chosen.** Windows' locking stops a program from seeing a file change under it mid-run. Unix's unlink semantics let updates proceed without coordination and never block on a busy file.

**Why it is a problem.** *Windows:* Microsoft says the primary reason installs require a restart is that files being updated are in use [S139]. Delayed operations run only at boot and require admin rights. They can also fail unnoticed: because the move happens after the caller has exited, the API's return value only reports whether the registry entry was written [S138]. Restart Manager (Windows Installer 4.0+) shuts down and restarts apps to free files, but critical system services still need a reboot [S139]. *Unix:* there is no forced reboot, but after a library security update, running daemons keep executing the *old, vulnerable* code with no indication. Tools like needrestart scan processes for deleted mappings (via `/proc/<pid>/maps`) and guess which services to restart. For interpreted languages, where the source files are read at run time, they admit the heuristic is imperfect [S140]. systemd recommends doing updates in a special offline boot mode to avoid conflicts between the libraries and services already running and those on disk [S136]. Either way, "is the update actually in effect?" has no reliable answer.

**Why it was not fixed.** These are deep filesystem semantics (open-file locking vs inode lifetime) and process models. Live patching only covers the kernel, and only narrowly: just traceable functions can be patched, and a patch can stay in transition indefinitely if tasks block it [S141].

**Alternatives that exist.** Restart coordination (Restart Manager) [S139]; offline update boot [S136]; A/B images, where the switch happens at a planned reboot [S134]; kernel livepatch for critical fixes [S141]. Hot code loading (e.g. Erlang) [unverified, not researched here].

**Cost of the alternatives.** Each one trades downtime against consistency: an offline or A/B reboot is predictable but disruptive; restart coordination needs apps to save and restore state [S139]; livepatching is limited and needs reliable stack traces [S141].

**Relevance to our OS.** medium — with immutable images (DM-090), we need an explicit model for which running components are on which version and how services are restarted; a microkernel-style restartable service design could make this cheaper.

**Sources.** [S134], [S136], [S138], [S139], [S140], [S141]

### DM-092: Firmware and boot-chain complexity (UEFI, BootHole, LogoFAIL)

- **Category:** Boot, update and recovery
- **Systems affected:** UEFI PCs and servers (x86 and some Arm), GRUB2/shim-based Linux, Windows Boot Manager
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Pre-OS firmware (UEFI) is a large runtime with drivers, filesystems, network stacks and image parsers. It runs with the highest privilege, before any OS protection exists. Secure Boot verifies signatures of executables in the chain (shim → GRUB → kernel), with a revocation list (dbx) kept in firmware variables [S142].

**Why it was chosen.** It provides a vendor-neutral, extensible boot environment for an ecosystem of many OEMs, IBVs and OSes, plus a third-party CA so that non-Microsoft OSes can boot under Secure Boot [S142].

**Why it is a problem.** *Signed code parsing unsigned input:* BootHole (CVE-2020-10713) was a buffer overflow in GRUB2's parsing of `grub.cfg`, a plain-text file that is typically not signed. It gave attacker code execution before the OS, despite Secure Boot, on virtually every Linux distribution, with more than 80 shims known to be affected [S142]. *Privileged parsers of attacker-controlled media:* LogoFAIL is a set of flaws in the image parsers firmware uses for boot logos. They span the major IBVs (AMI, Insyde, Phoenix; three CVEs) and let an attacker with local privileged access use a crafted image to change privileged UEFI settings [S143]. *Revocation is the weak point:* fixing BootHole required coordinated GRUB, shim, distro and dbx updates. Applying dbx before fixed bootloaders made machines unbootable. An earlier dbx revocation (of a Kaspersky bootloader) had bricked devices, and some distro BootHole fixes led to unsuccessful reboots [S142]. For BlackLotus (CVE-2023-24932), Microsoft staged revocation over years (2023 → enforcement not before January 2026). Once applied it cannot be undone while Secure Boot stays on, it breaks old recovery media, and some firmware fails to apply the DB/DBX updates [S144].

**Why it was not fixed.** Firmware is supplied by many separate parties: chip vendor, IBV, OEM, and OS. Shared IBV code spreads bugs across OEMs [S143], and revocation must never brick installed devices [S142][S144].

**Alternatives that exist.** Minimal, verifying boot ROM stubs: ChromeOS's read-only firmware verifies read-write firmware and the kernel with a permanently stored key in write-protected firmware [S146]. Oxide's `phbl` runs from the reset vector, puts the CPU into long mode, and directly loads an embedded host OS package [S145]. Unified signed boot images (kernel, initrd and cmdline signed together) remove unsigned config parsing [S149].

**Cost of the alternatives.** Minimal firmware gives up the generic "boot any OS from any device" property and needs platform-specific bring-up. The Oxide and ChromeOS approaches depend on the vendor controlling hardware and software together. The generic, multi-vendor UEFI ecosystem is the feature being paid for, which is why this is a tradeoff rather than a plain mistake.

**Relevance to our OS.** medium — on QEMU `virt` aarch64 we can choose a minimal path (direct kernel load, or a tiny verified stub); for real hardware we will inherit whatever firmware exists, so a small trusted boot surface is the controllable part.

**Sources.** [S142], [S143], [S144], [S145], [S146], [S149]

### DM-093: Verified/measured boot absent (or partial) by default

- **Category:** Boot, update and recovery
- **Systems affected:** General-purpose Linux distributions (Secure Boot covers only bootloader/kernel), BSDs; contrast ChromeOS, Android, (Windows/macOS not researched here)
- **Classification:** contested
- **Confidence:** medium

**What the design is.** On most general-purpose Unix-like systems, nothing verifies the OS beyond the kernel. Where Secure Boot is used, the locally generated initrd is unsigned, and typically no user-space components are measured (beyond what IMA measures) [S149].

**Why it was chosen.** The systems are mutable and locally administered: the initrd is generated on the machine, the owner edits anything, and the root filesystem changes during package updates (DM-090). Verifying a mutable root filesystem is not meaningful.

**Why it is a problem.** An attacker with brief offline access can modify the initrd (which unlocks disk encryption), and that persists across reboots without detection; Poettering calls the initrd "trivial to attack and modify offline" [S149]. In contrast, ChromeOS verifies from a read-only root of trust through firmware and kernel, then verifies each root-filesystem block on access against a hash tree, so a persistent modification is detected at boot and recovery can be triggered [S146]. dm-verity's design states that the root hash must be trusted because nothing else authenticates it [S147]. Android has enforced verified boot strictly since 7.0, with rollback protection so that an exploit cannot persist by downgrading [S148].

**Why it was not fixed.** Verified boot needs an immutable, vendor-built (or owner-signed) image and a key-management story. These conflict with the traditional mutable distro model and with owner control. Work such as signed unified kernel images and TPM-sealed secrets is recent [S149].

**Alternatives that exist.** ChromeOS (read-only firmware stub, signed kernel, dm-verity, recovery to a known-good image) [S146]; Android Verified Boot / AVB (chain from hardware root of trust, rollback indexes) [S148]; UKIs plus TPM measured boot on Linux [S149].

**Cost of the alternatives.** *Position A (security):* verified boot should be the default; ChromeOS argues owner control can be kept because the user can always reset to a known-good image, can choose to proceed with a development install, and later-stage verification is independent so developers can run their own builds [S146]. *Position B (owner control):* the FSF accepts Secure Boot only if users can authorize the software they want; it calls manufacturer-controlled restrictions "Restricted Boot" and demands that owners be able to disable them or install a free OS of their choice [S150]. The disagreement is therefore over who holds the keys more than over verification itself. Technically, verified boot also costs rollback-protection complexity (ChromeOS admits hash-partition replay is hard to prevent [S146]), a recovery path, and immutability of the system image. ChromeOS also states that verified boot cannot protect data the running system must modify (the stateful partition), which can still be used to subvert verified code [S146].

**Relevance to our OS.** medium — pairing an immutable image (DM-090) with dm-verity-style verification is cheap if designed in; who holds the keys is a policy decision for the architect.

**Sources.** [S146], [S147], [S148], [S149], [S150]

## 16. Text, encoding and locale

### DM-094: Case-insensitive file names and Unicode normalization in the file system

- **Category:** Text, encoding and locale
- **Systems affected:** Windows (NTFS, FAT), macOS (HFS+, APFS), Linux ext4/f2fs casefold, bcachefs; Git and other cross-platform tools as victims
- **Classification:** contested
- **Confidence:** high

**What the design is.** The file system, not the application, decides when two different byte or code-unit sequences name the same file. It does this by case-folding (Windows by default, macOS by default) and/or Unicode normalization (HFS+ stored a normalized form on disk, which Torvalds describes as almost but not exactly NFD; APFS preserves the name but hashes a normalized form) [S633][S641][S646].

**Why it was chosen.** Users do not think of "Readme" and "README" as different documents, and visually identical names in different normal forms are confusing. DOS/FAT and classic Mac OS were case-insensitive, so successors inherited it for compatibility. Linux added casefolding mainly for interoperability: Android's `/sdcard` and Samba previously emulated it in user space with known shortcomings [S644].

**Why it is a problem.** Name equality becomes a function of Unicode tables, versions and vendor quirks, and every layer has to agree with the file system's definition. In CVE-2014-9390, Git on case-insensitive or normalizing file systems could be made to overwrite `.git/config` via mixed case, HFS+-ignorable code points, or the NTFS 8.3 alias `git~1`, giving arbitrary command execution on clone [S642][S643]. Apple changed semantics mid-stream. Early APFS (iOS 10.3) was normalization-sensitive, which could cause `ENOENT` and mismatches after migration from HFS+. HFS+ used Unicode 3.2 while APFS uses Unicode 9.0 [S641]. Windows apps that change a name's case can no longer access files in case-sensitive directories, and before per-directory case sensitivity, files differing only by case could not be accessed by Windows apps [S647]. In the Linux kernel, ext4 initially could not combine casefold with encryption [S644], and Overstreet describes the dcache support for case-insensitive directories as "half baked" [S645].

**Why it was not fixed.** Compatibility in both directions: Windows and macOS users expect insensitivity, and Linux added it for interoperability. According to H. Peter Anvin, Microsoft has declared its recommended upcase table permanently frozen and, as far as he knows, only ever did 1:1 BMP mappings without normalization [S646].

**Alternatives that exist.** Byte- or code-unit-exact names, with any folding done in the UI or the application (Linux default; Windows' file I/O APIs treat names as opaque WCHAR sequences and do no normalization [S632]). Per-directory opt-in (ext4 casefold, NTFS per-directory case sensitivity since Windows 10 build 17107) [S644][S647]. A frozen 1:1 fold table [S646].

**Cost of the alternatives.** *Position A (Torvalds):* case-insensitive names are "horribly wrong", the only valid reason is backward compatibility, and anything more complicated than a simple fixed table is unjustified [S645][S646]. Ts'o likewise said adding Unicode tables to the kernel has "certainly been a headache" [S650]. *Position B (Overstreet; Krisman and Ts'o, who implemented ext4 casefolding in 2019):* real user and application needs (Android, Samba) justify supporting it, and user-space emulation does not work well [S645][S644]. Exact-match systems push confusable-name problems into UIs and break interop with Windows/macOS media.

**Relevance to our OS.** high. Name-equality semantics are part of the storage ABI and the security boundary, so they need to be decided once, including the Unicode version and stability policy.

**Sources.** [S632], [S633], [S641], [S642], [S643], [S644], [S645], [S646], [S647], [S650]

### DM-095: File names as arbitrary byte strings (control characters, leading dash, no encoding)

- **Category:** Text, encoding and locale
- **Systems affected:** POSIX/Unix, Linux, BSDs (macOS less so: APFS accepts only valid UTF-8 and rejects code points unassigned in Unicode 9.0 [S641]); shell and scripting ecosystems
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A POSIX file name component may contain any byte except `/` and NUL. That includes newline, tab, escape and other control bytes, names starting with `-`, and byte sequences that are not valid in any encoding. A pathname "might only be a string" rather than a character string [S651][S649].

**Why it was chosen.** It keeps the kernel encoding-agnostic: historically the kernel does not know which locale or code page is in use [S650], and some code pages assign printable characters to bytes in the control range [S649][S650]. Treating names as opaque bytes is simple, fast and never rejects a name some other system produced.

**Why it is a problem.** Most tooling assumes names are text separated by newlines or whitespace, so names with newline, tab or spaces break `find | while read`, `xargs` and word-splitting loops. A file named `-n` turns into an option for `cat *`. Escape sequences in names can manipulate terminals when listed [S318][S649]. The Austin Group bug that proposed forbidding bytes 1–31 says the permissiveness "has resulted in numerous security vulnerabilities and erroneous programs" and that correct handling is very hard even for expert POSIX developers [S649]. POSIX itself advises applications to avoid a leading hyphen [S651]. Without a declared encoding, names cannot be reliably displayed or compared. Languages must expose a separate OS-string type (Rust's `OsString`) because file names need not be valid UTF-8 [S652].

**Why it was not fixed.** A ban would make existing names illegal and fragment the standard; participants in the Austin Group bug argued that most deployed systems are Linux and Linux would not accept it [S649]. Ts'o argued that control bytes can be printable characters in some code pages (cp437) and that the kernel cannot know the locale [S650]. POSIX.1-2024 settled on a soft change: implementations are "encouraged" to reject newline in new names with `EILSEQ`, and a future version may require utilities to treat such names as errors. NUL-delimited tool options (`find -print0`, `xargs -0`) were handled in related bugs [S649][S318].

**Alternatives that exist.** Windows forbids bytes 1–31 and a set of punctuation in names [S633][S649]. APFS stores names as UTF-8 and rejects names containing unassigned Unicode code points [S641]. On LKML, Colomar suggested an optional kernel mode forbidding bytes 0–31 [S650].

**Cost of the alternatives.** Rejecting names breaks round-tripping of archives and of media created elsewhere. Mandating an encoding couples the FS to a specific Unicode version (APFS pins its unassigned-code-point rule to Unicode 9.0) [S641]. Windows' restrictions brought their own problems (DM-043). Classification note: the encoding-agnostic choice was defensible when encodings varied, but allowing control bytes such as newline caused problems from the start (the Austin Group bug cites the Unix-Haters Handbook) [S649], so this is recorded as a tradeoff rather than right-at-the-time-wrong-now.

**Relevance to our OS.** high. With no legacy, the native name type can be defined (for example, valid UTF-8 with no control characters), and the POSIX layer then has to decide how to map names that violate it.

**Sources.** [S633], [S641], [S318], [S649], [S650], [S651], [S652]

### DM-096: UCS-2/UTF-16 as the native Windows NT string type, with duplicated A/W APIs

- **Category:** Text, encoding and locale
- **Systems affected:** Windows NT family (Win32, NT kernel, NTFS); also languages/runtimes that adopted 16-bit chars [unverified: Java, JavaScript not researched here]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Windows represents text internally as 16-bit `WCHAR` units, now interpreted as UTF-16 [S655][S658]. Each text-taking Win32 function comes in an "A" version (bytes in the active ANSI code page), a "W" version (UTF-16), and a generic macro selected by `UNICODE` [S665][S654].

**Why it was chosen.** When NT was designed, Unicode was a 16-bit encoding (its first version, 1991–1995), so a fixed-width wide character looked like the simple, final answer. "UCS-2" describes implementations up to Unicode 1.1, before surrogates and UTF-16 arrived in Unicode 2.0 (1996) [S656]. Keeping "A" functions let existing code-page software run unchanged [S658].

**Why it is a problem.** The fixed-width premise failed: supplementary characters need surrogate pairs, so UTF-16 is variable-width anyway [S655][S656]. At the file system level, names are arbitrary arrays of 16-bit values, and unpaired surrogates are accepted [S657]. Names therefore need not be valid Unicode, and Rust has to store Windows OS strings in a "less-strict variant of UTF-8" [S652]. The A APIs convert through the active code page, so characters outside it are lost or fail to round-trip. Python had to switch Windows path handling for bytes paths to UTF-8 over the W APIs to fix this [S658]. Two API surfaces add documentation and testing burden, and the ANSI code page remains the default text encoding unless an app or runtime opts out [S635][S653].

**Why it was not fixed.** The ABI is frozen. Microsoft's route to UTF-8 reuses the A APIs: since Windows 10 1903 an app can declare `activeCodePage` UTF-8 in its manifest. But GDI ignores the per-process setting and requires a system-wide "Beta" option, and `CP_ACP` means UTF-8 only under that opt-in [S653]. Python made UTF-8 mode the default only in 3.15 [S635].

**Alternatives that exist.** UTF-8 as the interchange/native encoding with byte-oriented APIs. Microsoft now recommends UTF-8 for compatibility with web apps and *nix platforms [S653]. Plan 9's use of UTF-8 as native encoding is relevant but was not verified here [unverified]. APFS stores names as UTF-8 [S641].

**Cost of the alternatives.** UTF-8 is also variable-width. Byte-oriented Unix APIs without encoding validation led to DM-095. Converting at the boundary costs time and code [S653].

**Relevance to our OS.** high. Picking one native string encoding and validation rule for every syscall and IPC boundary is cheap now and impossible to change later.

**Sources.** [S635], [S641], [S652], [S653], [S654], [S655], [S656], [S657], [S658], [S665]

### DM-097: Locale as hidden process-global state (setlocale and locale-dependent parsing)

- **Category:** Text, encoding and locale
- **Systems affected:** ISO C / POSIX (C89, POSIX.1-2001), glibc, all C runtimes; C++ iostreams; Python and other runtimes built on libc
- **Classification:** mistake
- **Confidence:** medium

**What the design is.** C's `setlocale()` sets a program-wide locale (character classification, collation, numeric and monetary formatting, time, messages) from environment variables (`LC_ALL`, `LC_*`, `LANG`). Many standard functions such as `printf`, `strtod`, `isalpha` and `strcoll` silently depend on it. Programs start in the "C" locale [S659].

**Why it was chosen.** In C89-era single-threaded programs, one global switch let existing code become localized without changing every call site. Environment variables gave per-user configuration with no API changes [S659].

**Why it is a problem.** The locale is ambient global state. `setlocale` is MT-Unsafe [S659]. Python's docs warn that calling it from a library changes the whole program, that save-and-restore is expensive and affects other threads, and that some implementations crash on frequent changes [S664]. Functions used for machine formats then depend on user settings. C++'s `to_chars`/`from_chars` proposal lists `sprintf`, `strtol`, `atol` and `stringstream` as all locale-affected and unsuitable for formats like JSON/XML, hence new locale-independent primitives [S663]. The default "C" locale typically implied ASCII. Python needed PEP 538 to coerce it to a UTF-8 locale; PEP 538 notes that simply assuming UTF-8 inside Python (the PEP 540 approach) would make Python diverge from other locale-aware components in the same process and in subprocesses (e.g. GNU readline) [S662]. One process cannot easily format a machine-readable number and a user-facing one with different conventions.

**Why it was not fixed.** The C standard defines the locale as program-wide, and every C library inherits that [S664]. POSIX.1-2008 added thread-local locale objects (`newlocale`, `uselocale`, `duplocale`) instead of changing `setlocale` [S660][S661]. These coexist with the global locale (`LC_GLOBAL_LOCALE`) and have sharp edges: modifying a locale object may free the base object [S661].

**Alternatives that exist.** Explicit locale arguments or objects (`*_l` functions, `uselocale`) [S660][S661]. Locale-independent primitives for machine formats (C++17 `to_chars`/`from_chars`) [S663]. Runtimes that force UTF-8 regardless of locale (Python UTF-8 mode) [S635][S662].

**Cost of the alternatives.** Explicit locale parameters clutter every text API. Ignoring the user's locale can break interop with legacy tools that do honor it [S662]. Splitting "machine" and "human" formatting APIs doubles the surface.

**Relevance to our OS.** medium. Native APIs can keep localization out of core parsing and formatting entirely. The POSIX layer must still emulate `setlocale` for hosted C programs and the compiler.

**Sources.** [S635], [S659], [S660], [S661], [S662], [S663], [S664]

