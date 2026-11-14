# ADR-0335: `vi` is nvi2 and the pager is `less`, each configured to touch only what its line gives; `edit` stays the desktop's editor

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0107, ADR-0109, ADR-0153, ADR-0156, ADR-0168, ADR-0331, ADR-0333, ADR-0334, ADR-0337, ADR-0341, ADR-0342
- **Evidence:** CP-27, DM-087, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#bsd-and-other-permissive-sources, stage-10/survey.md#editors-and-pagers-compared, stage-10/survey.md#files-an-editor-or-pager-touches-unasked, stage-10/survey.md#less-and-more, stage-10/survey.md#nvi, stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy, stage-10/survey.md#small-editors, stage-10/survey.md#the-name-devtty, stage-10/survey.md#toybox-upstream, stage-10/survey.md#vim, stage-10/survey.md#write-by-rename-and-derived-files

## Context

Inside a terminal a person today has no editor and no pager: `edit` is a window of the desktop (ADR-0168), and the
shell's `help` pages itself. POSIX programs expect both to exist by name: git starts `vi` for a commit message and
`less` for its output, and `less` starts `vi` on `v`. This ADR chooses the two programs, how they are built, and
what they may touch. Which files they can reach while running is ADR-0337's subject; here the question is what each
does on its own initiative.

**What is known.**

- The candidates and their licences: Vim (the Vim licence, not on the permissive list); nvi2 (three-clause BSD,
  version 2.2.2 of 2025-10-08, with multibyte support); busybox `vi` (GPL version 2 only, not Unicode-capable);
  toybox `vi` (0BSD, 1,871 lines, in `pending`, off by default); kilo (BSD, about 1,300 lines, modeless, no undo,
  no UTF-8); `less` (GPL 3 or the two-clause "Less License"); toybox has no `less` and its `more` is in `pending`
  (`stage-10/survey.md#editors-and-pagers-compared`, `stage-10/survey.md#toybox-upstream`).
- nvi keeps its buffer in a Berkeley DB record file under `/tmp`, wants a recovery directory and a mailer for
  recovery notices, and can lock the edited file with `flock`; nvi2 needs `dbopen` from a DB 1 library, curses and a
  terminfo library, and optionally iconv (`stage-10/survey.md#nvi`).
- What each touches unasked: Vim a swap file beside the edited file, `~/.viminfo`, `~/.vimrc`, backup and undo
  files; nvi its start-up files in the home and current directory and the recovery directory; `less` a history
  file, key files, a preprocessor named by `LESSOPEN`; all three run a shell on request
  (`stage-10/survey.md#files-an-editor-or-pager-touches-unasked`).
- An editor that saves by writing a sibling and renaming needs rights on the directory; one that writes in place
  needs the one file. No surveyed system grants a file together with its derived names
  (`stage-10/survey.md#write-by-rename-and-derived-files`).
- `less` reads its keys from `/dev/tty` when its input is a pipe (`stage-10/survey.md#the-name-devtty`).
- OpenBSD confines its `vi` and `less` by removing powers, not files, and offers `vi -S` and `LESSSECURE` to remove
  the shell escapes (`stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy`).

**What is assumed.** That nvi writes the edited file in place and not by rename: the survey read its buffer and
recovery code, not its write path. 10c verifies it first (INV-0335-1 is the test); if it is false the choice is
reopened before anything else is built.

## Decision drivers

1. **`vi` means vi.** People and programs that type `vi` expect POSIX `vi` and `ex`
   (`stage-10/survey.md#editors-and-pagers-compared`).
2. **Unmodified ports with small patch sets** (CP-27).
3. **The one file the line names is enough to edit and save it** (ADR-0153's "create or replace this one name";
   `stage-10/survey.md#write-by-rename-and-derived-files`).
4. **Nothing is read or written behind the person's back**: no history, no start-up file, no swap file that the
   line or the program's own directory does not cover (`stage-10/survey.md#files-an-editor-or-pager-touches-unasked`;
   DM-087's lesson on per-program configuration scattered in the system).
5. **UTF-8 text** (ADR-0013).
6. **A licence the project accepts for a ported program** (ADR-0109).

## Considered options

### Option A: Vim, in its smallest build, and `less` (what Linux distributions install)

- **Description:** Vim built `--with-features=tiny`, with swap, backup and viminfo switched off; `less` as below.
- **Evidence for:** The editor most people mean by `vi` today; multibyte in every build; switches exist for each
  unasked file (`-i NONE`, an empty `directory`, `nobackup`) and a restricted mode (`stage-10/survey.md#vim`).
- **Evidence against:** The Vim licence is not one of the permissive licences (driver 6 needs a decision of its
  own). By default it writes a swap file beside the file and, depending on `backupcopy`, saves by rename (driver 3);
  each is a setting a start-up file can turn back on (driver 4). The largest candidate by far.
- **Cost:** *(estimate)* A large port; a compiled-in configuration to keep it inside its grant.
- **Precedent:** Every Linux distribution.

### Option B: nvi2, and `less` under the Less License

- **Description:** The maintained descendant of the BSD `vi`, with wide characters, and Nudelman's `less`.
- **Evidence for:** Drivers 1, 2, 5 and 6: it is POSIX `vi` and `ex`, BSD-licensed, multibyte, active
  (`stage-10/survey.md#nvi`). Its unasked files are few and each has a switch or a place in ADR-0341's directory
  (driver 4). `less` is the only real pager among the candidates and has a permissive licence option
  (`stage-10/survey.md#less-and-more`).
- **Evidence against:** nvi2 needs a DB 1 library that musl does not have, a recovery directory with Unix modes and
  a mailer, all to be supplied or cut (`stage-10/survey.md#nvi`). It has no syntax colouring and fewer comforts
  than Vim. Driver 3 rests on an assumption until 10c checks it.
- **Cost:** *(estimate)* Two ports and a small DB 1 library; a patch to cut recovery mail.
- **Precedent:** The BSDs' `vi`; Chimera Linux ships nvi as its `vi`
  (`stage-10/survey.md#bsd-and-other-permissive-sources`).

### Option C: The small ones: toybox's or busybox's `vi`, and `more`

- **Description:** Enable toybox's pending `vi` and `more`, or busybox's `vi` and `less`.
- **Evidence for:** No curses and no terminfo (ADR-0334 Option C); tiny; toybox's are 0BSD and already in the tree
  (`stage-10/survey.md#small-editors`).
- **Evidence against:** toybox's `vi` and `more` are in `pending`, unreviewed and off by default, its `vi` saves
  through `FILE.swp` and a rename (driver 3), and it knows a short command list (driver 1); busybox's are GPL
  version 2 only and its `vi` is not Unicode-capable (drivers 5 and 6) (`stage-10/survey.md#small-editors`,
  `stage-10/survey.md#toybox-upstream`). There is no `less` in toybox at all.
- **Cost:** *(estimate)* Days; and a replacement later.
- **Precedent:** Android (toybox), embedded Linux (busybox).

### Option D: No ported editor: the system's own (Plan 9's way)

- **Description:** Give `edit` a terminal mode, or write a small native full-screen editor, and a native pager;
  POSIX programs that want `vi` get a wrapper that starts it.
- **Evidence for:** Drivers 3 and 4 by construction; the grants and the chooser are native to it. Plan 9 wrote its
  editors new rather than port `vi` (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Driver 1: it is not `vi`, and people who live in a terminal ask for `vi`. A native program
  cannot draw a full screen yet (ADR-0331 item 9). Writing an editor is the scope growth CP-27 warns about.
- **Cost:** *(estimate)* A new program of several thousand lines, and a native terminal interface first.
- **Precedent:** Plan 9's `sam` and `acme`; kilo shows the lower bound and what it lacks.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`vi`.** nvi2 2.2.2, pinned in `ports/sources.lock`, built with wide characters, without iconv, against
   ADR-0334's library, installed as `/bin/vi` with `ex` and `view` as link objects to it.
2. **Its record file.** `dbopen` comes from a DB 1 library built with the port from the BSD `db` sources
   `[assumption: 10c pins which copy; nvi 1.81.6 bundles a reduced one]`. The record file is created in the line's
   own `/tmp` (ADR-0153 rule 10), so it is gone when the line ends and no other line sees it.
3. **Saving.** `vi FILE` is given `FILE` as ADR-0153's `create` kind: the one name, to read, change, create or
   replace. nvi writes the file itself, in place `[assumption, see Context]`. The `lock` option is compiled off
   (`flock` is not offered). Nothing is created beside the file.
4. **No recovery in stage 10.** The recovery directory, the recovery mail and `vi -r` are compiled out. A crash
   loses the unsaved session; the person is told by `help vi`. ADR-0341's directory is where recovery files would
   go later (open question).
5. **No start-up file.** `/etc/vi.exrc`, `~/.nexrc`, `~/.exrc` and the current directory's files are not read; the
   `exrc` option is fixed off. `EXINIT`, if the person exports it, is honoured: it is on the line or in the shell's
   start file, where it can be seen.
6. **Programs it starts.** `:!`, `:r !cmd` and filters run `/bin/sh` as a child with the editor's own table
   (ADR-0107), so a command typed inside `vi` reaches exactly what `vi` reaches. No `secure` mode is forced.
7. **`less`.** The current release of `less` under the Less License, linked with the terminfo half of ADR-0334's
   library, as `/bin/less`. Compiled-in defaults: no history file, no key files, no input preprocessor
   (`LESSOPEN` is ignored), no tags. `v` starts `vi` and `!` a shell, as children with the pager's table.
   `LESSSECURE` is not set: the table is the confinement. With its input from a pipe it reads keys from
   `/dev/tty` (ADR-0331 item 2).
8. **Descriptions**, in the image (ADR-0153 rule 10):

   ```
   name     vi
   usage    vi [-R] [-c COMMAND] [FILE...]
   program  /bin/vi
   operand  [FILE...]  create  the files to edit; each may be created or replaced
   option   -R --readonly  open the files to read only
   when     -R FILE read
   option   -c COMMAND text  an ex command to run first
   ```

   and for `less`, `operand [FILE...] read`. `view` and `ex` have their own with the same operand.
9. **Environment.** The shell sets no `EDITOR`, `VISUAL` or `PAGER`: git and `less` fall back to `vi` and `less`
   by themselves, and a person who wants another exports it.
10. **What `edit` remains for.** `edit` is the desktop's editor: a window, the chooser, the clipboard, no modes. It
    is what the file manager opens and what `edit FILE` from `zosh` starts. `vi` is the editor inside a terminal and
    the one POSIX programs call. They share files and nothing else. There is no `more`; the shell's hint for it
    names `less`.

**Examples.**

```
~/proj> vi notes.txt
```

The window shows the file; `i`, text, `Esc`, `:wq` saves and returns. Before running it:

```
~/proj> grants vi notes.txt
vi gets:
  in, out, err         the terminal
  ~/proj               read (current directory)
  ~/proj               create or replace notes.txt (operand FILE)
```

(with the lines every POSIX program has: `/bin`, `/usr`, `/etc` to read and a `/tmp` of its own). Inside,
`:e todo.txt` shows another file of the current directory, since that directory is readable; `:w` of it is refused
and the terminal says so (ADR-0337). `vi -R /config/settings` gives the file to read only.

```
~/proj> less big.log
~/proj> tcc -v &| less
```

On the desktop the same happens in the terminal window; double-clicking `notes.txt` in the file manager still
opens `edit`.

**Tensions with accepted ADRs.**

- *ADR-0153 rule 2*: a `create` operand means "create or replace this one name"; an editor is the first program
  that reads it first. The narrowed connection already carries `read` (rule 2's table), so nothing changes.
- *ADR-0154 rule 6* gives a hint when a person types `less`, `more` or `man` and nothing of that name exists; with
  `less` in the image the hint for `less` ends, and the one for `more` names `less`.
- *ADR-0168*: `edit` keeps its role; nothing of it changes.
- *ADR-0005*: nvi2's and `less`'s imports enter the surface by ADR-0342.

### Effect of the vision

For target 2 the two programs are ordinary untrusted-capable ports: built so that they ask for nothing outside
their line, and held to it by the table when they do. A shell escape inside them is no escape. Target 3 gets the
editor and pager people expect in a terminal, beside the desktop's own. Networking, threads: none.

## Consequences

- **Positive.** Real `vi` and `ex`, a real pager, permissive licences, UTF-8, and a grant of one file that is
  enough to edit it. Neither program leaves files the person did not ask for.
- **Negative.** No crash recovery and no start-up file at first; no syntax colouring; a DB 1 library to carry;
  driver 3 depends on a fact 10c must confirm.
- **Follow-up.** The two ports and the DB 1 library; their descriptions and `help` pages; the hint texts;
  `docs/dependencies.md`; `docs/tour.md` gains an editing step.

## Invariants

- **INV-0335-1.** With only its one-name entry for `FILE` and its `/tmp`, `vi FILE` reads, changes and saves
  `FILE`, and the directory holds the same names afterwards. *Check:* an in-QEMU script that runs `vi` on a file
  of a directory other than the current one, so that the narrowed entry is all it has of that directory (the
  keys typed by the runner), then compares the directory listing and the file's content.
- **INV-0335-2.** From start to exit with no command typed but quit, `vi` and `less` name no path outside their
  entries. *Check:* the same script reads the `REPORT` channel's count for each (ADR-0153 rule 11) and expects
  zero; a `strings` check of both binaries for the start-up and history file names that were compiled out.
- **INV-0335-3.** A program started from inside `vi` or `less` holds no entry its parent lacks. *Check:* the script
  runs `:!cat /config/settings` in a `vi` started without that grant and expects the refusal.
- **INV-0335-4.** `/bin/vi`, `/bin/ex`, `/bin/view` and `/bin/less` each have a description in the image, and no
  description of them offers an option that names a file the description does not give a kind. *Check:* the
  description lint of ADR-0336 (INV-0336-2) covers them.

## Open questions

- Where recovery files could live so that `vi -r` works: ADR-0341's directory fits, if the port's checks on the
  directory's mode can be met.
- A start-up file for `vi`: a file in ADR-0341's directory would be visible and owned, which `~/.exrc` is not.
- Whether Vim is wanted later as a second editor; its licence needs the architect's word first.

## Revisit triggers

- nvi2 turns out to save by rename, or to need more than the one name (INV-0335-1 fails).
- People ask for Vim's features in numbers that justify Option A.
- A native full-screen interface exists (ADR-0331's open question): Option D becomes buildable.

## Architect's decision
