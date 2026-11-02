# ADR-0153: What a program gets from the shell, and how a person grants more

- **Status:** proposed
- **Date:** 2026-11-03
- **Depends on:** ADR-0002, ADR-0003, ADR-0013, ADR-0053, ADR-0055, ADR-0101, ADR-0104, ADR-0105, ADR-0107, ADR-0108, ADR-0150, ADR-0152, ADR-0156
- **Evidence:** CP-01, CP-05, DM-009, DM-010, DM-014, DM-039, DM-041, DM-072, GR-004, GR-008, X-1, X-12, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, shell/survey.md#capability-shells-and-grant-syntax, shell/survey.md#what-this-suggests-for-zosh, CP-28, DM-087

## Context

A Zoraya process reaches only what its spawner listed (CP-01, CP-05, ADR-0003 INV-0003-1). The shell is the spawner
of almost everything a person runs, so the shell's choice of handles *is* Zoraya's security model as a person meets
it. Today `zosh` decides by program name: a hard-coded list gives `crashwriter`, `crashcheck` and `fsbench` a
connection to the volume root with every right, and `fanout` and `parent` the console, a job and the ramdisk; every
other program gets the console to write (`user/src/bin/zosh.rs`, `NEED_VOLUME`, `NEED_MORE`). Stage 6 adds POSIX
programs, which open paths named in their input through a namespace table the launcher builds (ADR-0002 rule 5,
ADR-0107), and a broker that owns pids and the terminal for them (ADR-0104, ADR-0105).

This ADR decides, for every command a person runs: what it receives by default; how the person grants more with a
short, readable syntax; how the shell shows what was granted; and how POSIX programs are run under the same rules.

**What is known.** Ambient authority by user identity is the Unix model and its confused-deputy and path-race
problems (DM-009, DM-010, DM-039, DM-041). Sandboxes retrofitted as syscall filters are policy-heavy (DM-014);
capability retrofits are cheap for programs that open what they are given and hard for programs that open paths late
(GR-008). Shells and launchers that decide per command exist but are niche (`shell/survey.md#capability-shells-and-grant-syntax`):
Plash put grants in the command line and gave every program read-only `/usr`, `/bin`, `/lib` and `/etc`; SHILL
declares a script's authority in contracts; Deno grants by flags (`--allow-read=./data`) and errors on a missing
grant; `unveil` names paths with `r`, `w`, `x`, `c`. Plan 9 and Fuchsia give each process a table of names to
objects built by its creator (GR-004, `alternatives-postmortem.md#plan-9--inferno`,
`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). TASK-02 measured the toolchain's working set of global
names as small and enumerable (ADR-0002 driver 2).

**What is assumed.** That most commands a person types name the files they act on as operands. That a person will
read a two-letter grant before a command. Neither is measured; the survey found no capability shell in wide daily use
(`shell/survey.md#what-this-suggests-for-zosh`).

## Decision drivers

1. Least authority by default, visible in the command line (CP-01, CP-05, DM-009).
2. The common case needs no grant: `cat notes.txt`, `cp a b`, `rm old.txt` must work as typed, or people will grant
   everything everywhere (DM-014's lesson that heavy policy is not used).
3. A grant is short, readable, and cannot come from data: no value, file or program output can widen what a command
   receives (DM-039, DM-072).
4. POSIX programs (tcc, make, dash scripts) run, through the broker and the namespace table, and what they see is
   just as explicit (ADR-0104, ADR-0107, X-1).
5. The person can always ask what a command would receive before running it.

## Considered options

### Option A: Everything the shell can reach (Unix baseline)
- **Description:** Every command gets the whole volume with every right, as a Unix process gets everything its user
  can open.
- **Evidence for:** Nothing ever fails for lack of access; scripts from elsewhere behave as expected; it is what
  `zosh` gives the three volume programs today.
- **Evidence against:** It is ambient authority by another name (DM-009, CP-01): any program can delete the volume.
  It precludes target 2 (agents in isolation).
- **Cost:** *(estimate)* None now; total later.
- **Precedent:** Unix shells.

### Option B: Nothing but streams; every path by an explicit grant (Deno, `unveil`)
- **Description:** A command gets `in`, `out` and `err`; anything else needs a grant such as `+r notes.txt cat
  notes.txt`.
- **Evidence for:** The smallest authority; every grant visible (`shell/survey.md#capability-shells-and-grant-syntax`).
- **Evidence against:** The common case needs a grant that repeats the operand, which is noise, and noise trains
  people to write `+w /` (driver 2).
- **Cost:** *(estimate)* Small in the shell.
- **Precedent:** Deno's flags; `unveil`; Plash's `plash-exec` defaults to nothing but system directories.

### Option C: Designation is authorization, from each command's description, plus explicit grants
- **Description:** Each command's description (ADR-0156) says which operands are paths and with what access
  (read, write, create, remove), and what it needs of the current directory. The shell gives the command exactly the
  paths the person typed in those positions, with that access, plus its streams and the current directory read-only.
  Anything more is an explicit grant written before the command: `+r PATH`, `+w PATH`.
- **Evidence for:** The person's act of naming a file is the grant, so the common case needs nothing (driver 2) and
  the authority is still exactly what the line shows (driver 1). Plash's shell put grants in the command line
  (`gcc -c foo.c => -o foo.o + .`) and SHILL makes a script's authority a declaration its users read
  (`shell/survey.md#capability-shells-and-grant-syntax`). The description is already needed for help and completion
  (ADR-0156), so it costs nothing extra to write.
- **Evidence against:** A wrong description grants the wrong access; programs without a description (POSIX ones)
  need either a description shipped beside them or explicit grants. A program that opens a path from inside a file
  it was given (an `#include`, a makefile's prerequisite) needs a directory grant: the Capsicum lesson (GR-008).
- **Cost:** *(estimate)* Operand kinds in the description format, a resolver shared with ADR-0107's, the grant
  syntax; around a thousand lines with tests.
- **Precedent:** Plash, SHILL (research), powerbox-style file dialogs; nothing at mainstream scale.

### Option D: Per-program permissions granted once, in configuration (manifests)
- **Description:** Each program has a stored permission set ("make may write the current directory", "tcc may read
  `/usr/include`"), set once by the person or the installer and applied on every run.
- **Evidence for:** No per-command syntax; mobile and Flatpak-style systems work this way `[outside evidence: not
  fetched, general knowledge]`.
- **Evidence against:** Authority is not visible on the line, and a stored grant applies to every future run with
  any operands (driver 1); the stored policy is configuration that drifts (DM-087's lesson applies). It answers
  "which program", not "which files".
- **Cost:** *(estimate)* A policy store and an editor for it.
- **Precedent:** Android permissions, Flatpak, macOS privacy prompts (not fetched).

## Recommendation

Recommended: **Option C**. It is a recommendation, not a decision.

**Every command receives** (native or POSIX):
1. `in`, `out`, `err`: the terminal through the shell, or the pipe ends and redirected files of ADR-0152.
2. Its **own job**, a sub-job of the shell's with `CREATE_PROCESS`, so it can start helpers that stay inside it
   (ADR-0003 rule 1, X-12).
3. The **current directory, read-only**: a `clone` of the shell's connection with `read` and `clone` only
   (ADR-0055), so relative names can be read and listed, never changed.
4. A **namespace table**: the record of ADR-0107 items 4 and 6 (prefix, kind, connection), longest prefix first, read
   at start-up by `zoraya-rt` for native programs and by `libposix` for POSIX ones. Paths a program names resolve
   lexically against it, with `..` clamped at an entry's root (INV-0002-4). A path outside every entry is `NotGranted`
   natively and `ENOENT` in `libposix` (ADR-0107 item 6).
5. Nothing else: no volume root, no console, no `Power`, no other job, no device.

**Designated operands.** A command's description (ADR-0156) gives each operand and option argument a kind; for each
path the person writes there, the shell adds a table entry:

| Kind | Entry the shell adds | Rights (ADR-0055) |
|---|---|---|
| `read` | the named file or directory | `read`, `clone` |
| `write` | the named existing file | `read`, `write`, `sync`, `clone` |
| `create` | the parent directory of the named path | `read`, `write`, `create`, `sync`, `clone` |
| `remove` | the parent directory of the named path | `read`, `remove`, `clone` |
| `rename` | the parent directories of both names | `read`, `rename`, `create`, `remove`, `clone` |
| `tree` | the named directory and everything below | every right but `publish` |

A description may also say `cwd: write`, for a program whose job is to change the current directory (`make`); the
current directory entry then gets the `tree` rights. A description cannot name a fixed path: authority comes only from
what the person typed and where they ran it. A redirection (ADR-0152 rule 3) is a designation too: the shell opens
the named file and the command receives that file alone. A designated `read` or `write` path that does not exist is
entered in the table as missing, so the program reports it as not found (`NotFound`, `ENOENT`) rather than as not
granted.

**Explicit grants.** Before the command name, `+r PATH` grants `read` on a file or directory tree and `+w PATH`
grants the `tree` rights. Several may be given. Rules:
- A grant is recognised only as a literal `+r` or `+w` token before the command name, written in the source; the
  value of a variable or substitution is never a grant, though `PATH` may contain one (`+w $out`).
- `PATH` is resolved by the shell, lexically, against its own table; a grant can name only what the shell holds.
- A grant applies to that one command (or pipeline element) and nothing after it. A function that needs grants writes
  them inside its body, where `help` shows them (ADR-0156).

**Showing it.** `grants` followed by a command line, its own grants included, prints the table that command would
receive and runs nothing; the toolchain set is shown as one line, `read (system)`. A background
job's line in `jobs` lists its grants. A native program that meets `NotGranted` says so in its error message, with the
grant that would allow it (ADR-0155).

**POSIX programs.** A program file whose image has no Zoraya description note (ADR-0156) is a POSIX program. `zosh`
runs it through the session's broker (ADR-0104): `zosh` holds a broker connection as a native parent (ADR-0002 rule
4), calls `spawn_prepare` and `spawn_commit` as `libposix` would, and builds the child's bootstrap record (ADR-0107)
itself. Its namespace table is the one above plus a fixed **toolchain set**, read-only: `/bin`, `/usr`, `/etc`, and
`/tmp` (the session's temporary service, writable), `/dev/null` and `/proc/self/exe` (ADR-0107). This set is the
measured working set of ADR-0002 driver 2 and Plash's default endowment
(`shell/survey.md#capability-shells-and-grant-syntax`). A description for a POSIX program may be shipped as a file in
`/zoraya/commands/NAME.cmd` (ADR-0156); then its operands are designated like a native program's (`tcc hello.c -o
hello` needs no grant).

**Examples.**

```
~/proj> cat notes.txt
buy milk
~/proj> cp notes.txt notes.bak
~/proj> grants cp notes.txt notes.bak
cp gets:
  in, out, err       the terminal
  ~/proj             read (current directory)
  ~/proj/notes.txt   read (operand FROM)
  ~/proj             create (operand TO: notes.bak)
~/proj> cat /config/settings
shell.suggestions = no
~/proj> tcc hello.c -o hello
~/proj> ./hello
hello, world
~/proj> tcc main.c -o main
main.c:1: error: include file '../common/util.h' not found
~/proj> +r ~/common tcc main.c -o main
~/proj> make
make: *** No targets specified and no makefile found.  Stop.
~/proj> set g '+w /'
~/proj> $g rm notes.txt
zosh: no command '+w /'
```

Naming `/config/settings` as `cat`'s operand is the grant; a redirection target is designated the same way, and the
command receives that one file (ADR-0152 rule 3). `tcc` reads `../common/util.h` because `main.c` names it, which no
description can know: that is a grant. The last two lines show that a grant cannot come from a value: `$g` is a
command name, not a grant. (`tcc`'s and `make`'s messages are theirs; Part 2 checks their exact text.)

### Effect of the vision

This is target 2's interface. An agent working through `zosh` reaches what each line designates; a launcher for a
sandbox gives the shell a narrower table and the same rules narrow every command below it. Target 3 (desktop): the
same rule is a file dialog's "designation is authorization", which the GUI can adopt later. Threads and networking: a
`+net HOST` grant is the natural extension once networking exists (CP-28); nothing here precludes it.

**Tensions with accepted ADRs.**
- *ADR-0104 item 12* has the broker spawn `/bin/sh` as the session's first process. Here `zosh` is the session's
  launcher and the broker is started beside it by `init` without a shell; `zosh` calls the spawn protocol for each
  POSIX command. Accepting this ADR adds that mode to ADR-0104.
- *ADR-0105 item 9* says a native shell and a POSIX session do not share a console, and `init` runs one or the other.
  Here they share one, because `zosh` owns the terminal and gives POSIX programs streams, not a tty connection: their
  `isatty` is false, so interactive POSIX programs (an interactive dash, a full-screen editor) are not supported from
  `zosh` until a later ADR lends them `ttyd`. `init` can still start a POSIX session alone for ADR-0110's tests.
- *ADR-0107*'s table is built "by the broker for the session's shell" and "by `libposix` for every child"; `zosh` is a
  third builder, of the same format.
- *ADR-0002 rule 5* lists the toolchain's working set as table entries; the toolchain set here is that list without
  `/` itself: a POSIX program sees the current directory and what is designated, not the whole volume.

## Consequences
- **Positive.** Least authority without ceremony for the common case; grants visible and short; one table format for
  native and POSIX programs; the same rule serves a later sandbox launcher.
- **Negative.** Every native command needs a correct description, and a wrong one is a security bug. POSIX programs
  that open paths from their input (`#include "../x.h"`, a makefile's `../lib`) need `+r` or `+w` grants, and their
  failures read as "No such file" (ENOENT), which `grants` and the manual must explain. Interactive POSIX programs do
  not run from `zosh` yet.
- **Follow-up.** The native start-up record in `zoraya-abi` and its reader in `zoraya-rt`; the resolver crate shared
  with ADR-0107; descriptions for every command; `/zoraya/commands/` for tcc, make and dash; `init`'s broker without a
  shell.

## Invariants
- **INV-0153-1.** A command started by `zosh` holds exactly: its three streams, its own job, its start-up record's
  connections, and nothing else; with no designated operand and no grant, its only directory connection is the
  current directory with `read` and `clone`. *Check:* an in-QEMU test of a program that enumerates its handle table
  and the rights of each connection (by `status` requests, ADR-0055) for several command lines.
- **INV-0153-2.** No connection a command receives has a right its kind (table above) or its grant does not give,
  and none reaches outside the named path. *Check:* a host test of the grant builder over every kind and grant, and an
  in-QEMU test in which a program tries every request outside and inside each entry.
- **INV-0153-3.** A grant comes only from a literal `+r` or `+w` token before the command name: no expansion,
  substitution, file content or program output produces one. *Check:* a host test of the parser and expander with
  values `+r`, `+w /`, and lines built from them, asserting the grant list is empty.
- **INV-0153-4.** `grants` prints exactly the table the command would receive. *Check:* a host test comparing
  `grants`'s output with the spawn request the runner builds for the same line.

## Open questions
- Whether `+w` should be split into `+w` (change files) and `+c` (create and remove), as `unveil` splits `w` and
  `c`; one level is simpler to read, two are closer to least authority.
- Whether the shell should offer a grant after a refused command (Deno's prompt), or only the hint; a prompt trains
  people to say yes.
- Which descriptions to ship for toybox applets, and whether toybox runs as POSIX programs with the toolchain set.

## Revisit triggers
- People routinely type `+w /` or `+w .` for ordinary commands: descriptions are missing or wrong, or the default is
  too narrow.
- A POSIX program in scope needs a tty from `zosh`: lend it `ttyd` (ADR-0105) by a superseding ADR.
- Networking: `+net`.

## Architect's decision
