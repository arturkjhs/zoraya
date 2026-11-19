# ADR-0399: Seeing and revoking installed programs and their saved grants: the `programs` builtin, what revoking reaches, and a log

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0153, ADR-0155, ADR-0169, ADR-0172, ADR-0267, ADR-0337, ADR-0396, ADR-0397
- **Evidence:** CP-01, CP-23, DM-087, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks

## Context

ADR-0396 installs a program and ADR-0397 keeps the words the person typed for it. A stored authority that the
person cannot list, change and withdraw is the drift ADR-0153 feared of manifests (DM-087), and a grant that
cannot be withdrawn is not a grant but a gift (CP-23: configuration and logs owned and attributable). This ADR
decides the surface a person has for them, in the shell and on the desktop, what revoking does to a program that
is running, and what is recorded.

**What is known.** The shell already shows what a line receives (`grants`), what runs (`jobs`, `top`, ADR-0267)
and rooms (`room`, ADR-0172 item 3), and ADR-0172 item 8 asks that every gesture of the compositor or panel has a
`zosh` line. A running program holds handles; a file connection is a handle to the service and `job_kill` on its
job ends everything it started (ADR-0172 item 2). Whether the file service or `netd` can narrow or revoke a
connection a program already holds is not established here. Plan 9 gave a process its namespace at creation and
changed it only by the process itself or its creator (`alternatives-postmortem.md#plan-9--inferno`), so a revoke
there means a new namespace for new processes. The survey's trusted intermediary is the place a person sees and
changes grants (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).

**What is assumed.** That a person revokes rarely, and when they do they care most that the next start is
without the grant and, second, that what is running stops holding it.

## Decision drivers

1. **Everything saved can be seen in one place, in the words it was typed** (CP-23).
2. **Revoking is one typed line and takes effect at once for every later start**; for running programs the
   person is told, not misled (CP-01).
3. **No new authority in the surface**: the same trusted place that writes the record shows it; no program is
   given a view of it.
4. **Every desktop gesture has its text** (ADR-0172 item 8).
5. **Small**: no new daemon, no database.

## Considered options

### Option A: Edit `/config/apps` by hand (Unix baseline: edit the file)

- **Description:** No command. The person opens the file in `edit` or `vi` and deletes a word.
- **Evidence for:** Nothing to build; ADR-0157's hand-edit rule applies (a bad line is reported once and ignored).
- **Evidence against:** A person must find the file; the hash is hex; nothing says what runs now; an edit is
  not logged; `vi` run with `+w /config` is itself a grant.
- **Cost:** *(estimate)* None.
- **Precedent:** `/etc` files (DM-087's complaint).

### Option B: A `programs` builtin and a view in `settings`, revoke effective at the next start, with `--kill` for what runs

- **Description:** `programs` lists installed programs and their saved words; `programs NAME` shows one;
  `programs revoke NAME WORD...` removes words at once from the record; running programs keep what they hold
  until they end, and the command lists them and offers `--kill`. The desktop's `settings` gets a Programs page
  that is the same text.
- **Evidence for:** Drivers 1 to 5. The records and the commands that change them are in one trusted place, the
  shell, as rooms are.
- **Evidence against:** A program that holds a handle keeps it until it ends or is killed; the person must decide
  to kill.
- **Cost:** *(estimate)* About 500 lines in `zosh`; the settings page about 300 lines later.
- **Precedent:** `room` (ADR-0172); Fuchsia's `ffx` component tools `[assumption: from general knowledge, not fetched]`.

### Option C: Live revocation: the file service and `netd` withdraw a connection a program holds

- **Description:** Every connection the shell hands out is registered so that revoking a word withdraws the
  matching connection at once; the program's next request fails.
- **Evidence for:** Revoking means revoking; no `--kill` needed.
- **Evidence against:** A new protocol in the file service (ADR-0055) and `netd`, a registry of what each program
  holds, and races between an open file and its withdrawal; programs would see sudden `EACCES` mid-write
  (a data-loss risk).
- **Cost:** *(estimate)* Large: two services, tests, a new rights model.
- **Precedent:** Capability revocation through a forwarder (KeyKOS-style) `[assumption: not fetched]`.

## Recommendation

**Recommendation (not a decision): Option B.** Option C is a revisit trigger for the day a long-running installed
program (the browser) makes `--kill` a poor answer.

1. **`programs`** is a builtin (ADR-0155 rule 8), with its subcommands each a literal word line:
   - `programs`: one row per installed program: name, short hash, installed date, number of saved words,
     number of running instances, and `changed` if the hash at `/apps` is not the record's.
   - `programs NAME`: the page: the description's summary, the files of the package with sizes, the hash, the
     saved words each with its meaning in `grants`' words, the request the package carried and which of its words
     were given (`asked, not given: +web`), and the instances running now (`top`'s rows).
   - `programs grant NAME WORD...`: saves words (ADR-0397 items 1 to 3); the printed result is as `install`'s.
   - `programs revoke NAME WORD...`, and `programs revoke NAME --all`: removes the words from the record. The
     line prints what it removed, and the instances that still hold what the word gave:
     `ed: +w ~/notes removed; 2 running programs still hold it (ed, ed). programs revoke --kill ed ends them.`
   - `programs revoke --kill NAME` also `job_kill`s every instance of `NAME` in the shell's job tree; nothing
     else is touched, and an instance of the same name in another room's shell is not reachable and is named.
   - `programs remove NAME [--data]`: deletes the package, the record and, with `--data`, the own directories
     `/data/*/NAME` (asking `delete the files of ed in 2 rooms? ` as `rm` of a tree does; the person's data is kept
     by default, ADR-0396 item 9).
   `programs` and `install` are refused in a shell without `/apps` and `/config`, in an untrusted room and when run by
   a program (ADR-0396 item 3).
2. **What revoking reaches.** The record, at once: the next start of any instance, including one a running
   program starts, is without the word (a child of a running instance inherits its table, ADR-0107; so item 1's
   running count includes children). What a running program already holds, it keeps until it ends, because a handle
   cannot be withdrawn here (Option C). The person's `grants` for a running job shows the entry with `(saved,
   revoked)`; `top` marks the job; `--kill` is the person's decision. This is written in the revoke line's
   output so that it is not a silent half-measure.
3. **Seeing a line's grants.** `grants` and the status line already mark a source per entry: `(room NAME)`,
   `(operand)`, `(grant)`, `(given while running)`; this adds `(saved)`. `jobs` shows each job's saved words with its
   other grants. The desktop's pills mark `(saved)` in the hover card.
4. **On the desktop.** A page `Programs` in `settings` (ADR-0169) shows the same rows and words as `programs` and
   `programs NAME`; a revoke is a button that shows its `zosh` line (`programs revoke ed +w ~/notes`) with the
   show-commands chord (ADR-0172 item 8) and acts through the same code the builtin uses (the shared settings
   crate of ADR-0157), writing the file atomically. It has no `--kill` button in the first version: the line
   says `still running in 2 windows`, and `top` is where the person ends them. A program cannot open this page
   or write the record: the page is a client of the compositor's trusted material (INV-0167-4's rule).
5. **A log.** `/config/apps.log` is an append-only text file of lines
   `2026-11-15T10:02:11 install ed sha256:1a2b3c4d +own +w +net` and
   `... revoke ed +w ~/notes`, written by the shell with the record (never by a program), capped at 64 KiB by
   dropping its oldest lines when it grows past it. `programs --log [NAME]` prints it. It records who changed what
   and when (words, not file contents); it does not record launches, so a start costs no volume write.
6. **Hints.** After a job whose refused paths a saved word would have covered (a revoked word, or a path the
   grant line gave), the hint of ADR-0153 rule 11 adds the `programs grant NAME WORD` line, as text only
   (ADR-0397 item 7).

**Examples.**

```
~> programs
NAME   HASH      INSTALLED   SAVED  RUNNING
ed     1a2b3c4d  2026-11-15      3        0
brws   9f8e7d6c  2026-11-15      2        1  changed
~> programs revoke ed +net sync.example.org:443
ed: +net sync.example.org:443 removed; no instance running.
~> programs revoke brws --all
brws: +web, +own ~/.local/share/browser removed; 1 running program still holds them.
     programs revoke --kill brws ends it.
```

**Tensions with accepted ADRs.**

- *ADR-0155 rule 8*: `programs` joins the builtins.
- *ADR-0172 item 8*: the desktop page is a gesture with a text line, as required.
- *ADR-0169*: `settings` gains a page.

### Effect of the vision

A supervisor of an agent sees which installed programs have standing grants and can end them with one line; a
person sees every word they ever gave a program, in the words they typed.

## Consequences

- **Positive.** One place to see and one line to withdraw; no program can see or touch it; the log answers
  "when did this get network".
- **Negative.** A running program keeps what it holds until killed; `--kill` can lose unsaved work (the line says
  how many programs it will end and asks as `kill` of a group does). The log is a text file on the volume that
  `+w /config` could edit, as the record can.
- **Follow-up.** Live revocation (Option C), if a long-running program needs it.

## Invariants

- **INV-0399-1.** `programs revoke NAME WORD` removes exactly the words named from the record, writes the file
  atomically, and the next start of `NAME`, and of a child of an instance started after the revoke, receives none
  of their entries. *Check:* a host test of the record's writer and of the planner, and an in-QEMU script that
  revokes `+w ~/notes` and runs the program again.
- **INV-0399-2.** A revoke prints every instance that still holds a revoked word, and `--kill` ends exactly
  the instances of `NAME` in the shell's job tree and no other job. *Check:* a host test of the instance list
  over a job tree with two programs and a second room, and an in-QEMU script with a running installed program.
- **INV-0399-3.** `programs`, `install` and the settings page change the record and the log only through the one
  writer of ADR-0397, and a program has no connection that can reach `/config/apps` or the page. *Check:* INV-0396-5's
  test extended to `/config/apps.log`.
- **INV-0399-4.** Every desktop gesture of the Programs page has a `zosh` line shown by the show-commands chord.
  *Check:* the page's host test lists each action with its line (ADR-0172 item 8's check, extended).
- **INV-0399-5.** The log has one line per `install`, `grant`, `revoke` and `remove`, never a launch, and stays
  under its cap. *Check:* a host test of the log's writer over many changes.

## Open questions

- Whether a revoke of a net word can be applied to a running program by `netd` narrowing the connection's scope,
  which would be the cheapest piece of Option C.
- Whether the log should be tamper-evident (a chained hash) once an agent can hold `+w /config`.
- Whether `programs` should show last-run times; that is a volume write on every start, left out.
- Whether the Programs page should also install; it would need a chooser for the package directory, which is
  ADR-0167's grant path.

- The Programs page of `settings` and INV-0399-4's check are not built (part 10j, ADR-0394 item 11): the settings window holds no `/config` and asks the panel for the rooms; the page needs a panel message and the panel the right to write the record, a decision of its own.

## Revisit triggers

- A long-running installed program (the browser engine) whose users revoke often: Option C.
- A second shell session that can run `programs` with a smaller scope than the first: the record needs a lock
  (ADR-0294's note on `/config/rooms` has the same gap).

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

Option B. A revoke edits the record at once. Without `--kill`, running copies keep their handles, and the revoke says so with the count ("N running copies keep access until they exit; --kill to stop them"). Live revocation is not needed for the browser engine. The 16 invariant rows belong to stage 10; the implementation is scheduled as part 10j, after 10g.
