# ADR-0155: Conventions every native command follows

- **Status:** proposed
- **Date:** 2026-11-03
- **Depends on:** ADR-0004, ADR-0013, ADR-0055, ADR-0101, ADR-0151, ADR-0152, ADR-0153, ADR-0156
- **Evidence:** DM-008, DM-052, DM-077, DM-095, CP-12, shell/survey.md#freebsd-sh-and-base-system, shell/survey.md#powershell, shell/survey.md#nushell, shell/survey.md#command-line-conventions, shell/survey.md#what-this-suggests-for-zosh, alternatives-postmortem.md#oberon

## Context

`zosh`'s file commands are built into the shell today (`ls`, `cat`, `write`, `append`, `mkdir`, `rm`, `mv`, `stat`,
`sync`, `cd`, `pwd`), with errors such as `zosh: cat: NotFound`, the kernel's status name. Under ADR-0153 the file
commands become programs that receive only what a line designates, so they need one style for names, options, help,
errors and exit codes. FreeBSD keeps such a style by review (style(9), `err(3)`, `getopt`,
`shell/survey.md#freebsd-sh-and-base-system`); PowerShell enforces naming by an approved verb list and generates help
from declarations (`shell/survey.md#powershell`); Nushell gives every command `--help` from its signature
(`shell/survey.md#nushell`). Integer error codes with no message are a known mistake (DM-052); errors should be
typed values at the request (CP-12).

## Decision drivers

1. One option syntax and one help format, enforced by code rather than review
   (`shell/survey.md#what-this-suggests-for-zosh`).
2. A successful command prints only its result; a failed one says what, on what, and what to try
   (`shell/survey.md#command-line-conventions`).
3. Names a person can guess: familiar ones for familiar operations, plain words for new ones (POLA,
   `shell/survey.md#freebsd-sh-and-base-system`).
4. Safe with any name: an operand that starts with `-` cannot be mistaken for an option when it comes from a glob
   (DM-095, DM-008).

## Considered options

### Option A: POSIX utility conventions (baseline)
- **Description:** One-letter options after `-`, grouped; options before operands; `--` ends them; `usage:` line on
  error; exit 0 or 1 (style(9)); messages as `prog: subject: error`.
- **Evidence for:** Known, short; FreeBSD's base system shows it is coherent when reviewed
  (`shell/survey.md#freebsd-sh-and-base-system`, `shell/survey.md#command-line-conventions`).
- **Evidence against:** One-letter options are not self-describing; no long form; options after operands are refused,
  which surprises people who type `ls dir -l`; the style is a document, not a mechanism.
- **Cost:** *(estimate)* Small.
- **Precedent:** POSIX, FreeBSD.

### Option B: GNU conventions
- **Description:** As A, plus `--long` options, `--help` and `--version`, options anywhere before `--`.
- **Evidence for:** Long forms make scripts readable (`shell/survey.md#command-line-conventions`: "full-length
  versions of all flags"); options after operands are forgiving.
- **Evidence against:** Each program parses and documents itself, so help and behaviour drift.
- **Cost:** *(estimate)* Small per program.
- **Precedent:** GNU tools.

### Option C: Verb-noun commands with generated help (PowerShell)
- **Description:** `Get-Item`, `Remove-Item`, approved verbs, named parameters, help generated from declarations.
- **Evidence for:** Discoverable by verb; one place for help (`shell/survey.md#powershell`).
- **Evidence against:** Long names to type; nothing familiar for a person who knows `ls`; aliases bring back the short
  names anyway.
- **Cost:** *(estimate)* A naming scheme and every command renamed.
- **Precedent:** PowerShell.

### Option D: B's syntax, parsed from each command's description (Nushell's signatures)
- **Description:** GNU-style syntax, but every native command's options and operands are declared once in its
  description (ADR-0156); one shared crate parses argv from it, prints `--help` from it and formats errors, so syntax,
  help and completion cannot disagree.
- **Evidence for:** Drivers 1 to 3; Nushell and PowerShell show generated help works (`shell/survey.md#nushell`,
  `shell/survey.md#powershell`).
- **Evidence against:** Every native command depends on the crate and the description format; unusual syntaxes do not
  fit.
- **Cost:** *(estimate)* A `no_std` crate of about 1,000 lines with tests; each command loses its own parsing.
- **Precedent:** Nushell, PowerShell, clap-style derive parsers `[assumption: not fetched]`. Outside Unix, Oberon's
  text-as-command interface is the postmortem's nearest precedent of commands that a system, not each program,
  presents uniformly (`alternatives-postmortem.md#oberon`); it has no option syntax to compare.

## Recommendation

Recommended: **Option D**. It is a recommendation, not a decision.

1. **Names.** The classic names stay for the classic file operations: `ls`, `cat`, `cp`, `mv`, `rm`, `mkdir`,
   `stat`, `sync`, `head`, `sleep`. Every new command is one plain lower-case English word or two joined by `-`: `find`,
   `search`, `settings`, `grants`, `history`, `status`. One name per command, one command per action; no aliases
   shipped.
2. **Options.** Every option has a long form `--name`; common ones also a short form `-n`. Short options without
   arguments group (`-fv`). An argument follows as `--name value`, `--name=value` or `-n value`. Options may appear
   anywhere before `--`; `--` ends them. An argument that starts with `./` is never an option, which is how a glob
   gives names starting with `-` (ADR-0150 rule 7).
3. **Every command** accepts `--help`, printing from its description:
   ```
   cp: copy a file
   usage: cp [--force] FROM TO
     -f, --force   replace TO if it exists
     --help        show this help
   more: help cp
   ```
   and exits 0. A usage mistake prints the problem, the usage line and the `more:` line to `err`, and exits 2.
4. **Output.** A successful command prints only its result, to `out`: `cp`, `mv`, `rm`, `mkdir` and `sync` print
   nothing; `--verbose` (`-v`) says what changed. A listing prints one item per line, so that it pipes (ADR-0152),
   with directories ending in `/`, sorted byte by byte (ADR-0013 rule 3); `find` sorts whole paths. `search` prints
   `FILE:LINE: TEXT` for what it finds in files and the bare `TEXT` when it reads `in`. `--long` (`-l`) adds columns. No command pages, prompts or colours its output; the
   shell owns the terminal (ADR-0154).
5. **Errors.** Every failure is one line to `err`: `NAME: SUBJECT: PROBLEM`, in lower case, no final period, then
   optionally indented lines `  hint: ...` saying what to try. `PROBLEM` comes from the status by one table in the
   shared crate: `NotFound` "not found", `AlreadyExists` "already exists", `NotEmpty` "not empty", `IsDirectory`
   "is a directory", `NotDirectory` "not a directory", `NotGranted` and `AccessDenied` "not granted" (with a hint
   naming the `+r` or `+w` grant, ADR-0153), `StorageFailed` "the volume has failed", `TooManyOpen` "too many open
   files", any other status its name in words. Never the bare status name, never a number alone (DM-052, CP-12).
6. **Exit codes.** 0 success; 1 failure; 2 wrong usage. A command whose description declares it a **question**
   (`question` in its header: `test`, `search`) uses 0 for "yes", 1 for "no", 2 for wrong usage and 3 for a failure,
   so the shell can tell "no" from an error (ADR-0151 rule 3). A command that stops because its `out` reader closed (`PeerClosed`, ADR-0101) exits 0
   and prints nothing (DM-077).
7. **The shared crate** (`zoraya-command`, `no_std`, host-tested) parses argv from the description, prints `--help`
   and usage errors, formats errors and maps statuses, and handles `PeerClosed`. A native command may not parse argv
   itself.
8. **Builtins and programs.** A builtin is what must act on the shell's own state or authority: `cd`, `pwd`, `set`,
   `unset`, `export`, `count`, `echo`, `test`, `math`, `source`, `exit`, `status`, `history`, `help`, `grants`,
   `jobs`, `fg`, `bg`, `wait`, `kill`, `settings` (the shell owns `/config`, ADR-0157), `slots` and `uptime`. `exit`
   is the one way to end the shell and turn the machine off; `poweroff` goes, as a second name for it.
   Everything that acts on files is a program and receives only what the line
   designates (ADR-0153): `ls`, `cat`, `cp`, `mv`, `rm`, `mkdir`, `stat`, `sync`, `head`, `sleep`, `find`,
   `search`. The demonstration programs (`hello`, `spin`, `stubborn`, `fault`, `exit-with`, `echo-args`) have
   descriptions and are listed by `help` apart from the commands; the `t_*` test programs and the services are not. `search TEXT [PATH...]` reads `in` when
   no path is given, and searches directories recursively. Builtins
   follow the same option, help, error and exit-code rules through the same crate. `write` and `append` go:
   redirection does their job (ADR-0152); `ls-ramdisk` becomes `ls /zoraya/bin`; `exit-code` becomes `status`; `run`
   goes, and the demonstration program `exit` is renamed `exit-with` so that `exit` is only the builtin.

**Examples.**

```
~> cp
cp: missing operand FROM
usage: cp [--force] FROM TO
more: help cp
~> cp a.txt b.txt
~> cp a.txt b.txt
cp: b.txt: already exists
  hint: cp --force a.txt b.txt replaces it
~> rm notes
rm: notes: is a directory
  hint: rm --recursive notes removes it and everything in it
~> ls
a.txt
b.txt
notes/
~> search milk notes
notes/list.txt:1: buy milk
~> search cheese notes
~> echo $status
1
~> search cheese nowhere
search: nowhere: not found
~> echo $status
3
~> find '*.txt'
a.txt
b.txt
notes/list.txt
```

### Effect of the vision

For target 2 a program generated or run by an agent fails in one predictable format a supervisor can parse; and
argument parsing that only the shared crate does means one place to harden against hostile argv.

**Tensions with accepted ADRs.** None. The removed builtins and the renamed demonstration program change tests, not
decisions (`docs/design.md`, "Using the shell", is rewritten from the tour).

## Consequences
- **Positive.** One syntax, one help format, one error format, by construction; programs smaller.
- **Negative.** Every native command is rewritten onto the crate; test scripts that use `write`, `ls-ramdisk`,
  `exit-code`, `run` and the program `exit` change.
- **Follow-up.** `zoraya-command`; descriptions for every command; the status-to-words table; test scripts.

## Invariants
- **INV-0155-1.** Every native command and builtin prints its `--help` from its description, exits 0 for it, and
  exits 2 for an unknown option or a missing operand. *Check:* an in-QEMU test that runs each command listed in the
  ramdisk and each builtin with `--help` and with `--no-such-option`, comparing `--help` with the description
  rendered on the host.
- **INV-0155-2.** No native program parses argv except through the shared crate. *Check:* a lint (a `grep` in
  `make test`) that no program under `user/src/bin` other than test programs reads its argv vector directly.
- **INV-0155-3.** Every error line a native command prints matches `NAME: SUBJECT: PROBLEM` or `NAME: PROBLEM`, with
  no raw status name. *Check:* a host test of the formatter over every `Status`, and the scripted sessions' error
  lines matched against the pattern.
- **INV-0155-4.** A native command whose `out` reader closes exits 0 without writing to `err`. *Check:* an in-QEMU
  test `cat big | head --lines 1` with each listing command in place of `cat`.

## Open questions
- Whether `rm` should ask before removing a directory tree at the terminal; Option D's answer is no prompts, only
  `--recursive`.
- Machine-readable output (`--records`, ADR-0152 Option C) for listing commands, when a need is measured.

## Revisit triggers
- A ported or new command whose syntax the description cannot express.
- People repeatedly typing the GNU spelling of an option that has another name here: add it as the long form.

## Architect's decision
