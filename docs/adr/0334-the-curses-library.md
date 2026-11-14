# ADR-0334: The curses library is ncurses, wide-character and static, reading the one terminfo directory

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0100, ADR-0109, ADR-0141, ADR-0331, ADR-0333, ADR-0342
- **Evidence:** CP-27, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#curses-libraries-compared, stage-10/survey.md#editors-and-pagers-compared, stage-10/survey.md#ncurses, stage-10/survey.md#netbsd-curses, stage-10/survey.md#programs-and-libraries-without-curses, stage-10/survey.md#python-curses-readline-and-the-repl, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal, stage-10/survey.md#what-ncurses-needs-from-libc

## Context

`vi` (ADR-0335) draws through curses; `less` reads terminfo through the lower half of the same library; Python's
`curses` module and its REPL in 3.13, `htop` and `tmux` need it later
(`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`,
`stage-10/survey.md#python-curses-readline-and-the-repl`). The image has no curses library and no terminfo reader.
This ADR decides which library is ported, in which configuration, and where it looks for the entry of ADR-0333.

**What is known.**

- ncurses is under an MIT-style (X11) notice. It builds with or without an on-disk database, can split its
  terminfo half into a separate library that is "about half the size of the total", and has a wide-character
  variant that is a different ABI (`stage-10/survey.md#ncurses`). Release 6.6's tarball is 3.79 MB; a competing
  project measured its shared library at 346 KB (`stage-10/survey.md#curses-libraries-compared`).
- netbsd-curses, the portable fork of NetBSD's library, is BSD-licensed, measured by its own authors at 150 KB,
  builds on musl, keeps its entries in one CDB file with a small embedded set, has no mouse functions, hides the
  structure members some programs touch, and needs patches for those programs (`stage-10/survey.md#netbsd-curses`).
- Programs can do without: toybox, busybox `vi` and kilo write ANSI sequences directly; termbox2 is one header with
  built-in tables; PDCurses draws to platform back ends instead of a terminal
  (`stage-10/survey.md#programs-and-libraries-without-curses`).
- Both curses libraries need termios, `SIGWINCH` and `SIGTSTP` handlers, and `poll` or `select` on the terminal
  (`stage-10/survey.md#what-ncurses-needs-from-libc`), which ADR-0331 and ADR-0342 supply.
- Python 3.14's REPL reads compiled terminfo files in ncurses' directory layout by itself; Python's `curses` module
  "is designed to match the API of ncurses" (`stage-10/survey.md#python-curses-readline-and-the-repl`).

**What is assumed.** That more curses programs follow `vi` (the task's survey list names `htop` and `tmux`). The
size of the static library in a Zoraya program is not measured; it is 10b's first number.

## Decision drivers

1. **The programs in scope build without patches to their curses use** (CP-27: a port's cost is its patch set;
   `stage-10/survey.md#curses-libraries-compared`).
2. **One terminfo format and one place**, the file of ADR-0333, read by the library and by Python's own reader
   (`stage-10/survey.md#python-curses-readline-and-the-repl`).
3. **Wide characters**, since file names and text are UTF-8 (ADR-0013; `stage-10/survey.md#ncurses`).
4. **A permissive licence** and a pinned source (ADR-0109).
5. **Size**, because every program is static and loaded whole at each `exec`
   (`stage-10/survey.md#curses-libraries-compared`).

## Considered options

### Option A: ncurses (what Linux distributions ship)

- **Description:** Port ncurses, static, wide-character, with its terminfo half as a separate library, reading the
  directory of ADR-0333.
- **Evidence for:** Drivers 1 to 4. It is the reference the other programs are written against: `tmux` depends on
  it, `htop` wants its wide variant, Python matches its API (`stage-10/survey.md#curses-libraries-compared`,
  `stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`). Its `tic` produces the files of ADR-0333, so
  the library and the entry come from one pinned archive.
- **Evidence against:** The largest of the candidates in source and binary (driver 5). Its configure script is
  long, and its build wants a host `tic` (`stage-10/survey.md#ncurses`).
- **Cost:** *(estimate)* One port with a `config.site` entry; a few hundred KB per program that links all of it,
  about half for one that links the terminfo half alone.
- **Precedent:** Every Linux distribution; the BSDs for ported software.

### Option B: netbsd-curses

- **Description:** Port the portable fork of NetBSD's curses and its `libterminfo`.
- **Evidence for:** Drivers 3, 4 and 5: BSD licences, under half the binary size by its authors' measure, a plain
  makefile, already built on musl (`stage-10/survey.md#netbsd-curses`).
- **Evidence against:** Driver 1: programs that touch curses structures need patches, mouse functions are no-ops,
  some functions are missing. Driver 2: its database is one CDB file, which Python's reader and ncurses-built
  programs do not read, so the image would carry two formats or lose Python's REPL
  (`stage-10/survey.md#netbsd-curses`, `stage-10/survey.md#python-curses-readline-and-the-repl`).
- **Cost:** *(estimate)* A smaller port now; a patch per later program.
- **Precedent:** NetBSD; sabotage Linux, whose authors list the patched programs.

### Option C: No curses library: only programs that write sequences themselves

- **Description:** Choose an editor and pager that emit ANSI directly (toybox's way) and port no library.
- **Evidence for:** Nothing to port, nothing to keep in step with the terminal but the sequences themselves; toybox's
  design says thirty years of terminals understand them (`stage-10/survey.md#programs-and-libraries-without-curses`).
- **Evidence against:** It decides ADR-0335 by exclusion: nvi and `less` read terminfo
  (`stage-10/survey.md#editors-and-pagers-compared`). Python's `curses`, `htop` and `tmux` stay impossible. Each
  such program carries its own idea of the terminal, which is what terminfo exists to prevent.
- **Cost:** *(estimate)* None now; the library later anyway, when the first curses program is wanted.
- **Precedent:** toybox and busybox; kilo.

### Option D: A curses that draws to the window directly (PDCurses and its Plan 9 flavour)

- **Description:** Port PDCurses with a new back end that speaks the terminal's protocol or the compositor's,
  skipping escape sequences and terminfo.
- **Evidence for:** No in-band control between program and terminal (ADR-0332 Option D's virtue); PDCursesMod
  already has a Plan 9 flavour, a system with no terminfo (`stage-10/survey.md#programs-and-libraries-without-curses`,
  `alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** It serves only curses programs: `less`, git's pager use, ssh and everything remote still
  need the VT path, so the terminal must have both. The competing project calls PDCurses unusable on Unix terminals
  (a competitor's claim, `stage-10/survey.md#programs-and-libraries-without-curses`).
- **Cost:** *(estimate)* A back end and a second display path through `zosh`.
- **Precedent:** PDCurses on DOS, Windows and SDL.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The port.** ncurses 6.6 `[assumption: the release current on 2026-11-13; 10b pins the archive and its
   SHA-256]`, in `ports/sources.lock`, built by `zcc` as two static archives in the sysroot: `libtinfow.a` (the
   terminfo half, `--with-termlib`) and `libncursesw.a` (`--enable-widec`), with headers in `/usr/include`. No C++
   binding (`--without-cxx`), no shared library, no programs for the target.
2. **One database, on disk.** The library is configured with `--with-default-terminfo-dir=/usr/share/terminfo` and
   no compiled-in entries. `tic` is built for the host from the same source and compiles the files of ADR-0333.
3. **Who links what.** A program that only positions the cursor through terminfo (`less`) links `libtinfow.a`; a
   curses program (`vi`) links both. A program built inside Zoraya with `tcc` finds the same headers and archives.
4. **No second reader.** No other curses, termcap or terminfo library enters the image; a later port that wants one
   is configured for ncurses or patched.
5. **What it asks of `libposix`** is ADR-0331's terminal and ADR-0342's names. ncurses installs its own `SIGWINCH`
   and `SIGTSTP` handlers; they run at the read's wait point (ADR-0106), which is where `wgetch` tests the flag.

**Examples.** A person never names the library. They see it through programs:

```
~/proj> vi notes.txt
```

draws with it. A program they build themselves finds it:

```
~/proj> tcc life.c -o life -lncursesw -ltinfow
~/proj> ./life
```

and `grants ./life` shows what any program of theirs gets: the terminal, the current directory to read, and `/usr`
with the entry inside.

**Tensions with accepted ADRs.**

- *ADR-0005* and *ADR-0141*: the library's imports must be exported by `libc.a`; ADR-0342 adds them with this port
  as the program behind each name.
- *ADR-0109*: one more archive in `sources.lock` and one more host tool (`tic`), built from that archive.
- *`CLAUDE.md`, Dependencies*: a ported library is recorded in `docs/dependencies.md` with its version, licence
  and purpose; the X11-style notice is in the MIT family the rule allows.

### Effect of the vision

None on the choice. The library runs inside each program and holds no authority; the entry it reads is in a
directory every program already has (ADR-0333). For target 2 an untrusted curses program is an ordinary writer of
the sequences ADR-0332 bounds. Target 3: the usual full-screen programs become portable. Networking, threads: none.

## Consequences

- **Positive.** The reference library, unpatched programs, one terminfo format shared with Python's reader, wide
  characters, one pinned source for library and entry.
- **Negative.** The largest candidate; a configure-based port; each static program carries its share of it.
- **Follow-up.** The port and its `config.site` lines; the measured archive and program sizes in the task report;
  `docs/dependencies.md`.

## Invariants

- **INV-0334-1.** The image and the sysroot hold exactly one terminfo reader library: ncurses' two archives.
  *Check:* a script over `ports/sources.lock` and the sysroot's `lib` directory, in `make check-ports`, that fails
  on another curses, termcap or terminfo archive.
- **INV-0334-2.** The library looks for entries in `/usr/share/terminfo` only by default and holds no compiled-in
  entry. *Check:* the port's build log records the configured directory and an empty fallback list, and an in-QEMU
  test runs a program that calls `setupterm("xterm-256color")` with success and `setupterm("no-such-terminal")`
  with the not-found result.
- **INV-0334-3.** A curses program gets the window's real size at start and a `KEY_RESIZE` after each change.
  *Check:* a C test program in the GUI script that prints `LINES` and `COLS` at start and on `KEY_RESIZE` while the
  runner resizes the window.

## Open questions

- The measured size of a static `vi` and `less` with this library, against Option B's; resolved by building both
  once if size becomes a complaint.
- Whether the legacy termcap calls (`tgetent`) should be offered from `libtinfow.a`; `less` can use either
  interface, and nothing in scope needs termcap.

## Revisit triggers

- Program load time is dominated by the library's size after ADR-0212's image cache exists.
- A program in scope cannot be built against ncurses.

## Architect's decision
