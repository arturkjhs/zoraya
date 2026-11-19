# ADR-0395: What part 10f decided where ADR-0337 is silent, and what it did not build

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0096, ADR-0153, ADR-0160, ADR-0224, ADR-0245, ADR-0311, ADR-0331, ADR-0332, ADR-0337, ADR-0386,
  ADR-0387
- **Evidence:** CP-01, CP-02, CP-05, desktop/survey.md#spoofing-and-the-trusted-path, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks, stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees

## Context

Part 10f builds ADR-0337 (the status row, the attention keys, the grant line) and the kill offer ADR-0387 item 4 moved to
System+K. ADR-0337 fixes what the person sees and the trust rules; it is silent on the wire, on how the window makes room
for a row, on the grant line's editing keys and character set, and on what a key does that no accepted ADR names. It also
asks for the `GRANTS` channel to the running program (item 7) and `narrow` with a list of names (item 8); this part built
both, for POSIX programs. This ADR records the choices so that none is silent. It changes no accepted decision; the gaps in
item 9 are gaps in the build, not deviations.

## Decision drivers

1. Nothing a program writes or sends reaches the row, the field or a grant (ADR-0337 driver 4, INV-0337-1, INV-0337-5;
   desktop/survey.md#spoofing-and-the-trusted-path, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks).
2. The person sees the truth, and is never shown a grant that did not happen (ADR-0337 driver 2;
   stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees).
3. Reversible and small: one more row, four more ordinals (two in the terminal's protocol, one in the compositor's, one in the file protocol) and one slot, no change to the grid the parser writes (INV-0096-1 to -3;
   CP-01 least authority is not touched, CP-02 rights only narrow: the grant line gives no more than the shell holds,
   CP-05 an untrusted room's grant line is refused).

## Considered options

### Option A: as built, the delivery for POSIX programs, a host and a native program not delivered

- **Description:** Everything ADR-0337 asks for the row, the keys, the field, the guard, the parse, the delivery of a file or
  a directory to the one running POSIX program (the `GRANTS` channel and `narrow` with a list), the kill offer and the
  network refusal report and hint is built and tested, in a window by `tests/gui/posix-grant.script` at four sizes. A
  `+net` word and a native program get a plain refusal on the row: `cannot be given to a running program yet`.
- **Evidence for:** Every trust rule is real and tested; nothing false is shown; the part's "done when" (`:e!` shows the file,
  a second program is refused it) passes in QEMU.
- **Evidence against:** A running program's network connection and a native program's table do not grow.
- **Cost:** *(as built)* about 1,500 lines with tests across `zoraya-abi`, `zoraya-proto-file`, `zoraya-fs-service`,
  `zoraya-fsclient`, `zoraya-posix`, `zoraya-shell`, `zoraya-term`, the compositor and `zosh`.
- **Precedent:** none; it is a stage of the ADR's own build.

### Option B: leave System+G unbuilt until the delivery exists

- **Description:** Build the row and the kill offer only; no grant line.
- **Evidence for:** No key without an effect. **Against:** The grant line's guard, parser and element choice would be
  written later without the review of this part; the status row's wording says `System+G: give` to the person.
- **Cost:** *(estimate)* less now, the same later.

### Option C: deliver whole directories now, without `narrow`'s list

- **Description:** A `+r DIR` or `+w DIR` for a directory that is not a prefix of the program's table arrives as a new
  entry; a file does not.
- **Evidence against:** The ADR's own example (`~/other.txt` while `~/notes.txt` is given) is a file in a directory the
  program already holds a narrowed entry of; it would not work, and the half that works is the half people do not need.

## Recommendation

**Recommendation (not a decision): Option A**, with the delivery as built:

1. A fixed slot `slot::GRANTS` (1006, after `FD0`) in `zoraya_abi::posix::slot`: the read end of a channel the shell made,
   whose write end it keeps per POSIX element (`Job`'s `granted`). A native `Startup` index is not built.
2. A message is `ZGR1` (`zoraya_abi::posix::Grants`), a count of at most 4, and that many directory entries (kind, handle
   index, prefix) with their connections. An entry for a prefix the table has replaces it; the shell keeps the union per
   directory (`zoraya_shell::delivery::Held`) and sends the whole entry, a file as its directory narrowed to a list of
   names (`NarrowNames`, file ordinal 44, each name with its rights) and a directory whole. The decoder refuses `/`,
   `/dev`, `/proc`, `/tmp` and below. A grant never shadows what the program holds whole (item 9 of the decisions below).
3. `libposix` reads the channel only when a path was refused, either outside the table or by the service (`EACCES`),
   installs what came (the table is bounded at 64 entries), and resolves the path once more.
4. The write end closes with the job, and the record with it.

What this part decided where ADR-0337 is silent (all reversible):

1. **The wire.** `zoraya-proto-term`: `Status { kind: Below<3>, text }` (ordinal 9; `kind` 0 a notice with the key's words
   at the right end, 1 plain text, 2 a field) and `Attention { kind: Below<2> }` (ordinal 10; 0 System+G, 1 System+K);
   `zoraya-proto-gui`: `Attention { window, kind }` (ordinal 47, a client event). `ttyd` ignores both.
2. **The chords.** System+G and System+K join `TAKEN_CHORDS` (13 keys). The compositor sends the focused window
   `Attention`, as it sends `Copy`; no key reaches any client. A client that does not know the message ignores it.
3. **The row.** `term` makes its window one row taller than the grid (80 x 24 grid, 25-row surface) and keeps the last row
   that fits for the status (an empty row is drawn on a ground of its own, between the window's and the selection's, so that a
   test can tell an empty row from none; a program can paint either colour in its own rows, and can draw a look-alike of the
   row: the real row is always the lowest, which is the only tell, ADR-0337's first open question stands); on a resize the grid is one row less than the window had. The row is drawn in the selection's
   colours so that it does not look like the program's screen. Its text is the shell's, cleaned again by `term`: a control
   character is a space, bidirectional and zero-width characters are dropped, a character of other than one column is
   `?`, and a text past the row is cut. The shell cuts a shown path at 60 bytes (ADR-0337 item 2: "fits the row whole").
4. **The attention mark.** The attention message reaches the shell's key loop as one byte of the key stream, `0xC0`
   (System+G) or `0xC1` (System+K), at its place among the keys: the bytes never occur in UTF-8, `Input` filters them
   out as it does the paste marks, so no key makes one. The mark only exists while a line is in front.
5. **The field.** Typed text, Backspace, Ctrl-U, Tab, Enter and Escape; no cursor movement, no history, no selection
   (arrow keys do nothing). Tab on an empty field moves to the next element of a pipeline; on text it completes the last
   word against the shell's own table. The attention key again closes the field; the other attention key replaces it.
   The text is at most 160 bytes. A paste goes in with its newlines as spaces.
6. **Literal words.** A word is refused if it holds a control character or any of `` $ ` ( ) { } [ ] * ? ; | & < > ' " \ ! # % ``;
   `~` and `~/` at the start of a path are the home directory, `~name` is refused, a relative path is joined to the current
   directory, a word that begins `+` is not a path. One refused word gives none of the line.
7. **The result.** Enter, after 500 ms, parses the text, checks it against what the shell holds, delivers it and answers to
   the field: success says `given until PROGRAM ends: WORDS`; failure closes the field and says why in one line, once (the
   row then goes back to what was pending). The field is not kept open after a failure (nothing typed is kept either).
8. **The kill offer.** System+K shows `kill PROGRAM? Enter kills, Escape keeps`; Enter after the same 500 ms guard ends the
   line's job; Escape or the key again keeps it. It is offered in an untrusted room's terminal too (it grants nothing);
   on the serial console neither key does anything.
9. **What is delivered, and what is not.** A file or a directory the shell holds goes to the one element named, as item 2 of
   the recommendation says. A grant below a directory the program holds whole, or at a system directory it holds, is refused
   (`the program holds PATH already; start it again with the grant`): the new entry would shadow, or replace, a wider one,
   and widening it would give more than the person typed. A path that does not exist is refused (`does not exist`, only after
   the shell found it inside its own reach). A directory is given with exactly the rights typed: if the program holds files
   in it with more, or holds a deeper entry, or has an entry the plan does not list (a program's own directory, ADR-0341 item
   3: never assumed read only, never replaced), the grant is refused with the same text, and a grant is one message of at most
   4 places, whole or not at all. The shell's lookup of a refused path opens it with no rights through its own root (there is
   no stat by name in the file protocol): ADR-0337 INV-0337-6's "opens nothing" is not met, and the Status says so. **Not built:** a `+net` word and a native program (no `Startup` index) are
   refused on the row; a write refused inside a read-only entry is reported only when the service answers `EACCES`;
   `user/src/namespace.rs` still reports once per path, not per path and access; the report limit of 16 restarts after a
   grant only in `libposix`; datagram and listen refusals are not reported; the planner (`grants.rs`) still refuses two files
   of one directory at start (ADR-0391 item 8), and the run-time list is the only place `narrow`'s list is used.
10. **A refusal is counted per path, access and element**; `libposix` reports a read and a write of the same path as two
    reports. The hint after a job and the row use the same filter; a path that does not pass is `a path that cannot be
    shown` in both.

## Consequences

- **Positive.** A refusal appears on the window's row as it happens, worded by the shell's own lookup, and a hostile report
  cannot put anything on it; the hint after a job no longer prints a path as it came; the kill offer exists in a terminal
  whose program has the keys; the grant line's input, guard, parse and checks are reviewed before the delivery is built.
- **Negative.** A host and a native program cannot be given anything while they run. The window is one row taller (and where
  the compositor fits it above the dock the grid has one row fewer than `grid_for` says). `Status` and `Attention` are new ordinals. The row's text is the
  shell's only, so a program cannot show a message there even in the way a title could be misused.
- **Invariants of ADR-0337.** INV-0337-1 to -3 and -5 to -7 are checked as their Status column says (partly); INV-0337-4
  waits for the delivery (ADR-0337's open-question note).
- **Follow-up.** A native `Startup` index and `+net` delivery; the planner's use of the list at start (ADR-0391 item 8); the
  `/data` rule in `Grants`' prefix check when ADR-0393 lands (a program's own directory is never replaced, ADR-0341 item 3); a
  fuzz target for `term`'s input with the row compared before and after.

## Invariants

- **INV-0395-1.** The status row's cells change only by the shell's `Status` message, and what `term` draws of its text
  has no control, bidirectional or zero-width character and no character of other than one column. *Check:*
  `cargo test -p zoraya-term` (`tests_status`).
- **INV-0395-2.** No `Input` byte is an attention mark, and no client receives System+G or System+K as a key. *Check:*
  `cargo test -p zoraya-shell paste` and `cargo test -p zoraya-compositor` (the attention chords).
- **INV-0395-3.** A path refused to a POSIX program is reported once per path and access. *Check:*
  `cargo test -p zoraya-posix --test files` (`a_path_is_reported_once_per_access`).
- **INV-0395-4.** A grant given while a program runs never shadows or replaces an entry the program holds whole, and a
  refused grant changes nothing. *Check:* `cargo test -p zoraya-shell delivery`.
- **INV-0395-5.** A `GRANTS` message decodes totally, adds only directory entries, and never `/`, `/dev`, `/proc`, `/tmp` or
  a name below them. *Check:* `cargo test -p zoraya-abi grants_message`.
- **INV-0395-6.** A connection narrowed to a list of names has each right on a name only if that name's own entry holds it, for
  every request that names something, and it cannot list or widen. *Check:* `cargo test -p zoraya-fs-service
  a_connection_narrowed_to_a_list a_list_gives_remove_rename`.


## Open questions

- Whether the row should be one row of the window or part of the compositor's frame (ADR-0337's first open question stands).
- Whether the field should have cursor movement before the delivery is used by people.
- Whether the status row should show a refusal for a line that is not in front when it comes back (`fg`); now it does not.

## Revisit triggers

- A program is found that writes to the row or opens the field.
- A GUI test that reads the grid is broken by the taller window.

## Architect's decision
(Left empty for the human.)
