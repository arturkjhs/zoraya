# ADR-0341: A program's own files: a directory per program and room, given by its description and shown where the program looks

- **Status:** accepted
- **Date:** 2026-11-13
- **Depends on:** ADR-0107, ADR-0153, ADR-0156, ADR-0167, ADR-0172, ADR-0245, ADR-0265, ADR-0310
- **Evidence:** CP-01, CP-21, CP-23, DM-010, DM-085, DM-086, DM-087, alternatives-postmortem.md#genode, network/survey.md#plan-9s-tls-filters-and-key-agent, stage-10/survey.md#files-an-editor-or-pager-touches-unasked, stage-10/survey.md#files-git-touches-unasked, stage-10/survey.md#files-the-ssh-client-touches-unasked, stage-10/survey.md#pattern-a-grants-fixed-at-start, stage-10/survey.md#what-a-posix-program-is-given-today, stage-10/survey.md#what-the-ssh-client-needs

## Context

Unix programs keep things of their own in the home directory, where the command line never names them: git its
`~/.gitconfig` and credentials, ssh its keys, its `known_hosts` and its configuration, `less` a history, `vi` a
start-up file (`stage-10/survey.md#files-git-touches-unasked`,
`stage-10/survey.md#files-the-ssh-client-touches-unasked`,
`stage-10/survey.md#files-an-editor-or-pager-touches-unasked`). A Zoraya program gets what its line names
(ADR-0153), so none of these files is reachable, and each attempt is a refused path that ADR-0337 would show on the
status line. The ports so far avoided the question by reading no configuration at all (ADR-0310 item 6: curl and
wget). git without an identity and ssh without a key and without a record of hosts are not usable. This ADR
decides where a program's own files live and how it is given them.

**What is known.**

- A graphical application already gets a directory of its own: the panel makes `/data/ROOM/APP` and gives it as a
  `tree` entry with every launch (ADR-0167 point 2, ADR-0245). Shell-launched programs get none
  (`stage-10/survey.md#what-a-posix-program-is-given-today`).
- ADR-0153 rule 2 says "a description cannot name a fixed path and cannot widen the current directory: authority
  comes only from what the person typed". Two bounded exceptions were accepted since: `needs net-status` and the
  roots directory that comes with a network grant (ADR-0224, ADR-0311).
- The ssh client ignores a private key that anyone but its owner can read, and refuses a configuration file
  others can write: checks on Unix modes and owners, which mean nothing on a system with one identity
  (`stage-10/survey.md#what-the-ssh-client-needs`; DM-010).
- A launcher can show a directory to a program under another name: Wasmtime's `--dir HOST::GUEST`
  (`stage-10/survey.md#pattern-a-grants-fixed-at-start`); ADR-0107's table is prefixes mapped to connections and
  allows the same.
- The first terminal's shell holds the whole volume to read (ADR-0172's decision of 2026-11-09), and every
  command gets the current directory to read (ADR-0153 rule 1); ADR-0265 limits a room's storage on its `+w`
  directories and its `/tmp`; OpenSSH finds its directory through the user database's home, git through `HOME`
  (read in the code and the sources by the review).
- Configuration that every program finds by a fixed name, and that libraries read implicitly, is what DM-085 and
  DM-087 record; CP-21 and CP-23 ask that configuration be owned and attributable.

**What is assumed.** That one directory per program is enough, and that programs which must share (the ssh family)
are few and known when the image is built.

## Decision drivers

1. **A program's secrets are its own**: no other program is given ssh's keys because it happens to run as the
   same person; reaching them takes a grant that names them (CP-01, DM-010).
2. **Unmodified programs find their files** where they look (`stage-10/survey.md#files-the-ssh-client-touches-unasked`).
3. **Owned and attributable**: one place, visible, per program, and per room so that an untrusted room's git has
   nothing of the person's (CP-23, ADR-0172).
4. **Nothing the volume or a program can redirect**: which directory a program gets is decided by the system
   image (INV-0153-6; DM-085).
5. **The same as the desktop's rule**, so that there is one notion of "an application's data" (ADR-0167 driver 4).
6. **No noise**: a program that runs normally must not fill the status line with refusals for dotfiles
   (ADR-0337 driver 2).

## Considered options

### Option A: Dotfiles in the home directory, readable by every program (Unix)

- **Description:** Every POSIX program gets the home directory, or at least its dotfiles.
- **Evidence for:** Everything works as documented; the person's habits and backups carry over.
- **Evidence against:** Any program reads `~/.ssh/id_ed25519` and `~/.git-credentials` (drivers 1 and 3); it is
  ambient authority over the most sensitive files there are (CP-01). The mode checks ssh relies on are the
  substitute Unix needed and are empty here (DM-010).
- **Cost:** *(estimate)* None; everything later.
- **Precedent:** Unix.

### Option B: The person grants them on the line or in the room

- **Description:** `+r ~/.gitconfig git commit`, or a room that lists the files. No new rule.
- **Evidence for:** ADR-0153 as written; every grant visible on the line (driver 4).
- **Evidence against:** The grant repeats on every line (ADR-0153 driver 2's noise), and a room that holds
  `~/.ssh` gives the keys to every program in the room (driver 1). A POSIX program is given directories only, so
  one file means its directory (ADR-0153 rule 10's note).
- **Cost:** *(estimate)* None.
- **Precedent:** `unveil` and Deno flags; Plash's `+` (`stage-10/survey.md#pattern-a-grants-fixed-at-start`).

### Option C: A directory of its own per program and room, from the description

- **Description:** An image description may say that its program has an own directory. The launcher gives it
  `/data/ROOM/NAME`, as the panel gives an application its data directory, and enters it in the program's table at
  the path the program expects.
- **Evidence for:** Drivers 1 to 6. It is the desktop's accepted rule extended to the shell (ADR-0245). The
  authority is over a directory made for the program, not over anything the person had. Keys are private by
  construction, which is what ssh's mode checks try to assert.
- **Evidence against:** It is a third thing a line receives that no word on it names (ADR-0153 rule 2). The
  program sees its files under one path and the person under another. A program started by another program does
  not get its own directory. Files move away from where Unix habit expects them.
- **Cost:** *(estimate)* A description key, 150 lines in the shell's planner, one more entry in the shell's own
  table, a lint.
- **Precedent:** ADR-0245's data directory; per-application storage on the phone systems `[assumption: from
  general knowledge, not fetched]`.

### Option D: No files: a settings service and a key agent hold them (Plan 9's factotum; Genode's routed configuration)

- **Description:** Configuration is entries of `/config/settings` (ADR-0157) that the shell passes to the program;
  secrets live in an agent process that signs or answers on request and never reveals them.
- **Evidence for:** Driver 1 at its strongest: the program never holds a key. Plan 9's factotum does this for every
  protocol, and ADR-0228 names it as the path for client keys "also SSH keys later"
  (`network/survey.md#plan-9s-tls-filters-and-key-agent`). Genode gives a component its configuration from its
  parent (`alternatives-postmortem.md#genode`). A system-wide typed store is the Windows registry's idea (DM-086,
  a tradeoff).
- **Evidence against:** Driver 2: git and ssh read files; each would be patched to ask a service, and ssh's agent
  protocol needs sockets `libposix` does not have. `known_hosts` and git's configuration are not secrets and are
  written by the programs themselves.
- **Cost:** *(estimate)* An agent, a protocol, and patches in every port.
- **Precedent:** Plan 9; ssh's own agent.

## Recommendation

**Recommendation (not a decision): Option C, with Option D's agent recorded as the later path for keys.**

1. **The key.** A description in the system image may carry one line `data NAME as PATH`, or
   `data NAME as PATH read`: for example `data git as ~/.config/git` and `data ssh as ~/.ssh`. `NAME` is a plain
   word; `PATH` begins with `~/`; `read` gives the directory to read only. A description outside the image cannot
   carry the line (INV-0153-6's rule).
2. **The directory** is `/data/ROOM/NAME`, `/data/home/NAME` outside a room: the place and the form ADR-0245
   uses for an application. The shell makes it when a program with that line first runs. For that the shell's own
   table gains `/data/ROOM` (`/data/home` for the first terminal), given by whoever starts the shell. It is the
   shell's own entry: it is not a grant of the room, is not written in `/config/rooms`, is not added to the room's
   lines and is not shown as a pill.
3. **What the program gets.** One more table entry: the prefix is `PATH` with `~` as the home directory of the
   image's user database (`/home`), the connection is that directory with `tree` rights (or `read`). For such a
   program the shell sets `HOME` to that same directory whatever `shell.home` is, and refuses a line that sets
   `HOME` itself: OpenSSH finds `~/.ssh` through the user database, git through `HOME`, and both must land on the
   entry. The program opens `~/.ssh/known_hosts` and reaches `/data/home/ssh/known_hosts`. Nothing else under the
   home directory is given. The own-directory entry is never merged: a `+r` or `+w` of the line on the same
   prefix is refused.
4. **Shown.** `grants` prints it in both names:

   ```
   /data/home/ssh       its own files (it sees them as ~/.ssh)
   ```

   and `help NAME` lists the files under FILES with both.
5. **Sharing is by name and decided in the image.** Programs whose descriptions carry the same `NAME` share the
   directory (`ssh`, `ssh-keygen`, `scp` and `sftp` share `ssh`). One file in the tree lists every `NAME` with the
   programs that use it, and a lint keeps the descriptions and the panel's application names to it. A program
   that only passes the directory on to a child (`scp` and `sftp`, for the `ssh` they start) carries it `read`.
6. **Per room.** A room's programs get the room's directory, created empty. A limited room's git has no
   identity and no credentials of the person's. Its directory must count against the room's storage limit:
   ADR-0265 item 5 puts the limit on the room's `+w` directories and its `/tmp`, so `/data/ROOM` of a limited
   room joins them, a stated change to that item.
7. **Children get it to read, never to write.** A program a POSIX program starts inherits its parent's table
   (ADR-0107); the own-directory entry carries a mark, and for a marked entry the parent's `libposix` asks the
   file service for a second connection with `read` rights only and hands the child that one. The child never
   holds a handle that can write, whatever its own runtime does. A link object that resolves into a program's
   own directory is not followed from outside that directory, so a repository cannot make git's own write land
   there through a planted link. So a
   hook, an editor or a helper started by git can read git's configuration and cannot change it, and the `ssh`
   that `scp` starts can read the keys and cannot rewrite `known_hosts`. A child gets no own directory of its own
   name: `ssh` started by git does not see ssh's (ADR-0339 item 6).
8. **The current directory does not reach into `/data`.** A read connection on a directory reaches everything
   below it, and nothing can cut a subtree out of one. So when the current directory is `/`, `/data`, a
   `/data/ROOM` or inside a program's own directory, a program gets **no** current-directory entry at all, as if
   its description said `cwd none` (ADR-0311 item 5: it starts where that item says, and relative operands still
   resolve). A path under `/data` reaches another program only when the person names it: as a literal operand
   (`cat /data/home/git/config`, `ls /data/home/ssh`), which designates as everywhere, or by a `+r` or `+w` word.
9. **A port looks nowhere else.** A program with a `data` line is built or configured so that it names no path in
   the home directory outside its own directory in normal operation, as curl and wget read no configuration
   (ADR-0310 item 6). Otherwise every run would report refused dotfiles (driver 6).
10. **The person reaches the files as files.** They are ordinary: `ls /data/home/ssh`, `edit /data/home/git/config`,
   a backup of `/data/home`. A program reads them only if a line names them.
11. **Modes mean nothing here.** Privacy is the entry, not the mode bits. `libposix` reports the mode a file was
    created with, so a port's own checks on its key files pass when the port created them properly.
12. **Later.** Keys in an agent that signs and never reveals (Option D) is the path ADR-0228 already names; this
    directory is then where the agent's program keeps them.

**Examples.**

```
~> git config --global user.name 'Ada'
~> ls /data/home/git
config
~> grants git status
git gets (a POSIX program):
  in, out, err         the terminal
  keys                 the terminal, while this line is in front
  ~                    read (current directory)
  /data/home/git       its own files (it sees them as ~/.config/git)
  /bin /usr /etc       read (system)
  /proc/self/exe       read (system)
  /tmp                 change anything in (this line's own)
  /dev/null
~> room proj git config --global user.name
~> cd /data/home/ssh
/data/home/ssh> sh -c 'cat id_ed25519'
```

The `room` line prints nothing: in the room `proj`, git's directory is `/data/proj/git`, and it is empty. The last
line is refused, though the current directory is ssh's own: `sh` was given no name there, and the current directory
gives none (item 8); `cat id_ed25519`, with the name typed as an operand, would print the key. On the desktop the file
manager shows
`/data/home/git/config` as any
file; opening it starts `edit`.

**Tensions with accepted ADRs.**

- *ADR-0153 rule 2* ("a description cannot name a fixed path ... authority comes only from what the person
  typed"). The `data` line names a path in the program's own view and gives a directory the person did not type.
  It is bounded as the earlier exceptions are: only image descriptions, only a directory created for that name,
  never an existing path of the person's, always shown by `grants`. Accepting this ADR accepts it as the third
  exception, after `needs net-status` and the roots directory (ADR-0224's tensions).
- *ADR-0153 rule 1* ("nothing else") and *INV-0153-1*: the enumeration gains the own directory.
- *ADR-0156*: one more key in the description format.
- *ADR-0172's decision of 2026-11-09* lists what the first terminal's shell holds (`+r /`, `+w ~`, `+w /config`,
  `+w /tmp`) and that a room's shell holds the room's grants: both gain `/data/ROOM`, with `tree` rights, so that
  the shell can make and give the directories. It is not a room grant and no pill shows it (item 2), which
  INV-0172-2's "exactly the room's entries" must be read with.
- *ADR-0153 rule 1*: the current directory is given to read wherever it is; item 8 gives none when it is `/`,
  `/data` or below. *ADR-0153 rule 10's note*: a POSIX program gets `HOME` "unless the line sets it";
  for a program with a `data` line the line may not (item 3).
- *ADR-0107*: `libposix` builds a child's table from its own; item 7 makes it lower one marked entry to `read`.
- *ADR-0265 item 5*: the storage limit of a limited room also covers its `/data/ROOM` (item 6).
- *ADR-0245*: the panel's and the shell's directories for one name and room are the same directory. A graphical
  application and a terminal program of the same name would share it; no such pair exists.
- *ADR-0107*: an entry whose prefix differs from the path its connection came from. The table allows it; `getcwd`
  and the refused-path reports use the program's view, and the shell translates for the person.

### Effect of the vision

For target 2 this is the difference between "an agent's git" and "my git": a room's programs have their own empty
state, bounded by the room's storage, and no line of that room can name the person's keys, which are outside
what the room's shell holds. A supervisor reads an
agent's `/data/agent-1` as plain files. Target 3: one notion of an application's data for the desktop and the
shell. Networking: where ssh's and git's trust state lives (ADR-0339, ADR-0340). Threads: none.

## Consequences

- **Positive.** git and ssh can keep configuration, known hosts and keys; no other program is given them unless a
  line names them; children cannot change them; rooms separate them; unmodified programs find them; the person sees
  them as files and `grants` shows them.
- **Negative.** A third exception to "only what the person typed". Two names for one directory. Unix habits
  (`~/.ssh`, `~/.gitconfig` in the home directory) do not hold: the files are under `/data`. A program started
  by another does not get its own directory.
- **Follow-up.** The key in `zoraya-describe`; the shell's planner and `grants`; `/data/ROOM` in the shell's table
  (the panel and `init`); the name list and its lint; `help`'s FILES section.

## Invariants

- **INV-0341-1.** A program receives an own-directory entry only from a `data` line of an image description for
  the path it was resolved to; a description on the volume, or a program run by another path, gives none.
  *Check:* INV-0153-6's host test extended with a `data` line in a volume description and a copy of the program
  under another name.
- **INV-0341-2.** The entry's connection is `/data/ROOM/NAME` for the room the line runs in and the `NAME` of the
  description, and no other room's or name's directory is reachable through it. *Check:* a host test of the
  shell's planner over rooms and names, and an in-QEMU script that writes a file as one program in two rooms and
  reads each back.
- **INV-0341-3.** `grants` prints the entry with the real path and the path the program sees. *Check:*
  INV-0153-4's comparison test, extended.
- **INV-0341-4.** Every `NAME` in a `data` line is in the list of names, with exactly the programs that carry
  it, and no application name of the panel equals one. *Check:* a lint in `make check` over `user/describe/`, the
  panel's application table and the list.
- **INV-0341-5.** A program with a `data` line reports no refused path at all when started and ended with no
  operand, and when it first writes its own files (`git config --global`, `ssh-keygen`), with `shell.home` set to
  another directory too. *Check:* an in-QEMU script per such program that reads the report count (ADR-0153
  rule 11).
- **INV-0341-6.** A child of a program with an own directory holds a connection to it that the file service
  serves for reading only, and no link object leads a write from outside into it. *Check:* a host test of
  `libposix`'s table for a child, and an in-QEMU script in which a git hook writes through the handle it was
  given, by the library and by a raw request, and is refused both ways, and a planted link is not followed.
- **INV-0341-7.** No program receives a path inside another name's own directory from the current-directory
  default; a literal operand or a literal `+r` or `+w` word is the only way. *Check:* a host test of the shell's
  planner with the current directory at `/`, at `/data`, at `/data/home` and inside `/data/home/ssh`, with and
  without an operand, asserting that no current-directory entry is given.

## Open questions

- Whether `vi` and `less` should have own directories (a start-up file, recovery files, a history); ADR-0335
  ships them with none.
- Whether a program started by another should get its own directory when its description has one; it would need
  the shell's planner on the path of every `exec`, which the broker does not have.
- A command that shows or edits a program's own files by name (`settings`-like), so that `/data` need not be
  typed.
- Whether a graphical and a terminal program of one name should share a directory.

## Revisit triggers

- A second program wants to read another's own files routinely (an editor for git's configuration): a grant word
  for it, or a shared name.
- Keys move to an agent (ADR-0228's Option D).
- People cannot find their files under `/data`.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

As recommended.
