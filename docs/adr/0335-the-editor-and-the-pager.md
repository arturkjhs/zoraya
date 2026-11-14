# ADR-0335: `vi` is nvi2 and the pager is `less`, each given its operands and nothing of the current directory; `edit` stays the desktop's editor

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0107, ADR-0109, ADR-0153, ADR-0156, ADR-0168, ADR-0311, ADR-0331, ADR-0333, ADR-0334, ADR-0337, ADR-0338, ADR-0341, ADR-0342
- **Evidence:** CP-27, DM-087, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#bsd-and-other-permissive-sources, stage-10/survey.md#editors-and-pagers-compared, stage-10/survey.md#files-an-editor-or-pager-touches-unasked, stage-10/survey.md#less-and-more, stage-10/survey.md#nvi, stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy, stage-10/survey.md#small-editors, stage-10/survey.md#the-name-devtty, stage-10/survey.md#toybox-upstream, stage-10/survey.md#vim, stage-10/survey.md#write-by-rename-and-derived-files

## Context

Inside a terminal a person today has no editor and no pager: `edit` is a window of the desktop (ADR-0168), and the
shell's `help` pages itself. POSIX programs expect both to exist by name: git starts `vi` for a commit message and
`less` for its output, and `less` starts `vi` on `v`. This ADR chooses the two programs, how they are built, and
what they may touch. Which files they can reach while running is ADR-0337's subject; here the question is what each
does on its own initiative, and what the line `vi FILE` gives.

**What is known.**

- The candidates and their licences: Vim (the Vim licence: charityware, GPL-compatible); nvi2 (three-clause BSD,
  version 2.2.2 of 2025-10-08, with multibyte support); busybox `vi` (GPL version 2 only, not Unicode-capable);
  toybox `vi` (0BSD, 1,871 lines, in `pending`, off by default); kilo (BSD, about 1,300 lines, modeless, no undo,
  no UTF-8); `less` (GPL 3 or the two-clause "Less License"); toybox has no `less` and its `more` is in `pending`
  (`stage-10/survey.md#editors-and-pagers-compared`, `stage-10/survey.md#toybox-upstream`).
- nvi2 needs `dbopen` from a DB 1 library, curses and a terminfo library, and optionally iconv; it wants a
  recovery directory and a mailer for recovery notices, and can lock the edited file with `flock`
  (`stage-10/survey.md#nvi`).
- Read in nvi2's source by the review (`stage-10/review-notes.md`, findings 2.1, 2.6, 2.7, 2.8, 2.20):
  - **It saves in place.** `file_write` opens the edited name with `O_CREAT|O_WRONLY|O_TRUNC` and writes into it;
    there is no temporary sibling and no rename. The save is therefore not atomic.
  - It reads the edited file through DB 1; the record file of its buffer is the recovery file in the recovery
    directory, or, with recovery cut, DB 1's own unlinked temporary file in `/tmp`. DB 1 tries to map the file
    and falls back to reading when the mapping fails.
  - Its ex command `:script` calls `openpty` and `setsid`, and its `lock` option calls `flock`; both are compiled
    in without a switch.
  - It reads `/etc/vi.exrc`, then `NEXINIT` or `EXINIT`, then `~/.nexrc` or `~/.exrc`, whatever the `exrc` option
    says (that option governs only the current directory's file).
  - It builds with CMake and Ninja and ships no other build files.
- What each touches unasked: Vim a swap file beside the edited file, `~/.viminfo`, `~/.vimrc`, backup and undo
  files; `less` a history file, key files, a preprocessor named by `LESSOPEN`; all three run a shell on request
  (`stage-10/survey.md#files-an-editor-or-pager-touches-unasked`).
- An editor that saves by writing a sibling and renaming needs rights on the directory; one that writes in place
  needs the one file. No surveyed system grants a file together with its derived names
  (`stage-10/survey.md#write-by-rename-and-derived-files`).
- In `zosh`, two entries for the same directory are merged (ADR-0153 rule 2): a `create` operand in the current
  directory joins the current directory's read entry, and the program gets "create and change anything in" that
  directory, as `cp a b` and `tcc -o` do today. ADR-0311 gave curl `cwd none` so that its one file stays one file.
- `less` reads its keys from `/dev/tty` when its input is a pipe (`stage-10/survey.md#the-name-devtty`).
- OpenBSD confines its `vi` and `less` by removing powers, not files, and offers `vi -S` and `LESSSECURE` to remove
  the shell escapes (`stage-10/survey.md#pattern-c-a-trusted-helper-with-a-fixed-policy`).

**What is assumed.** That the DB 1 copy chosen keeps the path that reads when a file cannot be mapped; Zoraya
answers `ENODEV` to a file mapping.

## Decision drivers

1. **`vi` means vi.** People and programs that type `vi` expect POSIX `vi` and `ex`
   (`stage-10/survey.md#editors-and-pagers-compared`).
2. **Unmodified ports with small patch sets** (CP-27).
3. **The one file the line names is enough to edit and save it, and is all the editor gets** (ADR-0153's "create
   or replace this one name"; `stage-10/survey.md#write-by-rename-and-derived-files`).
4. **Nothing is read or written behind the person's back**: no history, no start-up file, no swap file that the
   line or the program's own directory does not cover (`stage-10/survey.md#files-an-editor-or-pager-touches-unasked`;
   DM-087's lesson on per-program configuration scattered in the system).
5. **UTF-8 text** (ADR-0013).
6. **Size and build**: every program is static and loaded whole (ADR-0109).

## Considered options

### Option A: Vim, in its smallest build, and `less` (what Linux distributions install)

- **Description:** Vim built `--with-features=tiny`, with swap, backup and viminfo switched off; `less` as below.
- **Evidence for:** The editor most people mean by `vi` today; multibyte in every build; switches exist for each
  unasked file (`-i NONE`, an empty `directory`, `nobackup`) and a restricted mode (`stage-10/survey.md#vim`). Its
  licence is GPL-compatible, which a separate ported program may be (ADR-0109 already carries GNU make).
  With `backupcopy=yes` it also saves in place.
- **Evidence against:** By default it writes a swap file beside the file and may save by rename, so its defaults
  must be compiled the other way (driver 3). It is the largest candidate in source, and its size as a static
  Zoraya program is not measured (driver 6). Like nvi, its settings can be changed back by a start-up variable.
- **Cost:** *(estimate)* A large configure-based port; a compiled-in configuration to keep it inside its grant.
- **Precedent:** Every Linux distribution.

### Option B: nvi2, and `less` under the Less License

- **Description:** The maintained descendant of the BSD `vi`, with wide characters, and Nudelman's `less`.
- **Evidence for:** Drivers 1, 3 and 5: it is POSIX `vi` and `ex`, multibyte, active (`stage-10/survey.md#nvi`),
  and it saves in place, so the one name is enough. Its unasked files are few and each can be cut (driver 4).
  `less` is the only real pager among the candidates and has a permissive licence option
  (`stage-10/survey.md#less-and-more`).
- **Evidence against:** The patch set is not small (driver 2): the `:script` command, the `flock` call, recovery
  and its mail, the system and home start-up files, and a build of its own, because the ports have no CMake. It
  needs a DB 1 library that musl does not have. In-place saving means an interrupted save can lose the file. It
  has no syntax colouring and fewer comforts than Vim.
- **Cost:** *(estimate)* Two ports, a small DB 1 library, a hand-written makefile for nvi2 and four patches.
- **Precedent:** The BSDs' `vi`; Chimera Linux ships nvi as its `vi`
  (`stage-10/survey.md#bsd-and-other-permissive-sources`).

### Option C: The small ones: toybox's or busybox's `vi`, and `more`

- **Description:** Enable toybox's pending `vi` and `more`, or busybox's `vi` and `less`.
- **Evidence for:** No curses and no terminfo (ADR-0334 Option C); tiny; toybox's are 0BSD and already in the tree
  (`stage-10/survey.md#small-editors`).
- **Evidence against:** toybox's `vi` and `more` are in `pending`, unreviewed and off by default, its `vi` saves
  through `FILE.swp` and a rename (driver 3), and it knows a short command list (driver 1); busybox's are GPL
  version 2 only and its `vi` is not Unicode-capable (driver 5) (`stage-10/survey.md#small-editors`,
  `stage-10/survey.md#toybox-upstream`). There is no `less` in toybox at all.
- **Cost:** *(estimate)* Days; and a replacement later.
- **Precedent:** Android (toybox), embedded Linux (busybox).

### Option D: No ported editor: the system's own (Plan 9's way)

- **Description:** Give `edit` a terminal mode, or write a small native full-screen editor, and a native pager;
  POSIX programs that want `vi` get a wrapper that starts it.
- **Evidence for:** Drivers 3 and 4 by construction; the grants and the chooser are native to it. Plan 9 wrote its
  editors new rather than port `vi` (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Driver 1: it is not `vi`, and people who live in a terminal ask for `vi`. A native program
  cannot draw a full screen yet (ADR-0331 item 11). Writing an editor is the scope growth CP-27 warns about.
- **Cost:** *(estimate)* A new program of several thousand lines, and a native terminal interface first.
- **Precedent:** Plan 9's `sam` and `acme`; kilo shows the lower bound and what it lacks.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`vi`.** nvi2 2.2.2, pinned in `ports/sources.lock`, built with wide characters, without iconv, against
   ADR-0334's library, installed as `/bin/vi` with `ex` and `view` as link objects to it. The port has its own
   makefile in `ports/` (no CMake or Ninja enters the host tools) and these patches: the `:script` command is
   removed (it needs a pseudo-terminal, ADR-0338); the `flock` call and the `lock` option are removed; recovery,
   its directory and its mail are removed; the system and home start-up files are not read.
2. **Its buffer.** `dbopen` comes from a DB 1 library built with the port from the BSD `db` sources
   `[assumption: the implementation part pins which copy; nvi 1.81.6 bundles a reduced one]`. With recovery cut, the
   buffer's record
   file is DB 1's own unlinked temporary file in the line's `/tmp` (ADR-0153 rule 10), gone when the line ends.
3. **What the line gives: its operands, and nothing of the current directory.** The description says `cwd none`
   (ADR-0311 item 5's mechanism): `vi FILE` is given `FILE` as ADR-0153's `create` kind, the one name to read,
   change, create or replace, and no other name of its directory, whether or not that is the current directory.
   Another file, also one beside it, is a refusal that ADR-0337 shows and lets the person lift; a person who
   wants a directory says so (`+w . vi notes.txt`) or works in a room. Two files of one directory
   (`vi a.txt b.txt`) need a connection narrowed to both names, which is ADR-0337 item 8's change to `narrow` and
   is built with this port. With no operand in the current directory the program starts in `/bin`, read-only
   (ADR-0311 item 5): `vi` alone and then `:w notes.txt` is refused there, and `help vi` says to name the file on
   the line.
4. **Saving is in place and not atomic.** nvi truncates and rewrites the file. A crash, a kill or a full volume
   during `:w` can leave it short, and with recovery cut no other copy exists. `help vi` says so.
5. **No recovery in stage 10.** `vi -r` is not offered. A crash loses the unsaved session. ADR-0341's directory is
   where recovery files would go later (open question).
6. **No start-up file.** Only `NEXINIT` and `EXINIT` are honoured, if the person exports one: they are on the line
   or in the shell's start file, where they can be seen. The current directory's `.exrc` stays off, as nvi's own
   default has it.
7. **Programs it starts.** `:!`, `:r !cmd` and filters run `/bin/sh` as a child with the editor's own table
   (ADR-0107), so a command typed inside `vi` reaches exactly what `vi` reaches. No `secure` mode is forced.
8. **`less`.** One release of `less`, pinned by its implementation part, under the Less License, linked with the
   terminfo half of
   ADR-0334's library, as `/bin/less`, also `cwd none`. Compiled-in defaults: no history file, no key files, no
   input preprocessor (`LESSOPEN` is ignored), no tags. `v` starts `vi` and `!` a shell, as children with the
   pager's table. `LESSSECURE` is not set: the table is the confinement. With its input from a pipe it reads keys
   from `/dev/tty` (ADR-0331 item 3).
9. **Descriptions**, in the image (ADR-0153 rule 10):

   ```
   name     vi
   summary  edit text files (nvi, a POSIX program)
   usage    vi [-R] [-c COMMAND] [FILE...]
   program  /bin/vi
   cwd      none
   operand  [FILE...]  create  the files to edit; each may be created or replaced
   option   -R --readonly  open the files to read only
   when     -R FILE read
   option   -c COMMAND text  an ex command to run first
   ```

   and for `less`, `cwd none` and `operand [FILE...] read`. (The format wants a long form for every option and nvi
   has none; the implementing part either lets a description give a short form alone or has the shell pass the
   short one.) `view` and `ex` have their own with the same operand.
10. **Environment.** The shell sets no `EDITOR`, `VISUAL` or `PAGER`: git and `less` fall back to `vi` and `less`
    by themselves, and a person who wants another exports it.
11. **What `edit` remains for.** `edit` is the desktop's editor: a window, the chooser, the clipboard, no modes. It
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
vi gets (a POSIX program):
  in, out, err         the terminal
  keys                 the terminal, while this line is in front
  ~/proj               create or replace notes.txt (operand FILE)
  /bin /usr /etc       read (system)
  /proc/self/exe       read (system)
  /tmp                 change anything in (this line's own)
  /dev/null
```

Inside, `:e todo.txt` is refused, the terminal says so, and System+G gives the file (ADR-0337).
`+w . vi notes.txt` starts it with the whole directory, to change (a `+r .` would be merged with the file's entry
into the same thing, by ADR-0153 rule 2); `vi -R /config/settings` gives that file to read only.

```
~/proj> less big.log
~/proj> tcc -v &| less
```

On the desktop the same happens in the terminal window; double-clicking `notes.txt` in the file manager still
opens `edit`.

**Tensions with accepted ADRs.**

- *ADR-0153 rule 1* (every command receives the current directory, read-only, "unless its description says `cwd
  none`, as commands that read no files do"): `vi` and `less` read files and say `cwd none` all the same, for
  ADR-0311's reason: with the current directory, rule 2's merging would turn the one name into the directory.
- *ADR-0153 rule 2*: a `create` operand means "create or replace this one name"; an editor is the first program
  that reads it first. The narrowed connection already carries `read` (rule 2's table), so nothing changes.
- *ADR-0154 rule 6* gives a hint when a person types `less`, `more` or `man` and nothing of that name exists; with
  `less` in the image the hint for `less` ends, and the one for `more` names `less`.
- *ADR-0109*: two more archives and a DB 1 library; nvi2 is built by a makefile of the ports, not by its own
  CMake files.
- *ADR-0168*: `edit` keeps its role; nothing of it changes.
- *ADR-0005*: nvi2's and `less`'s imports enter the surface by ADR-0342.

### Effect of the vision

For target 2 the two programs are ordinary untrusted-capable ports: built so that they ask for nothing outside
their line, given exactly their operands, and held to it by the table when they try more. A shell escape inside
them is no escape. Target 3 gets the editor and pager people expect in a terminal, beside the desktop's own.
Networking, threads: none.

## Consequences

- **Positive.** Real `vi` and `ex`, a real pager, permissive licences, UTF-8, and a grant of one file that is
  enough to edit it and is all the editor has. Neither program leaves files the person did not ask for.
- **Negative.** A save is not atomic: an interrupted `:w` can lose the file, and there is no recovery. Opening a
  second file, even one beside the first, needs a grant. No start-up file; no `:script`; no syntax colouring. A
  DB 1 library, a makefile and four patches to carry.
- **Follow-up.** The two ports, the makefile and the DB 1 library; their descriptions and `help` pages; the hint
  texts; `docs/dependencies.md`; `docs/tour.md` gains an editing step.

## Invariants

- **INV-0335-1.** `vi FILE` holds of `FILE`'s directory only the one name, in the current directory and in any
  other; it reads, changes and saves `FILE`, the directory holds the same names afterwards, and `:w OTHER` in that
  directory is refused. *Check:* a host test of the shell's plan for `vi` with a file in and outside the current
  directory; an in-QEMU script that edits and saves each (the keys typed by the runner), tries `:w` of a sibling,
  edits two files of one directory in one `vi`,
  and compares the directory listings and the files' content.
- **INV-0335-2.** From start to exit with no command typed but quit, `vi` and `less` name no path outside their
  entries and leave nothing in `/tmp`. *Check:* the same script reads the `REPORT` channel's count for each
  (ADR-0153 rule 11) and expects zero, and lists the line's `/tmp` while they run; a `strings` check of both
  binaries for the names that were compiled out (the start-up and history files, `openpty`, `flock`).
- **INV-0335-3.** A program started from inside `vi` or `less` holds no entry its parent lacks. *Check:* the script
  runs `:!cat /config/settings` in a `vi` started without that grant and expects the refusal.
- **INV-0335-4.** `/bin/vi`, `/bin/ex`, `/bin/view` and `/bin/less` each have a description in the image, and no
  description of them offers an option that names a file the description does not give a kind. *Check:* the
  description lint of ADR-0336 (INV-0336-2) covers them.

## Open questions

- Whether a save should be followed by a sync, and whether an atomic save is worth a kind that gives one name and
  its temporary sibling.
- Where recovery files could live so that `vi -r` works: ADR-0341's directory fits, if the port's checks on the
  directory's mode can be met.
- A start-up file for `vi`: a file in ADR-0341's directory would be visible and owned, which `~/.exrc` is not.
- Whether `vi` should get the current directory to read after all (`:e` of a sibling without a grant), at the
  price of the merge that makes the whole directory writable; a `read`-only current directory that does not merge
  with a `create` name would need a change to ADR-0153 rule 2.
- Whether Vim is wanted later as a second editor.

## Revisit triggers

- People lose files to interrupted saves.
- The patch set against nvi2 grows with each release: Vim, or OpenBSD's `vi`, compared again.
- A native full-screen interface exists (ADR-0331's open question): Option D becomes buildable.

## Architect's decision
