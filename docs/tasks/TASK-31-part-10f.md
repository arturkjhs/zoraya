# TASK-31 part 10f: giving a file to a running program (report)

Branch `work/10f`, from `main` at `514b7d42`. Written by an agent (Sonnet). ADR number used: 0395 (proposed). Nothing is merged,
tagged or pushed to `main`. **The part is complete for POSIX programs:** a file or a directory the shell holds is given to the one
running program on the grant line (System+G), the row shows its refusals, System+K offers a kill, and `tests/gui/posix-grant.script`
plays ADR-0337's scenario in a window at four sizes. A `+net` word and a native program are refused on the row (gaps below).

## Done (each step is a commit)

1. **`zoraya-proto-term`:** `Status` and `Attention`; **`zoraya-proto-gui`:** `Attention`; System+G and System+K in the compositor.
2. **`zoraya-shell`** (pure, host-tested): `refusal.rs` (the path and host filters, the three cases, the count), `grantline.rs`
   (the field, the 500 ms guard, `parse_give`), `delivery.rs` (what a running program holds and what a grant makes of it), the host's
   row, `Scope`, the attention mark in the key stream.
3. **`term`, `zosh`:** the status row below the grid (`StatusRow`); live reading of each element's report channel; the row's words by the
   shell's own lookup; the kill offer; the hint after a job through the filter; **the delivery**: the write end of each POSIX element's
   `GRANTS` channel, the entries opened from the shell's own root and sent.
4. **`libposix`:** `slot::GRANTS` (1006) read on a refusal (outside the table or `EACCES` from the service), one retry; a refusal reported
   once per path and access; a `NotGranted` connect or resolve reported as a `NetReport` (ADR-0224 item 5).
5. **`narrow` with a list of names** (file ordinal 41; `zoraya-proto-file`, `zoraya-fs-service`, `zoraya-fsclient`): each name with its rights.
6. **The runner:** a `status TEXT` step that finds the status row by its ground (the window may be shorter than `grid_for` assumes: at
   1920x1200 the grid has 21 rows, vi's message line is on row 20).
7. **Docs, tests, mutations:** proposed **ADR-0395** (INV-0395-1 to -6; Total 839), the Status of INV-0337-1 to -7 (checked; the gaps are named in
   each row), `docs/design.md`, `docs/backlog.md`, `docs/architect-queue.md`, `tools/mutants/shell-10f.toml` (6 mutations, not run).

## Defects the real runs found and fixed

- The row's text from an answer to the grant line stayed after the next field closed: an answer is said once now.
- The runner read the status row at `grid_for`'s index; the compositor fits a window above the dock and the grid then has fewer rows.

## Gaps (stated)

- `+net` on the grant line and a native program (no `Startup` index) are refused on the row; a write refused inside a read-only entry is reported
  only when the service answers `EACCES`; native programs still report once per path; datagram and listen refusals are not reported.
- A grant below a directory the program holds whole, or at a system directory, is refused (it would shadow or replace it): start the program again
  with the grant. `grantable()` in `zoraya-abi` has no `/data` rule yet (ADR-0341 item 3): 10g (ADR-0393) merges first; add it when 10f rebases.
- The planner still refuses two files of one directory at start (ADR-0391 item 8); only the run-time delivery uses the list.
- The write end closing with the job is `Job`'s `Drop`, not tested; a pipeline of two programs with a grant to one is not scripted; no fuzz target of
  `term`'s input (none is named by ADR-0337 or plan.md; deterministic tests stand in); no mutation run (the dispatcher's).

## Commands run in this session

| Command | Result |
|---|---|
| `cargo test -p zoraya-proto-term`, `-p zoraya-shell`, `-p zoraya-term`, `-p zoraya-compositor`, `-p zoraya-posix --test files` | pass |
| `cargo build -p zoraya-user --bin term --bin zosh --target aarch64-unknown-none --profile user` | builds |
| `make check` (after step 3, and before the last commit) | exit 0 |
| `make adr-check` | 0 errors |
| `make fetch-ports`, then `make test-tour-terminal POSIX_TERM_SCRIPTS=tests/gui/posix-grant.script` | PASS, 59 steps, at sizes 1, 2, 1600x1000 and 1920x1200 under HVF |
| `make test-changed` | see the head of the final report |

## Hand checks for the architect (exact commands, in a desktop terminal window)

Start the desktop (`make run-gui`) and open a terminal (System+Enter), then:

1. `echo secret > other.txt; echo mine > notes.txt; vi notes.txt`
2. In `vi`: `:e! other.txt` Enter. Below the grid the row says `vi was not given ~/other.txt (to read)` with `System+G: give` at its right end.
3. Press System+G. The row becomes `give vi: `. Type `+r other.txt`, wait half a second, press Enter. The row says `given until vi ends: +r ~/other.txt`.
4. In `vi`: `:e! other.txt` Enter: the file (`secret`) is there. `:q!` Enter, then `jobs` shows nothing left.
5. `vi notes.txt`, `:e! other.txt`: refused again (nothing was stored). Press System+G, type `$HOME/x`, Enter: the row says why nothing is given; type `+r /nowhere`: `does not exist`.
6. Press System+K: `kill vi? Enter kills, Escape keeps`. Escape keeps it; System+K, wait half a second, Enter ends the line.
7. Type `curl http://example.org` (no `+net`): the row, while it runs, and the hint after it say `curl was not given example.org:80 (to reach)` and `+net example.org:80 curl ...`.
8. In a room's own terminal (`room new work +r ~`, `room work`) the line can give nothing new; in a shell that entered a room it gives what that shell holds.

## For the architect

Proposed **ADR-0395** (Option A as built). Recommend Option A. It adds a file-protocol message (`NarrowNames`, ordinal 41), a slot (`GRANTS`, 1006)
and two report messages; none changes an accepted decision.
