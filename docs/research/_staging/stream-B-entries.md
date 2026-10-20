# Stream B entries

## 2. Security, authority and identity

### DM-009: Ambient authority: a process may name and open anything its user can

- **Category:** Security, authority and identity
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS, Windows NT (any ACL/identity system with global namespaces)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** A process names resources through global namespaces (file paths, PIDs, port numbers, IPC keys) and the kernel decides access by checking the process's identity against the object's permissions. The process does not present or choose an authority; any request it makes is evaluated with all the authority it holds. Miller, Yee and Shapiro call the absence of selection "ambient authority" [S201].

**Why it was chosen.** Time-sharing systems needed to protect users from one another, and a per-user identity plus a per-object list is cheap to check and easy to administer. Programs were mostly trusted tools run by their user, so "program authority = user authority" was a reasonable approximation. Global names make composition easy: any program can open any path the user types.

**Why it is a problem.** Designation and authority are separated, so a program acting for several principals cannot say which authority a request uses. Hardy's Tymshare compiler had a license to write its own directory; a user passed the name of the billing file as the debug output path and the compiler overwrote it [S200]. Patching this by adding rules grew the "may open" predicate to 14 boolean operators, and each new clause broke a previously-secure program [S200]. The same pattern underlies path-based TOCTOU, setuid exploitation (DM-011), and sandbox escapes: a sandbox has to close every global namespace, and Capsicum found even `fexecve` reaches the global namespace via the ELF interpreter path [S202].

**Why it was not fixed.** Every POSIX API takes global names. Capsicum had to add a separate "capability mode" because removing global namespaces breaks `fork`/`exec` and most of libc's runtime assumptions [S202].

**Alternatives that exist.** Object-capability systems (KeyKOS/EROS, seL4) combine designation with authority so that holding a reference is the permission [S201]. Fuchsia starts a process with nothing; it gets only the handles and namespace entries its creator gives it [S204]. Capsicum retrofits capability mode and per-descriptor rights onto FreeBSD (shipping since 9.0) [S203].

**Cost of the alternatives.** Every resource must be passed explicitly, so launchers, shells and libraries need a delegation protocol; Capsicum needed a new runtime (libcapsicum) and a rethought process model [S202]. Arbitrary user-typed paths need a powerbox or a trusted file chooser. Existing Unix software is not structured for this, which limits retrofit adoption.

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

**Cost of the alternatives.** Per-app uids multiply identities and need custom kernel patches for things DAC never controlled (Android had to modify the kernel to gate socket creation by group) [S222]. MAC needs large policies (DM-015). Capabilities need explicit delegation (DM-009). Users still need a "the human is the principal" notion for sharing between people.

**Relevance to our OS.** high — choosing who the principal is (user, app, component, or the held handle) is a foundational ABI decision.

**Sources.** [S200], [S202], [S222]

### DM-011: `setuid`/`setgid` binaries

- **Category:** Security, authority and identity
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** mistake
- **Confidence:** high

**What the design is.** An executable file with the set-user-ID bit runs with the file owner's effective uid rather than the invoker's. This is the standard way to let unprivileged users perform specific privileged actions (`passwd`, `su`, `sudo`, `pkexec`, `mount`). Ritchie invented the bit in 1973 [S205].

**Why it was chosen.** It is a tiny mechanism (one mode bit) that delegates privilege without a daemon or IPC, which mattered on 1970s machines. It lets a program, not a person, hold a privilege, which was a real advance over shared root passwords.

**Why it is a problem.** The privileged program starts with an execution context entirely chosen by the attacker: argv, environment, fds, rlimits, signal dispositions, cwd, umask. Every one is an input. PwnKit (CVE-2021-4034): with `argc == 0`, pkexec's `argv[1]` overlapped `envp[0]`, and its out-of-bounds write reintroduced `GCONV_PATH`, which made glibc load an attacker library as root; the bug existed since pkexec's creation in 2009 [S206][S207]. Sudo's heap overflow CVE-2021-3156 is another root-from-any-user bug in a setuid program [S208]. The dynamic loader needs a special "secure-execution mode" that strips a long and growing list of variables [S209]. The uid-changing calls themselves are badly designed: Chen, Wagner and Dean found semantics differ between Linux, Solaris and FreeBSD, POSIX leaves "appropriate privilege" open, and misuse caused real sendmail vulnerabilities [S205].

**Why it was not fixed.** `su`, `passwd`, `sudo` and many distribution tools depend on it, and POSIX specifies it. Replacements need a privileged broker that every distribution has to adopt.

**Alternatives that exist.** Android removed all setuid/setgid programs in 4.3 and mounts /system nosuid for app processes [S210]. systemd's `run0` elevates through the service manager and polkit with no setuid bit and does not inherit the caller's credentials or environment [S211]. OpenBSD pledge ignores setuid bits for pledged processes and refuses to exec setuid targets under execpromises [S212]; OpenBSD's kernel refuses `argc == 0`, which blocked PwnKit [S206]. Plan 9 changes identity through a capability device and argues `su`/`login` would not need setuid [S214].

**Cost of the alternatives.** A broker daemon is a long-running privileged process with its own IPC attack surface (polkit itself is such a broker). Policy moves into configuration files that must be audited.

**Relevance to our OS.** high — a capability OS can exclude privilege-on-exec entirely; the POSIX layer must decide whether to emulate it.

**Sources.** [S205], [S206], [S207], [S208], [S209], [S210], [S211], [S212], [S214]

### DM-012: Root as an all-powerful superuser

- **Category:** Security, authority and identity
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS (before SIP); Windows Administrator/SYSTEM is analogous
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** User ID 0 bypasses permission checks and "can access all resources" [S205]. Every administrative operation, from binding port 80 to loading kernel modules, is gated on this one identity.

**Why it was chosen.** A single administrator on a small machine needed a simple escape hatch; one check (`uid == 0`) is trivial to implement and reason about. Fine-grained privilege would have needed policy machinery that early Unix deliberately avoided.

**Why it is a problem.** Any compromise of any root process is total, so every daemon that needs one privileged operation becomes a full-system risk. Linux introduced capabilities precisely because a compromised program with only some privileges can do less damage than one running as root [S216]. Root also cannot be meaningfully constrained by the system it administers: macOS had to add System Integrity Protection because before El Capitan root could modify any system file, and SIP now restricts root from protected locations [S215]. Root is the payoff for every setuid bug (DM-011).

**Why it was not fixed.** Decades of tools, scripts, packaging and documentation assume "run as root". Splitting root into capabilities happened, but the split became coarse again (DM-013).

**Alternatives that exist.** Plan 9 has no all-powerful account; the host owner is described as the closest analogue to root but "far weaker" [S214]. Capability kernels (seL4, Zircon) have no identity that bypasses checks; power is whatever handles a process holds. macOS SIP layers a non-root-bypassable integrity policy [S215].

**Cost of the alternatives.** Recovery and administration need a defined path (for SIP, booting to recovery to disable it [S215]); with no superuser, a misconfigured system can lock out its owner. Plan 9's model depends on a separate authentication server design [S214].

**Relevance to our OS.** high — deciding whether any principal bypasses checks, and how a human recovers a broken system without one, is a core security-model decision.

**Sources.** [S205], [S214], [S215], [S216]

### DM-013: Linux "POSIX capabilities" vs real capability systems (`CAP_SYS_ADMIN`)

- **Category:** Security, authority and identity
- **Systems affected:** Linux (POSIX 1003.1e draft, withdrawn)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Linux splits root's power into per-thread privilege bits (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, ...) held in permitted/effective/inheritable/bounding/ambient sets, with file capabilities in an xattr since 2.6.24 and an ambient set since 4.3 [S216]. These are privileges attached to a subject, not unforgeable references to objects.

**Why it was chosen.** It gives a compatible, incremental way to stop running daemons as full root without changing any API: the same `uid == 0` checks become `capable(X)` checks.

**Why it is a problem.** Granularity eroded. In Linux 3.2, 451 of 1,167 capability checks were `CAP_SYS_ADMIN` (16% in Linux 2.2), developers default to it because the name sounds right, and holding it often lets an attacker escalate to full root [S217]. The man page now tells kernel developers "Don't choose CAP_SYS_ADMIN if you can possibly avoid it" [S216]. The bits are still ambient: a process with `CAP_DAC_OVERRIDE` overrides checks on every file, so confused-deputy problems remain. The name also collides with real capabilities; LWN notes Capsicum's capabilities are "entirely different from Linux capabilities" [S219]. The set semantics across `execve` were complex enough to need a later ambient set [S216].

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

**Why it is a problem.** Each has a characteristic failure. seccomp: the kernel docs say "System call filtering isn't a sandbox"; filters cannot dereference pointers, so paths and structs cannot be checked, syscall numbers differ by ABI so filters must check the architecture, and policies are fragile against libc changes [S218]. Chromium's pre-Capsicum sandboxes ranged up to 22,000 lines and had holes; the Linux suid sandbox needed privilege to set up [S202]. pledge's promises are coarse, a violation kills the process [S212], and unveil warns that hiding files from arbitrary programs is dangerous and is cleared by `execve` [S213]. Capsicum needs real program restructuring and a new runtime [S202]; its Linux port was described in 2014 as invasive and a "hard sell" [S219]; I found no evidence it was merged [unverified].

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

**Why it is a problem.** Policies are big and hard to get right: the SELinux reference policy has over 30,000 statements, and misconfigured policies make access control ineffective [S223]. MAC was designed for administrator-defined system policy, not for applications compartmentalising themselves, so app developers cannot use it for least privilege [S202]. The hook framework let only one "major" module own the blobs; LWN called stacking a "perennial" problem in 2012 [S224], and in 2014 it was not expected soon [S219]; the kernel docs now describe "a close approximation" of stacking [S221]. Disabling SELinux to make software work is a widely reported admin practice [unverified — anecdotal only].

**Why it was not fixed.** The hook-based design is now the kernel ABI for every security module; the policy complexity reflects the complexity of the ambient-authority system being constrained.

**Alternatives that exist.** Android shows MAC can work when one vendor writes the whole policy: SEAndroid mitigated multiple published exploits [S222]. Capability systems make most policy implicit in handle distribution [S202].

**Cost of the alternatives.** SEAndroid needed kernel changes such as file labeling for yaffs2 and middleware MAC because all apps fork from zygote [S222]. Without MAC, system-wide invariants such as "no app may ever touch this device" have to be ensured by construction instead of by a declarative policy.

**Relevance to our OS.** medium — we could need system-wide policy for invariants, but should decide whether it is a layer over ambient authority or derived from capability distribution.

**Sources.** [S221], [S222], [S223], [S224], [S219], [S202]

### DM-016: Windows security descriptors/ACL complexity and token model

- **Category:** Security, authority and identity
- **Systems affected:** Windows NT and successors
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Every securable object has a security descriptor with an owner, a DACL of ordered allow/deny ACEs and a SACL; every process/thread has an access token listing user, group and logon SIDs, privileges, optional restricting SIDs, and impersonation state [S225]. Vista added Mandatory Integrity Control: integrity labels checked before the DACL, with no-write-up by default [S226]; AppContainer adds per-app SIDs.

**Why it was chosen.** NT was designed for enterprise domains and formal security evaluation [unverified], so it made access control uniform across all kernel object types and expressive enough for any policy. Tokens give a clean per-thread security context and support impersonation for servers.

**Why it is a problem.** Expressiveness exceeds what developers can reason about. Govindavajhala and Appel count up to about 30 rights on about 15 object kinds, and their scanner found several user-to-administrator escalations caused by commercial vendors' ACL misconfigurations on registry keys, files and services (CERT VU#953860) [S227]. Impersonation tokens make servers act with the client's authority, a confused-deputy surface [unverified as a measured trend]. Pre-Vista software assumed it ran as administrator, so Windows needed file/registry virtualization as an "interim" compatibility layer [S228] and integrity levels as a second, mandatory layer [S226].

**Why it was not fixed.** The model is consistent and is the basis of Active Directory; complexity lives in policy rather than in the mechanism, so it is retrofitted with layers (MIC, AppContainer) rather than replaced.

**Alternatives that exist.** Unix's simpler mode bits (less expressive, fewer misconfigurations per object) [S227]; capability handles (NT handles themselves carry granted access masks after open — see GR-003).

**Cost of the alternatives.** Simpler models cannot express delegation to specific groups or deny entries, and push complexity into setuid helpers (DM-011).

**Relevance to our OS.** medium — positive lesson: one uniform object/security model across all kernel objects; negative lesson: expressive ACL languages produce misconfiguration CVEs.

**Sources.** [S225], [S226], [S227], [S228]

### DM-110: Unprivileged user namespaces expose root-only kernel attack surface

- **Category:** Security, authority and identity
- **Systems affected:** Linux (container and sandbox retrofit via namespaces)
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Since Linux 3.8 any unprivileged process can create a user namespace in which it holds a full capability set; that lets it create other namespaces and use operations normally limited to root, scoped to the resources the namespace governs [S229]. It is the basis for rootless containers and for browser sandboxes.

**Why it was chosen.** Containers were built from separate namespaces retrofitted onto a kernel with a global root (DM-012); user namespaces let unprivileged users build sandboxes and containers without setuid helpers (DM-011), which were themselves a risk.

**Why it is a problem.** Every kernel subsystem must be correct about which capability checks are namespace-relative, and code once reachable only by real root becomes reachable by anyone. The man page records a series of fixes, e.g. `/proc/PID/setgroups` and requiring `CAP_SETFCAP` to map uid 0 from 5.12 [S229]. At Pwn2Own 2023, a 9-year-old nf_tables bug (CVE-2023-35001) was reachable by an unprivileged user on Ubuntu because user namespaces opened that surface [S231]. Canonical reports that unprivileged user namespaces are widely used in privilege-escalation chains, citing a Google figure of 44% of exploits seen [S230] (the Google kCTF post I could fetch gives a 60% io_uring figure and does not state the 44% figure [S220]).

**Why it was not fixed.** The sysctl switch is all-or-nothing and breaks applications that rely on it, so Ubuntu 23.10 added per-application AppArmor gating instead [S230]. Rootless containers and browsers depend on it.

**Alternatives that exist.** Per-app gating (Ubuntu) [S230]; capability systems where creating an isolated environment needs no privilege because a child only gets what it is handed (Fuchsia) [S204].

**Cost of the alternatives.** Disabling user namespaces forces sandboxes back to setuid helpers; gating requires profiles for each app [S230].

**Relevance to our OS.** high — shows the cost of building isolation by making privileged kernel code "safe for unprivileged use" instead of making isolation the default.

**Sources.** [S229], [S230], [S231], [S220], [S204]

## 14. Configuration, observability and system management

### DM-085: Environment variables as implicit global configuration

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS; Windows (per-process environment, but with a thread-safe API)
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Each process has a flat list of `NAME=value` strings inherited across `fork`/`exec`. Any library may read any variable at any time with `getenv`, and many do (locale, time zone, loader paths, DNS, proxy settings, character conversion).

**Why it was chosen.** It is the simplest possible way to pass optional configuration through a chain of programs without changing argument conventions, and it costs nothing on a PDP-11. Inheritance is useful: setting `PATH` or `TERM` once configures a whole session.

**Why it is a problem.** (1) Thread safety: glibc marks `setenv`/`unsetenv` MT-Unsafe and POSIX does not require reentrancy [S234]. Rust made `std::env::set_var` `unsafe` in the 2024 edition, because libc `getenv` is unsynchronized and libraries do not document which functions read the environment (DNS lookups do), so no program can guarantee soundness once threads exist [S235][S236]. glibc's attempt to make `getenv` thread-safe "in more cases" broke applications that managed `environ` themselves and needed a compatibility fix [S237]. (2) Security: the environment is attacker-chosen input to privileged programs; the loader must strip a long list of variables in secure-execution mode [S209], and PwnKit worked by smuggling `GCONV_PATH` back in [S206]. `LD_PRELOAD` silently injects code into every child [S209]. (3) Hidden coupling: Poettering cites heavy dependence on environment variables as one reason shell-based init scripts are unpredictable [S233]; `run0` deliberately does not inherit the caller's environment [S211].

**Why it was not fixed.** `environ` is a public global array in POSIX and C programs write it directly, so any locking scheme breaks someone [S237].

**Alternatives that exist.** Explicit configuration passed at spawn (Rust's recommended `Command::env` for children [S235]); service managers that construct the environment from configuration rather than inheritance [S211]; Windows exposes environment changes only via API calls (Rust documents `set_var` as always sound on Windows [S235]).

**Cost of the alternatives.** Losing ambient inheritance means every launcher must plumb configuration explicitly; the POSIX layer will still need an `environ` for the C compiler and its tools.

**Relevance to our OS.** high — the native process-start ABI decides whether an inherited mutable string table exists at all.

**Sources.** [S234], [S235], [S236], [S237], [S209], [S206], [S211], [S233]

### DM-086: Windows registry

- **Category:** Configuration, observability and system management
- **Systems affected:** Windows NT and successors
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** A system-defined hierarchical database of keys and typed values, used by the OS and applications for configuration [S238][S239], stored in hive files and implemented in the kernel.

**Why it was chosen.** Raymond Chen's account: INI files had no Unicode, per-file rather than per-key security, lost updates from concurrent writers, whole-file parsing on each access, a 32 KB limit, only two levels, string-only values, and lived in the admin-only Windows directory; the NT team designed the registry to fix these [S240]. These are real problems, and a central store also enables central administration.

**Why it is a problem.** (1) A shared mutable namespace for all programs: apps that assumed they could write `HKLM\Software` broke under non-admin users, so Vista added registry virtualization, redirecting writes to a per-user VirtualStore, which Microsoft calls interim and which breaks registry-as-IPC between a service and an app [S228]. (2) Kernel attack surface: Project Zero found 50 CVEs in the registry in 2022–2024; hives are a binary format parsed entirely in kernel by 30-year-old C, and any user can load an arbitrary hive via `RegLoadAppKey` [S241]. (3) Opaque state: no ownership by application, so removal and migration leave residue, and Microsoft warns not to edit data that is not yours [S238].

**Why it was not fixed.** Every Win32 application and Group Policy depends on it; the fix has been isolation layers (virtualization [S228], and per-package registries for packaged apps [unverified]).

**Alternatives that exist.** Per-application text files (DM-087), which Chen says reopen INI problems once programs write them [S240]; declarative, immutable system configuration (NixOS, DM-087).

**Cost of the alternatives.** File-based configuration loses typed values, per-key ACLs and transactional updates [S240].

**Relevance to our OS.** medium — argues for typed, per-component configuration owned by the component, and against a kernel-resident shared database.

**Sources.** [S238], [S239], [S240], [S241], [S228]

### DM-087: Per-program text configuration syntax in `/etc`

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, macOS
- **Classification:** tradeoff
- **Confidence:** medium

**What the design is.** Each program defines its own configuration file format, usually line-based text in `/etc`, edited by hand or by ad-hoc scripts. There is no common schema, type system, validation, or transaction.

**Why it was chosen.** Text works with every editor and with `diff`, `grep` and version control, needs no shared infrastructure, and each program can evolve its own format. Chen notes that text files that are only read avoid many INI problems [S240].

**Why it is a problem.** Errors are syntactic and semantic and found late. In 546 real misconfigurations across a commercial storage system and CentOS, MySQL, Apache and OpenLDAP, 70–85.5% were parameter mistakes, 38–54% of those were illegal values a checker could catch, and 12–30% were inconsistencies between parameters [S242]. Automated management needs a parser per format; Augeas exists to turn many native formats into a tree because editing them with sed/awk is unsafe [S243]. Updates are destructive in-place edits, so there is no rollback or reproducibility [S244].

**Why it was not fixed.** No central authority owns the format of third-party programs; every attempt at a common store (GConf/dconf, Elektra) covers only some programs [unverified].

**Alternatives that exist.** NixOS builds the whole system configuration from a purely functional specification, making it immutable, reproducible and rollback-able [S244]. Registry-style typed stores (DM-086). Config-editing layers (Augeas) [S243].

**Cost of the alternatives.** NixOS requires all packages to be built against its store model [S244]; typed stores lose the universality of text and need tools to inspect [S240].

**Relevance to our OS.** medium — we control every native component, so a single schema-checked configuration model is feasible; the POSIX layer will still meet text configs.

**Sources.** [S242], [S243], [S244], [S240]

### DM-088: Init systems: shell-script SysV init vs systemd

- **Category:** Configuration, observability and system management
- **Systems affected:** Linux distributions; comparable: Solaris SMF, macOS launchd, s6/runit
- **Classification:** contested
- **Confidence:** medium

**What the design is.** SysV init runs ordered shell scripts per runlevel; each script starts a daemon that daemonizes and is tracked by a PID file. systemd replaces this with declarative unit files, dependency-based parallel start, socket activation and cgroup-based tracking, and has grown into a suite covering logging, login and other services.

**Why it was chosen.** SysV init was simple, transparent and editable in a language every admin knew. Poettering's 2010 case for systemd: serialized startup, slow shell (`grep` invoked at least 77 times in `/etc/init.d`), double-forked daemons escaping supervision, and fragility from environment-dependent shell; socket activation removes most ordering [S233].

**Why it is a problem.** Position A (systemd proponents): scripts cannot supervise, restart or reliably track services and serialize boot [S233]; Solaris SMF had already shown a declarative service model with dependencies and instances [S250]. Position B (critics): systemd integrates too much into one project around PID 1, couples other software to its interfaces, and ignores portability; the s6 author calls it "technically unsustainable" [S249] and offers small supervision tools in the daemontools tradition instead [S251]. In Debian's 2014 dispute, Ian Jackson pushed for "loose coupling" so packages could not depend on a specific PID 1 [S246].

**Why it was not fixed.** It is a governance dispute as much as a technical one. Debian's technical committee chose systemd for jessie by the chair's casting vote [S245][S246]; a 2014 GR on coupling resolved that no GR was required [S247]; a 2019 GR with eight options chose "systemd but we support exploring alternatives" [S248].

**Alternatives that exist.** SMF [S250], launchd [unverified — not fetched], s6/runit supervision [S251], and systemd itself.

**Cost of the alternatives.** Minimal supervisors leave socket activation, dependency handling and logging to other tools [S251]; an integrated manager concentrates complexity and policy in one project [S249].

**Relevance to our OS.** high — we will need a service manager; the contested question is how much belongs in it and which component tracks service lifetime.

**Sources.** [S233], [S245], [S246], [S247], [S248], [S249], [S250], [S251]

### DM-089: Unstructured text logging (syslog)

- **Category:** Configuration, observability and system management
- **Systems affected:** Unix/POSIX, Linux, BSDs, network devices
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Programs send free-form text lines tagged with facility and severity to a syslog daemon over a local socket or UDP; the daemon appends them to text files. RFC 3164 (2001) only documented existing practice [S252].

**Why it was chosen.** Text is human-readable, tool-independent and robust to partial corruption; a fire-and-forget UDP sender never blocks the program logging. Early systems had little to log and one administrator to read it.

**Why it is a problem.** BSD syslog timestamps have no year or time zone, messages were limited to 1024 bytes over UDP, and there is no sender authentication, integrity or reliability [S252]. RFC 5424 notes implementations shared little beyond the PRI field [S253]. The journal design document lists the consequences: any local process can claim any identity or PID, analysis depends on fragile regular expressions, lookups are O(n), and attackers can edit plain logs [S254]; the kernel.org break-in motivated hash-chaining [S256].

**Why it was not fixed.** RFC 5424 added structured data, MSGID and RFC 3339 timestamps [S253], but the message body stays free text and old formats persist. The journald replacement was contested: its binary format was initially declared not to be standardized [S256], and today's docs still say the on-disk format is not a stable interface and readers should use the C API [S255].

**Alternatives that exist.** systemd-journald: indexed, structured key/value entries, trusted fields derived from kernel credentials, Forward Secure Sealing, corruption handling by rotation [S254][S255]. Windows ETW: kernel-level structured tracing with providers and consumers, enabled dynamically in production [S257].

**Cost of the alternatives.** Binary logs need specific tools to read and are less transparent to recover after corruption [S255][S256]; structured schemas need governance.

**Relevance to our OS.** medium — a new OS can make logs structured, with kernel-attested sender identity, from day one.

**Sources.** [S252], [S253], [S254], [S255], [S256], [S257]

### DM-111: cgroups v1 multiple hierarchies and the v1→v2 redesign

- **Category:** Configuration, observability and system management
- **Systems affected:** Linux
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Control groups group processes for resource control. v1 allowed many independent hierarchies, each with any set of controllers, and threads of one process could sit in different groups; v2 has a single unified hierarchy, processes only in leaves, and a defined delegation model [S232].

**Why it was chosen.** Flexibility: each controller could be organized independently, and cgroups were also adopted by init systems for process tracking because double-forked daemons escape the PID tree [S233].

**Why it is a problem.** The kernel's own v2 documentation lists the v1 failures: multiple hierarchies were impractical (a controller could be in only one, could not be moved once populated, and userland ended up managing several near-duplicate trees); per-thread placement blurred app API and system management; processes in a parent competed with child groups with controller-specific, sometimes undefined, results; delegation was loose, racy and exposed kernel internals; controllers used inconsistent names, units and defaults [S232].

**Why it was not fixed.** It was fixed only by a parallel second interface; v1 and v2 hierarchies coexist, and moving controllers between them is discouraged [S232], so tools had to support both for years.

**Alternatives that exist.** cgroup v2's single hierarchy, no-internal-process rule, and namespace-based delegation [S232].

**Cost of the alternatives.** v2 constrains layout (no processes in inner nodes, with exceptions for the root and threaded subtrees) [S232].

**Relevance to our OS.** medium — resource accounting and process grouping should be designed together with the process model rather than bolted on.

**Sources.** [S232], [S233]

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
