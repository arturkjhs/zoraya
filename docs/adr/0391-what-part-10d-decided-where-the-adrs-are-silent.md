# ADR-0391: What part 10d decided where ADR-0334, ADR-0335, ADR-0383 and ADR-0390 are silent

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0005, ADR-0153, ADR-0331, ADR-0333, ADR-0334, ADR-0335, ADR-0342, ADR-0383, ADR-0389, ADR-0390
- **Evidence:** CP-27, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#editors-and-pagers-compared, stage-10/survey.md#what-ncurses-needs-from-libc

## Context

Part 10d wires the three ports of `ports-early` (ncurses, less, nvi2 with DB 1.85; ADR-0390) into the image and
tests them. The accepted ADRs decide the programs (ADR-0334, ADR-0335), the entry's `kbs` (ADR-0383 Option C: "the
terminfo entry has `kbs=^?`, to be built in part 10d") and the names (ADR-0342 item 1, ADR-0390's list for this part:
`creat` and `sleep`). They are silent (the programs' options are in stage-10/survey.md#editors-and-pagers-compared) on how the entry is changed, where the files go in the image, which options a
description lets through, what happens to the build's other ports when a curses archive enters the sysroot, and
what `vttest` is pinned as (`docs/backlog.md`, part 10a's leftovers, asks for the pin). This ADR records those choices
so that none is silent. It changes no accepted decision, with one small exception, item 6, narrower than the accepted
text; item 8 is a gap, not a deviation.

The block below is the as-built list of the two names ADR-0390 left for this part. Each is imported by a program of
the image: `creat` by less's `-o` log file (`edit.c`), `sleep` by ncurses' `mcprint` (`lib_print.c`, which `vi`
and `less` do not link; the archive object is still linked into a program that pulls it, and tcc programs do), the
probe being the linker's undefined names against `libc.a` (stage-10/survey.md#what-ncurses-needs-from-libc). musl's
own are in directories this port does not retain (ADR-0141); `libposix` defines them.

```posix-surface-added
creat less
sleep ncurses
```

## Decision drivers

1. A name has a program behind it (ADR-0342 driver 1; CP-27 is the rule that a port's cost is its patch set).
2. The entry says what the terminal sends, and the change is one the pinned archive's own tools would make (ADR-0383).
3. A program gets what the line says (ADR-0335 item 3): a description lists what the person can pass and the shell
   plans exactly that.
4. The ports built before this part keep building as they were.

## Considered options

### Option A: change the one line of the pinned file in a copy, list the options of daily use, install into the sysroot
- **Description:** items 1 to 9 of the recommendation.
- **Evidence for:** drivers 1 to 4: no archive is patched, the check that the line was found fails loudly, and the
  image's files come from the build that makes the programs.
- **Evidence against:** `terminfo.sh` holds a four-line `awk` over ncurses' file; a release that renames the fragment
  stops the build until the script is read (which is the check's purpose).
- **Cost:** *(estimate)* 60 lines of shell, four description files, a stamp in the ports' makefile.
- **Precedent:** ADR-0310's use of the pinned archive's own file; Debian's `xterm-256color` carries `kbs=^?`.

### Option B: build the host ncurses with `--with-xterm-kbs=DEL` and compile through its `run_tic.sh`
- **Description:** ncurses' own option rewrites the same line while its build installs the database.
- **Evidence for:** the maintainers' own mechanism.
- **Evidence against:** it needs the install step of a host build whose products the repository otherwise does not
  use (`terminfo.sh` builds `tic` and `infocmp` only, ADR-0333 item 2), and ADR-0333 pins the compiler's output, which
  `terminfo.sh check` already compares file by file.
- **Cost:** *(estimate)* a longer host build; the same manifest.
- **Precedent:** Linux distributions that ship ncurses.

### Option C: leave the entry as it was and have `vi` and `less` map `^?` themselves (ADR-0383 Option A)
- **Description:** the terminal sends `0x7f`, the entry says `^H`, and each program is configured around it.
- **Evidence against:** the architect decided Option C of ADR-0383 on 2026-11-15; Plan 9's `rio` has no entry to
  disagree with (alternatives-postmortem.md#plan-9--inferno), which is the argument for one source of truth.
- **Cost:** *(estimate)* a patch per program.
- **Precedent:** ncurses' default for xterm before 2007.

## Recommendation

### Decision

Accepted as recommended, except item 8: `narrow` takes a list of names (ADR-0337 item 8), to be built (the architect, 2026-11-16). The refusal of item 8 stands until it is built.

**Recommendation (not a decision): Option A.**

1. **`creat` and `sleep`** in `libposix` (`ffi.rs`): `creat` is `open` with `O_WRONLY | O_CREAT | O_TRUNC`; `sleep`
   is `nanosleep` for whole seconds and returns 0, or after a handler ran the seconds left, rounded up (musl's truncates; POSIX allows both, and a re-sleep for the result is then never short)
   does. `ports/apps/ttyshim.c`, which stood in for the terminal's names and these two, is deleted with every use of
   it (less, nvi, OpenSSH, git): `libc.a` has all of them since part 10c and this part.
2. **The entry's `kbs`** (ADR-0383 Option C): `tools/term-terminfo/terminfo.sh` changes, in a copy of the pinned
   `terminfo.src`, the one line `kbs=^H` of the fragment `xterm+kbs` (which `xterm` and `xterm-256color` use) to `kbs=^?`,
   and fails unless it finds exactly one. Two of the four files of ADR-0333 change (`x/xterm`, `x/xterm-256color`); the
   manifest, the conformance dump and the key test (`kbs` is now covered by a test, and its exception is gone) follow.
3. **Files of the image.** `/bin/less` and `/bin/vi` are files; `/bin/ex` and `/bin/view` are absolute links to
   `/bin/vi` (nvi takes its mode from the name it is run by, which a fourth patch takes from `argv[0]`: the port had
   fixed `getprogname` at `vi`; a relative link was not followed by `zosh`'s lookup, ADR-0344 item 1). `/usr/share/terminfo` holds the four files of ADR-0333 (`tic`'s output on the host, copied by the
   rootfs rule). The two archives and the headers (`curses.h`, `ncurses.h` as a file, `term.h`, `termcap.h`, `unctrl.h`,
   `ncurses_dll.h`) go into the sysroot, so into the image's `/usr/lib` and `/usr/include` (ADR-0334 item 3).
4. **No other curses reader** (ADR-0334 item 4, INV-0334-1): CPython's configure would find the archives in the sysroot
   and build `_curses`; `PYTHON_CACHE` says `py_cv_module__curses=n/a` and `_curses_panel` too, as its comment already
   said "no readline, curses".
5. **Descriptions** (ADR-0335 item 9): short options as typed, with an invented long name; `cwd none` for all four.
   `vi`: `-R` (with `when -R FILE read`) and `-c COMMAND`. `ex`: those and `-s`. `view`: `-c`. `less`: `-F -i -I -N -R -S -s -X`.
   Not offered: `-r`, `-t`, `-w`, `-S` of vi, less's `-o`, `-O`, `+CMD`, `-k`, `-t` and every option that names a file
   or starts a program, since the grant of such a name is a decision of its own. `-R` of less lets colour sequences
   through to the terminal, which ADR-0332 bounds.
6. **`view` reads.** ADR-0335 item 9 says `view` and `ex` have "the same operand" as `vi` (create). `view` is vi
   opened read only, so its file operand is `read`: the table refuses `:w` and `:w!` whatever the editor's option says. A
   narrower grant than the accepted text; if the architect wants the same operand, one word changes.
7. **Hints.** The hint for `less` (ADR-0154 rule 6) is removed because a program of that name exists; `more`'s says
   `to page a file: less FILE` (ADR-0335 item 11). The tour's two lines and `docs/tour.md` follow.
8. **Two files of one directory** (`vi a.txt b.txt`): ADR-0335 item 3 (accepted) says a connection narrowed to both names, and
   the list of names in `narrow` (ADR-0337 item 8) is **not built** in this part. The planner does not widen instead: a line that
   gives `vi`, `ex`, `view` or `less` two names of one directory is **refused**, with the hint to write `+w DIR` (`+r DIR` for
   `view` and `less`) or to name one file; a word that starts with `+` (`vi +10 f`, `less +G f`) is refused for every `cwd none`
   description, with the hint `./NAME` for a file of that name and `-c COMMAND` for a command. `sed`'s two files keep ADR-0344
   item 2's rule. This is a gap in what the accepted ADR promises (two files in one `vi` without the directory), not a wider grant.
9. **DB 1 and `lock`** (ADR-0390's open questions): DB 1.85 stays (a maintained copy is a swap of the archive and
   `ports/db1/build.sh`); nvi's `lock` option stays and locks nothing (the `flock` call is patched out, a recorded
   difference from ADR-0335 item 1). No measurement on a 100 MB file was made in this part.
10. **`vttest`** is pinned: release 20251205 (`vttest-20251205.tgz`, MIT-style notice, Thomas E. Dickey), SHA-256
    `cd6886f9aefe6a3f6c566fa61271a55710901a71849c630bf5376aa984bf77cc`, the same bytes from `invisible-island.net`
    and `invisible-mirror.net`, in `ports/sources.lock`. `tools/term-record/build-vttest.sh` builds it for the host
    (not the target and not the image); `tools/term-record/record.py` records its cursor and screen menus like the other
    sessions. The by-hand checklist of ADR-0332 item 10 is part 10i's.
11. **A test program for the terminal** (`tests/posix/t_curses.c`, built into the POSIX image): `setupterm` of a known
    and an unknown name from `/tmp`, the keys of `keypad(TRUE)` (Backspace is `KEY_BACKSPACE`), and `LINES`, `COLS` at
    start and after each `KEY_RESIZE`.

12. **ncurses on a 64 KiB stack.** A process starts on 64 KiB, and `setupterm` faulted in every curses program: ncurses'
    `_nc_read_tic_entry`, `_nc_read_file_entry` and `_nc_read_termtype` each hold an array of 32 KiB (`MAX_ENTRY_SIZE`), three
    calls deep. `ports/patches/ncurses/0001-terminfo-buffers-on-the-heap.patch` takes the three arrays from the heap (the
    functions keep their names; the arrays become their callers' to give). The host's `tic` is not patched. A program that
    holds a 32 KiB array on a 64 KiB stack, a library's or its own, will meet the same limit.
13. **Keys of a pager after a native command.** A pipeline whose first element is native gives that element the keys
    (ADR-0387 item 2), so `ls | less` shows the first screen and `less` reads no key. `/bin/cat f | less` and `tcc -v &| less`,
    where a POSIX program is first, page. This part does not change ADR-0387; `help less` does not say it yet and the hint for
    `more` is the only word.

**Tensions with accepted ADRs.** Item 6 is narrower than ADR-0335 item 9; item 8 leaves ADR-0335 item 3's two-file form unbuilt (refused, not widened). ADR-0390 is proposed, not accepted; its "left for 10d" lines are done here.

### Effect of the vision

Target 2: the grants of the two programs are the ones the line names, as listed by `grants`; no line gets more than it names: the two-file case is refused (item 8). Target 3: the editor and pager people expect.

## Consequences

- **Positive.** `vi`, `ex`, `view` and `less` are in the image with their descriptions; the entry says what the terminal
  sends; the early shim is gone; the sysroot has curses for programs built inside Zoraya.
- **Negative.** `ls | less` does not page (item 13); `:e` of a name the grant does not give says "new file" until part 10f's status row says why; two files of one directory are refused until `narrow` takes a list (item 8); `ports` builds ncurses, DB 1, less and nvi on every run
  (minutes on CI's build job); a release of ncurses that renames `xterm+kbs` stops the entry's build.
- **Follow-up.** `narrow` with a list; a measurement of DB 1 on a large file.

## Invariants

- **INV-0391-1.** `creat` makes or cuts a file and writes only, `sleep` returns the seconds left rounded up, and each is
  imported by a program of the image. *Check:* `cargo test -p zoraya-posix --test files` and `posix-surface-check imports`
  in `make check-ports`.
- **INV-0391-2.** The compiled `xterm-256color` and `xterm` say `kbs=^?`, and the terminal's Backspace is the bytes of `kbs`.
  *Check:* `make check-terminfo` and `cargo test -p zoraya-term` (`backspace_and_the_views_own_keys`).
- **INV-0391-3.** The image holds `/bin/less`, `/bin/vi`, links `/bin/ex` and `/bin/view` to `/bin/vi`, the four entries and
  ncurses' two archives, and a curses program calling `setupterm` finds the entry. *Check:* `ports/check-curses.sh` in `make check-ports`
  and `make test-posix` (`zosh-curses.script`).
- **INV-0391-4.** A curses program with `keypad(TRUE)` reads Backspace as `KEY_BACKSPACE`, and gets the window's size at start
  and a `KEY_RESIZE` after each change. *Check:* `make test-tour-terminal` (`posix-curses.script`).
- **INV-0391-5.** `vi` and `less` plan to the grants of ADR-0335 item 3 from the real description files, and the options they
  let through are those the descriptions list. *Check:* `cargo test -p zoraya-shell` (`grants_tests.rs`).
- **INV-0391-6.** A line that gives `vi`, `ex`, `view` or `less` two names of one directory, or a word that starts with `+` as a file, is refused
  with a hint, and no plan gives the directory. *Check:* `cargo test -p zoraya-shell` (`grants_tests.rs`) and `make test-posix` (`zosh-curses.script`).

## Open questions

- Should `narrow` take a list of names in this stage (ADR-0337 item 8), or may the refusal of item 8 stand until a part needs it?
- Which DB 1: 1.85 or a maintained copy, once a 100 MB file has been measured?
- Whether `view` should have `vi`'s operand (item 6).
- How the keys should reach a pager after a native first element (item 13): a description key for "reads no standard input when given a file", or the last POSIX element of a pipeline taking the keys when the first reads none.

## Revisit triggers

- ncurses is updated (the `xterm+kbs` fragment).
- A curses program in the image needs `panel`, `menu` or `form`.
- `vttest`'s recordings show a difference with xterm in a menu the terminal claims.

## Architect's decision

Accepted 2026-11-16. Decided by the architect; recorded by an agent at the architect's request.

Option A, with one amendment. Item 8: `narrow` takes a list of names (ADR-0337 item 8), as decided; to be built (part 10f built `narrow` with a list at run time; part 10j or 10f for the rest, that is the planner's use of it for `vi a.txt b.txt`). Until it is built the refusal of item 8 stands, and INV-0391-6 changes with that build. The other open questions are covered as follows: DB 1 (item 9: 1.85 stays; its licence is accepted, the University of California having rescinded the advertising clause of the 4-clause BSD licence in 1999, so the licence reads as 3-clause BSD); `view`'s operand (item 6: the narrower `read` operand stands, as written); the keys of a pager after a native first element (item 13: stays as written, not decided further here).
