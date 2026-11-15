# ADR-0390: The names ncurses, less and nvi add to libc.a, what is left for the terminal part, and how the three were built

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0005, ADR-0100, ADR-0141, ADR-0331, ADR-0334, ADR-0335, ADR-0342, ADR-0343
- **Evidence:** CP-27, stage-10/survey.md#what-ncurses-needs-from-libc, stage-10/survey.md#editors-and-pagers-compared

## Context

ADR-0342 item 1 says each part of stage 10 that ports a program runs a link probe and records the names it adds in
a `posix-surface-added` block, a program behind each name. TASK-31 part `ports-early` builds the C programs of part 10d
ahead of the terminal (part 10c): ncurses 6.6 (the two archives of ADR-0334), less 710 (ADR-0335 item 8), nvi2 2.2.2
with Berkeley DB 1.85 (ADR-0335 items 1 and 2). Only their tests need the terminal. The machine that built them ran
no cargo and no QEMU: `libc.a` was relinked from the objects of musl and `libzoraya_posix.a` of the `poll` merge
(`71ba408`), with the block below; `libposix` is unchanged.

The probe is `posix-surface-check` plus the linker itself: each program was linked against `libc.a` and the linker's
undefined names are the list. Part of the list is musl's own code that the tier list hides (ADR-0141), and part is
what only `libposix` can supply.

| name | imported by | where it comes from |
|---|---|---|
| `iswprint`, `wmemcpy`, `wcsrtombs`, `tdelete`, `tfind`, `tsearch`, `twalk` | ncurses (`lib_add_wch`, `lib_cchar`, `lib_unget_wch`, `new_pair`, `lib_tparm`; `twalk` shows only when `tsearch` is exported, which configure then uses: a probe against the first `libc.a` missed it) | musl's own code, hidden: in the block |
| `iswcntrl`, `iswgraph`, `iswlower`, `iswpunct`, `iswupper`, `iswxdigit`, `wmemset` | nvi (`v_txt`, `ex_abbrev`, `ex_subst`, `ex_usage`, `v_increment`, `v_replace`) | musl's own code, hidden: in the block |
| `asprintf`, `strlcpy`, `strsep`, `issetugid` | nvi (`ex_cscope`, `main`, `util`) | musl's own code, hidden: in the block |
| `catopen`, `catgets`, `catclose` | nvi (`common/msg.c`) | musl's own code, hidden: in the block; musl's `catopen` finds no catalog and nvi uses its built-in English text |
| `getpwent`, `setpwent`, `endpwent` | nvi (`ex_argv.c`, `~user` expansion) | musl's own code, hidden: in the block; they read `/etc/passwd` as ADR-0342 item 7's lookups do |
| `execl` | nvi (`ex_argv.c`, `:!`, `:r !`) | `libposix` already defines it; hidden only by the tier list: in the block |
| `creat` | less (`edit.c`, the `-o` log file) | not musl's retained code (`src/fcntl` is not retained) and not `libposix`'s: **left for part 10d** |
| `sleep` | ncurses (`lib_print.c`, `mcprint`; not linked into `vi` or `less`) | the same: **left for 10d** (`nanosleep` is exported) |
| `select` | nvi (`cl_read.c`) | ADR-0342 item 5: **left for 10c** (or F1); `pselect` is exported |
| `tcsetattr`, `tcflush`, `cfgetospeed`, `cfgetispeed` | ncurses (`lib_ttyflags`, `lib_kernel`, `lib_baudrate`), less (`screen.c`) | the terminal names of ADR-0342 item 4: **left for 10c** (`libposix` has an older `tcsetattr`, hidden; its meaning is 10c's) |
| `getprogname`, `err`, `errx`, `warn`, `warnx` | nvi | BSD names (libbsd's on Linux): compiled into the program from `ports/nvi/compat.c`, not names of `libc.a` |

## Decision drivers

(CP-27: a name needs a program behind it. The evidence for the size figures is `stage-10/survey.md#editors-and-pagers-compared`, for the names `stage-10/survey.md#what-ncurses-needs-from-libc`.)

1. A name is exported because a program in the image imports it (ADR-0342 driver 1, INV-0342-1).
2. `libposix` is not changed by a port part that cannot build it; what only it can supply is listed for the part that
   can (the task's rule for this slot).
3. The programs should be built the way the architect will build them, so that parts 10d, 10g and 10h only wire them in.

## Considered options

### Option A: export the musl-own names, link the rest from a shim, list the rest (the recommendation)
- **Description:** the block below; `ports/apps/ttyshim.c` supplies `tcsetattr`, `tcflush`, `cfgetospeed`,
  `cfgetispeed`, `select`, `creat` and `sleep` to these early builds only (the pattern of `ports/apps/zshim.c`,
  ADR-0310 item 4). The shim is in no image and is deleted by the part that adds the real names.
- **Evidence for:** drivers 1 to 3.
- **Evidence against:** the three binaries link and can be sized, but are not working terminal programs until 10c.
- **Cost:** *(estimate)* none beyond the block.
- **Precedent:** ADR-0310 (the shim), ADR-0343 (probe and block).

### Option B: patch the programs so that they import none of the missing names
- **Evidence against:** `tcsetattr` and `select` are what a terminal program is; patching them out leaves no program.
  For `creat` and `sleep` the patch is two lines each (`open` and `nanosleep`), but each is a patch to repeat at every
  release (ADR-0310 option B's argument).
- **Precedent:** none that worked: toybox's three patches for absent names (ADR-0343) are the nearest.

### Option C: Linux's behaviour for all (ADR-0342 option A)
- **Evidence against:** see ADR-0342.
- **Precedent:** every Linux distribution.

## Recommendation

**Recommendation (not a decision): Option A.** What was built, and what each choice was:

1. **ncurses 6.6** (`make ncurses`): configured as ADR-0334 item 1 says (`--enable-widec --with-termlib
   --without-shared --without-cxx --without-cxx-binding --without-progs --disable-home-terminfo
   --enable-mixed-case --with-default-terminfo-dir=/usr/share/terminfo --with-terminfo-dirs=/usr/share/terminfo
   --without-fallbacks --enable-overwrite`), `-O2`. The host triple is `aarch64-unknown-linux-musl` so that `config.sub`
   accepts it (as for wget); `zcc` defines nothing of Linux, which the build log shows. Only the `include/` and
   `ncurses/` directories are built and installed: `libncursesw.a` (344030 bytes) and `libtinfow.a` (267374 bytes) and
   `curses.h`, `ncurses.h`, `term.h`, `termcap.h`, `unctrl.h`, `ncurses_dll.h` in `build/ports/curses/usr/`. They are
   **not** in the sysroot yet: copying them into `build/sysroot/usr` (and so into the image) is the one line part 10d
   adds when `vi` and `less` are wired in. The strings of the archive name `/usr/share/terminfo` and no home directory.
2. **less 710**: `configure` with `--with-regex=posix` (musl's `regcomp`) and `-ltinfow` alone; one patch for the
   compiled-in defaults of ADR-0335 item 8 (no key files, no tags, no history file, `LESSOPEN` and `LESSCLOSE`
   ignored). `LESSSECURE` is not set. `poll` (ADR-0398) is found by its `configure`.
3. **Berkeley DB 1.85** (`make db1`): the archive of Oracle's download site (the `db.1.85.tar.gz` of Sleepycat; the
   last release of the BSD code), compiled with the file list of its `PORT/linux/Makefile` by `ports/db1/build.sh`, the
   headers of `PORT/include` (`sys/cdefs.h` and `sys/queue.h` are copied beside them); one patch (a structure field named
   `errno`). `libdb1.a` is 98926 bytes. **Known limitation:** 1.85 has defects in its hash and btree code that later
   BSD releases corrected; nvi uses only the `recno` method. Whether to take a maintained copy (the BSDs' `libc/db`) is
   an open question below; the choice is reversible (the archive and `ports/db1/build.sh` change, nothing else).
4. **nvi2 2.2.2** (`make nvi`): its CMake and Ninja are not used (ADR-0335 item 1): `ports/nvi/build.sh` holds the
   source list, makes the headers CMake makes (`*_extern.h`, `options_def.h`, `ex_def.h`, `version.h`) and links with
   `-lncursesw -ltinfow -ldb1`. Three patches: no start-up files (ADR-0335 item 6: `NEXINIT` and `EXINIT` and the
   current directory's `.exrc` stay), `:script` removed (no pseudo-terminal, ADR-0338), `flock` removed (the `lock`
   option stays and locks nothing; ADR-0335 says it is removed: a **recorded difference**, removing the option is a
   larger patch). Recovery (ADR-0335 item 5) is not a patch: `ports/nvi/recover.c` replaces `common/recover.c` with
   five functions that do nothing or say recovery is not available. `bitstring.h` and `sys/queue.h`, which musl
   lacks, are small in-house implementations (`ports/nvi/`).
5. **Sizes** (ADR-0334's open question), `-O2`, static, linked with the call-logging `libc.a` of `POSIX_STATS=1`:

   | program | file | `.text` | `.rodata` | `.data` | `.bss` | debug sections |
   |---|---|---|---|---|---|---|
   | `less` | 2872992 | 897972 | 149857 | 26840 | 65240 | 1332150 (from libc.a's `-g`) |
   | `vi` (nvi) | 3247216 | 1168112 | 156095 | 4568 | 11696 | 1393361 |

   The loaded size is about 1.14 MB (`less`) and 1.34 MB (`vi`). Most of it is `libc.a` (musl's stdio, locale and
   regex, and `libposix`), which is what each static program carries; the ncurses archives are 0.6 MB together, of
   which a program pulls what it references.
6. **Names.** The block below is what part 10d takes into its as-built ADR with the terminal names; INV-0342-1's check
   of the block (`posix-surface-check imports`) runs from `ports/check-early.sh` when the three are built.

```posix-surface-added
asprintf nvi
catclose nvi
catgets nvi
catopen nvi
endpwent nvi
execl nvi
getpwent nvi
issetugid nvi
iswcntrl nvi
iswgraph nvi
iswlower nvi
iswprint ncurses
iswpunct nvi
iswupper nvi
iswxdigit nvi
setpwent nvi
strlcpy nvi
strsep nvi
tdelete ncurses
tfind ncurses
tsearch ncurses
twalk ncurses
wcsrtombs ncurses
wmemcpy ncurses
wmemset nvi
```

## Consequences

- **Positive.** The three programs and their libraries build and link from the pinned sources; part 10d has to wire
  them, add `creat`, `sleep` and the terminal names, and run them.
- **Negative.** The three binaries are untested: no QEMU ran in this slot. They cannot work as terminal programs
  until 10c (`tcsetattr` and `select` are shim stubs). DB 1.85 is old code.
- **Follow-up.** Part 10c: `tcsetattr`, `tcflush`, `cfgetospeed`, `cfgetispeed`, `select` (ADR-0342 items 4 and 5),
  then delete `ports/apps/ttyshim.c` and its rule. Part 10d: `creat` and `sleep` in `libposix`, the copy of the ncurses
  files into the sysroot, `vi.cmd`, `less.cmd` and the links `ex` and `view`, the tests, and a decision on DB 1.

## Invariants

- **INV-0390-1.** Every line of the block names a program, and that program's objects import the name. *Check:*
  `posix-surface-check imports docs/adr/0390-surface-for-curses-less-and-vi.md ncurses=DIR nvi=DIR less=DIR` in `make check-ports`
  (`ports/check-early.sh`, INV-0342-1's second half).
- **INV-0390-2.** The compile commands of ncurses, less, DB 1 and nvi in `build/ports/logs/` run `zcc` and none names `-D__linux`.
  *Check:* `ports/check-early.sh` in `make check-ports`.
- **INV-0390-3.** `libtinfow.a` names `/usr/share/terminfo`, and the three programs pass `zoraya-elf-check`. *Check:*
  `ports/check-early.sh` in `make check-ports`.

## Open questions

- Which copy of DB 1 (1.85 here; the BSDs' `libc/db` is maintained)? Measure with the recno access nvi uses: a 100 MB
  file, `:g` over it, and a write-back. Part 10d.
- Should the `lock` option be removed from nvi's option table (ADR-0335 item 1 says removed)? A larger patch to
  `common/options.c`; 10d decides.
- DB 1.85 is under the 4-clause UC licence (the advertising clause, withdrawn by UC in 1999, is still in the files); a maintained copy may have the 3-clause one. The pins: nvi2 is a GitHub-generated archive of a tag, DB 1.85 has one URL and no mirror, hashes are those of the first download.
- Whether `less` and `vi` need `-g` in the image (the debug sections are 1.3 MB of each file here, from `libc.a`'s own `-g`).

## Revisit triggers

- A maintained DB 1 is chosen; `libposix` adds `creat` or `sleep`; part 10c adds the terminal names.

## Architect's decision

(Left empty for the human.)
