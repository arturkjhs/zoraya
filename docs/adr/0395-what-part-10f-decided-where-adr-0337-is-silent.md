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
asks for two things that this part did not build: the `GRANTS` channel to the running program (item 7) and `narrow` with
a list of names (item 8). This ADR records the choices so that none is silent. It changes no accepted decision; the gap in
item 9 is a gap in the build, not a deviation.

## Decision drivers

1. Nothing a program writes or sends reaches the row, the field or a grant (ADR-0337 driver 4, INV-0337-1, INV-0337-5;
   desktop/survey.md#spoofing-and-the-trusted-path, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks).
2. The person sees the truth, and is never shown a grant that did not happen (ADR-0337 driver 2;
   stage-10/survey.md#pattern-e-refusal-and-what-the-person-sees).
3. Reversible and small: one more row, three more ordinals, no change to the grid the parser writes (INV-0096-1 to -3;
   CP-01 least authority is not touched, CP-02 rights only narrow: the grant line gives no more than the shell holds,
   CP-05 an untrusted room's grant line is refused).

## Considered options

### Option A: as built, with the delivery missing

- **Description:** The pieces of ADR-0337 that need no new transport are built and tested; the grant line parses and
  checks what is typed against what the shell holds, and then says `PARSED WORDS: cannot be given to a running program
  yet; start it with the grant instead`. Nothing is given.
- **Evidence for:** Every trust rule that concerns the row, the key and the parse is real and tested; nothing false is shown;
  the delivery needs a file-protocol change that two parts share (ADR-0337 item 8, backlog "narrow with a list of names").
- **Evidence against:** The ADR's "done when" (`:e!` shows the file) is not met; the key is built and has no effect.
- **Cost:** *(estimate)* what is built; delivery is about 600 lines across `zoraya-abi`, `zoraya-fs-service`,
  `zoraya-fsclient`, `libposix`, `zoraya-rt` and `zosh`.
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

**Recommendation (not a decision): Option A**, and the delivery as its own part (call it 10f-2) with this wire, which is
the least that ADR-0337 items 7 and 8 need:

1. A fixed slot `slot::GRANTS` (1006, after `FD0`) in `zoraya_abi::posix::slot`, and a header index in
   `zoraya_abi::native::Startup`: the read end of a channel the shell made, whose write end it keeps per element.
2. A message is `ZGR1`, a count, and that many entries as ADR-0107 encodes them (kind, prefix, the index of a connection
   among the message's handles). An entry for a prefix the table has replaces it: the shell keeps the union per
   directory (item 8's list of names) and sends the whole entry. The prefix of a program's own directory is refused
   (ADR-0341 item 3).
3. The runtime reads the channel only on a miss (`refused`, `NotGranted`), once, then resolves again.
4. The write end closes with the line's job (INV-0337-4).

What this part decided where ADR-0337 is silent (all reversible):

1. **The wire.** `zoraya-proto-term`: `Status { kind: Below<3>, text }` (ordinal 9; `kind` 0 a notice with the key's words
   at the right end, 1 plain text, 2 a field) and `Attention { kind: Below<2> }` (ordinal 10; 0 System+G, 1 System+K);
   `zoraya-proto-gui`: `Attention { window, kind }` (ordinal 47, a client event). `ttyd` ignores both.
2. **The chords.** System+G and System+K join `TAKEN_CHORDS` (13 keys). The compositor sends the focused window
   `Attention`, as it sends `Copy`; no key reaches any client. A client that does not know the message ignores it.
3. **The row.** `term` makes its window one row taller than the grid (80 x 24 grid, 25-row surface) and keeps the last row
   that fits for the status; on a resize the grid is one row less than the window had. The row is drawn in the selection's
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
7. **The result.** Enter, after 500 ms, parses the text and answers to the field: success says `given until PROGRAM ends:
   WORDS`; failure closes the field and says why in one line. The field is not kept open after a failure (nothing typed is
   kept either).
8. **The kill offer.** System+K shows `kill PROGRAM? Enter kills, Escape keeps`; Enter after the same 500 ms guard ends the
   line's job; Escape or the key again keeps it. It is offered in an untrusted room's terminal too (it grants nothing);
   on the serial console neither key does anything.
9. **What is not built, and says so.** The delivery above (the `GRANTS` channel and `narrow` with a list). Also not built:
   the report of a write refused inside a read-only entry (`EACCES`), the limit of 16 restarting after a grant (there is
   no grant to restart it), `jobs` marking a run-time grant, the network refusal report and hint of ADR-0224 item 5
   (`NotGranted` has no report kind yet; stage 8's owners did not build it either), a GUI script for the scenario of
   "What the person sees", and the native programs' runtime (`user/src/namespace.rs` still reports once per path, not per
   path and access).
10. **A refusal is counted per path, access and element**; `libposix` reports a read and a write of the same path as two
    reports. The hint after a job and the row use the same filter; a path that does not pass is `a path that cannot be
    shown` in both.

## Consequences

- **Positive.** A refusal appears on the window's row as it happens, worded by the shell's own lookup, and a hostile report
  cannot put anything on it; the hint after a job no longer prints a path as it came; the kill offer exists in a terminal
  whose program has the keys; the grant line's input, guard, parse and checks are reviewed before the delivery is built.
- **Negative.** The grant line gives nothing yet; a person who presses the key and types a grant is told to start the
  program again with it. The window is one row taller. `Status` and `Attention` are new ordinals. The row's text is the
  shell's only, so a program cannot show a message there even in the way a title could be misused.
- **Invariants of ADR-0337.** INV-0337-1 to -3 and -5 to -7 are checked as their Status column says (partly); INV-0337-4
  waits for the delivery (ADR-0337's open-question note).
- **Follow-up.** The delivery (recommendation, items 1 to 4); `narrow` with a list (also ADR-0391 item 8's second-file case);
  the GUI script; the net report; `jobs`' marking; a fuzz target for `term`'s input with the row compared before and after.

## Invariants

- **INV-0395-1.** The status row's cells change only by the shell's `Status` message, and what `term` draws of its text
  has no control, bidirectional or zero-width character and no character of other than one column. *Check:*
  `cargo test -p zoraya-term` (`tests_status`).
- **INV-0395-2.** No `Input` byte is an attention mark, and no client receives System+G or System+K as a key. *Check:*
  `cargo test -p zoraya-shell paste` and `cargo test -p zoraya-compositor` (the attention chords).
- **INV-0395-3.** A path refused to a POSIX program is reported once per path and access. *Check:*
  `cargo test -p zoraya-posix --test files` (`a_path_is_reported_once_per_access`).


## Open questions

- Whether the row should be one row of the window or part of the compositor's frame (ADR-0337's first open question stands).
- Whether the field should have cursor movement before the delivery is used by people.
- Whether the status row should show a refusal for a line that is not in front when it comes back (`fg`); now it does not.

## Revisit triggers

- A program is found that writes to the row or opens the field.
- A person presses System+G and the field gives nothing for more than one part after this one.
- A GUI test that reads the grid is broken by the taller window.

## Architect's decision
(Left empty for the human.)
