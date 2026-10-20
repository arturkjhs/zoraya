# Stream F sources (S600–S699)

[S600] Linux man-pages project, fcntl_locking(2) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man2/fcntl_locking.2.html, primary, checked 2026-10-19
  - supports: DM-036 (process-owned locks released on close of any fd; not inherited by fork; threads share locks; OFD locks since 3.15; no deadlock detection for OFD)
  - supports: DM-044 (Linux mandatory locking unreliable, optional since 4.5, removed in 5.15)
[S601] Jeffrey T. Layton, File-private POSIX locks, LWN.net, 2014, https://lwn.net/Articles/586904/, secondary, checked 2026-10-19
  - supports: DM-036 (close semantics, library code silently dropping locks, threads, flock whole-file only, Samba/Ganesha/SQLite/OpenJDK interest, rename to OFD)
[S602] SQLite developers, src/os_unix.c (comment "Posix Advisory Locking"), SQLite source tree (GitHub mirror), current trunk, https://raw.githubusercontent.com/sqlite/sqlite/master/src/os_unix.c, primary, checked 2026-10-19
  - supports: DM-036 (POSIX locks "broken by design"; unixInodeInfo per-inode tracking; deferred close via pUnused list)
[S603] The Open Group / IEEE, fcntl — POSIX.1-2024 (IEEE Std 1003.1-2024), pubs.opengroup.org, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/functions/fcntl.html, primary, checked 2026-10-19
  - supports: DM-036 (process-owned locks removed when any fd for the file is closed; F_OFD_* now standardized)
[S604] Jonathan Corbet, PostgreSQL's fsync() surprise, LWN.net, 2018, https://lwn.net/Articles/752063/, secondary, checked 2026-10-19
  - supports: DM-037 (PG assumption; pages marked clean; errors before open() not seen; Ts'o USB rationale; Chinner/Ts'o direct-I/O advice; OpenBSD/NetBSD similar)
[S605] Craig Ringer, PostgreSQL's handling of fsync() errors is unsafe and risks data loss at least on XFS, pgsql-hackers mailing list, 2018, https://www.postgresql.org/message-id/CAMsr%2BYHh%2B5Oq4xziwwoEfhoTZgr07vdGG%2Bhu%3D1adXx59aTeaoQ%40mail.gmail.com, primary, checked 2026-10-19
  - supports: DM-037 (retry-after-EIO silently succeeds; observed XFS corruption; proposal to PANIC; POSIX silent on post-failure state)
[S606] Anthony Rebello, Yuvraj Patel, Ramnatthan Alagappan, Andrea C. Arpaci-Dusseau, Remzi H. Arpaci-Dusseau, Can Applications Recover from fsync Failures?, USENIX ATC 2020, 2020, https://www.usenix.org/conference/atc20/presentation/rebello, primary, checked 2026-10-19
  - supports: DM-037 (ext4/XFS/Btrfs mark pages clean after failure; none of PostgreSQL, LMDB, LevelDB, SQLite, Redis handle it adequately)
[S607] Jonathan Corbet, Improved block-layer error handling, LWN.net, 2017, https://lwn.net/Articles/724307/, secondary, checked 2026-10-19
  - supports: DM-037 (errseq_t: error reported once per open file description; "will not be perfect")
[S608] PostgreSQL community, Fsync Errors, PostgreSQL Wiki, n.d. (living page), https://wiki.postgresql.org/wiki/Fsync_Errors, primary, checked 2026-10-19
  - supports: DM-037 (PANIC fix in 12 backpatched to 9.4; per-OS table: FreeBSD keeps buffers dirty, Linux/macOS/NetBSD/OpenBSD invalidate)
[S609] Apple Inc., fsync(2) — Mac OS X / iOS Manual Page, Apple Developer Documentation Archive, n.d., https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html, primary, checked 2026-10-19
  - supports: DM-037 (Darwin fsync does not flush drive cache; F_FULLFSYNC needed)
[S610] Jonathan Corbet, ext4 and data loss, LWN.net, 2009, https://lwn.net/Articles/322823/, secondary, checked 2026-10-19
  - supports: DM-038 (zero-length files after crash; ext3 data=ordered protection accidental; 2.6.30 rename/truncate heuristics)
[S611] Theodore Ts'o, Comment #45 on Ubuntu bug 317781 (ext4 data loss), Launchpad, 2009, https://bugs.launchpad.net/ubuntu/+source/linux/+bug/317781/comments/45, primary, checked 2026-10-19
  - supports: DM-038 (POSIX leaves unclean-shutdown state undefined; Firefox fsync cost; heuristics in 2.6.30; advice to use SQLite-like stores)
[S612] Thanumalayan Sankaranarayana Pillai, Vijay Chidambaram, Ramnatthan Alagappan, Samer Al-Kiswany, Andrea C. Arpaci-Dusseau, Remzi H. Arpaci-Dusseau, All File Systems Are Not Created Equal: On the Complexity of Crafting Crash-Consistent Applications, OSDI 2014, 2014, https://www.usenix.org/conference/osdi14/technical-sessions/presentation/pillai, primary, checked 2026-10-19
  - supports: DM-038 (60 vulnerabilities in 11 applications; persistence properties differ across 6 Linux file systems; Git append-then-rename example)
  - supports: DM-045 (applications hand-roll crash consistency and get it wrong)
[S613] Linux man-pages project, openat2(2) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man2/openat2.2.html, primary, checked 2026-10-19
  - supports: DM-039 (RESOLVE_* semantics; Linux 5.6; extensible open_how; EAGAIN on possible ".." race)
[S614] Daroc Alden, The difficulty of safe path traversal, LWN.net, 2026, https://lwn.net/Articles/1050887/, secondary, checked 2026-10-19
  - supports: DM-039 (14 runc CVEs since 2017; procfs overmounts; openat2 often blocked by seccomp; libpathrs; move to fd-based designs)
[S615] NIST NVD, CVE-2018-15664 (Docker cp symlink-exchange race), National Vulnerability Database, 2019, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2018-15664, primary, checked 2026-10-19
  - supports: DM-039 (symlink-exchange + traversal race in docker cp gives root read-write host access)
[S616] Fuchsia project, Dot Dot Considered Harmful, fuchsia.dev, n.d., https://fuchsia.dev/fuchsia-src/concepts/filesystems/dotdot, primary, checked 2026-10-19
  - supports: DM-039 (no ".." on servers; directory handle confines to subtree; client-side canonicalization relies on absence of symlinks)
[S617] Donald E. Porter, Owen S. Hofmann, Christopher J. Rossbach, Alexander Benn, Emmett Witchel, Operating System Transactions, SOSP 2009, 2009, https://www.cs.utexas.edu/users/witchel/pubs/porter09sosp-txos.pdf, primary, checked 2026-10-19
  - supports: DM-039 (>600 NVD "symlink attack" hits; TOCTTOU eliminable only by API change; Linux added close-on-exec flag to 15 syscalls)
  - supports: DM-045 (TxOS: ACID system transactions over Linux 2.6.22; ~10% overhead on transactional dpkg install; Windows TxF/TxR mentioned)
[S618] Rob Pike, Lexical File Names in Plan 9 or Getting Dot-Dot Right, USENIX ATC 2000, 2000, https://www.usenix.org/legacy/event/usenix2000/general/full_papers/pikelex/pikelex_html/index.html, primary, checked 2026-10-19
  - supports: DM-039 (symlinks make ".." ambiguous; Plan 9 lexical ".."; kernel-tracked names; fd2path)
[S619] Linux kernel documentation, Documentation for /proc/sys/fs/ (protected_hardlinks, protected_symlinks, protected_fifos, protected_regular), docs.kernel.org, current, https://docs.kernel.org/admin-guide/sysctl/fs.html, primary, checked 2026-10-19
  - supports: DM-039 ("long-standing class" of symlink TOCTOU races; sticky-dir restrictions)
  - supports: DM-040 (protected_hardlinks semantics)
[S620] Kees Cook, [PATCH v2] fs: hardlink creation restrictions, linux-kernel mailing list (lkml.iu.edu archive), 2012, https://lkml.iu.edu/hypermail/linux/kernel/1202.2/02340.html, primary, checked 2026-10-19
  - supports: DM-040 (hardlink TOCTOU across privilege boundaries; pinning vulnerable setuid files; POSIX permits restriction; "at" breakage; Ubuntu 1.5 years)
[S621] Kees Cook (posted by LWN), [PATCH 1/2] fs: add link restrictions, LWN.net patch posting, 2012, https://lwn.net/Articles/503671/, primary, checked 2026-10-19
  - supports: DM-040 (history of discussions 1996–2010; objections and rebuttals: POSIX, breakage, mkstemp, LSM-vs-VFS; based on Openwall/grsecurity)
[S622] Linux man-pages project, mount_namespaces(7) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man7/mount_namespaces.7.html, primary, checked 2026-10-19
  - supports: DM-041 (CLONE_NEWNS in 2.4.19; shared subtrees in 2.6.15 because full isolation too strict; locked mounts in less-privileged namespaces)
[S623] Linux man-pages project, user_namespaces(7) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man7/user_namespaces.7.html, primary, checked 2026-10-19
  - supports: DM-041 (unprivileged userns since 3.8; which filesystems mountable; block filesystems still need init-ns CAP_SYS_ADMIN; privilege restricted to protect set-user-ID programs)
[S624] Rob Pike, Dave Presotto, Ken Thompson, Howard Trickey, Phil Winterbottom, The Use of Name Spaces in Plan 9, Bell Labs (reprinted in Operating Systems Review), 1992, https://9p.io/sys/doc/names.html, primary, checked 2026-10-19
  - supports: DM-041 (per-process name spaces, no global namespace, bind/mount/union dirs, per-window /dev/cons; reliance on conventions)
[S625] Rob Pike, Dave Presotto, Sean Dorward, Bob Flandrena, Ken Thompson, Howard Trickey, Phil Winterbottom, Plan 9 from Bell Labs, Bell Labs, n.d. (c. 1995), https://9p.io/sys/doc/9.html, primary, checked 2026-10-19
  - supports: DM-041 (user processes build name spaces with mount/bind/unmount; "Plan 9 has no super-user")
[S626] Ijlal Loutfi, Ubuntu 23.10 restricted unprivileged user namespaces, Canonical blog, 2023, https://canonical.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces, secondary, checked 2026-10-19
  - supports: DM-041 (unprivileged userns exposes root-only kernel interfaces; cited Google figure: 44% of exploits needed them; AppArmor gating)
[S627] Jonathan Corbet, Once upon atime, LWN.net, 2007, https://lwn.net/Articles/244829/, secondary, checked 2026-10-19
  - supports: DM-042 (every read becomes a write; Molnar quotes; mutt/tmpwatch/backup dependence; relatime since 2.6.20; Linus's one-day interval)
[S628] Linux man-pages project / util-linux, mount(8) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man8/mount.8.html, primary, checked 2026-10-19
  - supports: DM-042 (relatime default since 2.6.30; 1-day rule; strictatime; lazytime)
[S629] The Open Group / IEEE, Base Definitions ch. 4 General Concepts, "File Times Update" — POSIX.1-2024, pubs.opengroup.org, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html, primary, checked 2026-10-19
  - supports: DM-042 (marked-for-update model; periodic update allowed; required update points)
[S630] Microsoft, fsutil behavior, Microsoft Learn, 2017 (updated 2026), https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/fsutil-behavior, primary, checked 2026-10-19
  - supports: DM-042 (NTFS disablelastaccess; NTFS defers last-access writes up to one hour)
  - supports: DM-043 (disable8dot3; 8.3 short-name generation cost)
  - supports: DM-045 ("TxF is presently deprecated, but the functionality is still available")
[S631] Jonathan Corbet, Atime and btrfs: a bad combination?, LWN.net, 2012, https://lwn.net/Articles/499293/, secondary, checked 2026-10-19
  - supports: DM-042 (atime on CoW + snapshots unshares metadata; reads consume space; relatime may not help)
[S632] Microsoft, Maximum Path Length Limitation, Microsoft Learn, 2024, https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation, primary, checked 2026-10-19
  - supports: DM-043 (MAX_PATH 260; \\?\ 32,767; opt-in requires registry AND manifest; relative paths always limited; shell vs FS mismatch; git example)
  - supports: DM-094 (Windows FS treats names as opaque WCHAR sequences; no normalization)
[S633] Microsoft, Naming Files, Paths, and Namespaces, Microsoft Learn, 2024, https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file, primary, checked 2026-10-19
  - supports: DM-043 (reserved names incl. with extension and superscript digits; no trailing space/period; \\?\ disables parsing; NT vs Win32 namespaces)
  - supports: DM-094 (case-insensitive by default; NTFS supports case-sensitive semantics)
  - supports: DM-095 (Windows forbids bytes 1–31 and reserved punctuation in names)
[S634] Raymond Chen, What's the deal with those reserved filenames like NUL and CON?, The Old New Thing (Microsoft devblogs), 2003, https://devblogs.microsoft.com/oldnewthing/20031022-00/?p=42073, secondary, checked 2026-10-19
  - supports: DM-043 (DOS 1.0 had no directories; magic names had to work with extensions; kept because batch files rely on them)
[S635] Python Software Foundation, Using Python on Windows (sections 4.6 Removing the MAX_PATH Limitation, 4.7 UTF-8 mode), docs.python.org, 2026, https://docs.python.org/3/using/windows.html, primary, checked 2026-10-19
  - supports: DM-043 (long paths need admin policy/registry; Python enabled support in 3.6)
  - supports: DM-096 (Windows still uses legacy ANSI code page as system encoding; UTF-8 mode default from 3.15; PEP 528/529)
  - supports: DM-097 (UTF-8 mode ignores locale/system encoding for text files)
[S636] Microsoft, CreateFileW function (fileapi.h), Microsoft Learn, 2022, https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew, primary, checked 2026-10-19
  - supports: DM-044 (dwShareMode semantics; 0 = no subsequent opens; ERROR_SHARING_VIOLATION; FILE_SHARE_DELETE governs delete and rename)
[S637] Microsoft, About Restart Manager, Microsoft Learn, 2018, https://learn.microsoft.com/en-us/windows/win32/rstmgr/about-restart-manager, primary, checked 2026-10-19
  - supports: DM-044 ("primary reason" installs need restart is files in use; Restart Manager shuts apps down)
[S638] Microsoft, FILE_DISPOSITION_INFORMATION_EX structure (ntddk.h), Microsoft Learn, 2021, https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex, primary, checked 2026-10-19
  - supports: DM-044 (FILE_DISPOSITION_POSIX_SEMANTICS: name removed while existing handles keep data; STATUS_CANNOT_DELETE with mapped view)
[S639] Microsoft, Alternatives to using Transactional NTFS, Microsoft Learn, 2018, https://learn.microsoft.com/en-us/windows/win32/fileio/deprecation-of-txf, primary, checked 2026-10-19
  - supports: DM-045 (TxF since Vista; "extremely limited developer interest"; complexity; may be removed; ReplaceFile/ESE/SQL alternatives)
[S640] Nikolay Borisov (reviewed by David Sterba), btrfs: Deprecate userspace transaction ioctls, Linux kernel commit 3558d4f88ec8 (GitHub mirror), 2017, https://github.com/torvalds/linux/commit/3558d4f88ec8, primary, checked 2026-10-20
  - supports: DM-045 (btrfs user transactions were added for Ceph; no widespread legitimate use; slated for removal in 4.17)
[S641] Apple Inc., Apple File System Guide — Frequently Asked Questions (retired document), Apple Developer Documentation Archive, n.d. (c. 2017–2018), https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html, primary, checked 2026-10-19
  - supports: DM-094 (HFS+ stores NFD-normalized names (Unicode 3.2); APFS preserves and hashes normalized form (Unicode 9.0); iOS 10.3 APFS was normalization-sensitive causing ENOENT; case-insensitive default on macOS)
  - supports: DM-095 (APFS stores names in UTF-8; rejects unassigned Unicode 9.0 code points)
  - supports: DM-096 (APFS UTF-8 vs HFS UTF-16 on-disk names)
[S642] NIST NVD, CVE-2014-9390 (Git .git/config overwrite on case-insensitive / normalizing file systems), National Vulnerability Database, 2014, https://services.nvd.nist.gov/rest/json/cves/2.0?cveId=CVE-2014-9390, primary, checked 2026-10-19
  - supports: DM-094 (ignorable Unicode codepoint, git~1 8.3 alias, mixed case on case-insensitive FS lead to arbitrary command execution)
[S643] Vicent Martí, Vulnerability announced: update your Git clients, GitHub Blog, 2014, https://github.blog/news-insights/company-news/vulnerability-announced-update-your-git-clients/, secondary, checked 2026-10-19
  - supports: DM-094 (affects HFS+, NTFS, FAT clients; fixed versions; Linux case-sensitive unaffected)
[S644] Jake Edge, Case-insensitive ext4, LWN.net, 2019, https://lwn.net/Articles/784041/, secondary, checked 2026-10-19
  - supports: DM-094 (motivations: Android /sdcard, Samba interoperability; per-directory flag; NFD internal comparison; Torvalds objection; invalid UTF-8 treated as opaque)
[S645] Kent Overstreet, Re: [GIT PULL] bcachefs fixes for 6.15-rc4 (quoting Linus Torvalds), linux-kernel mailing list (lkml.rescloud.iu.edu archive), 2025, https://lkml.rescloud.iu.edu/2504.3/02103.html, primary, checked 2026-10-19
  - supports: DM-094 (Torvalds: case-insensitive names "horribly wrong"; Overstreet: user/application needs justify them)
[S646] H. Peter Anvin, Re: [GIT PULL] bcachefs fixes for 6.15-rc4 (quoting Linus Torvalds), linux-kernel mailing list (lkml.rescloud.iu.edu archive), 2025, https://lkml.rescloud.iu.edu/2504.3/05029.html, primary, checked 2026-10-19
  - supports: DM-094 (Torvalds: compatibility only valid reason, prefer fixed 1:1 table, HFS "almost NFD" rejected; Anvin: Microsoft froze its upcase table, 1:1 BMP only)
[S647] Microsoft, Case Sensitivity (WSL), Microsoft Learn, 2021 (updated 2026), https://learn.microsoft.com/en-us/windows/wsl/case-sensitivity, primary, checked 2026-10-19
  - supports: DM-094 (per-directory case sensitivity since build 17107; Windows apps that change case break; files differing only by case inaccessible to Windows apps)
[S648] David A. Wheeler, Fixing Unix/Linux/POSIX Filenames: Control Characters (such as Newline), Leading Dashes, and Other Problems, dwheeler.com (personal essay), 2009 (updated 2024), https://dwheeler.com/essays/fixing-unix-linux-filenames.html, weak, checked 2026-10-19
  - supports: DM-095 (catalogue of failure patterns: newline, leading dash, non-UTF-8, find|while read; proposed fixes) — used as pointer/anecdote only
[S649] David A. Wheeler (reporter), Austin Group, Bug 0000251: Forbid newline, or even bytes 1 through 31 (inclusive), in filenames, Austin Group Defect Tracker, 2010–2024, https://www.austingroupbugs.net/view.php?id=251, primary, checked 2026-10-19
  - supports: DM-095 (rationale: vulnerabilities and erroneous programs; NTFS already forbids 1–31; accepted resolution: implementations "encouraged" to fail with EILSEQ on newline; future version may require it)
[S650] Alejandro Colomar, Re: newlines in filenames; POSIX.1-2024 (reply to Theodore Ts'o), linux-kernel mailing list (lkml.rescloud.iu.edu archive), 2025, https://lkml.rescloud.iu.edu/2504.3/00150.html, primary, checked 2026-10-19
  - supports: DM-095 (Ts'o objections: fragmentation, control chars printable in cp437, kernel doesn't know locale; Colomar: optional mode forbidding 0–31)
  - supports: DM-094 (Ts'o: adding Unicode case-folding tables caused ongoing headaches)
[S651] The Open Group / IEEE, Base Definitions ch. 3 Definitions (3.264 Portable Filename, 3.265 Portable Filename Character Set, Pathname) — POSIX.1-2024, pubs.opengroup.org, 2024, https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap03.html, primary, checked 2026-10-20
  - supports: DM-095 (portable set is A–Z a–z 0–9 . _ -; avoid leading hyphen; pathname "might only be a string" rather than character string)
[S652] The Rust Project, std::ffi::OsString, Rust standard library documentation, current, https://doc.rust-lang.org/std/ffi/struct.OsString.html, primary, checked 2026-10-19
  - supports: DM-095 (Unix strings are arbitrary non-zero bytes; separate OS string type needed)
  - supports: DM-096 (Windows strings are arbitrary 16-bit sequences, stored internally in a less-strict UTF-8 variant)
[S653] Microsoft, Use UTF-8 code pages in Windows apps, Microsoft Learn, current, https://learn.microsoft.com/en-us/windows/apps/design/globalizing/use-utf8-code-page, primary, checked 2026-10-19
  - supports: DM-096 (activeCodePage UTF-8 since 1903; GDI ignores per-process setting; -A APIs reused for UTF-8; Windows natively UTF-16; CP_ACP legacy)
[S654] Microsoft, Conventions for Function Prototypes, Microsoft Learn, current, https://learn.microsoft.com/en-us/windows/win32/intl/conventions-for-function-prototypes, primary, checked 2026-10-19
  - supports: DM-096 (generic/A/W triple via UNICODE macro; some functions W-only)
[S655] Microsoft, Surrogates and Supplementary Characters, Microsoft Learn, current, https://learn.microsoft.com/en-us/windows/win32/intl/surrogates-and-supplementary-characters, primary, checked 2026-10-19
  - supports: DM-096 (Windows uses UTF-16; 16 bits insufficient; surrogate pairs required for supplementary characters)
[S656] Unicode Consortium, FAQ — UTF-8, UTF-16, UTF-32 & BOM, unicode.org, current, https://www.unicode.org/faq/utf_bom.html, primary, checked 2026-10-19
  - supports: DM-096 (UCS-2 = implementations up to Unicode 1.1, before surrogates/UTF-16 added in 2.0; "UCS-2" implementations don't handle supplementary characters)
[S657] Michael S. Kaplan, UCS-2 vs. UTF-16 (not quite Kramer vs. Kramer), Sorting It All Out (MSDN blog, archived), 2005, https://archives.miloush.net/michkap/archive/2005/05/11/416552.html, secondary, checked 2026-10-20
  - supports: DM-096 (CreateFileW level takes arbitrary WCHAR arrays incl. unpaired surrogates; Windows 2000 "surrogate neutral"; XP UTF-16 support)
[S658] Steve Dower, PEP 529 — Change Windows filesystem encoding to UTF-8, Python Enhancement Proposals, 2016, https://peps.python.org/pep-0529/, primary, checked 2026-10-20
  - supports: DM-096 (ANSI APIs encode via active code page; lossy round-trip for characters outside the code page; OS represents all text as UTF-16)
[S659] Linux man-pages project, setlocale(3) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man3/setlocale.3.html, primary, checked 2026-10-19
  - supports: DM-097 (program-wide locale; "C" at startup; env-var driven; MT-Unsafe; C89/POSIX.1-2001 origin)
[S660] Linux man-pages project, uselocale(3) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man3/uselocale.3.html, primary, checked 2026-10-19
  - supports: DM-097 (per-thread locale; glibc 2.3; POSIX.1-2008)
[S661] Linux man-pages project, newlocale(3) — Linux manual page, man7.org, 2025, https://man7.org/linux/man-pages/man3/newlocale.3.html, primary, checked 2026-10-19
  - supports: DM-097 (locale objects; base may be freed on modify; LC_GLOBAL_LOCALE base undefined)
[S662] Alyssa Coghlan, PEP 538 — Coercing the legacy C locale to a UTF-8 based locale, Python Enhancement Proposals, 2016, https://peps.python.org/pep-0538/, primary, checked 2026-10-20
  - supports: DM-097 (C locale implies ASCII; divergence between components in one process and subprocesses; readline interop)
[S663] Jens Maurer, P0067R5: Elementary string conversions, revision 5, ISO/IEC JTC1/SC22/WG21, 2016, https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2016/p0067r5.html, primary, checked 2026-10-20
  - supports: DM-097 (sprintf/strtol/atol/stringstream all locale-dependent; to_chars/from_chars designed locale-independent for JSON/XML)
[S664] Python Software Foundation, locale — Internationalization services, docs.python.org, 2026, https://docs.python.org/3/library/locale.html, primary, checked 2026-10-20
  - supports: DM-097 (C standard makes locale program-wide; setlocale not thread-safe; calling it in a library affects whole program; some implementations crash on frequent changes)
[S665] Microsoft, Unicode in the Windows API, Microsoft Learn, 2021, https://learn.microsoft.com/en-us/windows/win32/intl/unicode-in-the-windows-api, primary, checked 2026-10-19
  - supports: DM-096 (three formats: generic, A ("ANSI" code page), W ("wide"); some newer functions Unicode-only)
