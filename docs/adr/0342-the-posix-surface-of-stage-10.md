# ADR-0342: The POSIX surface stage 10 adds: names enter by a link probe of each port, under meanings fixed here

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0005, ADR-0013, ADR-0100, ADR-0106, ADR-0141, ADR-0180, ADR-0225, ADR-0308, ADR-0310, ADR-0319, ADR-0331, ADR-0332, ADR-0338
- **Evidence:** CP-22, CP-27, DM-036, DM-072, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#ports-as-built, stage-10/survey.md#the-network-as-built, stage-10/survey.md#the-tty-service-as-built, stage-10/survey.md#the-userland-today, stage-10/survey.md#what-a-posix-program-is-given-today, stage-10/survey.md#what-git-needs, stage-10/survey.md#what-ncurses-needs-from-libc, stage-10/survey.md#what-the-missing-tools-need-from-libc, stage-10/survey.md#what-the-ssh-client-needs, stage-10/survey.md#window-size

## Context

ADR-0005 fixed the POSIX surface as lists of names, each justified by a measured workload, and said "the surface
changes only by a superseding ADR that adds rows with a measured workload". `libc.a` hides every name outside the
lists (ADR-0141), so a port that calls one does not link. ADR-0005 put raw mode, `SIGWINCH`, general `poll` and
`select`, record locks and timers out of scope, with the data of the time. Threads came in by ADR-0308, sockets by
ADR-0225 and ADR-0287, 22 names for curl and wget by ADR-0310. Stage 10's programs (ncurses, nvi2, `less`, the
rest of toybox, one-true-awk, git, OpenSSH's client) need names that are still out: `tcsetattr`, `poll`, the
user-database calls, the wide-character classification, and more (`stage-10/survey.md#the-tty-service-as-built`,
`stage-10/survey.md#the-userland-today`). This ADR decides how those names enter and what the ones with a design
content mean on Zoraya. It is written without a build, so it cannot list the names by measurement; it fixes the
rule and the meanings.

**What is known.**

- curses needs termios, `SIGWINCH` and `SIGTSTP` handlers and `poll` or `select` on the terminal
  (`stage-10/survey.md#what-ncurses-needs-from-libc`).
- toybox's T2 applets want about twenty names: `getline`, `mkdtemp`, `getgrgid`, `getgrnam`, `getgroups`,
  `getgrouplist`, `link`, `lchown`, `mknod`, `dirname`, `dprintf`, `getentropy`, `gethostbyname`, `sethostname`,
  `towlower` and others; it uses no `glob`, `nftw`, `fts` or `posix_spawn`
  (`stage-10/survey.md#the-userland-today`, `stage-10/survey.md#what-the-missing-tools-need-from-libc`).
- git needs `poll`, exclusive create and atomic rename, and runs without `mmap`, Unix sockets and `setitimer`
  (`stage-10/survey.md#what-git-needs`). OpenSSH's client needs a user-database entry, `poll`, and name lookups
  (`stage-10/survey.md#what-the-ssh-client-needs`).
- POSIX.1-2024 has `tcgetwinsize` and `tcsetwinsize` beside the ioctls (`stage-10/survey.md#window-size`).
- Zoraya has one identity, no hard links, no devices to make, no file mappings, no `setsid`, in-process record
  locks, and no timers that signal (`stage-10/survey.md#what-a-posix-program-is-given-today`). `poll`, `select`,
  `socketpair` and `getnameinfo` were decided by ADR-0225 and are not built
  (`stage-10/survey.md#the-network-as-built`).
- The ports' build can already link a set of programs against `libc.a` and list the undefined names
  (`t2-probe`, `stage-10/survey.md#ports-as-built`).

**What is assumed.** That no program of stage 10 needs a timer signal that cannot be compiled out. The probe of
each port settles it.

## Decision drivers

1. **A name is exported because a named program imports it** (CP-27, INV-0005-1).
2. **A name means one thing**, decided before the first port that leans on it, so that two parts do not give
   `poll` two behaviours.
3. **Where Zoraya lacks the thing, the call says so honestly**: an error a program handles, not a pretence
   (`stage-10/survey.md#what-a-posix-program-is-given-today`).
4. **Nothing that was excluded on purpose comes back by accident**: input injection, pseudo-terminals, record
   locks across processes (DM-072, DM-036, ADR-0338).
5. **UTF-8 without configuration** (CP-22).

## Considered options

### Option A: Export all of musl (a full libc, as on Linux; Starnix's fidelity)

- **Description:** Stop hiding names; every function musl has is linked if a program calls it.
- **Evidence for:** No port ever fails at link time; no lists to keep. Fuchsia's Starnix chose full fidelity to
  the Linux interface for the same reason (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence against:** musl's system-call layer is replaced by `libposix` (ADR-0100), so most names would link
  and then fail or misbehave at run time, which is what the export check exists to prevent (driver 3). The surface
  would stop being enumerable (CP-27), and the postmortem records Starnix's cost as years of work on the hard
  parts.
- **Cost:** *(estimate)* None to start; unbounded to make true.
- **Precedent:** Linux; Starnix.

### Option B: One tier list written now, from documentation

- **Description:** A file `posix-surface-T10.txt` listing every name the programs are expected to need, as T9
  lists the thread names.
- **Evidence for:** One list, reviewable at once; ADR-0308 did this for threads, where musl's own thread
  directory defined the set.
- **Evidence against:** Without a build the list is a guess: names nobody imports would be exported and tested
  for nothing, and missing ones would be found at link time anyway (driver 1). T9 could be enumerated from one
  directory; this set cannot.
- **Cost:** *(estimate)* A list of a few hundred names with unknown accuracy.
- **Precedent:** ADR-0308's T9; ADR-0225's TN.

### Option C: Names from a link probe of each port, recorded where the port is built, under meanings fixed now

- **Description:** Each implementation part links its programs against `libc.a`, takes the undefined names,
  and records them in a `posix-surface-added` block of its as-built ADR with the program behind each, as ADR-0310
  did. This ADR fixes in advance what the names with a design content do.
- **Evidence for:** Drivers 1 to 5. The measurement is the linker's, which is exact; the meanings are decided
  once; the mechanism and its check exist (`stage-10/survey.md#ports-as-built`).
- **Evidence against:** The surface is not known until the parts run, and it lands in several ADRs, so "what is
  T10" is the union of blocks, not one file. A name whose meaning was not foreseen here needs a decision in the
  middle of a port.
- **Cost:** *(estimate)* A probe target per port; the functions themselves, most of them musl's own code.
- **Precedent:** ADR-0142, ADR-0144, ADR-0210 and ADR-0310, each a small as-built addition.

### Option D: No growth: a shim per port, or native interfaces (Plan 9's separate POSIX environment)

- **Description:** `libc.a` stays as it is. A port links a private shim that supplies or refuses what it calls,
  as curl and wget link `libzshim.a`, or is patched to call native interfaces.
- **Evidence for:** The checked surface does not grow at all; each program's needs are visible in its shim.
  Plan 9 kept POSIX in a separate, limited environment beside its native one
  (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** A shim per program hides the same decisions in many places (driver 2 fails); ADR-0310
  called its shim "not a surface" because it only refuses. `poll` and `tcsetattr` are not refusals: they need the
  library's wait machinery and its terminal connection.
- **Cost:** *(estimate)* The same functions, written once per program.
- **Precedent:** ADR-0310's shim, for refusals only.

## Recommendation

**Recommendation (not a decision): Option C.**

**The rule.**

1. Each part of stage 10 that ports a program runs a link probe of it and adds the names it needs to `libc.a` in a
   `posix-surface-added` block of that part's as-built ADR, naming the program behind each name; the export check
   (INV-0005-1) then knows them. The union of those blocks is tier T10. A part that builds a function before any
   port of the stage imports it (the terminal names of item 4, in the part that builds the terminal) names its
   committed test program instead, and the first port's block takes the name over. The evidence for a name is
   thus that a program in the image imports it, where ADR-0005 asked for a measured workload that calls it; a
   name a port imports and never calls is exported all the same.
2. A name whose behaviour is not musl's own pure code and is not covered below gets a numbered item in that
   as-built ADR saying what it does.
3. `ports/apps/zshim.c`'s `select` and its `tcsetattr` refusal are retired when the real functions exist.

**The meanings.**

4. **Terminal modes.** `tcsetattr` with its three actions as ADR-0331 item 5 says; `cfmakeraw`; `cfgetispeed`,
   `cfgetospeed`, `cfsetispeed`, `cfsetospeed` (stored, without effect); `tcflush`; `tcdrain`; `tcflow` and
   `tcsendbreak` succeed and do nothing; `ctermid` gives `/dev/tty`; `ttyname` and `ttyname_r` give `/dev/tty` for a
   terminal fd; `tcgetwinsize` is `TIOCGWINSZ`; `tcsetwinsize` and `TIOCSWINSZ` on a terminal fail with `EPERM`.
   (`ttyname` and
   `getlogin` answer nothing today; both change.)
5. **Waiting.** `poll`, `ppoll` and `select` are exported and are the same wait as `pselect`: on a stream end or a
   socket by the handle's signals, on a terminal by the readiness request (ADR-0331 item 5), on a file or a
   directory ready at once. This ends ADR-0005's exclusion of general `poll` and `select` and builds what ADR-0225
   item 1 listed. `epoll` stays out.
6. **Timers that signal stay out.** `alarm` and `setitimer` are not exported (ADR-0106 item 10). A port that uses
   them for a timeout is built with its own switch for that (git's `NO_SETITIMER`) or patched to pass a deadline
   to its wait. If a probe shows a use that cannot be removed, that part proposes the timer with the program as
   evidence.
7. **Users.** `getpwuid`, `getpwnam`, `getgrgid`, `getgrnam` and their `_r` forms read `/etc/passwd` and
   `/etc/group` of the toolchain set, which hold one identity; `getgroups` and `getgrouplist` give its one group;
   `getlogin` gives its name. `setuid` and the rest stay unexported (ADR-0005).
8. **What the file system does not have.** `link` fails with `EPERM` (there are no hard links, ADR-0013); `mknod`
   of a FIFO is ADR-0180 item 9's, of any other node `EPERM`; `chown`, `fchown` and `lchown` to the one identity
   succeed and change nothing, and to any other fail with `EPERM` (ADR-0014 rule 5);
   `sethostname` fails with `EPERM`; `flock` stays unexported (ADR-0319's locks are in the process only); a file
   still cannot be mapped.
9. **Wide characters and the locale.** musl's classification and conversion functions that a port imports
   (`iswprint`, `iswalpha`, `towupper`, `towlower`, `wcswidth`, `mbstowcs`, `wcstombs` and their kin) are exported
   as they are; the terminal takes its widths from musl's tables, not the reverse (ADR-0332 item 9). The shell sets no
   `LANG` or `LC_*`; `setlocale(LC_ALL, "")` with none set gives a UTF-8 locale, and a test holds it. `iconv` is not
   exported; ports are built without it.
10. **Small names with no decision in them** (`getline`, `dirname`, `mkdtemp`, `dprintf`, `strncasecmp`,
    `getentropy` over `getrandom`, and the like) are musl's code or a thin call and need only their line in a
    block.
11. **Network names** follow ADR-0225 as decided: `getnameinfo` in its numeric form, `socketpair`, `getservbyname`
    from its table, `sendmsg` and `recvmsg` without ancillary data, when a probe asks for them.
12. **Not in stage 10, and guarded.** The pseudo-terminal functions (`posix_openpt`, `openpty`, `forkpty`,
    `login_tty`, `grantpt`, `unlockpt`, `ptsname`, `ptsname_r`, `getpt`; ADR-0338 says when they come), `setsid`,
    `ptrace`, `dlopen` and System V IPC are on a deny list beside the export check, which itself admits only listed
    names: a block that adds one of these fails the build, so they return only by an ADR that takes them off the
    list. An ioctl that injects input is never offered.

**Examples.** A person meets the surface only when they build a program themselves:

```
~/proj> tcc clock.c -o clock
~/proj> ./clock
```

where `clock.c` calls `poll` on the terminal with a timeout and `tcsetattr`, and now links and runs. A program
that calls `forkpty` still stops at the link, with tcc's message naming the function. On the desktop there is
nothing to see.

**Tensions with accepted ADRs.**

- *ADR-0005's exclusion table*: the rows "`poll`, `select` ... as general multiplexers", "Line editing
  (`tcsetattr`, raw mode)" and "Job control beyond Ctrl-C ... `SIGWINCH`" stop binding for the names items 4 and 5
  add; the rows for record locks (already changed by ADR-0319), timers, `ptrace`, System V IPC, `dlopen`, set-uid
  and `MAP_SHARED` stand. Accepting this ADR supersedes those three rows, as ADR-0308 superseded the thread row.
- *ADR-0005's rule of admission* ("adds rows with a measured workload") becomes "a program in the image imports
  it" (rule item 1); ADR-0005 and ADR-0100 item 6 tell names imported from names called, and this rule does not.
- *ADR-0014 rule 5* and *ADR-0180 item 9* are followed as written (item 8).
- *ADR-0104* (`getlogin` gives a fixed name) and *ADR-0105 item 6* (`ttyname`) are carried out; the code answers
  nothing for both today.
- *ADR-0106 item 10* (no `alarm`, no `setitimer`) stands (item 6).
- *ADR-0141*: the hiding of names outside the lists stands; the lists grow by blocks.
- *ADR-0180 item 9* made the user-database lookups that reach for a daemon fail; item 7 keeps that and answers
  from the files.
- *ADR-0225 item 1* listed `poll`, `select`, `socketpair` and `getnameinfo` in tier TN; they are built under that
  decision, and T10's blocks name the stage 10 programs that import them.
- *ADR-0310 item 4*: the shim loses the two names item 3 retires.
- *`CLAUDE.md`*: a push that changes a `posix-surface` block is not a documents-only push; the blocks land with
  the code that needs them, which is the case that rule expects.

### Effect of the vision

None on the choice for target 2: each added name is a library function over authority the program already has,
and item 12 keeps out the ones that would give more. For target 3 this is what lets ordinary programs build.
Networking: items 5 and 11 finish what ADR-0225 decided. Threads: `poll` and the terminal read are wait points of
the calling thread (ADR-0304).

## Consequences

- **Positive.** The surface grows by measurement, with a program behind each name; terminal modes, waiting,
  users and wide characters mean one thing each; what was excluded on purpose stays out by a check.
- **Negative.** The surface of stage 10 is not known in advance and is spread over several as-built ADRs. A port
  that needs a timer signal waits for a decision. Programs see one user and get `EPERM` for links and devices,
  which a few will report oddly.
- **Follow-up.** A probe target per port; the deny list in the export check; the functions of items 4, 5, 7 and
  9; `docs/design.md`'s list of what `libposix` offers.

## Invariants

- **INV-0342-1.** Every name `libc.a` exports that the lists before stage 10 did not hold is in a
  `posix-surface-added` block of an ADR, on a line that names a program, and that program's objects have the name
  among their undefined symbols. *Check:* the export check of INV-0005-1 for the first half; for the second, a
  script in `make check-ports` that reads each block line's program and the probe's list of that program's
  undefined names.
- **INV-0342-2.** `poll`, `select` and `pselect` report the same readiness for the same fd, and for a terminal,
  a stream end and a socket they report readable only when a read would not block. *Check:* a host test of
  `zoraya-posix` over the fake kernel for each kind of fd and each of the three calls.
- **INV-0342-3.** No name of item 12 is exported. *Check:* a deny list beside the export check, which fails the
  build if a block or the library adds one.
- **INV-0342-4.** With no `LANG` and no `LC_*` set, a program that calls `setlocale(LC_ALL, "")` decodes UTF-8:
  `MB_CUR_MAX` is 4 and a three-byte character converts to one wide character of the right value. *Check:* a C test
  in the libc conformance step.
- **INV-0342-5.** A program started by `zosh` gets an entry from `getpwuid(getuid())`, and `link` and `mknod`
  fail with `EPERM`. *Check:* a C test in the in-QEMU POSIX script.

## Open questions

- Whether any port needs `alarm` or `setitimer` after its switches are used; the probes answer it.
- Whether `nftw`, `glob` extensions or `wordexp` are needed by git's or OpenSSH's builds; the probes answer it.
- Whether T10 should also exist as one generated file for readers, built from the blocks.

## Revisit triggers

- A probe shows a name whose honest behaviour cannot be given (a timer, a shared mapping, a hard link a program
  cannot do without).
- A second identity or owners on files appear: item 7 and item 8's `chown` change.
- A port needs `iconv`.

## Architect's decision
