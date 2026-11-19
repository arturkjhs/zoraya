# ADR-0397: Saved grants of an installed program: typed once at install, kept in the record, never more than the shell holds

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0153, ADR-0172, ADR-0224, ADR-0324, ADR-0337, ADR-0341, ADR-0396
- **Evidence:** CP-01, CP-23, DM-009, DM-014, DM-039, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#pattern-a-grants-fixed-at-start, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks

## Context

A program installed under ADR-0396 has no designation and no own directory from its description. To be usable it
needs, on every line, `+net HOST`, `+w DIR` and its own directory (ADR-0341), which the person would type each
time. ADR-0153 considered stored per-program permissions (its Option D) and decided against them as the
only mechanism: authority is not visible on the line, and a stored grant applies to every future run with any
operands. ADR-0172 then accepted a stored form for rooms: literal words on a line of `/config/rooms`, resolved
against the creator's own scope. ADR-0337 item 6 decided that a grant given while a program runs "is written
nowhere", because a program can only be refused and can never make a grant stick (the trust boundary of
TASK-31's part 10b).

**What is known.** A stored grant is ambient authority by program name unless it is bound to something a
program cannot change (DM-009, DM-014); a path grant that names a path is open to the races of a path that moves
(DM-039); the pattern in the survey is a grant fixed at start by the one who starts (`stage-10/survey.md#pattern-a-grants-fixed-at-start`) or
a trusted intermediary that asks (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`); Fuchsia routes
capabilities to a component from its parent's manifest, not from the component's own (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`);
Plan 9 builds each process's namespace in its creator (`alternatives-postmortem.md#plan-9--inferno`).

**What is assumed.** That a person gives an installed program the same few things every time (a host, a notes
directory), and that anything that needs more is better asked on the line or on the grant line of ADR-0337.

## Decision drivers

1. **A program can never read, edit or widen its saved grants**, nor get more than the shell that starts it holds
   (CP-01, ADR-0172 driver 4).
2. **Saved grants are the person's typed words**, stored as those words and shown as those words (CP-23: owned and
   attributable).
3. **They are bound to what was installed**: a different program does not inherit them (ADR-0396 driver 5).
4. **Reconciled with ADR-0337 item 6**: nothing a program does, and nothing given while it runs, is stored.
5. **Small**: one file, one syntax, one place to read for a person and for a supervisor.

## Considered options

### Option A: No saved grants: every line carries its words (Unix with `sudo`-less habit; ADR-0153's lines)

- **Description:** An installed program gets what the line and the room give. A person who wants it saved writes an
  alias or a function in `/config/start.zosh` (ADR-0157 item 6), or a room.
- **Evidence for:** Nothing new; the grant is visible on the line each time (ADR-0153 driver 1).
- **Evidence against:** Functions are text the person edits and a program with `+w /config` could rewrite;
  per-shell, not per program; a function is not bound to a hash (driver 3). The own directory's
  `HOME` rule (ADR-0341 item 3) cannot be written as a line's word.
- **Cost:** *(estimate)* None.
- **Precedent:** Unix aliases.

### Option B: Saved as the person's literal words in the record of `/config/apps`, bound to the program's hash (a rooms-like line per program)

- **Description:** The record line of ADR-0396 item 4 carries the words: `ed sha256:HEX +own ~/.config/ed +w
  ~/notes +net api.example.org:443`. The shell, starting `ed`, checks the hash (ADR-0396 item 8), resolves each word
  against its own table and scope as if the person had typed it before the command, and the program receives the
  entries. A word the shell does not hold is dropped with a line.
- **Evidence for:** Drivers 1 to 5. It is the form ADR-0172 accepted for rooms, with the same reader and writer
  (ADR-0157's crate). A program's own directory (ADR-0341) is one more word, `+own`, so INV-0341-1 stays true
  (the person's record gives it, not a description).
- **Evidence against:** A third place where grants are written (line, rooms, now a program's record); a person
  can forget what is saved (answered by ADR-0399, not here).
- **Cost:** *(estimate)* About 400 lines in `zosh` (parse, resolve, drop, print), host tests; no file service
  change.
- **Precedent:** ADR-0172's rooms; Fuchsia's manifests as the parent's routing table.

### Option C: Saved by the trusted UI after asking while the program runs (a powerbox that remembers: macOS's privacy prompts, Flatpak portals)

- **Description:** When the program is refused something, the grant line (ADR-0337) offers "save this", and the
  decision is stored for the program.
- **Evidence for:** No typing before first use; the person decides at the moment they see why.
- **Evidence against:** It reverses ADR-0337 item 6 and the trust boundary of 10b: a program could provoke a
  prompt for a thing it chooses, the person's reflex Enter saves it for ever, and the status line is hostile
  data (ADR-0337 item 2). A saved decision keyed on a name or path rather than a hash follows a replaced file.
- **Cost:** *(estimate)* A prompt in the shell and the compositor's card; a larger attack surface.
- **Precedent:** macOS TCC, Android runtime permissions `[assumption: from general knowledge, not fetched]`.

## Recommendation

**Recommendation (not a decision): Option B.** Option C stays a revisit trigger; an install is where the person
chooses, and a refused path prints the line that would save it.

1. **The words.** A saved grant is one of these, literal, as ADR-0153 rule 3 and ADR-0224 item 1 write them:
   - `+r DIR`, `+w DIR`: a directory the shell holds (a file is its directory narrowed, ADR-0311 item 5; saving a file
     is refused: type it on the line);
   - `+net TARGET[:PORTS]`: ADR-0224's, `+net any` refused for saving;
   - `+web`: the `web` rule of ADR-0324 (public hosts, ports 80 and 443), a word that only a record has: a line
     cannot write it (ADR-0324 follow-up), and the record is the person's typed `+web` on `install` or `programs
     grant`; refused in an untrusted room's launches as ADR-0324 item 7 says;
   - `+own PATH`: the program's own directory of ADR-0341 items 2 to 7, seen at `PATH` (`~/` as there; the same
     `HOME` rule), as `data NAME as PATH`; the directory is `/data/ROOM/NAME`, per room, shared with no other name.
   Nothing else is saved: not `+listen`, not a file, not a command's argument, not an environment variable.
2. **Saving is typed.** The words go on the `install` line (ADR-0396 item 3) or on `programs grant NAME WORD...`
   (ADR-0399). They are literal tokens the shell's parser reads from the typed line; the value of a variable or a
   substitution is never one, as everywhere. A script run by the person may type them. Nothing reaches the
   store from a program, from the status line, from the grant line, from a drop or from a chooser.
3. **What a saved word may not name.** `install` and `programs grant` refuse, with the path: `/`, `/config`,
   `/apps`, `/data`, a `/data/ROOM` or `/data/home`, a path inside another program's own directory
   (ADR-0341 item 8), and the shell's own `~` as a whole when it is the volume root. `+w` over the person's home
   (`~`) is allowed but printed with the word `everything`. The words `grants` prints for it are the same.
4. **Bound to the program.** A record applies only to the program whose hash equals the record's (ADR-0396 item 8).
   An upgrade drops the words unless `install --keep` (ADR-0396 item 9). A copy of the program under another path
   is not an installed program and gets nothing (INV-0341-1's second half).
5. **How it is applied.** At start the shell adds the record's words to the line's grants, resolved against its
   own table and network scope (ADR-0153 rule 3, ADR-0224 item 1), after the room's and before the line's own
   words, so the line can add more and cannot remove one. A word the shell does not hold (the shell is in a
   room that holds less, or the directory is gone) is dropped for that run and the shell prints `zosh: ed: saved
   +w ~/notes is not held here` before the program starts: the program is never given more than the shell holds
   (driver 1). In an untrusted room no saved word is applied, and the line says so (`zosh: ed: saved grants are
   not applied in an untrusted room`), because ADR-0337 item 10 and ADR-0167 ask for the compositor's card there
   and no path for it exists.
6. **What the program can never do.** Read the record: `/config/apps` is in no table the shell builds for a
   program, and a `+r` or `+w` over `/config` is not saved (item 3); a typed one is the person's. Write it: the
   same. Learn its saved grants as a list: it holds the entries, and the person's `grants ed` shows them with the
   word `(saved)`. Widen them: the start checks the hash and applies words, never the program's request; its
   `request` file is shown at install and never read after. Keep them across a replaced binary (item 4).
7. **Reconciled with ADR-0337 item 6.** Grants given while a program runs are still written nowhere. A saved grant
   is not one of them: it is typed by the person outside the program's run, before or between runs, and applied
   at its start. After a job that was given something while it ran, `jobs`-style hints print one line `to keep
   this for ed: programs grant ed +w ~/other` (ADR-0399); the person types it or not. No keystroke on the
   grant line saves.
8. **First run and later.** The first run of an installed program shows nothing extra: the `install` line already
   printed every word and the hash (ADR-0396 item 3), which is the consent. Every later `grants ed` and `help ed`
   lists them, each with its word and `(saved)`; a window's pills show them as `(saved)` beside the room's
   (ADR-0172 item 5).
9. **Own directory and rooms.** `+own` gives the program, in each room, that room's directory
   `/data/ROOM/NAME`; the room's storage limit applies (ADR-0341 item 6). A saved `+w DIR` is not per room: it
   applies in every room that holds `DIR`.

**Examples.**

```
~> install ./ed-1.0 +own ~/.config/ed +w ~/notes +net sync.example.org:443
ed (sha256 1a2b3c4d) installed with:
  +own ~/.config/ed    its own files (it sees them as ~/.config/ed)
  +w ~/notes           change anything in ~/notes
  +net sync.example.org:443
~> grants ed
ed gets (a POSIX program):
  in, out, err         the terminal
  /data/home/ed        its own files (it sees them as ~/.config/ed) (saved)
  ~/notes              change anything in (saved)
  net: sync.example.org:443  reach (saved)
  /apps/ed             read (its package)
~> room agent ed
zosh: ed: saved grants are not applied in an untrusted room
```

**Tensions with accepted ADRs.**

- *ADR-0153 rule 3 and its Option D*: ADR-0153 refused manifests as the one mechanism; this keeps the words literal,
  visible in `grants` and bound to a hash, and says so (the same stated tension ADR-0172 had for rooms).
- *ADR-0337 item 6*: nothing given while running is stored; unchanged (item 7).
- *ADR-0324 follow-up*: `+web` in a record is the "room words could take it later" idea, narrowed to a record.
- *ADR-0341 items 1 and 3*: `+own` replaces a description's `data` line for installed programs only.

### Effect of the vision

A person gives a program what it needs once, in words they can read in a text file, and the system can show
exactly that to a supervisor. An agent in an untrusted room never inherits them.

## Consequences

- **Positive.** The installed browser or git is given its directory, its host and its notes once. No program can
  turn a prompt into a standing permission.
- **Negative.** A saved `+w ~` is a wide grant that persists; `everything` in the print is the only brake. Typing
  the words at install is a step a person who wants "just run" will skip, and then types them on every line.
- **Follow-up.** Showing and revoking them is ADR-0399.

## Invariants

- **INV-0397-1.** The only writers of `/config/apps` are `install` and `programs` of an interactive shell, run by
  the person's typed line, and every saved word is a literal word of that line. *Check:* a host test of the
  parser that feeds `install` and `programs grant` a word from a variable, a substitution and a pipe, asserting each
  is refused, and a test that a program's request file changes nothing.
- **INV-0397-2.** A program never receives, from its saved words, an entry its shell does not hold, and never the
  record itself. *Check:* a host test of the planner with a shell that holds less than a record names, asserting
  the dropped words and the line printed, and an in-QEMU script in which an installed program tries to open
  `/config/apps`.
- **INV-0397-3.** A record's words apply only when the hash of the loaded bytes equals the record's; a program
  under another path or hash gets none. *Check:* INV-0396-4's test extended with a record with words.
- **INV-0397-4.** `install` and `programs grant` refuse the words of item 3 and `+net any` and a file path. *Check:*
  a host test, one case per refusal.
- **INV-0397-5.** In an untrusted room no saved word is applied, and `+web` is refused there as ADR-0324 item 7
  says. *Check:* a host test of the planner over a trusted and an untrusted room.
- **INV-0397-6.** A grant given while a program runs is stored nowhere: after a run in which the grant line gave
  `+w ~/other`, `/config/apps` is byte-for-byte as before. *Check:* an in-QEMU script that gives a path on the
  grant line and compares the file.

## Open questions

- Whether `+r` should be saved at all (a read of a directory is already the default for the current directory).
- A saved word for a file, as a narrowed directory with one name; ADR-0337 item 8's list does the work for a line.
- Whether `+listen` should be saved (a server program); it needs ADR-0225's rules and is left out.
- Whether a saved word should apply in an untrusted room when the room holds it; refused until the
  compositor's card for typed grants exists (ADR-0167, ADR-0268).
- A per-room saved list (the same program with different words in a room): rooms carry their own words today.

- INV-0397-6's check (an in-QEMU script that gives a path on the grant line and compares `/config/apps`) is not built: the grant line is the compositor's and a serial script cannot type it, and the GUI tour has no installed program yet (part 10j, ADR-0394 item 11). The code path never calls `zoraya_settings::apps`: the delivery of a grant (`zoraya_shell::delivery`) knows no record.

## Revisit triggers

- A second path for saving (Option C) once the compositor's card for typed grants exists.
- A person-facing measure that people skip the install words and type `+w ~` on lines: then offer saving from a hint.

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

Option B. An upgrade drops saved grants unless `--keep`, and says which grants it dropped. Saved words never apply in an untrusted room. `+web` may be saved. The SHA-256 at each launch stands; it is to be measured for a 64 MB program in the implementing part. One command per package for now.
