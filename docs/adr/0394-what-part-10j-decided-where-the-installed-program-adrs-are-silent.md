# ADR-0394: What part 10j decided where ADR-0396, ADR-0397 and ADR-0399 are silent, and what it did not build

- **Status:** proposed
- **Date:** 2026-11-19
- **Depends on:** ADR-0153, ADR-0156, ADR-0157, ADR-0172, ADR-0324, ADR-0337, ADR-0341, ADR-0395, ADR-0396, ADR-0397,
  ADR-0399
- **Evidence:** CP-01, CP-23

## Context

Part 10j builds ADR-0396 (`install`, `/apps`, the record, the hash check at launch), ADR-0397 (the saved words) and
ADR-0399 (`programs`, the log). Those ADRs fix what the person sees and which rules hold; they are silent on the record's
date, on the name's exact form, on the package's limits, on how an upgrade is made safe, on what a hand-edited record may
do, on how `install` sees a typed word, and on what to do about the Programs page of `settings`, which needs a message
between the panel and the settings window that no accepted ADR defines. This ADR records the choices so that none is
silent. It changes no accepted decision; the one new refusal of ADR-0396 item 2 (`+w` over `/` or `/apps`) is built as
written and has a visible consequence (item 9). Least authority (CP-01) and one owned place for configuration (CP-23) are
the two rules every choice below was held to.

## Decision drivers

1. A program, a package, a record edited by hand or a variable never widens what a program receives (CP-01).
2. The record stays one file in one syntax the person reads and the shell writes through one module (CP-23, INV-0397-1).
3. Small and reversible: no change to the file service, to the compositor, or to a protocol.
4. A half-made install or upgrade never leaves a program that runs with the wrong files.

## Considered options

### Option A: as built: the record in `zoraya-settings`, the rules in `zoraya-shell`, the page deferred

- **Description:** Every item of the list under Recommendation. The Programs page of ADR-0399 item 4 and INV-0399-4 wait
  for a panel message; INV-0397-6 waits for a grant-line script.
- **Evidence for:** Drivers 1 to 4; the architect's scheduled scope (`install`, `/config/apps`, `programs`) is built and
  measured; the page needs a trust decision of its own.
- **Evidence against:** ADR-0399 item 4 is not built; a person who prefers the desktop has no page.
- **Cost:** *(estimate)* About 2,400 lines with tests; the page would be about 400 more plus a panel message.
- **Precedent:** ADR-0395 recorded what 10f left out in the same way.

### Option B: build the Programs page now, reading and writing `/config/apps` in `settings`

- **Description:** Give the settings window the right to read and write the record.
- **Evidence for:** Complete against ADR-0399 item 4.
- **Evidence against:** The window holds no `/config` (it asks the panel for the rooms, ADR-0267 item 4); giving it the
  record makes a GUI client one of the writers INV-0397-1 limits to the shell; ADR-0172 INV-0172-1's rule that no client
  changes the records of a grant would need an exception.
- **Cost:** *(estimate)* A new trust boundary, hence its own ADR and review.
- **Precedent:** none in the tree.

### Option C: a panel message for the page, the panel writing the record through the one writer

- **Description:** As rooms do: the window asks the panel (`AppsQuery`, `AppsRevoke`), the panel reads and writes
  `/config/apps` with `zoraya_settings::apps` and the log.
- **Evidence for:** Keeps INV-0397-1 (the writers are the shell and the panel's trusted code, through one module).
- **Evidence against:** The panel then holds `/config` write for a second file; a protocol, a page and a GUI test.
- **Cost:** *(estimate)* Large; its own part.
- **Precedent:** `RoomsQuery` and `room set` through the panel.

## Recommendation

**Recommendation (not a decision): Option A now, Option C as a part of its own.** As built:

1. **The record's date.** A line is `NAME sha256:HEX [--installed YYYY-MM-DD] WORD...`; the date is what `help NAME` and
   `programs` show (ADR-0396 item 6) and is not in the hash. A volume with no wall clock writes none and the log's time
   is `unknown`.
2. **The name.** The form a description's `name` takes (ADR-0156): a lower-case letter, then letters, digits and `-`, at
   most 24 bytes (the description's own limit, which `NAME.cmd` must carry; not 32). Also refused: a help topic, `grant`,
   `revoke`, `remove` (words of `programs`), a name `data-names.txt` lists, a program of `shell.path`, and a name whose
   `/apps/NAME` exists with no record line (a directory no `install` made; `programs remove NAME` removes it).
3. **The hash.** SHA-256 over the program's length (8 bytes, little endian), the program, the description's length and
   the description. `install` writes the description from the bytes it linted, so the hash covers what the lint saw; the
   launch check hashes the bytes in the loader's memory object before an image is made of it (`load_checked`), so a file
   changed after the check is not what runs.
4. **The copy.** A package is one command (`NAME`, `NAME.cmd`, optional `request`, other files and directories up to 6
   levels, 2,000 entries and 256 MiB); a link in it is refused. It is copied to `/apps/.new-NAME`, then the old
   package is removed and the new renamed into place; a failure removes the partial copy and leaves the old one.
5. **A typed word.** `install` and `programs grant` read their words from the values of the line: a word, or its
   argument, from a variable, a substitution or a pipe refuses the line (INV-0397-1). A path typed with `~` reaches the
   builtin expanded; the record keeps it as `~` (the shell's home), so `programs` and a revoke name it as typed.
6. **What is never saved or applied.** Nothing at or below `/config`, `/apps` or `/data`, and not `/` (ADR-0397 item 3
   read as a prefix rule), a file, `+net any`, `+listen`, a relative path. A hand-edited record that names such a
   place is read, and at launch the word is dropped with `zosh: NAME: saved WORD is not applied: ... cannot be given`, so
   the record is in no table whatever it says (INV-0397-2).
7. **Saved network words.** The scope is made from the saved `+net` words and `+web` (`netgrant::saved_scope`); in a room
   each lies inside the room's `+net` words, else it is dropped with the line of ADR-0397 item 5; outside a room the
   shell's own connection bounds it (`narrow` refuses a wider one). A native program takes saved `+r` and `+w` only.
8. **Revoke and kill.** `programs revoke` says how many copies keep access (the architect's wording). `--kill` ends the
   jobs of this shell that hold a copy, whole (a pipeline is one job, ADR-0172 item 2), and says that other rooms' shells
   are not reachable. `programs remove` refuses while a copy runs, asks before `--data` deletes the files of a program
   in each room, and removes a directory with no record.
9. **The one new refusal's consequence.** A typed `+w /` is refused on every line (ADR-0396 item 2); the test of
   ADR-0289 item 8 that used `+w /` to show `/system` stays read-only uses `+w /home` for the wide grant.
10. **The description of an installed program** is read for `help`, completion and usage only after it matched the
    record: once a session at a launch or `help NAME` (always re-checked), and shown from the checked copy.
11. **Not built.** The Programs page and INV-0399-4 (Option C, a part of its own); INV-0397-6's grant-line script; the
    one-line hint `to keep this for NAME: programs grant NAME WORD` after a job that was given something while it ran
    (ADR-0397 item 7, ADR-0399 item 6); `+own` and `+net` for a native program; a lock for two shells that run
    `programs` at once (the revisit trigger of ADR-0399).
12. **Known gaps found in review (`docs/tasks/TASK-31-review-10j.md`).** Only a typed `+w` and a room's `+w` are refused over
    `/apps` and `/`; a redirect, a `cp` or `rm` operand into `/apps` is the person's typed word and is let through (the hash
    catches the change). The package's own `NAME.cmd` gives the name: a package of an installed name replaces it, and
    `--keep` gives it that program's words (the output says `replaces`). A glob that expands to a directory name counts as
    typed for `programs grant`. `room new` accepts `+w /`.
13. **Measured.** SHA-256 at launch, 64.3 MB program, HVF on the M4: about 0.15 s on top of a load of about 1.0 s (the
    same file by its path): about 440 MB/s. The install of that file (copy and hash) took 1.7 s. Not measured under TCG.

## Consequences

- **Positive.** A volume program has help, completion, an own directory, a network and saved directories without any
  description being trusted; a changed file does not start; a failed upgrade leaves the old one.
- **Negative.** A typed `+w /` no longer works; a person who ran `+w /` writes the directory they mean. The page is
  missing; `programs` is the interface. A pipeline job that holds a copy is ended whole by `--kill`.
- **Follow-up.** The page (Option C); the hint line; INV-0397-6's script.

## Invariants

None new. The checks of ADR-0396, ADR-0397 and ADR-0399 are in `invariants.md`, with the statuses this part left them.

## Open questions

- Whether `+w` of a whole `~` should be saved only with `--everything` typed (ADR-0397's Negative).
- Whether a package may have several commands; the first stage has one.
- Whether the log should name the words of an install as the revoke does (it names their kinds, as ADR-0399's example).

## Revisit triggers

- The settings page is wanted (Option C).
- A long-running installed program makes `--kill` of a whole job a poor answer (ADR-0399's trigger).

## Architect's decision

(Left empty for the human.)
