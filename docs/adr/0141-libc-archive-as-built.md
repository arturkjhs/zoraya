# ADR-0141: `libc.a` as built: the files, the shim, hidden names and the check

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0005, ADR-0100, ADR-0109, ADR-0140
- **Evidence:** CP-27, DM-057, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#qnx, stage-6/sources.md#libc-implementations, task-02/libc-surface.csv

## Context

ADR-0100 item 2 fixes the cut between musl's retained C and the Rust half and lists the `SYS_*` names the shim
header routes "the few `syscall(SYS_x, ...)` uses inside the retained files" to; its open questions say the exact list
is known only once the shim exists, and ask whether `__init_libc`'s `AT_SECURE` path should be compiled out. Item 7
says every defined global symbol of `libc.a` not beginning with `_` must be in the tier list, and INV-0100-1 that the
check runs under `make test`. TASK-17 built the archive (musl 1.2.5 from `stage-6/sources.md#libc-implementations`,
Apple clang 21 on the M4) and found where the accepted text and the code part. This ADR records each place.

**What is measured.**

1. **The shim.** With every `SYS_*` left undefined, compiling the retained directories names exactly these calls:
   `read`, `readv`, `writev`, `close`, `fcntl`, `ioctl`, `mmap` (as `mmap2` in `__init_tls`), `set_tid_address`,
   `exit_group`, `exit` (`_Exit` loops on it after `exit_group`), and `futex` (in `__wake` and `__futexwait`, inline
   functions of `src/internal/pthread_impl.h` that every file including it compiles). ADR-0100's list had `lseek`,
   `clock_gettime` and `getpid`, which no retained file calls directly (`__stdio_seek` calls `__lseek`, and `time`
   calls `__clock_gettime`, both of replaced directories), and lacked `read`, `exit` and `futex`.
2. **Files that make other system calls.** 34 files of the retained directories do, and are not compiled
   (`ports/musl/excluded.txt`): process and resource state (`sysconf`, `abort`, `getrlimit`, `setrlimit`, `uname`,
   `ioctl`, `syscall`, ...), the stdio files that open, remove or rename by path (`fopen`, `remove`, `rename`,
   `tmpfile`, `popen`'s `pclose`, `__fopen_rb_ca`), and the clocks, timers and time-zone file of `src/time`.
3. **Directories the cut does not name.** `src/internal` (the scanners under `strtod` and `strtol`, `__syscall_ret`,
   `libc.c`), `src/conf` (`confstr`, `sysconf`) and `src/temp` (`mkstemp`, over `open`) are in neither list.
4. **The secure start.** `__libc_start_main`'s `AT_SECURE` path polls fds 0 to 2 with `SYS_poll` or `SYS_ppoll` and
   opens `/dev/null`: neither is a routed call.
5. **musl's public helpers.** Retained files define 750 global names outside the tier list
   (`task-02/libc-surface.csv` by item 7's rule): `frexpl` and `scalbnl`
   under `printf`, `strlcpy`, `regcomp`, the whole of `src/math`, `getpwnam`'s `nscd` client, and so on; the Rust
   static library adds `compiler_builtins`' exports (`bcmp`, `sin`, ...) and `rust_begin_unwind`. Every one of them
   violates item 7 as written, and most are called by tier functions, so they cannot simply be left out.
6. **The signal directory.** ADR-0100's cost estimate counts `sigemptyset`, `sigaddset` and `sigfillset` as musl C
   that stays; item 2 replaces `src/signal` whole.
7. **The `zoraya_posix_*` names.** Item 2 routes the shim to `zoraya_posix_*` functions, whose names do not begin
   with `_`, so item 7 read alone would count them as exports.
8. **The check's place.** The export check reads `build/sysroot/usr/lib/libc.a`, which only `make ports` builds;
   `make test` builds no port (TASK-17: "long builds stay behind `make ports`").

## Decision drivers

1. **The archive's surface is the tier list, enforced by a tool** (CP-27, INV-0005-1, INV-0100-1).
2. **No retained file reaches Linux** (INV-0100-2): a call the shim does not know must fail to compile.
3. **Keep musl unchanged where possible**: each patch is carried across musl versions (ADR-0100, Evidence against B).
4. **The stable boundary is the libc's, not the system calls'** (DM-057): programs see the tier list's functions, and
   nothing else of musl.

## Considered options

### Option A: Export what musl exports (baseline)

- **Description:** Build musl's retained files as musl's own Makefile does and accept every public name; check only
  the deny list.
- **Evidence for:** No renaming; programs could call any of musl's 1,613 functions that compile.
- **Evidence against:** Drops INV-0005-1 and INV-0100-1 in effect: the surface would be musl's, not the measured one
  (CP-27); 6c would have to keep every helper working.
- **Cost:** Least work; an unbounded surface.
- **Precedent:** musl on Linux; Fuchsia's libc exports musl's names over fdio
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option B: Compile only the files the tier list needs

- **Description:** Start from the files that define tier names and add the files defining what they call, until
  closed.
- **Evidence for:** A smaller archive.
- **Evidence against:** The closure still contains public helpers (`frexpl` under `printf`, the `nscd` client under
  `getpwnam`), so item 7 still fails; and it departs from item 2's directory list.
- **Cost:** *(estimate)* A closure tool and the same hiding problem.
- **Precedent:** None in the files.

### Option C: Compile the directories, hide every name outside the list (recommended)

- **Description:** Compile ADR-0100 item 2's retained directories with the shim; leave out the files the shim
  refuses, by an explicit list; then rename every defined global that is not in the tier list, not internal and not a
  `zoraya_posix_*` name to `__zoraya_<name>`, in every object that defines or uses it, before archiving.
- **Evidence for:** Drivers 1 to 4. Internal calls keep working (both ends are renamed); a program that calls a hidden
  function fails to link, which is how a port shows a need beyond the measured surface. QNX's libc likewise keeps its
  own internals out of its interface (`alternatives-postmortem.md#qnx`).
- **Evidence against:** An extra build step that rewrites objects; the hidden code is still in the archive and still
  has to be correct when a tier function calls it.
- **Cost:** *(estimate)* 120 lines of Rust in `posix-surface-check` (`hide`), as built.
- **Precedent:** Symbol versioning and visibility maps do the same in shared libraries; Fuchsia's libc
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

## Recommendation

**Recommendation (not a decision): Option C**, with these readings of ADR-0100:

1. **Files.** The retained directories are item 2's, plus `src/internal`, `src/conf` and `src/temp`, whose files are
   C over the shim or over public functions; a file is left out exactly when the shim refuses it, and
   `ports/musl/excluded.txt` lists each with the reason. Item 2's two exceptions stay the only files taken from
   replaced directories.
2. **The shim** (`ports/musl/zoraya/syscall.h`, replacing `src/internal/syscall.h`) routes the eleven calls of what is
   measured, item 1, each to `zoraya_posix_<call>(long ...)` returning a value or `-errno`; `exit` goes to
   `zoraya_posix_exit_group` and `mmap2` to `zoraya_posix_mmap`. `futex` is answered with `-ENOSYS` in the header
   itself, without a call: a process has one thread (ADR-0100 item 5), so no futex has a waiter and no lock is
   contended. Any other call is an undeclared function and an error.
3. **The secure start is compiled out** (`ports/musl/zoraya/0001-no-secure-start.patch`, under `__zoraya__`): Zoraya
   starts no program set-uid (ADR-0005), so the path cannot be taken, and compiling it out keeps the shim's list short.
   This answers ADR-0100's open question.
4. **Hiding.** `posix-surface-check hide` renames the names of item 5 (750 today) to `__zoraya_<name>` in all 1,250
   objects and prints the list; it refuses if a new name is taken. It rewrites the objects itself (a new string table
   at the end of the file), because the pinned toolchain's `rust-objcopy` needs the `llvm-tools` component, which the
   minimal profile does not install.
5. **Internal names** for item 7 are those beginning with `_` and those beginning with `zoraya_posix_`, the shim's and
   the start-up's (item 2, ADR-0109 item 4).
6. **The signal directory** goes to Rust whole, as item 2 says; the cost estimate's three functions are among the
   stub's names (ADR-0140).
7. **What the check enforces** (`posix-surface-check check`): no export outside the list (INV-0100-1, INV-0005-1), none
   on the exclusion table (INV-0005-2), every name of the list defined (the `_` rows too), every strong reference of
   every member defined in the archive, no member name twice, the archive's symbol index equal to what the members
   define (two ELF readers, the archiver's and the checker's, must agree), and no `svc` of any immediate outside
   permitted members (INV-0100-2; none are permitted while the stub has no `zoraya-rt`).
8. **Where it runs.** `make ports` ends with `make check-ports`, which runs the check on the real archive and on four
   broken copies, and `make test` runs the tool's host tests (including the staleness of the list). INV-0100-1's
   "run by `make test`" holds once ADR-0110 item 9 puts the check into `make test-posix`, a step of `make test`.

## Consequences

- **Positive.** The archive exports 240 names plus the three `_` rows, exactly the tier list, and the build proves it.
  Linking a program is a test of the measured surface.
- **Negative.** A port that needs a function outside the list fails to link until an ADR adds it (as ADR-0142 asks
  for dash). The archive is rewritten on every build.
- **Follow-up.** 6c builds `zoraya-posix` into the same pipeline. The `zoraya_posix_*` shim functions take the `-errno`
  convention.

## Invariants

- **INV-0141-1.** Every name `posix-surface-check hide` renames is outside the tier list and not internal, and after
  the renaming no member of `libc.a` defines or references the old name. *Check:* host tests of `hide` and `rename`,
  and the export check over the archive.
- **INV-0141-2.** `ports/musl/excluded.txt` names exactly the files of the retained directories that the shim refuses.
  *Check:* the build fails on a refused file that is not listed, and `make check-ports` compiles every listed file and
  fails if one compiles.

## Open questions

- Whether `pthread_setcancelstate` and the `nscd` client's socket calls should become no-ops in the Rust half or the
  `passwd` parsers be patched not to call them.

## Revisit triggers

- A musl update: the shim's list and the excluded files are found again by compiling.
- A second thread in a POSIX process (T3): `futex` needs a real route.

## Architect's decision

