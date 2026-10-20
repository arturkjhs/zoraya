# V7 verified entries

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
