# ADR-0210: The surface the workloads' own programs need

- **Status:** proposed
- **Date:** 2026-11-04
- **Depends on:** ADR-0005, ADR-0100, ADR-0110, ADR-0141, ADR-0144
- **Evidence:** CP-27, task-02/libc-surface.csv, task-02/method.md#m1-libc-api-surface

## Context

ADR-0005's tier list is the libc surface of the *tools* TASK-02 measured: dash, GNU make, tcc and the applets. The
programs the workloads *build* are not in it: TASK-02's interposer marked stage-2 tcc, `lua`, `sqlite3` and the
`configure` probes `built:` and did not count them (`task-02/method.md#m1-libc-api-surface`, "Attribution"). On
Linux they linked against the whole of musl's `libc.so`. On Zoraya `tcc` links them against `libc.a`, which exports
exactly the tier list (ADR-0141). TASK-22 found that W2 and W3 cannot link:

- **W3** (`tcc -O2 -DSQLITE_THREADSAFE=0 -c sqlite3.c && tcc -c shell.c && tcc -o sqlite3 shell.o sqlite3.o -lm`,
  inside Zoraya under HVF): 22 unresolved names, `ceil chmod dlclose dlerror dlopen dlsym fabs fsync gethostname
  getpwuid getrusage mkdir mmap munmap pclose popen rewind rmdir symlink system unsetenv utimes`.
- **W2** (Lua's `make posix` with the host-built tcc for Zoraya, `ports/Makefile`'s `tcc-host`, against the same
  sysroot; the same tcc source and `libc.a`, so the same link): `abs acos asin atan2 ceil clearerr clock cos
  difftime exp fabs flockfile floor fmod frexp ftello funlockfile getc gmtime_r ldexp localeconv log log10 log2
  pclose popen pow sin sqrt system tan ungetc`.

W1's stage-2 tcc and W8's `mktab` link with the tier list (the first is the same program as the ported tcc). Of the
45 names (the union, less `dlopen`, `dlsym`, `dlclose` and `dlerror`), `task-02/libc-surface.csv` has 9 at T2, 20 at
T3, 4 `unused-applets`, 2 `comparison` and 10 not at all (`abs ceil difftime fabs flockfile floor frexp funlockfile
ldexp localeconv`: libm and stdio names no measured tool imports). 35 are in `libc.a` already, hidden: 33 musl's
(`math`, `stdio`, `time`, `locale`, `passwd`, `env`, `stdlib`), and `mkdir` and `munmap` the Rust half's. Ten are in
no object: `chmod rmdir symlink fsync gethostname utimes getrusage mmap` (musl's files make system calls the shim does
not route, or are in directories ADR-0141 does not retain), `pclose` (musl's `pclose.c` is excluded for its
`__sys_wait4`) and `system` (`src/process/` is not retained).

`dlopen` and its three are on ADR-0005's exclusion table and INV-0005-2's deny list. SQLite's Unix VFS references
them for run-time extensions (`unixDlOpen`), which the W3 check never loads.

## Decision drivers

1. **The surface changes only by ADR, with a workload behind each row** (CP-27, INV-0005-1).
2. **The acceptance workloads must run with the measured commands** (ADR-0110 driver 6).
3. **Excluded features stay excluded until an ADR re-examines them** (ADR-0005's revisit trigger: "an in-scope
   workload fails for want of an excluded feature").
4. **Least new code in the Rust half**: wrappers of the `*at` calls it has.

## Considered options

### Option A: export what the products need, as musl would (the Linux baseline)

- **Description:** Add the 45 names to the exports in a `posix-surface-added` block: un-hide the 37 that exist;
  write the ten that do not, each over a call the library has (below). `dlopen` and friends stay out, and W3's two
  compiles (`sqlite3.c` and `shell.c`, which calls `sqlite3_load_extension`) take `-DSQLITE_OMIT_LOAD_EXTENSION`,
  the one change to a measured command.
- **Evidence for:** Drivers 1, 2 (except the one flag) and 4. Each name has its workload. Musl's own code serves 33 of the 35 that exist.
- **Evidence against:** The exports grow from 248 to 293 names, past T1's measured 248 (CP-27's "small"); the list
  is no longer "the tools' surface" but "the tools' and their products'". A flag on W3's command.
- **Cost:** *(estimate)* 150 lines of Rust, a line of the shim, one musl file retained, one file un-excluded.
- **Precedent:** ADR-0144 (two names for toybox's `sort` and `wc`, the same mechanism).

### Option B: export the deny-listed `dlopen` family as stubs that fail

- **Description:** As A, and `dlopen` returns NULL with `dlerror` "dynamic loading is not supported", so W3's
  command is the measured one.
- **Evidence for:** No flag; the true behaviour of a static system (a `.load` fails at run time).
- **Evidence against:** Supersedes INV-0005-2's deny list for four names, against ADR-0005's exclusion; programs
  that probe `dlopen` at configure time find it and take a path that fails later.
- **Cost:** *(estimate)* A, plus 20 lines and the deny list's change.
- **Precedent:** musl's own `dlopen` in a static link fails at run time the same way.

### Option C: a second archive for products (`libzoraya-extra.a`) that tcc links by default

- **Description:** Keep `libc.a` at the tier list; put the products' names in a second archive tcc adds to every
  link.
- **Evidence for:** The tier list stays the tools' surface exactly.
- **Evidence against:** The same functions exported to the same programs, under a name that hides it; the export
  check would need a second list anyway.
- **Cost:** *(estimate)* A, plus a tcc patch and a second check.
- **Precedent:** none found; Fuchsia's partial POSIX Lite grew in one library.

### Option D: do not run W2 and W3 until T2 is accepted

- **Description:** Leave the list; W2 and W3 fail.
- **Evidence against:** ADR-0005 makes W2 and W3 T1 acceptance tests; INV-0110-1 fails.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The names**, in the block below, each with the workload whose link needs it.
2. **The ten new ones**: `chmod`, `rmdir`, `symlink` and `utimes` are `fchmodat`, `unlinkat(AT_REMOVEDIR)`,
   `symlinkat` and `utimensat` from `AT_FDCWD`; `gethostname` is `uname`'s node name, as musl's file; `getrusage`
   gives zeros, as `wait3` does (no accounting); `mmap` is the C name of the shim's `zoraya_posix_mmap`; `pclose` is
   musl's file, with the shim routing `__sys_wait4` to `waitpid`; `system` is musl's `src/process/system.c`, which
   calls only functions `libc.a` has (`posix_spawn`, `sigaction`, `waitpid`), with a `pthread_testcancel` that does
   nothing (one thread, ADR-0100 item 5).
3. **`fsync`** finds a file's writes already in the service and durable for an `O_SYNC` description (ADR-0103 item
   14); for a description written through it asks `status`, as `close` does (ADR-0103 item 6), so a failed volume is
   `EIO`. Nothing else is made durable: the connection has no `SYNC` right unless opened `O_SYNC`. A stream is
   `EINVAL`. A stated deviation from POSIX's durability.
4. **W3's two compiles gain `-DSQLITE_OMIT_LOAD_EXTENSION`**, the only change to a measured command, until the
   architect chooses between this and Option B. Without it the link fails on the four deny-listed names; with it,
   `shell.c`'s `.load` command is compiled out, which the check (a query) never runs.

```posix-surface-added
abs
acos
asin
atan2
ceil
chmod
clearerr
clock
cos
difftime
exp
fabs
flockfile
floor
fmod
frexp
fsync
ftello
funlockfile
getc
gethostname
getpwuid
getrusage
gmtime_r
ldexp
localeconv
log
log10
log2
mkdir
mmap
munmap
pclose
popen
pow
rewind
rmdir
sin
sqrt
symlink
system
tan
ungetc
unsetenv
utimes
```

**Tensions with accepted ADRs.**

- *ADR-0005*: the exports exceed T1's measured list by the products' names; driver 1 holds, each name has a workload.
- *ADR-0110 driver 6*: one flag on W3's two compiles.
- *ADR-0141*: one more musl file retained (`system.c`) and one un-excluded (`pclose.c`).

## Consequences

- **Positive.** W2 and W3 link with musl's own code; the products' surface is written down for the first time.
- **Negative.** 45 more exports; INV-0005-3's call log now sees products' calls too, which TASK-02 did not count.
- **Follow-up.** The block's names in `libc.a`; the W3 flag; `t_posix products` in `make test-posix`; the products'
  links in W2 and W3 themselves.

## Invariants

- **INV-0210-1.** `libc.a` exports every name of this block, each of the ten written here behaves as item 2 and 3
  say, and W2 and W3 link inside Zoraya. *Check:* `make check-ports` (`posix-surface-check check` with this ADR in
  the list), `make test-posix`'s `t_posix products`, the host test `fsync_takes_a_file_and_refuses_a_pipe`
  (`zoraya-posix`), and `make test-posix-long`'s W2 and W3.

## Open questions

- Option A or B for `dlopen` (the W3 flag).
- Whether T2's acceptance should fold these rows into the tier list.

## Revisit triggers

- T2 accepted: the rows that are T2 in the CSV move into the T2 list.
- A product that needs a name outside this block: a superseding ADR, as here.

## Architect's decision
