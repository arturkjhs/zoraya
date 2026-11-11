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
