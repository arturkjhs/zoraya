# ADR-0333: `TERM` is `xterm-256color`, and its entry is a pinned file in `/usr/share/terminfo`

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0107, ADR-0109, ADR-0153, ADR-0157, ADR-0331, ADR-0332
- **Evidence:** CP-21, DM-085, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#how-ncurses-finds-a-terminal-description, stage-10/survey.md#netbsd-curses, stage-10/survey.md#programs-and-libraries-without-curses, stage-10/survey.md#python-curses-readline-and-the-repl, stage-10/survey.md#ssh-and-the-terminal, stage-10/survey.md#terminfo-entries-compared, stage-10/survey.md#the-tty-service-as-built, stage-10/survey.md#what-a-posix-program-is-given-today, stage-10/survey.md#what-a-small-terminal-implements, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal, stage-10/survey.md#which-term-to-claim

## Context

A full-screen program learns what its terminal can do from the environment variable `TERM` and the terminfo entry
of that name. Today `TERM` is `dumb` in a POSIX session and unset from `zosh`, and the image holds no terminfo file
(`stage-10/survey.md#the-tty-service-as-built`). Python's REPL refuses a terminal without `cup`, `clear`, `el` and
`bel`; Vim requires cursor motion; curses refuses a generic entry
(`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`). So a terminal ships with a name and an entry, or
nothing full-screen runs. This ADR decides the name, where the entry lives, and what a program sees.

**What is known.**

- A new name works only where its entry is. kitty and st document breakage after ssh to a host without their entry
  and under `sudo`; foot's entry in a private path "simply does not work" in some situations. `xterm-256color` is
  present on every host; ncurses also carries entries for st, foot, alacritty and kitty
  (`stage-10/survey.md#which-term-to-claim`).
- Claiming an xterm name has costs: foot's author lists them (the entry may gain capabilities the terminal lacks;
  programs that see `xterm` assume xterm's behaviour beyond the entry), and Vim's documentation says such terminals
  "quite often ... are not 100% compatible" (`stage-10/survey.md#which-term-to-claim`).
- The ssh client sends the local `TERM` to the far side unchanged, where the far side's entry of that name is used
  (`stage-10/survey.md#ssh-and-the-terminal`).
- Programs with no terminfo reader fall back to built-in tables keyed by name: Vim's builtin termcaps, termbox2's
  table, NetBSD curses' embedded set (`ansi`, `dumb`, `vt100`, `vt220`, `wsvt25`, `xterm`)
  (`stage-10/survey.md#programs-and-libraries-without-curses`, `stage-10/survey.md#netbsd-curses`).
- ncurses finds an entry through `TERMINFO`, `~/.terminfo`, `TERMINFO_DIRS` and then its compiled-in directory; it
  can also hold entries inside the library, or take one from `TERMINFO=hex:...`. Python 3.14's own reader takes a
  directory tree only (`stage-10/survey.md#how-ncurses-finds-a-terminal-description`,
  `stage-10/survey.md#python-curses-readline-and-the-repl`).
- Every POSIX program already has `/usr` to read, as part of the toolchain set
  (`stage-10/survey.md#what-a-posix-program-is-given-today`).
- The resolved `xterm-256color` entry has 297 capabilities; foot's has 264, st's 214
  (`stage-10/survey.md#terminfo-entries-compared`).

**What is assumed.** That reaching other machines with ssh, and running their full-screen programs, is an everyday
use of stage 10 (the task says so). That the far side's `xterm-256color` entry differs from the pinned one only in
ways ADR-0332's "ignore what is unknown" rule absorbs.

## Decision drivers

1. **It works on the far side of ssh without copying a file there** (`stage-10/survey.md#which-term-to-claim`,
   `stage-10/survey.md#ssh-and-the-terminal`).
2. **Programs with built-in tables work too** (`stage-10/survey.md#programs-and-libraries-without-curses`).
3. **What is claimed is what is implemented**, checkably (ADR-0332 driver 4).
4. **No new grant and no per-user state**: a program finds the entry in what it is already given
   (`stage-10/survey.md#what-a-posix-program-is-given-today`); configuration that travels in the environment is
   the kind that drifts (DM-085, CP-21).
5. **One source** for the entry, pinned and reproducible (ADR-0109).

## Considered options

### Option A: Claim `xterm-256color`, with a pinned copy of its entry in the image

- **Description:** `TERM=xterm-256color`. The entry is compiled from the `terminfo.src` of the pinned ncurses
  release and installed as a file; ADR-0332 implements every capability in it.
- **Evidence for:** Drivers 1 to 5. The name has an entry on every host and in every built-in table. The pinned
  entry is a finite specification the terminal is tested against (INV-0332-1), which answers foot's first objection
  (the entry changing under the terminal) for local programs.
- **Evidence against:** foot's second objection stays: a program that sees `xterm` may assume more than the entry
  (window reports, title stack, every private mode). On a far host the entry is that host's version, not the pinned
  one. The terminal cannot be told apart from xterm by name.
- **Cost:** *(estimate)* The size of ADR-0332's Option B, which is sized for this entry; a build step.
- **Precedent:** The survey's sources name the reach of the entry, not which terminals claim it; foot recommends
  against it for itself and says it "will, in many cases, work" (`stage-10/survey.md#which-term-to-claim`).

### Option B: An own name, `zoraya`, with an own entry

- **Description:** A new entry lists exactly what the terminal does. It ships in the image; a copy is offered to
  ncurses' database.
- **Evidence for:** Driver 3 without reservation: the entry says the truth, extensions can be added at will, and
  programs cannot assume xterm. foot, st, alacritty and kitty all chose this
  (`stage-10/survey.md#what-a-small-terminal-implements`).
- **Evidence against:** Driver 1 fails until every far host has the entry: the documented experience of kitty and
  st, which kitty answers with a tool that copies files over ssh (`stage-10/survey.md#which-term-to-claim`). Driver
  2 fails: no built-in table knows the name. Programs that test `TERM` for `xterm` to enable colour or the mouse do
  not.
- **Cost:** *(estimate)* Writing and maintaining an entry; a rule that makes ssh send another name, and then the
  terminal must satisfy that other name anyway.
- **Precedent:** foot, st, alacritty, kitty; each documents the ssh problem.

### Option C: A small old name: `vt220` or `linux`

- **Description:** Claim an entry that exists everywhere and is small (107 and 119 capabilities).
- **Evidence for:** Drivers 1, 2 and 5; the smallest set to implement (`stage-10/survey.md#terminfo-entries-compared`).
- **Evidence against:** Neither has the alternate screen in its entry, `vt220` has no colour, `linux` has eight
  colours and tells programs they are on the Linux console; no 256 colours, no bracketed paste names
  (`stage-10/survey.md#terminfo-entries-compared`). `less` and `vi` would leave their text on the screen at exit.
- **Cost:** *(estimate)* Smallest; and a second migration later.
- **Precedent:** The Linux console; serial terminals.

### Option D: No terminal database: programs ask the terminal, or there is nothing to ask (Plan 9)

- **Description:** Either the terminal answers capability queries in band (foot's `XTGETTCAP`), or, as in Plan 9's
  windows, there are no capabilities because there is no cursor addressing.
- **Evidence for:** Nothing to ship and nothing to mismatch across ssh: foot offers the query so that "you would no
  longer have to install foot's terminfo on remote hosts" (`stage-10/survey.md#which-term-to-claim`). Plan 9 shows a
  system can live without the database (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** curses, `less`, nvi and Python read terminfo and do not send the query
  (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`). The query's reply carries names the program
  chose, which ADR-0332 item 5 refuses. Plan 9's way excludes every program in the stage's list.
- **Cost:** *(estimate)* None for the system; every port for the programs.
- **Precedent:** foot's query (a supplement, not a replacement); Plan 9.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The name.** A POSIX program that receives the terminal of a window (ADR-0331 item 2) has
   `TERM=xterm-256color` in its environment unless the line sets `TERM` itself. A program with no terminal has no
   `TERM` from the shell.
2. **The entry is a file.** `/usr/share/terminfo/x/xterm-256color`, in ncurses' directory-tree form, compiled at
   image-build time by the host's `tic` from the `terminfo.src` of the ncurses release that ADR-0334 pins in
   `ports/sources.lock`. With it ship `xterm` (for programs that strip the suffix), `vt100` and `dumb`, and no
   other entry. `/usr` is in every POSIX program's table already, so the entry needs no grant, no `TERMINFO`
   variable and no copy inside each program; ncurses, `less` and Python's reader all find it at their default path.
3. **The entry is the specification.** ADR-0332 implements every capability of the pinned `xterm-256color` and its
   conformance tool reads this same file (INV-0332-1). When the pin moves, the tool lists what changed.
4. **Nothing else in the environment.** The shell sets no `TERMINFO`, `TERMINFO_DIRS`, `LINES`, `COLUMNS` or
   `COLORTERM`. Direct colour is accepted (ADR-0332 item 2) and not advertised.
5. **The serial console.** There the terminal is whatever the person's host runs, which Zoraya cannot know. `TERM`
   comes from a setting, `console.term` (ADR-0157), default `vt100` `[assumption]`: a subset nearly every emulator
   honours. The test session keeps `TERM=dumb` unless its session file says otherwise.
6. **Across ssh** the name travels as it is (ADR-0340); the far side uses its own `xterm-256color` entry, and
   sequences of that entry which the terminal does not know are ignored (ADR-0332 item 1).

**Examples.**

```
~/proj> sh -c 'echo $TERM'
xterm-256color
~/proj> sh -c 'echo $TERM' | cat
xterm-256color
~/proj> sh -c 'echo $TERM' < notes.txt > t.txt 2> e.txt; cat t.txt

~/proj> TERM=vt100 vi notes.txt
```

The second command prints the name because `sh` still has the terminal on its input and its error stream; the third
prints an empty line, because none of its three streams is the terminal. `grants sh` shows `/usr   read` as it does
today:
the entry is inside it. On the desktop nothing is shown; a person sees only that `vi` draws correctly.

**Tensions with accepted ADRs.**

- *ADR-0104 item 12 and ADR-0110 item 1* set `TERM=dumb` for the session; that stands for the test session.
- *ADR-0153 rule 10*'s note: a POSIX program gets `PATH` and `HOME` "unless the line sets them"; `TERM` joins them
  under the condition of item 1.
- *ADR-0157*: one more setting, `console.term`, declared by the shell.
- *ADR-0109*: the image build gains a host tool dependency, `tic`, built from the pinned ncurses source for the
  host so that no installed version is trusted (recorded in `docs/dependencies.md` by the implementing part).

### Effect of the vision

None on the choice for target 2: an entry in a read-only directory every program already has adds no authority, and
no variable tells a program where to look (driver 4). For target 3 it is what lets a person treat the terminal as
the one they know. Networking: the choice is made for ssh (driver 1). Threads: none.

## Consequences

- **Positive.** Local ports, programs with built-in tables and remote programs all find a description; the claim
  is checked capability by capability; no grant, no variable, one pinned source.
- **Negative.** A program may assume xterm behaviour outside the entry and find it missing (ADR-0332's revisit
  trigger collects those). The terminal has no name of its own. The image carries four small files and the build
  one more host tool.
- **Follow-up.** The build step and its check; `console.term`; the shell's environment rule; a line in
  `docs/tour.md` once `vi` exists.

## Invariants

- **INV-0333-1.** The files under `/usr/share/terminfo` are exactly `x/xterm-256color`, `x/xterm`, `v/vt100` and
  `d/dumb`, each the output of the pinned `tic` on the pinned `terminfo.src`. *Check:* a build check that recompiles
  them and compares SHA-256 with a committed manifest, in `make check-ports`.
- **INV-0333-2.** A POSIX program started by `zosh` has `TERM` from the shell exactly when it has the terminal on
  at least one of fds 0 to 2, and then its table covers the entry's file. *Check:* a host test of the shell's
  environment and table for lines with and without a terminal; an in-QEMU script printing `$TERM` in both cases.
- **INV-0333-3.** The shell never sets `TERMINFO`, `TERMINFO_DIRS`, `LINES`, `COLUMNS` or `COLORTERM` for a
  program. *Check:* the same host test, asserting their absence.

## Open questions

- Whether to advertise direct colour (a `COLORTERM` convention exists `[outside evidence: not fetched]`); until a
  ported program needs it, nothing is set.
- Whether the terminal should have a way to be recognised (a variable or the device-attribute version) for
  programs that want to enable Zoraya-specific behaviour; none exists yet.
- The default of `console.term`; a measurement of which host terminals people use `make run` in would settle it.

## Revisit triggers

- A ported program misbehaves because it assumes xterm beyond the entry, and the fix is not a sequence to add:
  Option B with a rule for ssh.
- The terminal gains features no xterm entry can describe (ADR-0332's native direction): an own entry beside this
  one.
- ncurses changes the entry in a way the terminal should not follow.

## Architect's decision
