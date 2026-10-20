# Stream F entries

## 6. Filesystems, namespaces and storage semantics

### DM-036: POSIX advisory record locks are owned by the process and dropped on any close()

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** POSIX (specified behaviour), Linux, BSDs, macOS, any POSIX-conformant system
- **Classification:** mistake
- **Confidence:** high

**What the design is.** `fcntl(F_SETLK/F_SETLKW)` byte-range locks belong to a (process, inode) pair, not to the file descriptor or open file description. POSIX says that when a process closes *any* descriptor for a file, all of that process's locks on the file go away, even locks taken through a different open file description [S603]. The locks are not inherited across `fork()`, and threads in one process never conflict with each other [S600].

**Why it was chosen.** When the interface was designed, processes were single-threaded and a process was the natural unit of ownership. Tying locks to the process made cleanup on exit simple and gave byte-range granularity, which databases need and BSD `flock()` (whole-file only) does not provide [S601]. Nobody was thinking about libraries that open files behind the caller's back, or about threads.

**Why it is a problem.** Lock state is effectively global to the process, so code that does not know about the lock can destroy it. A library routine that opens, reads and closes the same file (Layton's example is `/etc/passwd`; hard links to the file trigger it too) silently drops the caller's lock, which can lead to silent data corruption [S601][S600]. Because threads share ownership, the locks cannot coordinate threads; Layton's demo shows threads overwriting each other's appends [S601]. SQLite's source calls POSIX locks "broken by design". It has to keep a reference-counted `unixInodeInfo` per inode, do its own lock bookkeeping, and defer `close()` of descriptors (the `pUnused` list) until the last lock is released [S602]. Every serious user (Samba, NFS Ganesha, SQLite, OpenJDK) had to build similar workarounds [S601].

**Why it was not fixed.** The semantics are in the standard and existing programs depend on them, so changing `F_SETLK` was not an option. Linux added a parallel lock type instead.

**Alternatives that exist.** Open file description (OFD) locks (Linux 3.15, originally "file-private" locks) are byte-range locks owned by the open file description. They are inherited across `fork()`/`dup()`, released only on the last close, and conflict between separate opens even within one process [S600][S601]. They are now standardized as `F_OFD_*` in POSIX.1-2024 [S603]. BSD `flock()` has the right ownership but only whole-file granularity [S601].

**Cost of the alternatives.** OFD locks do no deadlock detection [S600]. The two lock families interact (they always conflict with each other), so mixed use in one program is subtle [S601]. Retrofitting took a decade-old API plus a new set of commands, and it left two lock models in the standard.

**Relevance to our OS.** medium. Any lock primitive should be owned by a handle or capability with explicit lifetime. The POSIX layer still has to emulate the broken semantics if hosted C programs rely on them.

**Sources.** [S600], [S601], [S602], [S603]

### DM-037: fsync() error semantics: errors reported once, dirty data dropped ("fsyncgate")

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Linux (ext4, XFS, Btrfs), OpenBSD, NetBSD, macOS; POSIX (underspecified); PostgreSQL, SQLite, LevelDB, LMDB, Redis as victims
- **Classification:** mistake
- **Confidence:** high

**What the design is.** Buffered writes land in the page cache and are written back asynchronously. If writeback fails, Linux file systems mark the affected pages clean (discarding or keeping stale contents) and report the error once to a later `fsync()` [S604][S606]. POSIX says nothing about the file's state after `fsync()` fails [S605].

**Why it was chosen.** Kernel developers' rationale: the most common write error is a removed USB stick. Keeping pages dirty indefinitely for a device that is gone would exhaust memory, so the data is dropped [S604]. Error state lived in the page cache's mapping and was historically cleared when reported. Before `errseq_t` (4.13/4.16) only the first caller after an error could see it [S607].

**Why it is a problem.** PostgreSQL retried `fsync()` after EIO on the assumption that the dirty data was still pending. On Linux the retry *succeeds*, because the error flag was cleared, and the checkpoint advances past writes that never reached disk. Ringer reported real corruption on XFS [S605]. PostgreSQL's checkpointer often opened files just before `fsync()`, so it never saw errors from writeback that happened before its `open()` [S604]. Rebello et al. found that ext4, XFS and Btrfs all mark pages clean after failure, that in-memory and on-disk state diverge, and that none of five widely used applications handle fsync failure correctly, with data loss and corruption as outcomes [S606]. The meaning of "successful fsync" also differs by platform. On Darwin, `fsync()` does not flush the drive's cache, and a separate `F_FULLFSYNC` is needed [S609].

**Why it was not fixed.** Keeping pages dirty was rejected over memory exhaustion. A sticky per-inode error was considered unlikely because it would diverge further from POSIX [S604]. `errseq_t` improved reporting to "once per open file description" but is explicitly "not perfect" [S607]. Kernel developers told PostgreSQL to use direct I/O, which Andres Freund called "a metric ton of work" [S604]. PostgreSQL's response was to PANIC and recover from WAL; this landed in v12 and was backported to 9.4 [S608].

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

**Why it was chosen.** Leaving ordering unspecified lets file systems delay allocation, batch and reorder writes, which is a big performance win. ext4's delayed allocation could hold data for up to a minute versus about five seconds on ext3 [S610]. `fsync()` on every update was expensive; Ts'o cites Firefox 3's fsync-on-close stalls [S611]. Ts'o's position was that apps needing durability must call `fsync()`/`fdatasync()` [S611].

**Why it is a problem.** After ext4 shipped, users found zero-length configuration files after crashes. Desktop apps used write-then-rename without `fsync`, and ext4 committed the rename before the data [S610]. The pattern had "worked" only because ext3's `data=ordered` mode, designed as a security measure against exposing stale blocks, flushed data before metadata as a side effect [S610]. Pillai et al. tested six Linux file systems and found persistence properties vary widely. For example, append-then-rename ordering holds on ext3-ordered but not under delayed allocation. They found 60 crash vulnerabilities in 11 mature applications, including a Git case where a crash after rename breaks all later commits [S612]. Correct code needs knowledge the API never states, such as fsyncing the parent directory after create or rename.

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

**Why it is a problem.** An attacker who can modify any directory along the path can swap a component between check and use. The kernel's own docs call symlink TOCTOU "a long-standing class of security issues" [S619]. In 2009 a search of the NVD for "symlink attack" returned over 600 hits [S617]. In CVE-2018-15664, `docker cp` was vulnerable to a symlink-exchange race giving root read-write access to the host [S615]. runc has had 14 path-safety CVEs since 2017, including 2025 bugs that combine mounts and symlinks to reach `/proc/sys/kernel/core_pattern` [S614]. Symlinks also make `..` ambiguous: `getwd` and `cd ..` disagree [S618]. Porter et al. argue that the counter-measure arms race shows TOCTOU can be eliminated "only by changing the API". They note Linux had to add close-on-exec flags to 15 syscalls to close one race [S617].

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

**Why it is a problem.** A root process following a hard link planted in a shared directory such as `/tmp` crosses a privilege boundary, which is the hardlink TOCTOU race [S620][S619]. A user can "pin" a vulnerable setuid/setgid binary by linking to it, so an administrator's upgrade leaves the old exploitable copy reachable [S620][S621]. Users are surprised they can link files they cannot access [S620]. Symlink and hardlink fixes were discussed from 1996 onward, and the patch history lists repeated objections [S621].

**Why it was not fixed.** Objections included POSIX conformance, breaking applications, "just use `mkstemp()`/`O_EXCL`", and whether the fix belonged in the VFS or an LSM [S621]. Linux adopted the Openwall/grsecurity restriction in 2012 as `protected_hardlinks` (and `protected_symlinks`). Linking then requires owning the file or having read and write access [S621][S619]. Cook argued POSIX already permits this check [S620]. Known breakage was small: some `at` daemons [S620].

**Alternatives that exist.** Restricting link creation to owners or holders of read and write access (Linux, Ubuntu for about 1.5 years before mainline) [S620]. Plan 9 builds aliasing through per-process bind/mount rather than persistent links [S624]; whether Plan 9 file servers support hard links at all could not be verified [unverified].

**Cost of the alternatives.** Some legitimate tools relying on unrestricted links break [S620]. Removing hard links altogether also removes a cheap snapshot/dedup idiom used by backup tools [unverified]. The restriction is a sysctl, so the old semantics persist where it is disabled [S619].

**Relevance to our OS.** medium. If names are references to objects, the authority to create a new name for an object should be tied to authority over the object, not just the directory.

**Sources.** [S619], [S620], [S621], [S624]

### DM-041: A single global mount namespace, with mounting reserved for root

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Traditional Unix, Linux (before 2.4.19 / 3.8), BSDs, macOS; Windows drive letters are a related global namespace
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Classic Unix has one file tree shared by all processes. `mount` alters that shared tree and therefore requires superuser privilege. Per-process views (chroot aside) did not exist.

**Why it was chosen.** One tree is simple to administer and explain. Because the tree is shared and setuid programs trust paths like `/etc/passwd`, letting an ordinary user mount over a directory would let them subvert privileged programs. Linux's docs say many features were restricted to root because they could confuse set-user-ID programs [S623].

**Why it is a problem.** Users and applications cannot compose their own view of resources, such as sandboxes, per-application `/tmp`, or private device views, without privilege. That pushed sandboxing into setuid helpers and root daemons. Linux retrofitted mount namespaces (`CLONE_NEWNS`, 2.4.19) [S622]. Full isolation proved too strict, so shared-subtree propagation followed in 2.6.15, adding a notoriously complex state machine [S622]. Unprivileged mounting arrived only via user namespaces (3.8+), limited to specific filesystem types; block-device file systems still need init-namespace privilege [S623]. Making root-only kernel code reachable to unprivileged users widened the attack surface. Canonical cites a Google figure that 44% of observed exploits required unprivileged user namespaces, and Ubuntu now gates them with AppArmor [S626].

**Why it was not fixed.** Setuid binaries and global path assumptions are everywhere, so namespaces had to be opt-in and bolted on. Each relaxation needed new safety rules, such as locked mounts and flags that cannot be changed in less-privileged namespaces [S622].

**Alternatives that exist.** Plan 9 makes the name space a per-process attribute with no global name space. Processes build it with `mount`, `bind` and union directories, and each window gets its own `/dev/cons` [S624]. Plan 9 "has no super-user" and user processes construct their namespaces themselves [S625].

**Cost of the alternatives.** Plan 9 programs depend on agreed conventions for where things live, because nothing is global [S624]. On Linux, retrofitted unprivileged namespaces became an exploit enabler [S626]. Per-process namespaces complicate debugging and administration ("which tree is this process seeing?") [unverified as a measured cost].

**Relevance to our OS.** high. Whether the namespace is per-process and composable by unprivileged code is a foundational decision that interacts directly with the capability model and the absence of setuid.

**Sources.** [S622], [S623], [S624], [S625], [S626]

### DM-042: atime updated on every read

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Unix/POSIX, Linux (default changed 2.6.30), Windows NTFS (last-access time), Btrfs and other CoW file systems
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Every file records a last-access time. POSIX's model marks `st_atime` for update when data is read. The actual write may be deferred, but it must happen by the time the file is closed or stat'ed [S629].

**Why it was chosen.** On small, shared systems atime supports useful policies: detecting unread mail (mutt), cleaning unused temp files (tmpwatch), and backup and hierarchical-storage decisions [S627]. Disk writes for metadata were a modest cost relative to workloads of the time.

**Why it is a problem.** It turns every cached read into a disk write. Ingo Molnar called it "by far the biggest IO performance deficiency" in Linux in 2007 [S627]. On copy-on-write file systems the cost multiplies. On Btrfs, a recursive grep after a snapshot rewrites inode metadata and unshares it from the snapshot, so reading files consumes disk space, and relatime may not help much [S631]. Read-only workloads therefore also wear flash and fill snapshots.

**Why it was not fixed.** Programs depend on atime, so distributions avoided `noatime` as a default [S627]. Linux compromised with `relatime` (2.6.20): it updates atime only if it is older than mtime/ctime or more than a day old, and has been the default since 2.6.30 [S627][S628]. `lazytime` keeps timestamps in memory for up to 24 hours [S628]. NTFS similarly defers last-access writes by up to an hour and can disable them [S630].

**Alternatives that exist.** `noatime`/`relatime`/`lazytime` [S628]. NTFS's deferred and optional last-access [S630]. Commenters on LWN proposed opt-in per-file atime or a separate atime store [S631] (proposal only).

**Cost of the alternatives.** relatime still breaks "accessed within the last N" queries [S627]. Disabling last-access affects backup and remote-storage tools [S630]. Dropping atime removes a cheap usage signal that tiering and cleanup tools use.

**Relevance to our OS.** low. Easy to avoid from scratch (make access tracking opt-in), but the POSIX layer must decide what `st_atime` reports.

**Sources.** [S627], [S628], [S629], [S630], [S631]

### DM-043: Windows MAX_PATH and DOS-reserved device names

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows (Win32 API layer; NTFS itself is less restricted), applications ported to Windows (Git, Python)
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** high

**What the design is.** Win32 path APIs historically limit paths to `MAX_PATH` (260 characters). Longer paths need the `\\?\` prefix (about 32,767 characters, absolute paths only), or, since Windows 10 1607, an opt-in that requires *both* a machine-wide registry or Group Policy setting *and* a `longPathAware` manifest in each application [S632]. Names such as CON, PRN, AUX, NUL, COM1–9 and LPT1–9 are reserved in every directory, even with an extension (`NUL.txt` is NUL), and Windows also treats superscript digits as part of COM/LPT names [S633].

**Why it was chosen.** DOS 1.0 had no subdirectories. Magic device names let users redirect output to the printer or discard it without temporary files. Because tools appended extensions automatically, the names had to work with any extension [S634]. A fixed small path buffer suited 1980s memory budgets.

**Why it is a problem.** Fixed limits leak into every application. Cloning a Git repository with long names into a deep folder fails [S632]. Relative paths are always limited to MAX_PATH, even with opt-in [S632]. The shell and the file system disagree, so the API can create paths that Explorer cannot handle [S632][S633]. Names that are legal on NTFS through `\\?\` or other OSes become inaccessible or dangerous through Win32 (reserved names, trailing dots and spaces) [S633]. 8.3 short-name aliases are still generated by default on many volumes, adding lookup cost and a second name for every file [S630][S633].

**Why it was not fixed.** Compatibility. Raymond Chen notes batch files and documentation still rely on the reserved names [S634]. Lifting MAX_PATH globally would break programs with fixed 260-character buffers, hence the dual opt-in [S632]. Python enabled long-path support in 3.6 but still needs an administrator to set the policy [S635].

**Alternatives that exist.** NT's native object namespace has no such limits. Win32 is a translation layer on top of it, and `\\.\` and `\\?\` expose it [S633]. Unix places devices in `/dev` rather than reserving names everywhere.

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

**Why it is a problem.** Updates of in-use files are blocked. Microsoft's own docs say the primary reason installs and updates require a restart is that files being updated are in use [S637]. Any third-party handle, such as an antivirus scanner, indexer or backup agent, can make deletes and renames fail transiently, which makes atomic replace-by-rename unreliable [unverified as a measured frequency]. Correctness depends on every opener choosing the right share flags.

**Why it was not fixed.** Applications rely on exclusivity for integrity. Microsoft added mitigations instead. Restart Manager shuts down and restarts applications holding files [S637]. POSIX-style delete (`FILE_DISPOSITION_POSIX_SEMANTICS`) removes the name immediately while existing handles keep the data, though a mapped view still blocks deletion [S638].

**Alternatives that exist.** Unix decouples names from open files: unlink and rename succeed while the file is open, and old handles keep the old inode. Linux tried mandatory locking but found it racy with `write()` and `mmap()` ("unreliable"), made it optional in 4.5 and removed it in 5.15 [S600].

**Cost of the alternatives.** Under Unix semantics, running processes keep using old binaries and libraries after an update, so "updated" does not mean "in effect". Advisory-only coordination means nothing protects a file from a careless writer. Mandatory locking failed in practice on Linux [S600].

**Relevance to our OS.** medium. The interaction between names, open handles and updates determines how live system updates work, and both extremes have documented failure modes.

**Sources.** [S600], [S636], [S637], [S638]

### DM-045: No usable transactional filesystem API (Windows TxF deprecated)

- **Category:** Filesystems, namespaces and storage semantics
- **Systems affected:** Windows (TxF/TxR, Vista+), Linux Btrfs (user transaction ioctls), research TxOS; by absence: POSIX, Linux, BSDs, macOS
- **Classification:** contested
- **Confidence:** medium

**What the design is.** Mainstream file APIs offer single-call atomicity at best (`rename`) and no way to group several file operations atomically. Attempts to add transactions exist but have not stuck.

**Why it was chosen.** Transactions across arbitrary kernel objects are hard: isolation, conflict handling, resource limits and interaction with non-transactional actors. The pragmatic position is that applications needing ACID use a database, and Microsoft's own guidance points to ESE or SQL Server [S639].

**Why it is a problem.** Without it, applications hand-roll crash consistency and get it wrong (DM-038) [S612]. Multi-file updates such as installers and package managers can leave systems half-upgraded. TxOS cites a Vista SP1 prerequisite patch that caused endless reboot cycles [S617]. TOCTOU races persist because check and use cannot be grouped [S617].

**Why it was not fixed.** Microsoft shipped TxF in Vista, but reports "extremely limited developer interest" because of "complexity and various nuances". It says the API "may not be available in future versions" and recommends `ReplaceFile`, Windows Installer, ESE or SQL FILESTREAM instead [S639]. `fsutil` documents TxF as deprecated but still present [S630]. Btrfs added userspace transaction ioctls for Ceph; they were deprecated in 2017 (slated for removal in 4.17) because there was no widespread legitimate use [S640].

**Alternatives that exist.** TxOS (SOSP 2009) put ACID system transactions over many kernel resources in Linux 2.6.22. It reported about 10% overhead for a transactional software install, negligible overhead for non-transactional work, and a transactional ext3 built in under a month [S617]. It remained a research prototype.

**Cost of the alternatives.** *Position A (transactions are the right abstraction):* TxOS shows that overheads are modest and that one mechanism fixes both crash-consistency and TOCTOU classes [S617]. *Position B (they don't pay for themselves):* two production attempts (TxF and Btrfs) were abandoned for complexity and lack of users [S639][S640], and simpler primitives such as atomic replace cover most needs [S639]. Unresolved.

**Relevance to our OS.** high. Whether the storage layer offers grouped atomic operations, and how small that interface can be, is open, and the record shows a broad API alone does not guarantee adoption.

**Sources.** [S612], [S617], [S630], [S639], [S640]

## 16. Text, encoding and locale

### DM-094: Case-insensitive file names and Unicode normalization in the file system

- **Category:** Text, encoding and locale
- **Systems affected:** Windows (NTFS, FAT), macOS (HFS+, APFS), Linux ext4/f2fs casefold, bcachefs; Git and other cross-platform tools as victims
- **Classification:** contested
- **Confidence:** high

**What the design is.** The file system, not the application, decides when two different byte or code-unit sequences name the same file. It does this by case-folding (Windows by default, macOS by default) and/or Unicode normalization (HFS+ stored names in a variant of NFD; APFS preserves the name but hashes a normalized form) [S633][S641].

**Why it was chosen.** Users do not think of "Readme" and "README" as different documents, and visually identical names in different normal forms are confusing. DOS/FAT and classic Mac OS were case-insensitive, so successors inherited it for compatibility. Linux added casefolding mainly for interoperability: Android's `/sdcard` and Samba previously emulated it in user space with known shortcomings [S644].

**Why it is a problem.** Name equality becomes a function of Unicode tables, versions and vendor quirks, and every layer has to agree with the file system's definition. In CVE-2014-9390, Git on case-insensitive or normalizing file systems could be made to overwrite `.git/config` via mixed case, HFS+-ignorable code points, or the NTFS 8.3 alias `git~1`, giving arbitrary command execution on clone [S642][S643]. Apple changed semantics mid-stream. Early APFS (iOS 10.3) was normalization-sensitive, which caused `ENOENT` and mismatches after migration from HFS+. HFS+ used Unicode 3.2 while APFS uses Unicode 9.0 [S641]. When Windows apps change a name's case they break in case-sensitive directories, and files differing only by case are inaccessible to Windows apps [S647]. Kernel dcache, encryption and negative-dentry handling all needed special cases; ext4 initially could not combine casefold with encryption [S644].

**Why it was not fixed.** Compatibility in both directions: Windows and macOS users expect insensitivity, and Linux added it for interoperability. Microsoft froze its upcase table (1:1, BMP only) to keep names stable [S646].

**Alternatives that exist.** Byte- or code-unit-exact names, with any folding done in the UI or the application (Linux default; Windows' Win32 file APIs do no normalization themselves [S632]). Per-directory opt-in (ext4 casefold, NTFS per-directory case sensitivity since Windows 10 build 17107) [S644][S647]. A frozen 1:1 fold table [S646].

**Cost of the alternatives.** *Position A (Torvalds):* case-insensitive names are "horribly wrong", the only valid reason is backward compatibility, and anything beyond a simple fixed table is unjustified [S645][S646]. Ts'o likewise called adding Unicode tables to the kernel a source of ongoing headaches [S650]. *Position B (Overstreet, Krisman/Ts'o in 2019):* real user and application needs (Android, Samba) justify supporting it, and doing it in the kernel beats buggy user-space emulation [S645][S644]. Exact-match systems push confusable-name problems into UIs and break interop with Windows/macOS media.

**Relevance to our OS.** high. Name-equality semantics are part of the storage ABI and the security boundary, so they need to be decided once, including the Unicode version and stability policy.

**Sources.** [S632], [S633], [S641], [S642], [S643], [S644], [S645], [S646], [S647], [S650]

### DM-095: File names as arbitrary byte strings (control characters, leading dash, no encoding)

- **Category:** Text, encoding and locale
- **Systems affected:** POSIX/Unix, Linux, BSDs (macOS less so: APFS stores UTF-8 and rejects unassigned code points [S641]); shell and scripting ecosystems
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** A POSIX file name component may contain any byte except `/` and NUL. That includes newline, tab, escape and other control bytes, names starting with `-`, and byte sequences that are not valid in any encoding. A pathname "might only be a string" rather than a character string [S651][S649].

**Why it was chosen.** It keeps the kernel encoding-agnostic. The kernel does not know the user's locale, and encodings varied by site and era [S650]. Treating names as opaque bytes is simple, fast and never rejects a name some other system produced.

**Why it is a problem.** Most tooling assumes names are text separated by newlines or whitespace, so names with newline, tab or spaces break `find | while read`, `xargs` and word-splitting loops. A file named `-n` turns into an option for `cat *`. Escape sequences in names can manipulate terminals when listed [S648]. The Austin Group bug that proposed forbidding bytes 1–31 says the permissiveness "has resulted in numerous security vulnerabilities and erroneous programs" and that correct handling is very hard even for experts [S649]. POSIX itself advises applications to avoid a leading hyphen [S651]. Without a declared encoding, names cannot be reliably displayed or compared. Languages must expose a separate OS-string type (Rust's `OsString`) because file names need not be valid UTF-8 [S652].

**Why it was not fixed.** A ban would make existing names illegal and fragment the standard. Ts'o argued that control bytes can be printable characters in some code pages (cp437) and that the kernel cannot know the locale [S650]. POSIX.1-2024 settled on a soft change: implementations are "encouraged" to reject newline in new names with `EILSEQ`, and a future version may require it. It also added NUL-delimited tool options [S649][S648].

**Alternatives that exist.** Windows forbids bytes 1–31 and a set of punctuation in names [S633][S649]. APFS stores names as UTF-8 and rejects names containing unassigned Unicode code points [S641]. An optional kernel mode forbidding 0–31 has been proposed for Linux [S650].

**Cost of the alternatives.** Rejecting names breaks round-tripping of archives and of media created elsewhere. Mandating an encoding couples the FS to Unicode versions (APFS's unassigned-code-point rule changes as Unicode grows) [S641]. Windows' restrictions brought their own problems (DM-043).

**Relevance to our OS.** high. With no legacy, the native name type can be defined (for example, valid UTF-8 with no control characters), and the POSIX layer then has to decide how to map names that violate it.

**Sources.** [S633], [S641], [S648], [S649], [S650], [S651], [S652]

### DM-096: UCS-2/UTF-16 as the native Windows NT string type, with duplicated A/W APIs

- **Category:** Text, encoding and locale
- **Systems affected:** Windows NT family (Win32, NT kernel, NTFS); also languages/runtimes that adopted 16-bit chars [unverified: Java, JavaScript not researched here]
- **Classification:** right-at-the-time-wrong-now
- **Confidence:** medium

**What the design is.** Windows represents text internally as 16-bit `WCHAR` units, now interpreted as UTF-16 [S655][S658]. Each text-taking Win32 function comes in an "A" version (bytes in the active ANSI code page), a "W" version (UTF-16), and a generic macro selected by `UNICODE` [S665][S654].

**Why it was chosen.** When NT was designed, Unicode 1.x promised every character in 16 bits, so a fixed-width wide character looked like the simple, final answer. "UCS-2" describes implementations up to Unicode 1.1, before surrogates and UTF-16 arrived in Unicode 2.0 [S656]. Keeping "A" functions let existing code-page software run unchanged [S658].

**Why it is a problem.** The fixed-width premise failed: supplementary characters need surrogate pairs, so UTF-16 is variable-width anyway [S655][S656]. At the file system level, names are arbitrary arrays of 16-bit values, and unpaired surrogates are accepted [S657]. Names therefore need not be valid Unicode, and Rust has to store Windows OS strings in a "less-strict variant of UTF-8" [S652]. The A APIs convert through the active code page, so characters outside it are lost or fail to round-trip. Python had to switch all Windows path handling to W APIs to fix this [S658]. Two API surfaces double the documentation and testing burden, and the system ANSI code page stays legacy by default [S635].

**Why it was not fixed.** The ABI is frozen. Microsoft's route to UTF-8 reuses the A APIs: since Windows 10 1903 an app can declare `activeCodePage` UTF-8 in its manifest. But GDI ignores the per-process setting and requires a system-wide "Beta" option, and `CP_ACP` means UTF-8 only under that opt-in [S653]. Python made UTF-8 mode the default only in 3.15 [S635].

**Alternatives that exist.** UTF-8 as the interchange/native encoding with byte-oriented APIs. Microsoft now recommends UTF-8 for portability with *nix and the web [S653]. Plan 9's use of UTF-8 as native encoding is relevant but was not verified here [unverified]. APFS stores names as UTF-8 [S641].

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

**Why it is a problem.** The locale is ambient global state. `setlocale` is MT-Unsafe [S659]. Python's docs warn that calling it from a library changes the whole program, that save-and-restore races with other threads, and that some implementations crash on frequent changes [S664]. Functions used for machine formats then depend on user settings. C++'s `to_chars`/`from_chars` proposal lists `sprintf`, `strtol`, `atol` and `stringstream` as all locale-affected and unsuitable for JSON/XML interchange, hence new locale-independent primitives [S663]. The default "C" locale implied ASCII. Python needed PEP 538 to coerce it to UTF-8, and doing so diverges from other components in the same process and in subprocesses (e.g. readline) [S662]. One process cannot easily format a machine-readable number and a user-facing one with different conventions.

**Why it was not fixed.** The C standard defines the locale as program-wide, and every C library inherits that [S664]. POSIX.1-2008 added thread-local locale objects (`newlocale`, `uselocale`, `duplocale`) instead of changing `setlocale` [S660][S661]. These coexist with the global locale (`LC_GLOBAL_LOCALE`) and have sharp edges: modifying a locale object may free the base object [S661].

**Alternatives that exist.** Explicit locale arguments or objects (`*_l` functions, `uselocale`) [S660][S661]. Locale-independent primitives for machine formats (C++17 `to_chars`/`from_chars`) [S663]. Runtimes that force UTF-8 regardless of locale (Python UTF-8 mode) [S635][S662].

**Cost of the alternatives.** Explicit locale parameters clutter every text API. Ignoring the user's locale can break interop with legacy tools that do honor it [S662]. Splitting "machine" and "human" formatting APIs doubles the surface.

**Relevance to our OS.** medium. Native APIs can keep localization out of core parsing and formatting entirely. The POSIX layer must still emulate `setlocale` for hosted C programs and the compiler.

**Sources.** [S635], [S659], [S660], [S661], [S662], [S663], [S664]

## Stream notes

**Searched / fetched.** fcntl/fcntl_locking man pages, POSIX.1-2024 fcntl and XBD ch.3/ch.4, Layton's LWN article and LKML patch threads, SQLite os_unix.c. LWN fsyncgate coverage, Ringer's pgsql-hackers post, PostgreSQL wiki, Rebello ATC'20, LWN errseq_t, Apple fsync(2). LWN ext4 delalloc coverage, Ts'o's Launchpad comment, Pillai OSDI'14 (PDF text extracted locally). openat2(2), LWN 2026 path-traversal article, CVE-2018-15664, Fuchsia "dotdot", Pike's lexical names paper, TxOS SOSP'09 (PDF extracted locally). Kernel sysctl docs and Kees Cook link-restriction patches. mount_namespaces(7), user_namespaces(7), Plan 9 papers, Canonical userns post. LWN atime articles, mount(8), NTFS fsutil. Microsoft MAX_PATH / naming / CreateFile / Restart Manager / FILE_DISPOSITION_INFORMATION_EX / TxF deprecation / WSL case sensitivity / UTF-8 code page / A-W prototypes / surrogates docs. Raymond Chen, Michael Kaplan. Btrfs transaction-ioctl deprecation commit. Apple APFS FAQ, CVE-2014-9390, LWN case-insensitive ext4, 2025 LKML bcachefs thread (Torvalds/Overstreet/Anvin), Austin Group bug 251, Colomar/Ts'o LKML 2025, Wheeler essay. Unicode UTF FAQ, PEP 529, PEP 538, P0067R5, Python locale docs, glibc setlocale/uselocale/newlocale man pages, Rust OsString docs.

**Fetch method note.** A tool rate-limit interrupted WebFetch partway through. Some later sources (Austin bug 251, Microsoft UTF-8/A-W/surrogate docs, man pages for setlocale/uselocale/newlocale, PEPs, P0067R5, Rust docs, Python locale docs, POSIX XBD ch.3, btrfs commit, LWN btrfs-atime) were fetched with curl and their HTML stripped and read locally. Two PDFs (Pillai, TxOS) were text-extracted with Ghostscript. All are genuinely fetched; none are marked unverified.

**Dropped/merged.** None dropped. DM-040 was narrowed to the security problems of hard links (the hardlink-to-inaccessible-file issue); broader "semantic complications" claims (backup tools, directory hard links) could not be sourced and are marked [unverified] or omitted.

**New entries.** None added (reserve DM-150–DM-159 unused). Candidates considered but not written for lack of time/sources: Darwin `fsync` vs `F_FULLFSYNC` (folded into DM-037), 8.3 short-name aliases (folded into DM-043).

**Open questions.**
- Whether Plan 9 file servers support hard links at all (DM-040) — not verified.
- Plan 9 native UTF-8 (DM-096) — not verified in this stream; streams J/K may have a source.
- Exact merge version of protected_hardlinks/symlinks (3.6?) not confirmed.
- Windows reparse-point TOCTOU specifics (DM-039) and frequency of sharing-violation failures caused by AV/indexers (DM-044) not sourced; marked [unverified].
- DM-045 contested: TxOS's low overhead vs two abandoned production attempts; not resolvable from sources.

**Weak/caution sources.** S648 (Wheeler essay) is weak and is used only as a catalogue of failure patterns; the binding claims cite S649/S651. S626 (Canonical blog) relays a Google statistic (44%) that was not checked against Google's report. S608 (PostgreSQL wiki) is a living community page with no fixed date. S657 (Kaplan) is an archived mirror of an MSDN blog. S621 is the patch text as posted on LWN, labelled primary.
