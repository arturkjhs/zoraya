# ADR-0153: What a program gets from the shell, and how a person grants more

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0002, ADR-0003, ADR-0013, ADR-0033, ADR-0046, ADR-0052, ADR-0053, ADR-0055, ADR-0101, ADR-0104, ADR-0105, ADR-0107, ADR-0108, ADR-0150, ADR-0152, ADR-0156
- **Evidence:** CP-01, CP-05, CP-28, DM-009, DM-010, DM-014, DM-039, DM-041, DM-072, DM-087, GR-004, GR-008, X-1, X-12, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, shell/survey.md#capability-shells-and-grant-syntax, shell/survey.md#what-this-suggests-for-zosh

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
3. A grant is short and readable, and no data can create one: a grant token never comes from a value, a file or a
   program's output (DM-039, DM-072). Operands that come from values do designate, within limits (rule 7 below).
4. POSIX programs (tcc, make, dash scripts) run, through the broker and the namespace table, and what they see is
   just as explicit (ADR-0104, ADR-0107, X-1).
5. The person can always ask what a command would receive before running it, and the answer is the whole truth.

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
  (read, write, create, remove). The shell gives the command exactly the paths the person typed in those positions,
  with that access, plus its streams and the current directory read-only. Anything more is an explicit grant written
  before the command: `+r PATH`, `+w PATH`.
- **Evidence for:** The person's act of naming a file is the grant, so the common case needs nothing (driver 2) and
  the authority is still what the line shows (driver 1). Plash's shell put grants in the command line
  (`gcc -c foo.c => -o foo.o + .`) and SHILL makes a script's authority a declaration its users read
  (`shell/survey.md#capability-shells-and-grant-syntax`). The description is already needed for help and completion
  (ADR-0156), so it costs nothing extra to write. Plan 9 and Fuchsia already build each process's names from what its
  creator chooses (GR-004, `alternatives-postmortem.md#plan-9--inferno`,
  `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`); this option makes the creator's choice follow the line.
- **Evidence against:** A wrong description grants the wrong access; programs without a description (POSIX ones)
  need either a trusted description or explicit grants. A program that opens a path from inside a file it was given
  (an `#include`, a makefile's prerequisite) needs a directory grant: the Capsicum lesson (GR-008). Giving one name
  rather than a whole directory needs a narrowed connection that the file protocol does not have yet (rule 2).
- **Cost:** *(estimate)* Operand kinds in the description format, a resolver shared with ADR-0107's, the grant
  syntax, a narrowing request in the file service; around 1,500 lines with tests.
- **Precedent:** Plash, SHILL (research), powerbox-style file dialogs; nothing at mainstream scale.

### Option D: Per-program permissions granted once, in configuration (manifests)
- **Description:** Each program has a stored permission set ("make may write the current directory", "tcc may read
  `/usr/include`"), set once by the person or the installer and applied on every run.
- **Evidence for:** No per-command syntax; mobile and Flatpak-style systems work this way `[assumption: from general
  knowledge; no source was fetched]`.
- **Evidence against:** Authority is not visible on the line, and a stored grant applies to every future run with
  any operands (driver 1); the stored policy is configuration that drifts (DM-087's lesson applies). It answers
  "which program", not "which files".
- **Cost:** *(estimate)* A policy store and an editor for it.
- **Precedent:** Android permissions, Flatpak, macOS privacy prompts `[assumption: not fetched]`.

## Recommendation

### Decision

Accepted with a change (architect, 2026-11-03), and with additions the architect asked for after reading the tour:

- **Automatic designation by kind.** A `read` operand is granted automatically for every command that has a
  description. The kinds that change things (`write`, `create`, `remove`, `remove-tree`, `rename`, `tree`) are granted
  automatically only to **base-system commands**: the native commands of the system image and the POSIX programs its
  `/zoraya/commands/` describes (rule 10). For any other program those operands are not granted, and the person
  writes `+w`. Rule 2 below is amended accordingly.
- **`+w` stays one level**, not split into change and remove; the open question stays open, with a revisit trigger
  for the agent-isolation direction.
- **Output names.** A `create` operand (`-o FILE`, `TO`) means "create or replace this one name": the narrowed
  connection may create the name or write over the file already there, and nothing else.
- **Refused paths are reported.** Rule 11 below: the runtime (`zoraya-rt`, `libposix`) tells refused paths apart from
  missing files and reports them to `zosh`, which prints after the program ends which path it tried and which grant
  would allow it.
- **The changes to accepted ADRs** declared below are accepted: `narrow` in ADR-0055, the broker changes in ADR-0104,
  ADR-0105 item 9 (with the revisit condition "before the first interactive POSIX program"), path resolution in
  ADR-0107 and INV-0002-4 (recorded as INV-0153-8), and rule 11's report slot in ADR-0107's record.

The original recommendation follows.

Recommended: **Option C**. It is a recommendation, not a decision.

**1. Every command receives** (native or POSIX):
- `in`, `out`, `err`: the terminal through the shell, or the pipe ends and redirected files of ADR-0152.
- Its **own job**, inside the job of its command line, which is a sub-job of the shell's job, with `CREATE_PROCESS`,
  so it can start helpers that stay inside it (ADR-0003 rule 1, X-12). For a POSIX program the broker creates the
  process job inside the line's job (POSIX programs, below).
- The **current directory, read-only**: a `clone` of the shell's connection with `read` and `clone` only
  (ADR-0055), unless its description says `cwd none`, as commands that read no files do (`echo-args`, `sleep`).
- A **start-up record** (item 8) with its namespace table.
- Nothing else: no volume root, no console, no `Power`, no other job, no device.

**2. Designated operands.** A command's description (ADR-0156) gives each operand and option argument a kind; for
each path the person writes there, the shell adds a table entry. The entries for names that do not exist yet, or that
the command removes, are **narrowed connections**: a directory connection whose requests may name only one component
in it. ADR-0055 has no such connection; this ADR adds one request, `narrow(name, rights, server_end)`, under ADR-0052
(a new ordinal), which the service enforces like any connection's rights.

| Kind | Entry the shell adds | Rights (ADR-0055) | `grants` shows |
|---|---|---|---|
| `read` | the named file or directory | `read`, `clone` | `read` |
| `write` | the named existing file | `read`, `write`, `sync`, `clone` | `change` |
| `create` | the parent directory, narrowed to the name | `read`, `write`, `create`, `sync`, `clone` | `create or replace NAME` |
| `remove` | the parent directory, narrowed to the name | `read`, `remove`, `clone` | `remove NAME` |
| `remove-tree` | as `remove`, plus `tree` on the named directory | as `remove`, plus `tree`'s | `remove NAME and all in it` |
| `rename` | each parent, narrowed to its name | `read`, `rename`, `create`, `remove`, `clone` | `rename NAME` |
| `tree` | the named directory and everything below | every right but `publish` | `change anything in` |

*Amended by the Decision:* only `read` is automatic for every described command; the other kinds are automatic for
base-system commands alone. A `create` name may be created or, if it exists, replaced.

An option may change an operand's kind (`rm --recursive` makes its operand `remove-tree`, ADR-0156). A description
cannot name a fixed path and cannot widen the current directory: authority comes only from what the person typed. A
redirection (ADR-0152 rule 3) is a designation too: the shell opens the named file and the command receives that file
alone. A designated `read` or `write` path that does not exist when the line starts is entered as missing: the
program sees `NotFound` (`ENOENT`) for it, even if it appears later; a file that may not exist yet is `create`, not
`write`. Two entries with the same prefix are merged into one with the union of their rights.

Until the file service has `narrow` (it is outside the shell; Part 2 builds it only if the architect accepts this
change to ADR-0055), the shell gives the whole parent directory for `create`, `remove` and `rename`, and `grants`
words it by the rights given: `create and change anything in ~/project`, `remove anything in ~/project`, `rename,
create and remove anything in ~/project`. It shows exactly what the command receives.

**3. Explicit grants.** Before the command name, `+r PATH` grants `read` on a file or directory tree and `+w PATH`
grants the `tree` rights. Several may be given. Rules:
- A grant is recognised only as a literal `+r` or `+w` token before the command name, written in the source; the
  value of a variable or substitution is never a grant.
- The word after `+r` or `+w` must expand to exactly one argument; an empty list, several elements or a glob is an
  error before anything runs (`zosh: +w: needs exactly one path`).
- `PATH` is resolved by the shell, lexically, against its own table; a grant can name only what the shell holds.
- A grant applies to that one command (or pipeline element) and nothing after it. A function that needs grants writes
  them inside its body, where `help` shows them (ADR-0156).

**4. Paths from values.** An operand written literally designates wherever it points. A function's parameters and
`$args` keep the mark of the words the caller wrote: a path typed literally at the call stays literal inside the
function, so a function reaches what the bare command would. Any other operand that came from a variable or a
substitution designates only inside the current directory or inside an explicit grant on the same
line; anywhere else the line is refused before it runs (`zosh: ../x: from $files, outside the current directory`,
with a hint naming the grant). So `rm $(cat list.txt)` can remove only below the current directory unless the person
writes `+w` for more: data cannot reach past what the line shows.

**5. Link objects.** When a designated path is or passes through a link object (ADR-0013 rule 7), the shell resolves
it with ADR-0107 item 7's rules and designates the target only if the target lies inside the current directory or an
explicit grant on the line; otherwise the line is refused with the link and its target named. The program receives
the target, never authority over where the link points but the person did not look.

**6. Resolution.** The table records the current directory by its absolute canonical path. Every relative path a
program names is joined to that path, normalised lexically (`..` drops the previous component, at `/` it stays), and
then matched by longest prefix. So from `~/project`, `../include/greet.h` is `~/include/greet.h`, which a
`+r ~/include` entry serves; with no such entry it is outside every entry and refused. Nothing escapes an entry:
whatever a path normalises to, it is served only by an entry that covers it.

**7. Showing it.** `grants` followed by a command line, its own grants included, prints every entry the command would
receive, with its rights in words (table above), and runs nothing; the toolchain set of POSIX programs is shown on
four lines as in the example, and an entry merged from two reasons is shown on one line per reason. `jobs` lists each job's grants and designations. A native program that meets
`NotGranted` says so in its error message, with the grant that would allow it (ADR-0155).

**8. The start-up record.** Native programs read a record with ADR-0107's entry encoding (items 4 and 6) under a
native header: `zoraya_abi::native::Startup`, magic `ZNS1`, a size for versioning, the current directory's absolute
path, the entries, and the slots of `in`, `out`, `err` and the job. It has none of `Bootstrap`'s POSIX facts (pid,
uid, umask, ignored signals). It arrives on a channel at a fixed slot, read once by `zoraya-rt` before `main`, as
ADR-0107's record is read by `libposix`. Its decoder is total (INV-0153-5).

**9. Which programs are POSIX.** A native program carries an ELF note `.zoraya.native` that `zoraya-rt`'s linker
script writes; a program without it is a POSIX program. Whether it has a description (ADR-0156) is a separate
question: a native program with none (a test program) gets no designations, only item 1.

**10. POSIX programs.** `zosh` runs them through the session's broker (ADR-0104):
- `init` starts the broker beside `zosh`, without a first shell; the broker gives `zosh`, through `init`, a **native
  client connection** and a connection to the root of the temporary service it started (ADR-0107 item 12), in which
  `zosh` makes each line's `/tmp`. `zosh` has no pid.
- The line's job has no `posix` flag. For each POSIX element `zosh` creates a sub-job of the line's job with the
  `posix` flag (ADR-0046) and calls `spawn_prepare(Group(job))` with it: the broker creates the process job inside
  that sub-job instead of in its session job, so every element of the line is inside the line's job and inside
  `zosh`'s tree, and native elements stay out of `posix` jobs (ADR-0002 rule 3, INV-0002-2). `zosh` calls
  `spawn_commit`, waits on the process job, and calls `reap` for each pid, as the parent.
- `zosh` builds the child's bootstrap record (ADR-0107) itself: its entries are those above plus a fixed **toolchain
  set**: `/bin`, `/usr`, `/etc` and `/proc/self/exe` read-only, `/dev/null`, and a `/tmp` of its own (a directory
  `zosh` makes on the session's temporary service for this line and removes when the line's job ends, so two command
  lines share no temporary files, while the processes of one line, such as a `make` tree, do). This set is the
  measured working set of ADR-0002 driver 2 and Plash's default endowment
  (`shell/survey.md#capability-shells-and-grant-syntax`).
- A POSIX program's description is trusted only for the programs the system image lists: `/zoraya/commands/` in the
  ramdisk (read-only, part of the image, ADR-0033), where each description names the full path it applies to
  (`program /bin/tcc`). It applies only when the command resolved to that path through `shell.path`; `./tcc`, a copy
  elsewhere, or a file the person built gets none. Nothing on the volume can add or change a trusted description.

**11. Refused paths** (added by the architect's decision). The start-up record (rule 8) and ADR-0107's bootstrap
record carry the write end of a **report channel** at a fixed slot, `REPORT`. When a program names a path that no
entry covers, its runtime (`zoraya-rt` natively, `libposix` for POSIX programs) still fails the call as before
(`NotGranted`, `ENOENT`), and also writes a report `{ path, access: read | write }` with the absolute, normalised path
on that channel, once per distinct path and at most 16 per process, never waiting: a report that does not fit is
dropped and counted. A path inside an entry that does not exist is a missing file, and is never reported. After the
job ends, `zosh` reads the reports and prints, below the program's own output, for each directory involved (at most
three, then `and N more`):

```
  hint: tcc tried to read ~/include/greet.h, which it was not given
  to allow it: +r ~/include tcc main.c -o main
```

The grant offered is `+r` or `+w` on the directory that holds the path, and the line is the person's line with that
grant in front. A native program's own message says only `not granted` (ADR-0155 rule 5); the hint is the shell's,
the same for both kinds of program.

**Examples.**

```
~/proj> cat notes.txt
buy milk
~/proj> cp notes.txt notes.bak
~/proj> grants cp notes.txt notes.bak
cp gets:
  in, out, err         the terminal
  ~/proj               read (current directory)
  ~/proj/notes.txt     read (operand FROM)
  ~/proj               create notes.bak (operand TO)
~/proj> cat /config/settings
shell.suggestions = no
~/proj> tcc hello.c -o hello
~/proj> ./hello
hello, world
~/proj> tcc main.c -o main
main.c:1: error: include file '../common/util.h' not found
  hint: tcc tried to read ~/common/util.h, which it was not given
  to allow it: +r ~/common tcc main.c -o main
~/proj> +r ~/common tcc main.c -o main
~/proj> set old $(cat ~/old-files.txt)
~/proj> rm $old
zosh: /config/settings: from $old, outside the current directory
  hint: if that is meant: +w /config rm $old
~/proj> set g '+w /'
~/proj> $g rm notes.txt
zosh: +w /: no such command
```

Naming `/config/settings` as `cat`'s operand is the grant; a redirection target is designated the same way. `tcc`
reads `../common/util.h` because `main.c` names it, which no description can know: that is a grant. A list read from
a file can only name files below the current directory unless the line grants more. A grant cannot come from a value:
`$g` is a command name, not a grant. (`tcc`'s messages are its own; Part 2 checks their exact text.)

### Effect of the vision

This is target 2's interface. An agent working through `zosh` reaches what each line designates; a launcher for a
sandbox gives the shell a narrower table and the same rules narrow every command below it. Rule 4 is what makes it
hold for agents, whose operands often come from data. Target 3 (desktop): the same rule is a file dialog's
"designation is authorization", which the GUI can adopt later. Threads and networking: a `+net HOST` grant is the
natural extension once networking exists (CP-28); nothing here precludes it.

**Tensions with accepted ADRs.**
- *ADR-0055* has no connection narrowed to one name. Rule 2 adds `narrow` under ADR-0052; until it exists the shell
  grants and shows the whole parent directory.
- *ADR-0104 item 12* has the broker spawn `/bin/sh` as the session's first process. Here `init` starts the broker
  without a shell and gives `zosh` a native client connection; `zosh` prepares, commits and reaps as a parent with no
  pid (rule 10).
- *ADR-0104 items 1 and 4* put every process job inside the broker's session job, in a group job the broker makes.
  Rule 10 adds a fourth form of `spawn_prepare`'s group, `Group(job)`, a job handle from the caller's tree; the broker
  still holds the process job and the pid. A `kill(-1)` from a POSIX program in the session then reaches only the
  session job's processes, not `zosh`'s lines: stated, and acceptable since `zosh`'s lines are not that program's
  session.
- *ADR-0105 item 9* says a native shell and a POSIX session do not share a console, and `init` runs one or the other.
  Here they share one, because `zosh` owns the terminal and gives POSIX programs streams, not a tty connection: their
  `isatty` is false, so interactive POSIX programs (an interactive dash, a full-screen editor) are not supported from
  `zosh` until a later ADR lends them `ttyd`. `init` can still start a POSIX session alone for ADR-0110's tests.
- *ADR-0107* builds the table "by the broker for the session's shell" and "by `libposix` for every child"; `zosh` is
  a third builder. Its item 6 resolves a relative path "against cwd's entry and path"; rule 6 here records cwd as an
  absolute path so that `..` can leave the cwd entry into another entry. This changes INV-0002-4's wording for tables
  `zosh` builds: "`..` at an entry's root resolves to the entry's root" becomes "`..` normalises against the absolute
  path, at `/` it stays, and a path is served only by an entry that covers it". Its purpose, that resolution never
  escapes an entry, holds; its check, over the shared resolver, must test both forms. Accepting this ADR records the
  new wording beside INV-0002-4.
- *ADR-0107 item 5* lists the fixed slots of the bootstrap record; rule 11 adds `REPORT`, a channel's write end, to
  them and to the list INV-0002-5's check enumerates (as ADR-0107 already extended it).
- *ADR-0002 rule 5* lists the toolchain's working set as table entries; the toolchain set here is that list without
  `/` itself, and with a `/tmp` per command line instead of per session (ADR-0002 rule 7 needs `/tmp` shared within
  one `make` tree, which one line is).

## Consequences
- **Positive.** Least authority without ceremony for the common case; grants visible, short and never made from data;
  one table format for native and POSIX programs; the same rule serves a later sandbox launcher.
- **Negative.** Every native command needs a correct description, and a wrong one is a security bug. The current
  directory is readable by every command that does not say `cwd none`: in `~` that is all of home, the largest thing
  most commands get, shown only by `grants`. POSIX programs that open paths from their input (`#include "../x.h"`, a
  makefile's `../lib`) need `+r` or `+w` grants, and their failures read as "No such file" (`ENOENT`), which
  `grants` and the manual must explain. `+w` gives remove and rename too. Interactive POSIX programs do not run from
  `zosh` yet. Until `narrow` exists, `create`, `remove` and `rename` reach the whole parent directory.
- **Follow-up.** `zoraya_abi::native::Startup`, its reader in `zoraya-rt`, the `.zoraya.native` note; the resolver
  crate shared with ADR-0107; `narrow` in the file protocol and service, if accepted; descriptions for every command;
  `/zoraya/commands/` for tcc, make and dash; `init`'s broker without a shell; `Group(job)` in the broker.

## Invariants
- **INV-0153-1.** A command started by `zosh` holds exactly: its three streams, its own job, its start-up record's
  connections, and nothing else; with no designated operand and no grant, its only directory connection is the
  current directory, and no request but reading and listing succeeds through it. *Check:* an in-QEMU test of a
  program that enumerates its handle table and tries every request of ADR-0055 through each connection, for several
  command lines.
- **INV-0153-2.** No connection a command receives allows a request its kind (table above) or its grant does not
  allow, and none reaches outside the named path or name. *Check:* a host test of the grant builder over every kind
  and grant, and an in-QEMU test in which a program tries every request outside and inside each entry, including
  through link objects.
- **INV-0153-3.** No data makes a grant or widens a designation: a grant comes only from a literal `+r` or `+w` token
  followed by a word of exactly one argument, and an operand from a value designates only inside the current directory
  or a grant on the line. *Check:* a host test of the parser, expander and grant builder with values `+r`, `+w /`,
  empty and two-element lists after `+w`, operands `../x`, `/x` and link objects from variables and substitutions,
  asserting the grant list or the refusal.
- **INV-0153-4.** `grants` prints exactly the entries and rights the command would receive. *Check:* a host test
  comparing `grants`'s output with the spawn request the runner builds for the same line.
- **INV-0153-5.** `Startup::decode` is total, and a record that encodes then decodes is equal. *Check:*
  `cargo test -p zoraya-abi` with truncations and random mutations, as for `Bootstrap` (INV-0107-1).
- **INV-0153-6.** A description outside the system image never designates: only descriptions in `/zoraya/commands/`
  of the ramdisk, for the program path they name, and descriptions inside native programs are read. *Check:* a host
  test of the description lookup with a description file on the volume and a program run by `./NAME`.
- **INV-0153-7.** A path refused because no entry covers it is reported to the shell once per process and path, a
  path that is missing inside an entry is never reported, and the shell's hint names each reported path with the
  grant that covers it. *Check:* a host test of the resolver and reporter, and an in-QEMU test with a C program
  through `libposix` and a native program, each opening one path outside their entries twice and one missing path
  inside, against the printed hint.
- **INV-0153-8.** In a table `zosh` builds, a relative path is joined to the current directory's absolute path and
  normalised, `..` at `/` stays at `/`, and a path is served only by an entry that covers it. *Check:* the shared
  resolver's host test of INV-0002-4, run in both forms (entry-relative for the broker's and `libposix`'s tables,
  absolute for `zosh`'s), with adversarial paths.
- **INV-0153-9.** Operand kinds other than `read` are granted automatically only from descriptions of base-system
  commands; any other description gives at most `read`. *Check:* a host test of the grant builder with each kind
  from a system-image description and from a native program's own description outside the image.

## Open questions
- Whether `+w` should be split into `+w` (change and create) and `+d` (remove and rename), as `unveil` splits `w` and
  `c`; one level is simpler to read, two are closer to least authority. The architect kept one level for now.
- Whether the shell should offer a grant after a refused command (Deno's prompt), or only the hint; a prompt trains
  people to say yes.
- Which descriptions to ship for toybox applets, and whether toybox runs as POSIX programs with the toolchain set.

## Revisit triggers
- People routinely type `+w /` or `+w .` for ordinary commands: descriptions are missing or wrong, or the default is
  too narrow.
- Before the first interactive POSIX program is wanted from `zosh`: lend it `ttyd` (ADR-0105) by a superseding ADR
  (the architect's condition on ADR-0105 item 9).
- The agent-isolation direction is chosen (Vision, target 2): reconsider splitting `+w`, and whether base-system
  commands should keep automatic write designation.
- The architect declines `narrow`: rule 2's fallback becomes the rule, and `create`, `remove` and `rename` are
  recorded as reaching the parent directory.
- Networking: `+net`.

## Architect's decision

Accepted with a change, 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
Designation is automatic only for reading; for writing (every kind but `read`) it is automatic only for base-system
commands, and every other program needs an explicit `+w`. `+w` is not split into change and remove for now; a revisit
trigger names the agent-isolation direction. From the tour: a program's access to a path outside what it was given
is told apart from a missing file by `libposix` (and `zoraya-rt`) and reported to `zosh`, which prints after the
program ends which path it tried and which `+r`/`+w` allows it (rule 11, INV-0153-7); an output operand means "create
or replace this one name". The changes to accepted ADRs declared here are accepted: `narrow` in ADR-0055, the broker
changes in ADR-0104, ADR-0105 item 9 with the revisit condition "before the first interactive POSIX program", path
resolution in ADR-0107 and INV-0002-4, and the report slot in ADR-0107's record.
