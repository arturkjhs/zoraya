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
