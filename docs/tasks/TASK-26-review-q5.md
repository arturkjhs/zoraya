# TASK-26 review of Q5

An independent review by the `opus-review` subagent, which did not write the code, of `git diff 7a39a57...work/q5`.
The code read around the diff: `crates/zoraya-fs/src/volume.rs` (the "Storage limits" block, `unname`,
`delete_object`, `create_obj`, `rename`, `resize`, `write`, `create_unnamed`, `publish`, `close`, `free_orphans`,
`mutate`), `check.rs`, `format.rs`, `tests/limits.rs`, `crates/zoraya-proto-file/src/lib.rs`,
`crates/zoraya-fs-service/src/lib.rs` (dispatch of 36, 38 and 39, `may_name`, `set_limit`, `get_limit`),
`crates/zoraya-shell/src/grants.rs` (`Access::rights`), `crates/zoraya-posix/src/errno.rs`, `tests/posix/t_posix.c`
(`diskfill`) and `tools/zoraya-mkfs/src`. It was checked against ADR-0265 (rules 1 to 4 and 7, INV-0265-1 and -2),
proposed ADR-0290, ADR-0263 items 1, 4 and 5, ADR-0153's grant words, ADR-0268's disk-filler row and plan row Q5.

The reviewer ran these commands on an export of `work/q5` (outside the repository, with its own target directory):
- `cargo test -p zoraya-fs -p zoraya-fsclient -p zoraya-fs-service -p zoraya-mkfs -p zoraya-command
  -p zoraya-proto-file -p zoraya-chooser`: all pass.
- `cargo test -p zoraya-posix`: **one test fails** (finding 1).
- `cargo clippy --all-targets -D warnings` on the seven touched crates: clean.
- Three scratch tests, added to the exported copy of `tests/limits.rs` only and not committed: they reproduce
  findings 3 and 4 and measure finding 5.

There was no QEMU run. Dispositions are left to the main model.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `crates/zoraya-posix/src/errno.rs:247-251` | `cargo test -p zoraya-posix` fails. The test still expects status 16 to be unknown (`EIO`). The report says this crate passes. |
| 2 | error | `crates/zoraya-fs-service/src/lib.rs:1003-1008`, `:1974-1984`; `crates/zoraya-shell/src/grants.rs:45-47` | A connection narrowed to one name with create's rights ("create or replace NAME") can remove or raise the storage limit of NAME. No test covers this. |
| 3 | error | `crates/zoraya-fs/src/volume.rs:945`, `:966`, `:866`; `check.rs:526`, `:934` | `subtree_used` saturates. After a new limit on a tree whose charge passes `u64::MAX`, a later `unlink` underflows `used`, and the volume fails with `MetadataCorrupt`. Reproduced. |
| 4 | warning | `crates/zoraya-fs/src/volume.rs:796-799`, `:905-924`; ADR-0290 Option A | When a limited directory is removed, an open unlinked file in it is charged to the unlimited parent and grows without bound. Reproduced. |
| 5 | warning | `crates/zoraya-fs/src/volume.rs:1596` | `set_limit` runs as a freeing request (`mutate(true, ..)`), so new limit records are written into the delete reserve on a full volume. |
| 6 | warning | `crates/zoraya-fs/src/volume.rs:933-972`, `:1611-1614`; ADR-0290 Consequences | Any connection with `WRITE`, including an untrusted room's, can repeat a scan of its whole subtree and of the volume's orphan list, all in one request. ADR-0290 assumes that only the keeper does this. |
| 7 | warning | `crates/zoraya-fs/tests/limits.rs:430-640` | The model and crash tests never keep a handle open across requests, unlink an open file, remove a directory that holds an orphan, crash with orphans present, or change a limit during the run. These are exactly the paths of ADR-0290's new rules. |
| 8 | warning | `crates/zoraya-command/src/lib.rs:487`; ADR-0263 items 1 and 5 | The word for `OVER_QUOTA` is "over its disk limit", but ADR-0263 item 5 says "over quota". The `Disk` field that ADR-0263 item 1 puts in `fsd`'s reply is missing, and neither gap is recorded. |
| 9 | warning | `crates/zoraya-proto-file/src/lib.rs:435-451` | No codec test covers `SetLimit`, `GetLimit` or `LimitReply`, or the new bounds (`remove` 2, status 18, a truncated name). |
| 10 | note | `crates/zoraya-fs-service/src/lib.rs:1010-1013`, `:1986-2007` | `GetLimit` checks no right. A narrowed connection, which may not list its directory, learns how many bytes the whole directory holds. |
| 11 | note | `crates/zoraya-fs/src/check.rs:476-487`, `:892` | `fsck` accepts an orphan that is charged to a removed (orphan) directory, and `repair` keeps it. The engine's `limited_chain` fails on such an orphan, so the next mount's `free_orphans` fails. |
| 12 | note | `crates/zoraya-fs/src/format.rs:292`, `:399` | A version-1 volume is refused as `Corrupt("unknown format version")`. No test covers this, and the message reads as damage, not as a volume to rebuild. |
| 13 | note | `tools/zoraya-mkfs/src/lib.rs:147-157` | The new `set_limit` was inserted under `populate`'s doc comment. `set_limit` now carries both texts, `populate` has none, and the root handle `set_limit` opens is never closed. |
| 14 | note | `crates/zoraya-fs/src/volume.rs:905-924`, `:827-845` | Every `rmdir` walks the volume's whole orphan list. Every growing request walks to the root, and a room can make that walk as deep as its limit allows (one block per directory). |

## Findings

**1. error: `cargo test -p zoraya-posix` fails.** `every_file_status_maps_once_and_only_ok_to_zero` loops over
`0..16` and then asserts `from_file(16) == EIO, "a status the protocol does not have"`. Status 16 is now `OVER_QUOTA`,
which maps to `EDQUOT` (122). The run here printed `left: 122, right: 5` at `errno.rs:251`. The report's "Tests run"
line lists `zoraya-posix` as passing, which is not true of `work/q5`. The loop also never checks 16 and 17 for a
single mapping.
*Fix:* loop over `0..18`, and assert `from_file(18) == EIO`. Then rerun and correct the report.

**2. error: "create or replace NAME" can remove a room's disk limit.** In the service, `SetLimit` checks only
`may_name(i, name)` and `has(i, WRITE)`. ADR-0153's narrowed grants carry `WRITE`:
`Access::Create(name)` is `READ | WRITE | CREATE | SYNC | CLONE` on the parent, narrowed to `name`. So does the
chooser's `Save` ("the folder narrowed to the one name, with create's rights", `docs/design.md`).
*Scenario:* in `home`, the person runs a program designated with an output operand,
`tool --out ~/sandbox/agent-1`, or an application's Save sheet is answered with that name. The program receives
`~/sandbox` narrowed to `agent-1` with `WRITE`. It sends `SetLimit { remove: 1, name: "agent-1" }` and succeeds,
or it sets the limit to `u64::MAX`. The room's limit is gone, and the room's disk filler then fills the person's
volume (INV-0265-3, ADR-0268's disk-filler row).

The grant's words ("create or replace agent-1") do not include "change its disk limit". ADR-0265 rule 4's premise
("a limit is set from above") does not anticipate a narrowed parent connection.

For the record, the untrusted room itself cannot change the limit of its own `+w` directory:
- `Name` is one component, and `RelPath` refuses `..`.
- The room holds the limited directory itself, not its parent.
- The room's shell cannot designate a narrowed connection to a parent outside its grants.

What the room can do is remove or raise limits on subdirectories inside its tree. That only loosens bounds below
its own, which ADR-0265 allows.

*Fix (smallest):* refuse `SetLimit` on a narrowed connection
(`if self.only_of(i).is_some() { return self.denied(i, t) }`), and add an `fsclient` test with `narrow`.
*Fix (better):* a separate `rights::LIMIT` bit. `Access::Tree` carries it for trusted lines, and `Create`, `Change`
and `Save` never carry it. This is an ABI shape, so it needs an ADR, either an amendment proposed beside ADR-0290 or
an open question in ADR-0265 for the architect. The current test only checks a connection without `WRITE`.

**3. error: a saturated subtree sum makes a later delete fail the volume.** `charge_of` of a sparse file at
`MAX_FILE_SIZE` (`i64::MAX`) is `2^63 + 4096`. Two such files under a directory with no limit add up past `u64::MAX`,
and `subtree_used` saturates (`total.saturating_add`). A new limit then records `used = u64::MAX`.

The scratch test does the following: `mkdir a`, then `set_size` of `a/x` and `a/y` to `i64::MAX`, then
`set_limit(root, "a", Some(u64::MAX))`, then `unlink a/x` and `unlink a/y`. The first unlink is `Ok`. The second
underflows in `adjust_chain` (`checked_sub` gives `None`, which goes to `inconsistent()`) and returns `Err(Corrupt)`,
with the failure `Some(MetadataCorrupt)`. The whole volume is now `Failed` because of a delete. This breaks
INV-0265-2 ("a shrinking request never fails for a limit") and ADR-0265 driver 3.

Any trusted program can make such files (`truncate -s 8E`). The room keeper sets limits on existing directories
(ADR-0265 rule 5), and a person's `+w ~/sandbox/agent-1` may already hold such a file. `fsck` sums with the same
saturation (`check.rs:526`, `:934`), so it calls the record clean.
*Fix:* make `subtree_used` use `checked_add` and refuse the new limit with `OverQuota` (or `InvalidArgument`) when
the sum overflows. `fsck` should report an overflow, not saturate. Add the test above to `limits.rs`.

**4. warning: removing a limited directory frees its open orphans from the limit.** `unname` of a directory calls
`repoint_orphans(id, p)`, which charges the orphans to the parent, and then deletes the directory's `K_LIMIT`.

The scratch test sets a limit of 4 blocks on `sandbox/agent`. It opens `sandbox/agent/f`, which gets `OverQuota`
at 8 blocks, then unlinks `f`, runs `rmdir sandbox/agent`, and writes 64 blocks to `f`. That write returned
`Ok(262144)`.

*Scenario:* an agent in a room keeps a temporary file open and unlinked (the usual `tmpfile` idiom) and writes in a
loop. The person sees the disk use and stops it by deleting the room's directory, or by `room drop` followed by
`rm -r`. From that moment the agent's writes are unlimited, and it fills the volume. This is driver 1 of ADR-0290 and
CP-14, the disk filler.

ADR-0290 cites XFS as precedent ("keep the project id in the inode"). In XFS the charge stays with the project,
which is the opposite of what is built here. The behaviour is decided in a proposed ADR, so this is a warning, but
the ADR's text and its precedent disagree.
*Fix:* keep a removed directory's `K_LIMIT` and `K_PARENT` while orphans are charged to it, and drop both when the
last such orphan is freed: in `delete_object`, and on mount after `free_orphans`. Repoint only from directories
without a limit. `fsck` would then accept a limit record on an orphan directory that still has charged orphans.
Alternatively, record the question in ADR-0290 for the architect.

**5. warning: `set_limit` uses the delete reserve.** `Volume::set_limit` calls `mutate(true, false, ..)`, so its
space check leaves out `DELETE_RESERVE`. A new limit inserts a record and rewrites the path to it; lowering a limit
or setting a limit of `None` frees nothing.

On a full volume, a client with `WRITE` on many existing directories can call `set_limit` on each of them. Every new
record is permanent, so they accumulate in the reserve until `unlink` fails with `NoSpace` for everyone, which
breaks ADR-0016 and the "on a full volume" half of INV-0265-2. The scratch test did not exhaust the reserve: 400 new
limits on a full 256-block volume left 66 blocks free, and the following `unlink` was `Ok`. The risk therefore needs
thousands of directories made beforehand. The existing `rename` is also freeing and can grow names, which is
pre-existing.
*Fix:* `mutate(limit.is_none(), false, ..)`, or freeing only when no record is added. Add a test: fill the volume,
then check that a new limit is `NoSpace` and that an `unlink` still works.

**6. warning: an untrusted client can trigger the stall that ADR-0290 accepts only for the keeper.** A new limit
runs `subtree_used`: a scan of the subtree, plus the whole orphan list with `is_within` (a walk to the root) for each
orphan, all inside one request. The engine serves one request at a time, so this stalls every client of the volume.

ADR-0290's Consequences assume that "the room keeper sets limits on room directories that are empty or small".
However, the room's own programs hold `WRITE` on their tree and may set limits on its subdirectories (ADR-0265
rule 4). Setting, removing and setting again repeats the scan without bound. With a 2 GiB limit the room can hold
about 500,000 objects.

This is the same class of problem as the `sync` loop that ADR-0265 rule 9 answers.
*Fix:* finding 2's `LIMIT` right (rooms' programs would not get it), a bound on the objects one `set_limit` may
scan (refuse a larger tree with `LimitReached`), or a rate limit per connection. In every case, add it to
ADR-0290's open question "Whether the subtree scan of a new limit needs a bound".

**7. warning: the model test does not exercise ADR-0290's orphan rules.** `random_request` opens and closes every
handle within one request. No orphan therefore survives a request, and `counters_true`'s oracle counts named
entries only. `setup` sets the limits once, and no random step sets, lowers or removes one.

As a result, the paths that are new in Q5 are covered only by the hand-written
`a_file_unlinked_while_open_keeps_its_charge_until_closed` and are never crashed:
- `repoint_orphans`;
- `subtree_used`'s orphan branch;
- `delete_object` from `close` and from `free_orphans` at mount;
- `publish` from a directory that was removed meanwhile.

INV-0265-1's row ("after every request and after a crash") therefore claims more than the test shows.
*Fix:* keep a small pool of open handles across steps in `random_request` (unlink while open, `create_unnamed` and
`publish` later, rmdir of their directories), and add random `set_limit` steps. Make the oracle include orphans,
charged by their `fparent`, and run the crash test with handles open, so that the mount's `free_orphans` runs on
crash images.

**8. warning: the status word and the field differ from ADR-0263.** ADR-0263 item 5 says that a native command
reports `NAME: SUBJECT: over quota`, and ADR-0265's example says `cp: copy.iso: over quota (...)`. For the ABI status,
`zoraya-command` already says "over quota" (`lib.rs:439`). For the file status it says "over its disk limit".

ADR-0263 item 1 also says that `fsd` carries the refusing field (`Disk`) in its reply. `QuotaField` has only
`Memory` and `Processes`, and `Done` carries 0 on an error. Neither the report nor an open question records this
deviation.
*Fix:* use "over quota" for the file status. Record the missing `Disk` field as an open item of Q5 or as a question
for Q6a, which prints the hint.

**9. warning: no codec tests for the new messages.** The crate's round-trip helper (`check(...)`) is not extended to
`SetLimit`, `GetLimit` or `LimitReply`. No test shows that:
- `remove = 2` is refused;
- a status of 18 is refused;
- a name with a length past the buffer is refused.

The macro is probably total, but the review question "decode totality for the two new requests" is answered by
nothing here.
*Fix:* add the three messages to the round-trip test, with one hostile case each.

**10. note: `GetLimit` has no rights check.** ADR-0265 rule 8 says "on any connection that can stat it". The service
checks nothing. A connection narrowed with `only` (to the parent of a `create` operand) may not list its directory,
but it can still read the directory's `limit` and `used`, that is, how much the whole tree holds.
*Fix:* refuse `GetLimit` on narrowed connections, as `readdir` is refused at `:1699`, or require `READ`.

**11. note: `fsck` is weaker than the engine needs.** INV-0290-1 and `check.rs:476-487` accept an orphan whose
charged directory is any directory inode, including a removed one that has no `K_PARENT`. On such a volume,
`limited_chain` from that directory fails with `inconsistent()`. `close` or the mount's `free_orphans` then fails the
volume. `repair` (`named.or(valid_old)`) keeps the dangling charge. The engine cannot produce this state today,
because `repoint_orphans` runs first, but a checker is the oracle for crash images.
*Fix:* require the charged directory to be the root or to have a parent link. Have `repair` charge the orphan to the
nearest live ancestor, or to the root.

**12. note: version-1 volumes.** Both header decoders return `Corrupt("unknown format version")`, so a version-1
image fails to mount as a damaged volume would. No test builds a version-1 header and checks that the mount and
`fsck` report it as a version mismatch, and that `repair` does not touch it.
*Fix:* add the test. Consider a distinct reason that tells the person to rebuild with `zoraya-mkfs`.

**13. note: `zoraya-mkfs` doc comment.** The insertion split `populate`'s doc. `set_limit`'s rustdoc now begins with
"Copies the tree under `src`...", and `populate` lost its doc. `set_limit` also opens `v.root()` and never closes it.

**14. note: walks that grow with the volume.** `repoint_orphans` scans all orphans on every `rmdir`, including all
open unnamed files of every client. `limited_chain` does one `tget` per level, and a room can build a chain of
directories as deep as its limit allows. Neither walk is measured. Q5's measurement covers depth 3 only.

## Checked and found sound

- **`OverQuota` changes nothing.** Every growing path runs inside `mutate`, whose snapshot is restored on `Err`, and
  `OverQuota` is not retried. `checked_add` refuses `used + n` past `u64::MAX` as `OverQuota`, so a single huge
  `set_size` or `write` cannot saturate a limited chain.
- **Order of operations.** `rename` (`Replace`) and `publish` unname the replaced entry before moving the charge in.
  `move_charges` takes everything out before it adds anything, so an exchange that fits is not refused partway.
- **Directories crossing a limit.** `CrossesLimit` is checked before any change. It covers an exchange in which only
  the destination is a directory, and it compares the sets of limited ancestors, so moves within one limited tree
  are free.
- **Charges by request.** A directory costs one block. A link costs one block plus its target in blocks, in
  `create_obj`, `subtree_used` and `fsck` alike. A rewrite in place does not change the charge (`o == n`).
  Shrinking `set_size`, `O_TRUNC`, `unlink`, `rmdir` and `close` only subtract and are never refused for a limit.
- **Removed directories.** Creating in a removed directory, through `create_obj`, `create_unnamed`, `rename` or
  `publish`, is refused by `is_orphan` before any charge.
- **Crash consistency.** The counters, `K_FPARENT` and the data are in the same copy-on-write tree, and every crash
  image in the test passes `fsck`.
- **`Code`.** No other bound of file statuses exists: the `GONE` values are 1000 and `u32::MAX`. `EDQUOT` = 122 is
  correct for AArch64.
- **`FILE` table.** `OVER_QUOTA` maps to `EDQUOT` and `CROSSES_LIMIT` to `EXDEV`, apart from the failing test of
  finding 1.
- **`diskfill`.** It checks `EDQUOT`, `unlink` at the limit, `EXDEV` for a directory, and a write outside the limit.
  (The script runs it as a trusted program, not in a room, so INV-0265-3 rightly stays open.)

## Dispositions

1. Fixed: the test loops over `0..18` and `from_file(18)` is `EIO`; `cargo test -p zoraya-posix` run and passing.
2. Fixed: `SetLimit` is refused with `ACCESS_DENIED` on a narrowed connection (`only_of`), `GetLimit` too and without `READ`; host test `a_narrowed_connection_cannot_change_a_limit`. The rule (a full directory connection with `WRITE`) and the `LIMIT` right are in proposed ADR-0292.
3. Fixed: `subtree_used` uses `checked_add` and refuses the new limit with `OverQuota`; `fsck` reports a tree charged past `u64::MAX` as damage and `repair` removes the record; test `a_sum_past_u64_is_refused_and_not_clean` (two sparse `i64::MAX` files).
4. Fixed by refusing: a directory with a charged open orphan and a limit on or above it cannot be removed (`NotEmpty`), also by rename over it; with no limit on or above it removal works as before (an existing test requires removal on a full volume) and the orphans are charged to the parent. ADR-0290 text changed to match. Tests `a_directory_with_a_charged_open_file_stays`.
5. Fixed in code: `set_limit` is freeing only for `None`; a new limit keeps the reserve. The test cannot make a new limit fail where the old code passed (a limit fits in the 5 blocks a create does not); it checks the reserve stays and `None` works on a full volume.
6. Fixed: `SCAN_BOUND` = 50,000 objects, `ScanTooLarge` (`LIMIT_REACHED`); test with 50,001 directories. The repeat-by-a-`WRITE`-holder remains bounded per request, not rate limited: open item for Q6a/ADR-0292.
7. Fixed: the model test keeps up to four handles open across requests (unlinked meanwhile), writes through them, and changes, removes and restores limits; `fsck` is the oracle with orphans, the API oracle when none is open; the crash test keeps handles open to the crash, so the images hold orphans and the mount frees them.
8. Fixed: the word is "over quota". Open: the `Disk` field of ADR-0263 item 1 is not in `Done` (the reply carries a count only); Q6a's hint can derive it from the status `OVER_QUOTA` of a file call.
9. Fixed: round-trip, totality and hostile cases for `SetLimit`, `GetLimit`, `LimitReply`.
10. Fixed with 2.
11. Fixed: `fsck` requires the charged directory of an orphan to be the root or a directory with a parent link; `repair` charges to the root otherwise.
12. Fixed: the message says to rebuild with `zoraya-mkfs`; header test. Not done: a mount test on a whole version-1 image.
13. Fixed: docs restored; the root handle is closed.
14. Open: a directory removal scans the orphan list once (one lookup when it is empty), and the chain walk is measured at depth 3 only; deeper chains are not measured.

## Re-check of the fixes

A re-check by the `opus-review` subagent of the changed lines only, `git diff 3230ceb...work/q5` (head `e592317`),
against the dispositions above, ADR-0265, proposed ADR-0290 and proposed ADR-0292. Commands run on an export of
`work/q5` with its own target directory:
- `cargo test -p zoraya-fs -p zoraya-fs-service -p zoraya-proto-file -p zoraya-posix -p zoraya-fsclient`: all pass
  (231 tests, 0 failed; `limits.rs` 16 passed, 1 ignored).
- Two scratch tests, added to the exported `tests/limits.rs` only and not committed:
  - A limit on `A`; a file in `A/d` is opened and unlinked; `A/d` is renamed to `A/e`. Then `rmdir A/e` is `NotEmpty`.
    After `set_limit(A, None)` the `rmdir` works. After `set_limit(A, Some(4 blocks))`, `used` is 4096, the write of 8
    blocks is `OverQuota`, `fsck` is clean, and after `close` `used` is 0. No escape.
  - On a full volume (filled inside a limited `room`): lowering the existing limit gives `Err(NoSpace)`; removing it
    gives `Ok`; setting it again gives `Err(NoSpace)` (re-check 4).
- `grep saturating crates/zoraya-fs/src`: no saturating sum of charges is left. The one that remains in charging is
  `charge_of` (`volume.rs:64`), which cannot saturate for `size <= i64::MAX` and is the same function in the engine
  and in `fsck`.

No error found. Four warnings, five notes.

| # | Severity | Where | Finding |
|---|---|---|---|
| R1 | warning | `crates/zoraya-fs/src/volume.rs:810-812`; ADR-0265 rule 2, INV-0265-2; ADR-0290 driver 3 | `rmdir` of an empty directory is now refused because of a limit. Accepted ADR-0265 and ADR-0290's own driver 3 say a freeing request never is, and the ADR does not say that it deviates. |
| R2 | warning | `crates/zoraya-fs-service/src/lib.rs:1009`, `:1982-1985`; `crates/zoraya-shell/src/grants.rs:44`, `:140`, `:346-350` | A full directory connection with `WRITE` but without Tree's rights can still change a child's limit. Two shapes produce one: `write` on a directory ("change DIR"), and two `create` operands in one directory. |
| R3 | warning | `docs/adr/0290-what-the-engine-charges.md:35`; `docs/adr/invariants.md:493`, `:512` | ADR-0290 still says "`get_limit` is open to any connection". INV-0265-1's Status still says the API oracle runs "after each request", and INV-0290-1 still says "an orphan's is a directory". |
| R4 | warning | `crates/zoraya-fs/src/volume.rs:1650`; ADR-0292, ADR-0290 | On a full volume, a limit can only be removed, not changed. A keeper that resets a limit (remove, then set) leaves the room without one. Neither ADR says so. |
| R5 | note | `docs/adr/0292-who-may-set-a-limit.md:19`, `:53` | Driver 3 says that a room "may narrow" limits below its directory. In fact it may set, raise, lower or remove any limit below, the keeper's included. Only the outer limit stays. |
| R6 | note | `crates/zoraya-fs/tests/limits.rs:848-853` | Finding 5's test passes with the old `mutate(true, ..)` (as the disposition says). The fix has no test that fails without it, and it is not in `tools/mutants/`. |
| R7 | note | `crates/zoraya-fs/tests/limits.rs:861-879`; `volume.rs:986`, `:1010` | The scan bound is tested from one side only: 50,001 is refused, but nothing shows that 50,000 is accepted (`>` against `>=` survives). Its cost on the guest is not measured. |
| R8 | note | `crates/zoraya-fs/tests/limits.rs:607-650`, `:619` | The model test no longer checks `used <= limit` after a request, and it counts no orphans, no `repoint_orphans` and no `NotEmpty` refusals of the new rule. `REFUSED` and `CROSSED` show that a run reached its limit paths; nothing shows that it reached these. |
| R9 | note | `crates/zoraya-fs/src/check.rs:965-972` | For a tree charged past `u64::MAX`, `repair` removes the limit record, which loosens a bound without a word in the ADR. The engine cannot produce this state any more. |

**R1. warning: `rmdir` refused for a limit.** ADR-0265 rule 2 (accepted) says: "Requests that shrink (`unlink`,
removing a directory, a smaller `set_size`) are never refused for a limit". INV-0265-2 says "a shrinking request never
fails for a limit", and ADR-0290 driver 3 says "A request that frees space is never refused (ADR-0265 rule 2)". The new
`unname` returns `NotEmpty` for an empty directory only when a limit is on or above it (`volume.rs:810-812`), and so
does a `rename` over it.

*Scenario:* the person runs `rm -r ~/sandbox/agent-1` while an agent there holds a `tmpfile`. The `rm` gets
`ENOTEMPTY` for a directory that `ls` shows empty, and it keeps getting it until the agent's handle closes.

The rule is safe: the scratch test above found no way out of the limit. But it changes an accepted rule.
ADR-0290's new text (line 30) gives the behaviour without naming rule 2 or INV-0265-2, and it keeps driver 3 as it
was. Q6a's `room drop` must therefore end the room's processes before it removes the directory, and nothing records
that.

*Fix:* in ADR-0290, say that this deviates from ADR-0265 rule 2 and INV-0265-2, and ask the architect to choose
between this rule and the other one (keep the removed directory's `K_LIMIT` and `K_PARENT` until its last charged
orphan is freed, so that `rmdir` always works). Change driver 3 to match. Add to Q6a's notes that `room drop` closes
the room's handles before `rm -r`.

**R2. warning: `WRITE` on a full directory connection is wider than Tree.** The service checks only
`only_of(i).is_none()` and `WRITE`.
- `Kind::Write` makes `Access::Change` on the path itself (`grants.rs:140`), with `READ | WRITE | SYNC | CLONE`
  (`:44`), whatever the path is. A `write` operand that names a directory therefore gives a full connection with
  `WRITE` to it.
- `by_path` (`:346-350`) drops the name when two entries for one directory name different children, so two `create`
  operands in `~/sandbox` give a full connection to `~/sandbox` with `WRITE`.

*Scenario:* a program on the person's line, designated with `--out ~/sandbox/a --out ~/sandbox/b` (the words are
"create and change anything in ~/sandbox"), or with a `write` operand of `~/sandbox` (the words are "change
~/sandbox"), sends `SetLimit { remove: 1, name: "agent-1" }` and succeeds. Then agent-1's disk filler fills the volume.

A room cannot do this inside itself, because its designations stay within its grants. So this does not block the
ADR-0268 gate, as long as no room holds `+w` on a directory above another limited directory, which is ADR-0292's
revisit trigger. The shell paths were read, not run. Whether the shell opens a `write` operand that is a directory
as a directory connection was not run.

*Fix (no ABI change):* in the service, require Tree's shape for `SetLimit`, that is `WRITE | CREATE | REMOVE | RENAME`.
Neither shape has `REMOVE`. Add an `fsclient` test with `clone_with(root, READ | WRITE | CREATE)`. Alternatively,
Option B, and record either choice in ADR-0292.

**R3. warning: stale text.**
- ADR-0290 line 35 contradicts ADR-0292 and the code (`lib.rs:1017`).
- The Status of INV-0265-1 (`invariants.md:493`) says that `random_requests_keep_the_counters_true` "recomputes every
  `used` through the API after each request". It now does so only when no handle is open (`limits.rs:619`), and runs
  `fsck` every 25 requests.
- INV-0290-1's text says that an orphan's charged directory "is a directory". `fsck` now also requires a name or the
  root (`check.rs:478`).

*Fix:* correct the three texts. ADR-0290 is proposed. INV-0290-1's statement is part of a proposed ADR, so its row may
change with it.

**R4. warning: changing a limit on a full volume.** `mutate(limit.is_none(), ..)` makes a change of an existing limit
(the same record size) a non-freeing request, so it fails with `NoSpace` on a full volume. This matches ADR-0265
rule 2's "rewrite in place". A new limit after a removal also fails, so a keeper that resets a room's limit by
"remove, then set" on a full volume leaves the room unlimited (scratch test above).
*Fix:* record in ADR-0290 or ADR-0292 that on a full volume a limit can be removed but not changed or set, and that
the keeper never removes a limit in order to change it. Alternatively, let a change of an existing record use the
reserve: it adds no key, as an `unlink` adds none.

**R5. note: what a room can do to the limits below it.** It cannot reach its own root or anything above it. `Name` is
one component, with no `.`, `..` or `/` (`zoraya-protocol` `Name::is_valid`), `parent_of` with one component starts at
the connection's directory, and a link object gives `NotDir`. Narrowed clones keep `only` (`lib.rs:459-464`, `:1471`,
`:2112`). Below its root it can set, raise, lower or remove any limit, including one that the keeper set, for example
on a `tmp` below the room's directory. It can also create `CrossesLimit` boundaries and keep directories with
`NotEmpty`. The outer limit always binds (`limited_chain` walks to the root). ADR-0292 says "narrow" and "set".
*Fix:* say it exactly in ADR-0292's Consequences. Q6a must give a room's `/tmp` its own limited directory as the
connection itself, never as a child of a directory that the room holds.

**R6. note: finding 5 has no test that fails without the fix.** Add the old line to a list in `tools/mutants/` with a
note that it is a known survivor, or build a volume whose free space lies exactly between the request's need and the
reserve.

**R7. note: the scan bound.** Add a test with exactly `SCAN_BOUND` objects that is accepted. The bound counts the
volume's whole orphan list, but orphans are bounded by `MAX_OPEN` (896, `zoraya-fs-service/src/lib.rs:32`), so
other clients' open files cannot push a small tree past it. The "50 ms" is a host figure, and ADR-0290 calls it an
assumption.

**R8. note: what the model test shows.** Add counters, like `REFUSED`, for orphans created, directories repointed and
`NotEmpty` from the new rule, and assert that each is above 0. After each request, check `used <= limit` for every
directory whose `used` grew in it.

**R9. note: `repair` and an overflowing tree.** Removing the record is defensible, but it loosens a bound. Say so in
ADR-0290 (fsck's rules), or have `repair` set `used = limit` so that growth stays refused.

### Checked and found sound

- **Narrowed connections.** `SetLimit` (`lib.rs:1009`) and `GetLimit` (`:1017`) are refused on a narrowed connection.
  `GetLimit` is also refused without `READ`. `may_name` is no longer needed for `SetLimit`. A narrowed connection
  opens its name as a directory with read rights only (`:1355`), so it gets no `WRITE` on the child. The test
  `a_narrowed_connection_cannot_change_a_limit` passes.
- **Sums.** `subtree_used` refuses a sum past `u64::MAX` with `OverQuota` before anything is written, and the mutation
  is rolled back. `fsck` reports such a tree and skips its `used` comparison, and `repair` removes the record.
  `adjust_chain` was already checked. `a_sum_past_u64_is_refused_and_not_clean` passes.
- **Orphan rule.** An orphan can be charged to a removed directory only through `repoint_orphans`, which runs only when
  no limit is on or above the directory. `limited_chain(parent)` is a subset of `limited_chain(dir)`, so the move
  loosens nothing. A later new limit counts repointed orphans (`subtree_used` uses `is_within` on `fparent`). The
  refusal and the move happen inside `mutate`, in one commit, so a crash image shows either the state before or the
  state after. `fsck` now requires the charged directory to be the root or a directory with a parent link, and
  `repair` falls back to the root. The crash test keeps handles open to the crash, and `fsck` is clean on every image
  and after the mount's `free_orphans`.
- **Scan bound.** The bound applies to dirents and orphans alike, and the refusal happens before anything is written.
  The bound is reached only when a new limit is set: a change of an existing record does not scan.
- **errno and codec.** `0..18` maps once each, and `from_file(18)` is `EIO`. The codec test covers both `remove`
  values, `GetLimit`, `LimitReply` with the largest status, `remove` 2, status 18, `..` as a name and a truncated
  message, and `exercise` runs on every encoding.
- **Words.** `file_problem(OVER_QUOTA)` is "over quota". The missing `Disk` field is recorded in disposition 8.

### Dispositions of the re-check

- R1/W1: no behaviour change; ADR-0290 now says plainly that the rule deviates from ADR-0265 rule 2 and INV-0265-2 for one case, why, and puts the alternatives to the architect in its open questions; driver 3 and the `room drop` note match.
- R2/W2: `SetLimit` needs `WRITE | CREATE | REMOVE | RENAME`; fsclient test `only_a_connection_with_the_rights_of_a_tree_sets_a_limit` (WRITE+CREATE, WRITE alone, WRITE+CREATE+REMOVE refused; all four allowed). Mutant `L-set-limit-rights`. Recorded in ADR-0292. The shell paths were read, not run.
- R3/W3: fixed: ADR-0290 line on `get_limit`, INV-0265-1's Status (the API oracle runs only with no handle open), INV-0290-1's text (ADR and invariants.md).
- R4/W4: a change of an existing record is now a freeing request (it adds no record) and works on a full volume (test `on_a_full_volume_an_existing_limit_can_be_changed`); a new or re-set limit after removal is still `NoSpace`, recorded in ADR-0290 and ADR-0292 (a keeper sets before it removes; the reserve is not extended to new records).
- R5: ADR-0292 consequences say exactly what a room can do below its root and that Q6a gives `/tmp` its own limited directory.
- R6: still no test that fails without the `existing` or `is_none` condition for a new record (a volume whose free space lies exactly between a limit's need and the reserve was not built); listed as a known survivor in `tools/mutants/fs-limits.toml` (`L-set-limit-new-record-frees`).
- R7: test `a_tree_of_exactly_the_scan_bound_gets_a_limit` (mutant `L-scan-bound`); the 50 ms stays a host assumption.
- R8: not done: counters for orphans created, repointed and `NotEmpty` from the new rule, and a per-request `used <= limit` check, are not in the model test; the dedicated tests cover those paths and the crash test keeps handles open. Open item.
- R9: kept removal; recorded in ADR-0290 (setting `used = limit` would leave `fsck` reporting the tree; the engine cannot make the state any more).
