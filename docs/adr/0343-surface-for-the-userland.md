# ADR-0343: The names toybox's finished commands and one-true-awk add to libc.a, and what each means

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0005, ADR-0013, ADR-0014, ADR-0100, ADR-0141, ADR-0180, ADR-0336, ADR-0342
- **Evidence:** CP-27, stage-10/survey.md#what-the-missing-tools-need-from-libc, stage-10/survey.md#the-userland-today

## Context

ADR-0342 item 1 says each part of stage 10 that ports a program runs a link probe and records the names it adds in a
`posix-surface-added` block of an as-built ADR, a program behind each name. Part 10e (TASK-31) builds the commands of
`ports/toybox-applets.txt` (ADR-0336 item 1). Linking them against `libc.a` as it was at `0c3ada9` (`make -f
ports/Makefile toybox`, `build/ports/logs/toybox.log`) left these names undefined, each with the objects that
import it:

| name | imported by |
|---|---|
| `dirname`, `dprintf`, `getline`, `strncasecmp`, `strnlen`, `fscanf` | `dirname`, `patch`, `cpio`, `uniq`, `tar`, `xxd` |
| `getgrnam` | toybox's library (`xwrap`) |
| `link` | `ln` |
| `lchown`, `mknod` | `cpio` |
| `fchdir` | `find` |
| `readv` | `dd` |
| `settimeofday` | `date` |
| `sigsetjmp` | `gzip` (toybox's `toys.rebound`) |
| `statfs`, `pathconf` | `stat` |

The measured gap is the survey's (stage-10/survey.md#what-the-missing-tools-need-from-libc, stage-10/survey.md#the-userland-today), and CP-27 is the rule that a name needs a program behind it. one-true-awk (part 10e, ADR-0336 item 3) leaves twelve more, found the same way (`build/ports/logs/awk.log`): `feclearexcept`, `fetestexcept` (math.c), the ctype functions `isalpha`, `isdigit`, `isgraph`, `islower`, `isprint`, `isupper`, `random`, `srandom`, `towupper` and `wctomb`, all musl's own code. ADR-0342 items 7, 8 and 10 decide the meaning of the rows from `dirname` to `mknod`; the six rows `fchdir`, `readv`,
`settimeofday`, `sigsetjmp`, `statfs` and `pathconf` are not covered. This ADR records the block and the meanings the
accepted ADRs leave open, as built in 10e.

## Decision drivers

1. A name is exported because a named program imports it (ADR-0342 driver 1, INV-0342-1).
2. Where Zoraya lacks the thing the call says so with an error a program handles (ADR-0342 driver 3).
3. The simplest, least surprising and reversible choice where the accepted ADRs are silent.

## Considered options

### Option A: export the six names and give each one a meaning
- **Description:** `fchdir` and `readv` are built over what `libposix` has (a descriptor of a directory the
  program holds; `readv` as reads into each buffer in order), `settimeofday` fails with `EPERM` (the clock is the
  system's, ADR-0014 rule 5's reasoning), and `statfs` and `pathconf` get a meaning of their own.
- **Evidence for:** drivers 1 to 3; the names are the ones the link probe printed.
- **Evidence against:** each name is more surface to keep true.
- **Cost:** *(estimate)* 150 lines of Rust and tests.
- **Precedent:** ADR-0310's block.

### Option B: patch each use out of toybox
- **Description:** `find` without `-execdir`'s `fchdir`, `stat` without `-f`'s `statfs` and `pathconf`, `date`
  without `-s`, `dd` without `readv`.
- **Evidence for:** no new names.
- **Evidence against:** the patches are toybox's, repeated at each version (ADR-0310 option B's argument); `fchdir`
  and `readv` are standard POSIX and other ports will import them.
- **Cost:** *(estimate)* several small patches that move with upstream.
- **Precedent:** ADR-0310 option B, rejected there.

### Option C: Linux's behaviour for all (a full libc, ADR-0342 option A)
- **Evidence against:** see ADR-0342.

## Recommendation

**Recommendation (not a decision): Option A for `readv`, `settimeofday` and `sigsetjmp`, Option B for `fchdir`,
`statfs` and `pathconf`**, which is what 10e built.

1. The block below lists the names with the program that imports each; the export check (INV-0005-1) and
   `posix-surface-check imports` (INV-0342-1) read it.
2. `settimeofday` fails with `EPERM` whatever the arguments; `date -s` reports it.
3. `readv` reads into each buffer in order, as `read` would into one buffer of their total length (at most 1024
   buffers; a null pointer is `EFAULT`); it is `libposix`'s, over the `writev` pattern.
4. `fchdir`, `statfs` and `pathconf` are **not exported**. Patch 0002 of `ports/patches/toybox/` defines the three as
   failing with `ENOSYS` inside toybox (`lib/lib.h`), so `stat -f` reports that error, and makes `find`'s
   `-execdir` and `-okdir` an error at parse time ("-execdir is not supported on Zoraya"); a plain `-exec` runs in the
   directory `find` was started in. A later port that needs `fchdir` brings Option A's meaning for it: the
   directory `fd` was opened on, if the entries the program holds cover it, as `chdir` does for a path.
5. `sigsetjmp` and `siglongjmp` are musl's own code; the signal mask is saved and restored through
   `sigprocmask`, which is exported.

```posix-surface-added
feclearexcept awk
fetestexcept awk
isalpha awk
isdigit awk
isgraph awk
islower awk
isprint awk
isupper awk
random awk
srandom awk
towupper awk
wctomb awk
dirname toybox
dprintf toybox
fscanf toybox
getgrnam toybox
getline toybox
lchown toybox
link toybox
mknod toybox
readv toybox
settimeofday toybox
sigsetjmp toybox
strncasecmp toybox
strnlen toybox
```

## Consequences

- **Positive.** Toybox's list links; the surface grows by the names a program imports.
- **Negative.** `stat -f`, `df` and `find -execdir` are absent; `date -s` cannot set the clock. The patch that
  removes them moves with each toybox version (Option B's cost).
- **Follow-up.** Later parts (10d, 10g, 10h) add their own blocks; `awk`'s block is item 6 of ADR-0344.

## Invariants

- **INV-0343-1.** Every line of the block names a program, and that program's objects import the name. *Check:*
  `posix-surface-check imports docs/adr/0343-surface-for-the-userland.md toybox=DIR` in `make check-ports`
  (INV-0342-1's second half).

## Open questions

- Whether `getgrnam` should read the group file or answer from the one identity directly (ADR-0342 item 7 says the
  files of the toolchain set).

## Revisit triggers

- A second program needs `statfs` or `fchdir`.

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

As recommended.
