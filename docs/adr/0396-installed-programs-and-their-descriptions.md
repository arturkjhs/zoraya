# ADR-0396: Programs installed on the volume: the package, who installs it, and the description it carries

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0107, ADR-0108, ADR-0153, ADR-0155, ADR-0156, ADR-0157, ADR-0212, ADR-0336, ADR-0337, ADR-0341
- **Evidence:** CP-01, CP-21, CP-23, DM-010, DM-087, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#genode, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks

## Context

Every program Zoraya runs today is in the image, or is a file the person built or copied and starts by its path
(`./a.out`, ADR-0108). A program in the image has a trusted description in `/zoraya/commands/` or in its ELF; the
shell reads it for help, completion and designation, and "nothing on the volume is read as a description" (ADR-0156
item 2). A description outside the image never designates (INV-0153-6), and a description on the volume gives no
own directory (INV-0341-1). So a program that is not in the image has no help page, no completion, no own
directory, and every grant it needs is typed on every line. That is the right default for a file a person
just compiled. It is not usable for the programs the architect expects to keep on the volume, the browser engine
among them (backlog request 3; ADR-0411 item 3 gives the engine's UI process a description with an own directory
and a `web` grant, which an image program has and a volume program cannot).

**What is known.** A description is a security statement: it names operand kinds (what the shell designates from
the line), a `data` directory, `needs` words and settings (ADR-0153 rule 2, ADR-0341 item 1, ADR-0157). Whoever
writes it chooses part of what the program receives, so a program that writes its own description widens itself.
ADR-0336 item 5 makes every image command carry a description, checked by a lint at build time; a volume cannot be
linted at build time. Fuchsia resolves a component's declared needs through a manifest the package carries and an
installer the user trusts, and gives the component only what its parent routes to it
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`); Genode lets the parent decide what each child's
configuration may ask for (`alternatives-postmortem.md#genode`). A trusted intermediary that asks the person is the
pattern the survey records for grants (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).

**What is assumed.** That an installed program is installed rarely and by a person at a shell, not by another
program; that the volume's writer set is "the person and what the person gave `+w`", so a file on the volume can
be changed by any program the person gave a writable directory above it; that SHA-256 of a 1.4 MiB program is
cheap against the 25 ms the whole read costs (`docs/backlog.md`, images), not measured here.

## Decision drivers

1. **An installed program cannot write or widen its own description or its own authority** (CP-01, DM-010: what a
   program is must not be what a user's writable files say it is).
2. **The person decides what is installed, with their own typed words**, as ADR-0153 says authority comes only
   from what the person typed.
3. **A person finds an installed program like any other**: `help NAME`, completion, a name on the path.
4. **The change to accepted ADRs is stated and small**: INV-0153-6 and INV-0341-1 stay true as written wherever
   they can (CP-21, CP-23: one owned place for configuration).
5. **Nothing silent after an upgrade**: a different program is a different thing to trust.

## Considered options

### Option A: No installation: a volume program is a file, grants on every line (today; the Unix baseline for a program in `~/bin`)

- **Description:** Nothing changes. A program on the volume runs by its path and gets what the line names.
- **Evidence for:** No new rule; INV-0153-6 and INV-0341-1 hold; nothing to forge.
- **Evidence against:** No help, no completion, no own directory, no saved grant: the browser engine and git-like
  programs outside the image are unusable (driver 3). DM-087: configuration then lives in the person's
  scripts and aliases, per shell, unowned.
- **Cost:** *(estimate)* None now; every user pays on every line.
- **Precedent:** Unix `~/bin` and `$PATH`.

### Option B: The package carries its description and its request, and the program's own directory trusts them (Flatpak and Android style)

- **Description:** A package is a directory with the program, a `.cmd` description and a request; the shell reads the
  description from the volume as it reads an image's and honours `data` and `needs` lines.
- **Evidence for:** One mechanism with the image's; no record to keep.
- **Evidence against:** A program that can write its own package (a writable directory above it, an upgrade
  that replaces files) rewrites its own authority (driver 1). INV-0153-6 and INV-0341-1 are reversed, not
  extended. Nobody confirmed anything.
- **Cost:** *(estimate)* Small in the shell; a security review of every writer of the volume.
- **Precedent:** Manifests of mobile systems `[assumption: from general knowledge, not fetched]`.

### Option C: A package installed by a builtin the person types; the shell keeps a record that binds the description to the program's hash (Fuchsia's package resolver and Genode's parent as the trusted intermediary)

- **Description:** `install` is a shell builtin. It copies a package the person names into `/apps/NAME`
  (not writable by any program unless the person typed `+w` over it), computes the SHA-256 of what it copied, and
  writes one line to `/config/apps` that binds `NAME` to that hash. The description is read from `/apps/NAME`
  only for help, completion and usage, with a smaller set of keys than an image description's, and only while the
  program's hash is the recorded one. It designates nothing and carries no authority; what the program may receive
  is in the record (ADR-0397), put there by words the person typed on the `install` line.
- **Evidence for:** Drivers 1 to 5. The person's typed line is the consent (driver 2). INV-0153-6 and INV-0341-1
  stay true: the volume's description still never designates and gives no `data` line; the own directory of an
  installed program comes from a person-typed record (ADR-0397), not from a description.
- **Evidence against:** A third place a program's authority is kept (the line, `/config/rooms`, and now
  `/config/apps`); `/config/apps` is a file the shell writes and the person could edit by hand. A package from
  elsewhere needs a format.
- **Cost:** *(estimate)* The builtin and the record's reader about 500 lines in `zosh` and the shared settings
  crate, a description parser mode, host tests; a hash of the program at each launch.
- **Precedent:** Fuchsia's packages are content-addressed and resolved by a trusted component; Genode's init takes
  the child's needs from its parent's configuration (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`,
  `alternatives-postmortem.md#genode`).

### Option D: Descriptions signed by a publisher key

- **Description:** A package carries a signature; the shell holds a list of trusted publishers.
- **Evidence for:** Upgrades of one publisher's programs need no re-confirmation.
- **Evidence against:** A key store, a revocation list and a trust UI, none of which exist; one publisher
  today (the architect's tree).
- **Cost:** *(estimate)* Large; a decision of its own.
- **Precedent:** Linux distributions' package signing.

## Recommendation

**Recommendation (not a decision): Option C.** Everything in it is the least committing choice that makes a
volume program usable and keeps both accepted invariants true.

1. **The package.** A directory with: `NAME` (the program file, one ELF or a script with `#!`), `NAME.cmd` (a
   description in ADR-0156's format), optionally `request` (one grant word per line, ADR-0397 item 1: what the
   publisher asks for, shown to the person and never applied), and the files the program reads (`share/...`).
   `NAME` is a plain word: ADR-0311's name characters, 1 to 32 of them, not a name of an image command, of a
   builtin, of a function in the shell, of a room or of another package. The package may hold several commands
   (`bin/a`, `bin/b`) each with its `.cmd`; the first stage allows one.
2. **Where it lives.** `/apps/NAME/` on the volume: the program, its description, `share/`. It is not under
   `/home`, `/data` or `/config`. No program receives an entry over `/apps` except its own package directory, read
   only (as it receives `/proc/self/exe`, ADR-0153 rule 10): the authority is over files the installer put there
   for it. A line that gives `+w` over `/apps`, `/` or a path inside a package is refused before it runs
   (`zosh: /apps/ed: installed programs are changed with install`), the one new refusal.
3. **Who installs.** Only the `install` builtin of an interactive `zosh` (ADR-0155 rule 8's builtins: it acts on the
   shell's own authority, the volume root of the first terminal). `install` is refused in a shell that does not
   hold `/apps` and `/config`, in a script run by a program, and in an untrusted room; a builtin cannot be
   reached from a program's output. `install PATH` (the package named as a literal operand, which designates a read
   as ADR-0153 rule 4 says) prints what the package asks, the files it would copy and the hash, and, unless the line
   carries the words of ADR-0397 item 2, installs it **with no grant at all**; the printed hint is the line to type
   for what it asked.
4. **The record.** `/config/apps`, one line per program, ADR-0157's word syntax and crate, written atomically:
   `NAME sha256:HEX [GRANT...]`. The hash covers the program file and the description together (the bytes of
   `NAME` then `NAME.cmd`, with a length prefix each). The line is the identity of what is installed. A program
   cannot read or write `/config` unless the person typed it (INV-0396-5).
5. **The description of an installed program.** Read from `/apps/NAME/NAME.cmd` by the shell, in the same total,
   bounded parser, with a smaller set of keys: `name`, `summary`, `usage`, `operand` (the kind is read for
   completion and shown, and **treated as `text`**: nothing is designated), `option`, `exit`, `question` and
   the body. `data`, `needs`, `program`, `cwd`, `setting`, `when` and any key the image lint knows beyond these
   are refused at install with the key and line. So the description gives help and completion and no authority; to
   read a file the person writes `+r` as for any program not in the image (ADR-0153's Decision). Text from the
   file is hostile data as ADR-0337 item 2 says: only characters ADR-0311 allows in names and ordinary text, no
   control character, shown with a length limit.
6. **Finding it.** The shell looks a command name up in builtins, functions, `shell.path` (the image), then the
   installed names in `/config/apps`, in that order; an installed program cannot shadow an image command, and
   `install` refuses a name an image command has. `help NAME` begins `NAME is an installed program, /apps/NAME/NAME
   (sha256 1a2b3c4d, installed 2026-11-15)`; `help` lists installed programs in a section of their own, so a person
   sees which pages came from the volume (ADR-0156 item 4). A description whose hash does not match the record is
   not shown: `help NAME` says `changed since installed`, and the program does not start (item 8).
7. **The lint.** `install` runs, over the package, what can be checked without the program's source: the parser
   (total, bounded, ADR-0156 item 1); item 1's name rules; every command of the package has a description; the
   description's `name` is `NAME`; no key outside item 5's set. It does not check that an operand's kind matches
   the program's behaviour, which ADR-0336 item 5's lint does for the image by testing the program; that is why a
   volume description designates nothing. `make check` cannot run it on a volume; the host test of the parser and
   of the install check runs on sample packages in the repository.
8. **Launch checks the hash, once, on the bytes loaded.** When the shell starts an installed program it makes the
   image handle from the file (ADR-0108), hashes the bytes it hands to the loader, compares them with the record
   and starts the program or refuses with `zosh: NAME: not what was installed (install again)`. The check is on the
   bytes loaded, so a file changed after the check is not what ran. A cache keyed on the file service's
   (volume, object id, version), as ADR-0212 proposes for images, is the later saving.
9. **Upgrade and removal.** `install NAME` again with a different package replaces `/apps/NAME` and the record's
   hash. The saved grants are dropped unless the line carries `--keep`; with `--keep` they are printed and kept.
   A changed description with the same program is also a new hash (the hash covers both). `programs remove NAME`
   (ADR-0399) deletes the package and its record; the program's own directory under `/data` is the person's
   data and is kept.
10. **A program started by another program** is found by path as today, so it has no installed identity: it gets
    the table its parent gave it (ADR-0107) and nothing from `/config/apps`. A launcher that must start an
    installed helper (the browser's processes, ADR-0411) passes handles by `posix_spawn`'s grant list.

**Examples.**

```
~> install ./ed-1.0
ed (sha256 1a2b3c4d): 2 files, 38 KiB, command ed
it asks for:   +own ~/.config/ed    +w ~/notes
installed with no grants. to give what it asks:
  install ./ed-1.0 +own ~/.config/ed +w ~/notes
~> help ed
ed is an installed program, /apps/ed/ed (sha256 1a2b3c4d, installed 2026-11-15)
...
~> echo x > /apps/ed/ed
zosh: /apps/ed/ed: installed programs are changed with install
```

**Tensions with accepted ADRs.**

- *ADR-0156 item 2* ("nothing on the volume is read as a description"): a volume description is read for help
  and completion, for a program the person installed, while its hash matches. It designates nothing, so
  INV-0153-6 is unchanged; ADR-0156 item 2 is amended by this ADR.
- *ADR-0153 rule 3* (a grant is a literal token before the command) and *ADR-0172*: a third place where literal
  words are stored (`/config/apps`), decided in ADR-0397.
- *ADR-0155 rule 8*: `install` and `programs` (ADR-0399) join the builtins.
- *ADR-0336 item 5*'s lint covers the image; item 7 here is its install-time counterpart and is weaker, said so.

### Effect of the vision

An installed program is as visible and as bounded as an image program: its page says where it came from, its
authority is words the person typed, and a copy that was changed does not run. A supervisor reading `/config/apps`
sees every program that can ever receive something beyond its line.

## Consequences

- **Positive.** Programs outside the image get help, completion and (ADR-0397) saved grants without any
  description being trusted. The browser engine can be installed instead of imaged, if the architect wants.
- **Negative.** A hash of the program at each start (a second pass over the bytes; the loader reads them anyway). A
  new refusal (`+w` over `/apps`). An upgrade drops grants unless `--keep`. A package format of four files to
  document.
- **Follow-up.** A package builder (`make` rule or a tool) that makes the directory; fetching a package over the
  network is not decided here (`curl` and `tar` do it today).

## Invariants

- **INV-0396-1.** No program, whether in the image or installed, holds a connection to `/apps` or `/config`
  with a right to write unless the person typed a word that names it; the refusal of item 2 is the only case the
  shell refuses a typed word. *Check:* a host test of the shell's planner over lines with `+w` words at `/apps`,
  `/apps/ed`, `/` and `/config`, and an in-QEMU script that runs an installed program with `+w ~` and asserts it
  cannot create a file in `/apps/ed`.
- **INV-0396-2.** A description read from the volume designates nothing and gives no own directory, no `needs` and
  no setting; every operand kind in it is treated as `text`. *Check:* INV-0153-6's and INV-0341-1's host tests
  extended with an installed package whose description names `read`, `create`, `data` and `needs` lines.
- **INV-0396-3.** `install` refuses a package whose description has a key outside item 5's set, a `name` other than
  the package's, a program without a description, or a name that an image command, a builtin or another package
  has. *Check:* a host test of the install check over sample packages, one per refusal.
- **INV-0396-4.** An installed program starts only when the SHA-256 of the bytes the loader receives equals the
  record's, and an image command is never shadowed by an installed name. *Check:* a host test of the launch
  check with a program changed by one byte, and of the lookup order with a name that exists in the image.
- **INV-0396-5.** No program's table holds `/config/apps` or `/config` unless the person typed a word for it;
  `install` and `programs` are refused when the shell holds neither or when the line is run by a program or in an
  untrusted room. *Check:* a host test of the planner, and an in-QEMU script in which an installed program tries
  to open `/config/apps` and `/apps/NAME/NAME.cmd` for writing.

## Open questions

- A package with several commands, and a command that is a script the person edits.
- Whether the program's read-only package entry should also hold `share/` writable for a program that unpacks
  caches (the engine's shader cache); the own directory (ADR-0397) is the place for it today.
- How a package gets a name that cannot collide with a future image command (the image wins at every lookup, and
  `install` refuses a clash at install time only).
- Whether designation of `read` operands for an installed program should be allowed after install (it would reverse
  INV-0153-6 for installed programs); not recommended before one program needs it.
- The hash's cost on the 1.4 MiB program under HVF and TCG: not measured.

## Revisit triggers

- A second publisher, or programs installed over the network: Option D.
- A program installed on the volume that needs read designation: the item above.
- The volume gains a read-only mount: `/apps` could be one, and item 2's refusal go.

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

Option C, including the amendment of ADR-0156 item 2: a volume description is read for help and completion only and designates nothing.
