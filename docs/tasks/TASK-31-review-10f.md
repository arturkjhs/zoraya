# TASK-31: review of part 10f, the status row, the grant line, System+G and System+K (branch `work/10f`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff 514b7d42...origin/work/10f` at `945c088b` (the merge base with `origin/main` is `514b7d42`): `zoraya-abi`
(`Grants`, `NetReport`, `slot::GRANTS`), `zoraya-proto-term`, `zoraya-proto-gui`, `zoraya-compositor`, `zoraya-keymap`,
`zoraya-term` (`status.rs`), `zoraya-shell` (`refusal.rs`, `grantline.rs`, `delivery.rs`, `tty_host.rs`, `paste.rs`, the
tests), `zoraya-proto-file` (`NarrowNames`), `zoraya-fs-service`, `zoraya-fsclient`, `zoraya-posix` (`process.rs`,
`net.rs`, `ffi.rs`), `user/src/bin/term.rs`, `user/src/bin/zosh.rs`, `user/src/posix_child.rs`, `user/src/files.rs`,
`tools/test-runner/src/gui.rs`, `tests/gui/posix-grant.script`, `tools/mutants/shell-10f.toml`, ADR-0395 (proposed), the
rule-6 note on ADR-0337, `docs/adr/invariants.md`, `docs/adr/index.md`, `docs/design.md`, `docs/backlog.md`,
`docs/architect-queue.md`. Read against ADR-0337 (in full), ADR-0224 item 5, ADR-0331 to ADR-0342 (ADR-0341 item 3 in
particular), ADR-0387 item 4, plan row 10f, the stage's "Trust-boundary focus", and the part's report, whose claims were
re-run where cheap, not taken. No QEMU was run (the GUI script's result at four sizes is the implementer's).

## What was run

| Command | Result |
|---|---|
| `cargo test -p zoraya-shell --lib` | 246 pass |
| `cargo test -p zoraya-term --lib` | 189 pass, 1 ignored |
| `cargo test -p zoraya-abi`, `-p zoraya-compositor`, `-p zoraya-fs-service`, `-p zoraya-proto-file`, `-p zoraya-proto-term` | all pass (84; 202 + 2 ignored; 8 + 72 + 3; 9; 10) |
| `cargo test -p zoraya-posix --test files --test net` | 64 + 18 pass |
| A scratch test in `zoraya-fs-service/tests/service.rs` (removed): a list `[("a.txt", READ\|RENAME\|CREATE\|REMOVE\|CLONE), ("b.txt", READ\|CLONE)]`, then `Remove { name: "b.txt" }` on the narrowed connection | **`OK`, and `b.txt` is gone**: finding 2 |
| `make check` (on this branch, before the commit) | exit 0 |
| `grep -c '^\| INV-' docs/adr/invariants.md` | 839, equal to the Total |

Hand mutations, each applied alone and reverted (a `python3 -I` script outside the tree; the crate's tests were run):

| Mutation | Result |
|---|---|
| `Feed::input` keeps 0xC0/0xC1 | killed |
| `StatusRow` keeps bidi/zero-width; keeps control characters; keeps wide characters | killed, killed, killed |
| fs-service: no per-name rights check on `open`; `NarrowNames` rights need not cover the list | killed, killed |
| `grantable` accepts `/` | killed |
| **`Grants::decode` accepts an entry kind other than directory** | **survived** (INV-0395-5's "adds only directory entries" is untested) |
| libposix: no retry after `take_grants`; one report per path regardless of access | killed, killed |
| **libposix: `reports_sent` not reset after a grant** | **survived** (ADR-0337 item 1's "the limit restarts after a grant" is untested) |
| libposix: no `NetReport` on a `NotGranted` connect | killed |
| `program_name` keeps non-ASCII; `refusal` shown for a line not in front; attention on the console; `$` literal; `~name` accepted; `held.path` not checked; dot-only names shown; a file below a held whole accepted; paste newlines kept | all killed |
| **`Pending` without the `MAX_PENDING` bound** | **survived** (the bound on what a program can make the row redraw is untested) |

`tools/mutants/shell-10f.toml`'s six entries were not run here (each is close to one of the killed mutations above).

## Findings

### 1. error: a `+r DIR` on the grant line gives the program write rights over the whole directory

- **Where:** `crates/zoraya-shell/src/delivery.rs:94-100` (`for (p, _, r) in next.names ... all |= r`), asserted by
  `crates/zoraya-shell/src/status_tests.rs:534` (`a_directory_is_whole_and_takes_the_names_given_in_it` expects
  `Spec::Whole { prefix: "/home/u", rights: CHANGE | READ }`); opened by `user/src/bin/zosh.rs` `give()` through
  `open_entry`, which asks the shell's root for those rights.
- **What:** when a directory is given whole, the rights of every name the program held in it are ORed into the rights of
  the *whole* directory. A connection cannot carry per-name rights for a whole directory, so a name held with `CHANGE`
  makes the whole tree writable.
- **Scenario:** `vi notes.txt` in the first terminal (vi holds `~` narrowed to `notes.txt` with `CHANGE`). System+G,
  `+r ~`, Enter. The row says `given until vi ends: +r ~`; vi's `~` entry is now a whole connection with
  `READ|WRITE|SYNC|CLONE`, and `:w! ~/.profile` (or any existing file anywhere under `~`, `.ssh/authorized_keys`
  included) overwrites it. This breaks INV-0337-3 ("what is given is exactly what the line's text parses to") and
  ADR-0337 driver 2 (the row shows a grant that is not the one made); CP-02 (rights only narrow) as the ADR cites it.
- **Fix:** never widen a whole beyond what was typed. When the program holds names in the directory with rights the
  typed word does not give (`r & !want != 0`), refuse with the same `start it again with the grant` text (or give
  `want` and keep the names in a separate narrowed entry, which the table cannot express at one prefix, so refusing is
  the honest choice). Change the test to assert the refusal and add a case `+w DIR` over a name held `READ`.

### 2. error: a connection narrowed to a list enforces per-name rights on `open` only; remove, rename, link and publish use the union

- **Where:** `crates/zoraya-fs-service/src/lib.rs:1438-1450` (`Remove`, `MakeDirectory`), `:1507-1512` (`MakeLink`),
  `rename` at `:2571` and `publish` at `:2601` (`token_may_name`): each checks `may_name` and the connection's overall
  right (`has(i, REMOVE)` and so on), never `rights_on(i, name)`. The new check at `:1903-1909` covers `open` alone.
- **What:** the connection's overall rights are the union of the list's (`zosh` `give()` sends `rights: all`, the fold
  of every name's). A right that one name holds is therefore usable on every other name of the list.
- **Scenario (run, see the table):** a list `a.txt` with `READ|RENAME|CREATE|REMOVE` (an `Access::Rename` or
  `Access::Remove` plan entry: `mv`, `rm`, `sed -i` hold these, `crates/zoraya-shell/src/grants.rs:54-57`) and `b.txt`
  with `READ`. `Remove("b.txt")` answers `OK` and the file is gone; `Rename` and a `Publish` with `REPLACE` onto
  `b.txt` follow the same path. In the shell's flow: a POSIX program started with a narrowed `rename`/`remove`
  entry is given `+r` of a second file of that directory on the grant line, and can delete or replace it.
  INV-0395-6 ("opens each with at most its own rights, nothing else, and cannot ... widen") is claimed `checked` and is
  false for every request but `open`.
- **Fix:** in every request that names something on a narrowed connection (`Remove`, `MakeDirectory`, `MakeLink`, the
  source of `Rename`, the target name through the token in `Rename` and `Publish`), require the right the request needs
  in `rights_on(i, name)` (for the token, `rights_on(target, name)`), not only in the connection's. Add the scratch test
  above (and `Rename`, `Publish` with `REPLACE`) to `a_connection_narrowed_to_a_list...`.

### 3. error (merge condition): with ADR-0341's own directory (10g), the grant line replaces it, and `grantable()` has no `/data` rule

- **Where:** `crates/zoraya-abi/src/posix.rs:645` (`grantable`: `/`, `/dev`, `/proc`, `/tmp` only),
  `user/src/bin/zosh.rs:1789` (`held_of`: a `Directory` entry the merged plan does not list is recorded as held whole
  with `READ`), `delivery.rs:82-120` (a whole held with fewer rights is widened and re-sent).
- **What:** on `origin/work/10g` the own-directory entry is a `Directory` entry at the *view* prefix (`~/.ssh` of the
  user database) connected to `/data/home/NAME`; it is not in `grants::merge(plan)` as such, so `held_of` records it as
  `READ` whatever its rights. ADR-0341 item 3 (accepted): "The own-directory entry is never merged: a `+r` or `+w` of
  the line on the same prefix is refused." After both parts merge, `+w ~/.ssh`-of-the-view (or `+r` when the entry is
  `tree`, since `READ` is recorded) on the grant line is a widening of a held whole: the shell opens that prefix *from
  its own root* (the person's real directory, not `/data/home/ssh`) and libposix's `install_grants` replaces the
  entry's handle. The program's own directory is silently swapped for another one. ADR-0395's "never replaces" and
  INV-0395-4 do not hold for that entry. The backlog and the report name the `/data` rule as a follow-up; it is not
  only `/data`: `held_of`'s `READ` default is the hole.
- **Scenario:** after the merge, `ssh host` runs; System+G, `+w /home/.ssh`, Enter (the first terminal holds `/`):
  ssh's known_hosts and keys now come from `/home/.ssh` of the volume, and what it writes goes there.
- **Fix:** whichever of 10f and 10g merges second: mark the own-directory entry in `Held` as untouchable (refuse any
  grant at, above-and-replacing, or below it), refuse `/data` and below in `grantable()` (and add it to INV-0395-5's
  text and test), and make `held_of` refuse to deliver to a prefix whose rights it cannot find instead of assuming
  `READ`.

### 4. warning: reading the report channels while a line runs has no bound per wake; a program can starve the key loop

- **Where:** `user/src/bin/zosh.rs:3450` (`read_reports`: `loop { channel_read ... }` until `WouldBlock`), called on
  every wake of `wait_foreground`, before `attention_keys`.
- **What:** a program writing reports to its `REPORT` end as fast as it can (it need not use libposix's limit of 16; it
  can write the channel directly) keeps the shell in the loop as long as it keeps up with the reader. Each message
  costs the shell two linear scans (`live`, up to 256, and `Pending`, up to 64). Keys, System+K and Ctrl-C are read
  only after the loop.
- **Scenario:** a POSIX program in a loop `channel_write(slot::REPORT, ZPR1 of "/x")`; the person's System+K or
  Ctrl-C is not processed while the shell drains. Before 10f the channel was read only after the job.
- **Fix:** read at most N messages (say 64) per wake and go on; the level-triggered subscription wakes again. A test
  in `zoraya-shell` cannot reach this (it is `zosh`), so note it in the report if not tested.

### 5. warning: a delivery of more than one message can be partly made while the row says it failed

- **Where:** `user/src/bin/zosh.rs` `give()`, `while !files.is_empty()` at `:3683` to `:3707`.
- **What:** specs are sent in chunks of `MAX_GRANTED` (4). If chunk 2's `Grants::encode` or `channel_write` fails,
  chunk 1 is already installed by the program, but the function returns `Err`, the row says why nothing was given, and
  `g.held` keeps the old record. The next grant is computed from a record that does not match the program's table.
- **Scenario:** a grant line with five paths in five directories, the last a prefix the decoder refuses (`+r /dev/x`)
  or a program whose end closes between the writes.
- **Fix:** encode every chunk before writing any (all or nothing for the encoding), and on a write failure after the
  first say what was given (`given until vi ends: ...` for the sent part) and update `held` with the sent specs; or
  refuse a line that needs more than one message.

### 6. warning: a whole directory given above a narrower entry the program holds is not what the row says

- **Where:** `crates/zoraya-shell/src/delivery.rs:82-120` (a directory grant checks only wholes *above* it and names
  *at* it).
- **What:** the resolver takes the longest prefix (`crates/zoraya-posix/src/path.rs`), so a deeper entry the program
  holds, narrowed to names or whole with fewer rights, still decides every path below it after the new whole arrives.
- **Scenario:** `vi ~/a/n.txt` (vi holds `~/a` narrowed to `n.txt`), then `+r ~`: the row says `given until vi ends:
  +r ~`, and `:e ~/a/m.txt` is refused. Or a program holding `~/a` whole `READ`, given `+w ~`: writes under `~/a` are
  refused. ADR-0337 driver 2 (never shown a grant that did not happen).
- **Fix:** include the deeper entries in the specs (re-send each deeper entry with the union of its rights and the new
  whole's, or drop a deeper names entry by sending the whole for it too), or refuse with the "start it again" text.

### 7. warning: the runner's `status -` passes when the row is not there, and its search can take a program's row

- **Where:** `tools/test-runner/src/gui.rs:559-586` (`status_at`), `:964-1021` (`wait_status` with `-`);
  `tests/gui/posix-grant.script:18` and `:44`.
- **What:** `status_at` returns `""` when no cell near the bottom has the selection's ground. An empty status row is
  drawn on the window's background, so an empty row, a row that is not drawn at all (`status_fits` false, a term
  without the row) and a window that is not there all read `""`: `status -` cannot fail. The comment's premise
  ("the selection's ground, which no program's cell is") is false: a program can paint that exact colour with
  `CSI 48;2;R;G;B m`, and the person's own mouse selection uses it; the search from `rows - 4` upward returns the first
  such row, which can be the program's.
- **Scenario:** the two `status -` steps after `vi notes.txt` would pass with `term` drawing no status row at all.
- **Fix:** find the row by geometry (the window's height from the compositor, or the terminal's reported `Size` + 1)
  and require it to be inside the window; make `-` assert that the row exists and is the background; search from the
  bottom up. Add a host test with a program row on the selection ground above an empty status row.

### 8. warning: the lookup behind the row's words opens the reported path

- **Where:** `user/src/bin/zosh.rs:255` (`LookUp::exists`: `self.0.open(path, 0).and_then(|f| f.stat())`).
- **What:** INV-0337-6 says the lookup "opens nothing and follows no link object". It does open (with no rights),
  synchronously, in the key loop, for every distinct path a program reports (up to `MAX_PENDING`, 64, per front line).
  An open has a cost the program chooses (a path on a slow or stalled service blocks the shell) and is a request a
  service sees on the shell's behalf for a name the program chose.
- **Fix:** a stat by name on the parent (no connection made), or reword INV-0337-6's Status to say what is done; at
  least move it out of the per-message path (classify only the latest refusal when the row is drawn, which it already
  is, but bound it).

### 9. warning: three claimed checks are not tested (surviving mutations)

- **Where:** `crates/zoraya-abi/src/posix.rs:678` (`head[0] != EntryKind::Directory`), `crates/zoraya-posix/src/process.rs:805`
  (`reports_sent = 0` after a grant), `crates/zoraya-shell/src/refusal.rs:181` and `:207` (`MAX_PENDING`).
- **What:** each mutation in the table above survives the crate's tests: INV-0395-5's "adds only directory entries",
  ADR-0337 item 1's "the limit restarts after a grant" (ADR-0395 item 9 states it as built) and the bound on the row's
  redraws a hostile program can cause.
- **Fix:** a `Grants::decode` case with kind 1..4; a libposix test that reports 16 paths, receives a grant, and sees a
  17th reported; a `Pending` test that the 65th distinct report returns `false` and is counted in `more()`. Add the three
  to `tools/mutants/shell-10f.toml`.

### 10. warning: documents say the delivery is not built

- **Where:** `docs/adr/index.md:198` (ADR-0395's row: "the delivery (`GRANTS` and `narrow` with a list) is the next
  part"); `docs/adr/0395-...md:122` ("INV-0337-4 waits for the delivery"), and `:24` ("three more ordinals": `Status` 9,
  `Attention` 10, gui `Attention` 47 and `NarrowNames` 41 are four, plus slot 1006); `user/src/bin/zosh.rs:269`
  (`Holds::net`: "until the `GRANTS` channel exists").
- **Fix:** bring the three into line with what is built (the ADR's own Recommendation and the invariants' Status are
  right).

### 11. note: the `GRANTS` channel is authenticated by who holds its other end only

- **Where:** `crates/zoraya-posix/src/process.rs:790` (`take_grants`), `user/src/bin/zosh.rs` (`Given.tx`).
- **What:** what the program reads on slot 1006 can only have been written by the holder of the other end, which is
  the shell alone; the program cannot forge a message to itself, and if it could, it could only install handles it
  already holds. The read is bounded (it stops at the first error, and the shell writes only on Enter), and each
  `open`/`stat` retries once. Sound. Two loose ends: the program can *write* on its end (the channel is two-way), and
  those messages queue at the shell's `tx`, which nobody reads, up to the kernel's per-channel bound, for the life of
  the job; and only `open` and `stat` retry (`access`, `chdir`, `opendir` paths not through `open`, `rename`, `execve`
  fail once after a grant). Suggest giving the program its end without the write right if the kernel allows, and naming
  the calls that retry in ADR-0395 item 3.

### 12. note: a program can still put a refusal of its choosing on the row, and paint a look-alike row in the grid

- **What:** the row's words are the shell's, but the *path* or *host* is whatever the program reports
  (`refusal.rs`'s own doc says so; ADR-0337 accepts it). Combined with item 5 (the row names the element), a program
  can ask for `~/.ssh/id_ed25519` and be shown `vi was not given ~/.ssh/id_ed25519 (to read)  System+G: give`. A
  program can also draw, in the grid's last row, a line in the selection's colours that looks like the real row (a
  fake `give vi:` field, or `given until vi ends: ...`), and when the real row is empty it is drawn on the window's
  background, so the fake sits where a person expects the real one. The only tell is one row of position. ADR-0337's
  first open question (row in the window vs the compositor's frame) stands; suggest drawing the real row always on a
  ground and edge no grid cell can make (a frame line above it), and recording the look-alike in ADR-0395's
  consequences.

### 13. note: small things

- `grantline.rs` `keys`: an Escape followed by another key in the same read (`ESC x`) is taken as Alt+x and skipped:
  the field neither closes nor takes `x`.
- `give()`'s errors from `open_entry` are `zoraya_command::error` texts, written for the screen with a `zosh:` prefix and
  a hint line; on the row the newline becomes a space and the text is cut at the row.
- ADR-0395 item 9 says a grant at a system directory is refused; `+r /` reaches `Grants::encode`, which refuses, and
  the person reads `cannot be sent`, not the reason.

## What was checked and found sound

- **No program output reaches the row or the field.** `term` decodes `Status` only on the shell's channel and draws it
  outside the grid; nothing hands a child the shell's terminal end. Every row text is the shell's: `refusal::notice`
  from a path that passed `shown_path` (normalised by the shell, `[A-Za-z0-9._-+~@,=/]` only, 60 bytes, no dot-only
  name) or a host that passed `shown_target`, the program name through `program_name` (printable ASCII, 32), the field
  from the person's keys and pastes; `term` cleans again (controls, bidi and zero-width, non-one-column). ESC, U+202E,
  newline, `;`, Hebrew, mixed scripts, 4096 bytes and invalid UTF-8 all end as `a path that cannot be shown` or `?`.
  The hint after a job goes through the same filter. `NetReport::decode` admits only host text.
- **System+G and System+K reach no client as keys.** They are in `TAKEN_CHORDS`; the compositor sends `Attention` to
  the focused client only (host test); `Input` drops 0xC0/0xC1 and no UTF-8 encodes them; a paste is text and never
  enters the key stream as marks; the shell pushes the mark only on the terminal's `Attention` and only while a line is
  in front; the prompt drops it.
- **The field.** Opens only by `attention`, only for the line in front, not on the console, not for a grant in an
  untrusted room; while open every key and paste is its (`Host::keys`, `paste`, `attention_keys` in `zosh`); 500 ms
  guard; closes with the line's end, stop or background (`clear_row`), so a reused job number never inherits it;
  the element is fixed at opening and a later report does not move it.
- **One program, stored nowhere.** `granted` and `elements` are pushed in the same order for every element; the write
  end is per element and closed in `Job::drop`; a later job gets a new channel; handles, not pids, so pid reuse does
  not apply. The typed path: literal words only (`$`, backquote, patterns, quotes refused), `~` and `~/` only, `..`
  normalised by `join`, checked against the shell's table with the access; a link is not followed by the shell's open.
- **libposix.** `install_grants` adds directory entries only, bounded at 64, replaces a same-prefix directory entry,
  closes unused handles; the resolver takes the longest prefix wherever it is, so order does not matter. The decoder's
  bit set is bounded (`index < 64`).
- **`narrow` with a list** (for `open`): names are `Name`s (no `/`, `.`, `..`, empty, control), no duplicates, at most
  16, each within the parent's rights and the new connection's; a narrowed list narrows only; listing refused.
- **Bookkeeping.** 839 invariant rows equal the Total; ADR-0395 is the only new ADR number and 0390 to 0394 are not
  reused; `make check` passes.

## Counts

3 errors (findings 1 to 3), 7 warnings (4 to 10), 3 notes (11 to 13).

## Dispositions (the implementation agent, 2026-11-19)

Each finding was checked against `work/10f` after the merge of `origin/main` (which carries part 10g).

| # | Finding | Disposition |
|---|---|---|
| 1 | `+r DIR` widens to write over the tree | Fixed in `178e00de`: `Held::give` refuses when names the program holds there have rights the typed word lacks (`delivery.rs`, test in `status_tests.rs`). |
| 2 | A narrowed list's per-name rights hold for `open` only | Fixed in `95d6ee8e`: `Remove`, `MakeDirectory`, `MakeLink`, `Rename` and `Publish` check `has_on` / `token_has_on` (`zoraya-fs-service`, tests in `tests/service.rs`). |
| 3 | The own directory of ADR-0341 can be replaced after 10g | Fixed: `TableKind::Own` and `Held::fixed` (an entry the plan does not list is never assumed `READ`; nothing is given at, above or below it), in `c242a041`; `grantable()` does not refuse `/data` (a grant of `/data/x` is a valid entry in the `zoraya-abi` test and ADR-0341 item 3 does not say `Grants` refuses it): the own directory is protected by `Held::fixed`, not by that list. Recorded as not done in `docs/backlog.md`. |
| 4 | Report channels unbounded per wake | Fixed in `178e00de` (bound per wake). |
| 5 | A delivery of several messages partly made | Fixed in `178e00de` (encode all first; all or nothing). |
| 6 | A whole directory above a narrower entry | Fixed in `178e00de` (refused with the "narrower place" text). |
| 7 | The runner's `status -` | Fixed in `178e00de` (`gui.rs`). |
| 8 | The lookup opens the reported path | Fixed in `178e00de`. |
| 9 | Three claimed checks untested | Mutations killed in `178e00de`; the `zoraya-shell` mutation run is reported in the part report. |
| 10 | Documents say the delivery is not built | Corrected in `178e00de` and `945c088b`. |
| 11 to 13 | Notes | Recorded in `docs/backlog.md` (the part 10f leftovers), not fixed. |
