# ADR-0156: Help, the manual and command descriptions from one source

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0033, ADR-0108, ADR-0151, ADR-0153, ADR-0154, ADR-0155
- **Evidence:** DM-054, CP-23, shell/survey.md#freebsd-sh-and-base-system, shell/survey.md#fish, shell/survey.md#nushell, shell/survey.md#powershell, shell/survey.md#command-line-conventions, shell/survey.md#what-this-suggests-for-zosh

## Context

`zosh`'s `help` prints five fixed lines. TASK-18 asks for `help` and a manual viewer where one source feeds
`--help`, completion and the manual, so that they cannot disagree. ADR-0153 adds a fourth reader: the shell grants
designated operands from the same description, so a description is now also a security statement, and ADR-0155 makes
the shared crate parse argv from it.

**What is known.** FreeBSD writes manuals in mdoc with a fixed section order and usage lines by style(9), kept in
agreement by review (`shell/survey.md#freebsd-sh-and-base-system`). PowerShell generates syntax and the parameter list
from the declaration and takes prose from comments (`shell/survey.md#powershell`); Nushell does the same from a
signature and doc comments, and shows descriptions in completion menus (`shell/survey.md#nushell`). fish keeps
completions in per-command files separate from the program, and can generate them from manual pages
(`shell/survey.md#fish`). clig asks help to lead with examples (`shell/survey.md#command-line-conventions`).

## Decision drivers

1. One source per command for usage, options, operand kinds, help text and manual; nothing written twice
   (`shell/survey.md#what-this-suggests-for-zosh`).
2. The description travels with the program, so an installed program and its description cannot be of different
   versions.
3. A plain text format a person reads and writes without tools, parsed by one total, host-tested parser (DM-054's
   lesson about ad-hoc text interfaces; CP-23).
4. Documentation answers questions where they arise: `--help` at the command, `help NAME` at the prompt, completion
   on Tab.

## Considered options

### Option A: Manual pages beside programs (baseline)
- **Description:** A man page per command in mdoc, `man NAME`; each program writes its own usage text; completion
  separate.
- **Evidence for:** Known; mdoc has a good section order (`shell/survey.md#freebsd-sh-and-base-system`).
- **Evidence against:** Three sources that drift (driver 1); a man page renderer is large.
- **Cost:** *(estimate)* A roff subset renderer, thousands of lines.
- **Precedent:** Unix.

### Option B: Help generated from code declarations (PowerShell, Nushell)
- **Description:** Options and operands declared in Rust (attributes or a macro); prose in doc comments; help and
  completion read from the compiled program.
- **Evidence for:** One source in code (`shell/survey.md#powershell`, `shell/survey.md#nushell`).
- **Evidence against:** The shell must run or load the program to read it, or the build must extract it; POSIX
  programs have no such declaration; long manual prose in Rust attributes is awkward.
- **Cost:** *(estimate)* A proc-macro or a build step.
- **Precedent:** PowerShell, Nushell.

### Option C: One description file per command, compiled into the program
- **Description:** `NAME.cmd`, a plain text file with a header (summary, usage, operands with kinds, options, exit
  statuses, settings) and a body (description, examples, see also). The build embeds it in the program's ELF as a
  section; the shared crate parses argv and prints `--help` from it; the shell reads it from the program file for
  completion, grants and `help`; POSIX programs get a `.cmd` file installed beside the system.
- **Evidence for:** Drivers 1 to 4. The text is readable and reviewable like a man page; the embedding keeps it with
  the program (driver 2).
- **Evidence against:** A new format to specify and parse; the ramdisk must expose descriptions without running
  programs; an external description for a POSIX program can still drift from that program.
- **Cost:** *(estimate)* A parser crate (about 600 lines), a build step per program, a pager.
- **Precedent:** fish's per-command completion files (separate, not embedded); the rest is new. The postmortem
  records no manual or help system of a non-Unix system to compare (its Plan 9 and Oberon sections are about
  namespaces and the text interface); PowerShell's generated help (Option B) is the non-POSIX precedent.

## Recommendation

### Decision

Accepted as recommended (architect, 2026-11-03). `help NAME` begins by saying what `NAME` is and where it comes
from (item 4): the agent proposed this line to carry out the hint for `which` that the architect asked for (ADR-0154
rule 6), and the architect accepted it (2026-11-03).

The original recommendation follows.

Recommended: **Option C**. It is a recommendation, not a decision.

1. **Format.** A UTF-8 text file. The header is lines `KEY VALUE` until a line `---`; then the body.
   ```
   name     cp
   summary  copy a file
   usage    cp [--force] FROM TO
   operand  FROM  read    the file to copy
   operand  TO    create  the name of the copy
   option   -f --force    replace TO if it exists
   exit     0  copied
   exit     1  not copied
   ---
   Copies the file FROM to a new file TO. TO must not exist unless --force is given.

   EXAMPLES
     cp notes.txt notes.bak
     cp --force draft.txt final.txt

   SEE ALSO
     mv, rm
   ```
   Keys: `name`, `summary` (one line), `usage` (one or more), `operand NAME KIND TEXT` (kinds of ADR-0153: `read`,
   `write`, `create`, `remove`, `remove-tree`, `rename`, `tree`, or `text` for a non-path),
   `option SHORT LONG [ARG KIND] TEXT`, `when OPTION OPERAND KIND` (an option that changes an operand's kind:
   `when --recursive PATH remove-tree`), `cwd read|none`, `question` (ADR-0155 rule 6), `program PATH` (only in
   `/zoraya/commands/`: the program file the description is for, ADR-0153 rule 10), `exit CODE TEXT`, `setting NAME TYPE DEFAULT TEXT` (ADR-0157), `topic` instead of `name` for a
   page that is not a command. Body sections are upper-case lines; the order is that of mdoc
   (`shell/survey.md#freebsd-sh-and-base-system`): DESCRIPTION (the untitled first part), EXAMPLES, FILES, EXIT
   STATUS, SEE ALSO. The parser is total and bounded; a file it refuses fails the build.
2. **Where it lives.** A native program's description is in its ELF as a section `.zoraya.command`, which the loader
   ignores (ADR-0033, ADR-0108 item 5: only `PT_LOAD` is loaded). The ramdisk packer also stores each program's
   description as a ramdisk entry `NAME.cmd`, so the shell reads it without an image. Builtins' and topics'
   descriptions are compiled into `zosh`. POSIX programs' descriptions are entries `commands/NAME.cmd` of the
   ramdisk, which the shell shows as `/zoraya/commands/`, part of the system image and read-only; each names the
   program file it is for, and the shell trusts it for that file alone (ADR-0153 rule 10). Nothing on the volume is
   read as a description.
3. **Readers.** `zoraya-command` (ADR-0155) parses argv and prints `--help`; the shell completes (ADR-0154), grants
   (ADR-0153) and shows help from the same parsed form. Functions get theirs from the comment lines above `fn`
   (ADR-0151): the first line is the summary, and parameters are `text` operands.
4. **`help`.** `help` lists topics, then commands with their summaries; `help NAME` shows the full page; `help
   --search WORD` lists the pages whose name or summary contains the word. Topics shipped: `language`, `grants`,
   `jobs`, `keys`, `settings`, `errors`, `tour` (this task's tour, abridged). *Added by the Decision:* the page
   begins with a line saying what `NAME` is: `cp is a program, /zoraya/bin/cp`, `cd is built into the shell`, `backup
   is a function, from /config/start.zosh`.
5. **The viewer.** A page longer than the screen is shown a screen at a time with a last line
   `-- space: next page, q: quit --`; output that goes to a pipe or file is not paged. The viewer is in the shell
   and uses only the ADR-0096 subset.

**Examples.**

```
~> help
Topics: language grants jobs keys settings errors tour
Commands:
  bg        continue a stopped job in the background
  cat       print files
  cd        change the current directory
  cp        copy a file
  ...
~> cp --help
cp: copy a file
usage: cp [--force] FROM TO
  -f, --force   replace TO if it exists
  --help        show this help
more: help cp
~> help --search copy
cp        copy a file
~> help cp
cp is a program, /zoraya/bin/cp

CP

copy a file

usage: cp [--force] FROM TO

  FROM   the file to copy (read)
  TO     the name of the copy (created)
  -f, --force   replace TO if it exists

Copies the file FROM to a new file TO. TO must not exist unless --force is given.

EXAMPLES
  cp notes.txt notes.bak
  cp --force draft.txt final.txt

EXIT STATUS
  0  copied
  1  not copied

SEE ALSO
  mv, rm
```

### Effect of the vision

For target 2 the description is the program's declared interface: what it may be given is readable before it runs,
and an agent can read `help NAME` the same way a person does. A program from an untrusted source would need its
description checked (operand kinds are authority); that check belongs to the launcher of such a program.

**Tensions with accepted ADRs.** *ADR-0033*'s ramdisk holds programs only; it gains description entries, which
`directory_lookup` (ADR-0043) never returns as images. Nothing else.

## Consequences
- **Positive.** `--help`, completion, grants and the manual agree by construction; a reviewer reads one file per
  command.
- **Negative.** A new text format to maintain; every command needs a description before it builds; external
  descriptions of POSIX programs can drift from them.
- **Follow-up.** The parser crate, the build step, the ramdisk entries, descriptions for every command and builtin,
  topic pages, the viewer.

## Invariants
- **INV-0156-1.** The description parser is total: any byte string gives a description or an error with a line
  number, in bounded memory. *Check:* a deterministic random test in `make test-unit` and a target in `fuzz/`.
- **INV-0156-2.** Every command of ADR-0155 rule 8, every demonstration program it names, every builtin and every
  entry of `/zoraya/commands/` has a description the parser accepts, and every option the program accepts is in it.
  *Check:* a build step that parses each description and fails on a missing or refused one; INV-0155-1 for options.
- **INV-0156-3.** `NAME --help`, `help NAME` and Tab completion of `NAME --` list the same options. *Check:* a host
  test rendering all three from each description.

## Open questions
- Whether to embed descriptions in the ELF and in the ramdisk both, or only in the ramdisk until programs come from
  the volume (ADR-0108).
- Translations: none now; the format has no language key.
- *Note, 2026-11-03 (TASK-18 Part 2, step 1):* item 2 is built as written: each described program carries
  `.zoraya.command` (`user/user.ld` keeps it in no segment) and the ramdisk holds `NAME.cmd` beside it, and the
  packer refuses a program whose section differs from its file. But no system call lets a process read a ramdisk
  entry's bytes (`directory_lookup` gives an image, `directory_entry` a name), so `zosh` cannot read `NAME.cmd` from
  the ramdisk. Until such a call exists (a kernel ABI decision, for the architect), `zosh` has the same files
  compiled in, checked by `user/build.rs`, so all three copies come from one source file and one build.
  *Architect, 2026-11-04 (recorded by an agent):* no new system call; the compiled-in descriptions are accepted
  for now. The target, open: `zosh` reads `NAME.cmd` as a file through a file service (a read-only service over the
  ramdisk, or the programs' directory on `fsd`), with no change to the ABI.

## Revisit triggers
- A program whose interface cannot be described by operands and options (a sub-command tree such as `git`): add
  sub-command sections.
- External descriptions found out of date with their POSIX programs: generate them from the programs' `--help`.

## Architect's decision

Accepted as recommended, 2026-11-03. Decided by the architect; recorded by an agent at the architect's request. The first line of
`help NAME` (item 4), proposed by the agent to carry out the `which` hint the architect asked for in ADR-0154, is
accepted by the architect (2026-11-03).
