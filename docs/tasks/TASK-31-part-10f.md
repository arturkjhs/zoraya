# TASK-31 part 10f: giving a file to a running program (report)

Branch `work/10f`, from `main` at `514b7d42`. Written by an agent (Sonnet). ADR number used: 0395 (proposed). Nothing is merged,
tagged or pushed to `main`. **Part 10f is not complete:** the status row, the attention keys, the kill offer and the grant line's
input, guard and parse are built and host-tested; **the grant is not delivered to the running program** (the `GRANTS` channel and
`narrow` with a list of names are not built), the network refusal hint is not built, and nothing was played in QEMU by a script of
its own. Read "Gaps" first.

## Done (each step is a commit)

1. **`zoraya-proto-term`:** `Status { kind, text }` (ordinal 9) and `Attention { kind }` (ordinal 10), with round-trip, totality and
   bound tests.
2. **`zoraya-shell`** (new, pure, host-tested): `refusal.rs` (the path filter of ADR-0337 item 2, the three cases, the words of the
   row, the count per path, access and element, the hint's path), `grantline.rs` (the field with its 500 ms guard, the kill offer, the
   literal-word parser `parse_give`, completion), and in `tty_host.rs` the host's status row, `attention`, `refusal`, `gave`,
   `Scope` (first terminal, untrusted room, console), the `Effect`s `Status`, `Give`, `Kill` and `Complete`, and the row's clearing
   when the line ends, stops or changes. `paste.rs` gains the attention mark (bytes 0xC0 and 0xC1 in the key stream; `Input` never
   carries them).
3. **`term`, compositor, `zosh`:** `zoraya_term::StatusRow` and the window's extra row (the grid keeps its size; one more row below it);
   `gui::Attention` (ordinal 47) and System+G and System+K in `TAKEN_CHORDS`; in `zosh` the live reading of each element's report
   channel, the row's words by the shell's own lookup, the attention marks and the field's keys taken before the program's, the
   kill offer's Enter ending the line's job, the grant line's answer, and the hint after a job printed through the filter.
4. **`libposix`:** a refusal is reported once per path **and access**.
5. **Docs:** proposed **ADR-0395** (INV-0395-1 to -3; Total 836), statuses of INV-0337-1 to -3 and -5 to -7 (checked, "partly" where
   it says so), a note in ADR-0337's open questions for INV-0337-4 (rule 6), `docs/design.md`, `docs/backlog.md`,
   `docs/architect-queue.md`.

## Gaps (stated, not hidden)

- **The grant line gives nothing.** Enter parses the text, checks it against what the shell holds, and answers
  `+r ~/other.txt: cannot be given to a running program yet; start it with the grant instead`. Needed: `slot::GRANTS`, a native
  `Startup` index, the runtime's read on a miss, `narrow` with a list of names (ADR-0337 items 7 and 8; the same list closes 10d's
  second-file refusal), the per-element write end closed with the job, `jobs`' mark. The wire is written in ADR-0395's
  recommendation. INV-0337-4 stays `not yet applicable (stage 10)`.
- **The network refusal report and hint** (ADR-0224 item 5): not built (no report kind for `NotGranted`).
- **A write refused inside a read-only entry** is not reported; native programs still report once per path.
- **No QEMU script** for "What the person sees"; the live reading in `zosh` and the row in `term` are checked by the build and the host
  tests of the parts, and by `make test-changed` where it passed (below).
- **The window is one row taller.** Programs see the same grid. A hand check of the look is the architect's.
- **No mutation run** for `zoraya-shell` (`make mutants-host --in-diff`): the other lane was busy with `make test-changed`; it is the
  dispatcher's to schedule.

## Commands run in this session

| Command | Result |
|---|---|
| `cargo test -p zoraya-proto-term`, `-p zoraya-shell`, `-p zoraya-term`, `-p zoraya-compositor`, `-p zoraya-posix --test files` | pass |
| `cargo build -p zoraya-user --bin term --bin zosh --target aarch64-unknown-none --profile user` | builds |
| `make check` (after step 3, and before the last commit) | exit 0 |
| `make adr-check` | 0 errors |
| `make test-changed` | `make check` part and the host tests (38 test binaries) passed; **the QEMU steps did not run**: the image build stopped at `dash-0.5.13.5.tar.gz is missing: run 'make fetch-ports'` (this worktree has no ports cache; `fetch-ports` downloads). So `term`'s taller window and `zosh`'s live reading were never run in a guest |

## Hand checks for the architect (what the part builds, in a desktop terminal)

Start a desktop session (`make run-gui`), open a terminal (System+Enter).

1. **The row and the refusal.** Type `vi ~/notes.txt`, then `:e ~/other.txt`. Below the grid the window's row says
   `vi asked for ~/other.txt, which does not exist` (or `vi was not given ~/other.txt (to read)  System+G: give` if the file exists).
   `:q` ends `vi`; the row clears; the hint after the job names the grant.
2. **The kill offer.** In `vi ~/notes.txt` press System+K: the row says `kill vi? Enter kills, Escape keeps`. Escape keeps; System+K and
   Enter (after half a second) ends it.
3. **The grant line, as far as it goes.** In `vi`, press System+G: `give vi: `. Type `+r ~/other.txt`, Enter (after half a second): the
   row says it cannot be given yet. Type `+r $HOME/x` or `+r /etc/passwd` and Enter: the row says why nothing is given. Keys typed
   while the field is open never reach `vi`.
4. **Hostile report.** Not reachable by hand without a program that writes to the report channel; `cargo test -p zoraya-shell
   status_tests` plays the ADR's list (ESC, space, U+202E, a newline, `;`, Hebrew, mixed Latin and Cyrillic, 4096 bytes).

The ADR's scenario (`:e!` showing the file) cannot be run until the delivery exists.

## For the architect

Proposed **ADR-0395** (Option A: what is built, the delivery as the next part with the written wire). The delivery part also owns
`narrow`'s list. Recommend Option A.
