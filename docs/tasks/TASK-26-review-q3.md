# TASK-26 review of Q3 (fsd part)

An independent review by the `opus-review` subagent, which did not write the code, of `git diff e592317...work/q3`.
`e592317` (the Q5 head after its review fixes) is **not** an ancestor of `work/q3`: the branch forks from `3230ceb`
(Q5 before its review), so the triple-dot diff is `3230ceb..work/q3` (041cb93). `git merge-tree e592317 work/q3`
conflicts in `crates/zoraya-fsclient/tests/service.rs`, `docs/adr/index.md` and `docs/tasks/TASK-26-report.md`;
`crates/zoraya-fs-service/src/lib.rs` merges cleanly (Q5's ADR-0292 refusals of `SetLimit`/`GetLimit` on a narrowed
connection survive the merge). Everything below was read and run on `work/q3` alone.

Code read around the diff: `crates/zoraya-fs-service/src/lib.rs` (`Account`, `session_account`, `carve`,
`free_account`, `charge`/`release`, `room`, `add_connection`, `close_connection`, `drop_buffer`, `step`, `fold_sync`,
`answer_syncs`, `after_engine`, `ready`, `flush_held`, `send`, `fifo_open`, `fifo_answer`, `fifo_wake`,
`clone_connection`, `share`, `attach`, the token code, `set_limit`), `crates/zoraya-fs/src/volume.rs` (`sync`, `tick`,
`commit`, `commits`, `commit_deadline`, `device_lost`), `user/src/bin/fsd.rs` (`interest`, `wait`),
`crates/zoraya-events/src/lib.rs` (`unsubscribe`), the new tests, `crates/zoraya-proto-file` and `zoraya-fsclient`.
Checked against ADR-0262 (item 2, the table, INV-0262-1..3), proposed ADR-0291, ADR-0265 item 9 and INV-0265-5,
ADR-0263 items 1 and 3, ADR-0268's service-floods row, proposed ADR-0292 and plan row Q3.

Commands run (in a detached worktree of `work/q3` outside the repository, the repository's `target/`):
- `cargo test -p zoraya-fs-service -p zoraya-fsclient -p zoraya-proto-file`: all pass (65 + fifo + time, 10, 7).
- `cargo clippy -p zoraya-fs-service -p zoraya-fsclient -p zoraya-proto-file --all-targets -- -D warnings`: clean.
- Two scratch tests, appended to that copy of `tests/service.rs` only and removed afterwards: both fail on `work/q3`,
  reproducing findings 1 and 2.

No QEMU run. Dispositions are left to the main model.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `crates/zoraya-fs-service/src/lib.rs:631-647` (`free_account`), `:596` | A sub-account of a dead top account is re-parented with `top: false`: the room keeps its budget outside the column and frees a column slot, so a fifth 32-account is carved. INV-0262-3 broken; the row says checked. Reproduced. |
| 2 | error | `crates/zoraya-fs-service/src/lib.rs:882-885`, `:960-968` | On `DriverClosed`, `answer_syncs` runs before `device_lost()` fails the volume; afterwards the loop never calls `after_engine` again by itself, so a folded `sync` waits for ever unless another client sends a request. Reproduced. |
| 3 | warning | `crates/zoraya-fs-service/src/lib.rs:1066-1083` | `NoMemory` (the machine's or the service's memory, not the client's) counts against the account's held bound: a second untrusted connection that meets `NoMemory` loses its connection, where before it was held and retried. Pressure from one room can close another room's connections. |
| 4 | warning | `crates/zoraya-fs-service/src/lib.rs:1953-1955`; ADR-0291 open question | A `DURABLE` write from an untrusted account commits at once. A one-byte `O_SYNC` write loop is the same stall that ADR-0265 item 9 removes for `sync`, and INV-0265-5's planned test (a `sync` loop) would pass with the hole open. |
| 5 | warning | ADR-0262 item 2 "At the bound", ADR-0263 item 3; `docs/tasks/TASK-26-report.md` (Q3 open items) | No refusal count per account for the launcher. The report's open items do not list it. |
| 6 | warning | `crates/zoraya-fs-service/tests/service.rs:2535-2606`; `docs/adr/invariants.md` rows INV-0262-2, -3, INV-0291-1 | The model test never uses `Clone`, `Share`, `Narrow`, tokens, held replies, FIFOs or the death of the session's account. Its column check uses the same `top` flag that finding 1 defeats. The rows claim more than it checks. |
| 7 | note | `crates/zoraya-fs-service/src/lib.rs:1547-1563`, `:1590-1595` | A deferred FIFO answer (`fifo_wake`) charges the dup to the waiting connection's account without `room`. That account's `open` can pass its `open_limit` while the reply is held. |
| 8 | note | `crates/zoraya-fs-service/src/lib.rs:245-249`, `:263` | `worst_case().total()` counts 5 accounts. Sub-accounts can be up to one per connection: about 896 accounts, and a chain of up to 32 per top. |
| 9 | note | ADR-0291 INV-0291-1; `lib.rs:631-647` | "Equal after every request" is false once the session's account ends: its carve is dropped, and later frees of re-parented top accounts return to nobody. |
| 10 | note | `crates/zoraya-fs-service/src/lib.rs:917`, `:946-947`, `:929-956` | Interest results are ignored in `fold_sync`/`answer_syncs`. `answer_syncs` allocates and scans on every engine call while any `sync` waits. |
| 11 | note | `crates/zoraya-fs-service/src/lib.rs:994-998` | Any wake of a syncing connection closes it. |
| 12 | note | ADR-0292 and `Account.untrusted` | Who may set limits: the account is the natural place. |
| 13 | note | `docs/adr/0262-who-pays-for-service-objects.md:200-202` | A new open-question bullet holds a measurement, not a question. |

## Findings

**1. error: a room can move its budget out of the untrusted column.** `carve` sets `top = !p.untrusted` once, and
`free_account` re-parents the children of a dead account (`c.parent = x.parent`) without carrying `top`. Scenario: the
launcher carves room account A (32) from the session. The room sends `CloneAccount { budget: 31 }` on A's connection,
which gives B. Then it closes A's connections (or A's only one, if the launcher handed it over). `free_account(A)`
returns A's leftover 1 to the session. B now answers to the session with `top: false`, so `tops` is 3 and the next
`CloneAccount` from the session succeeds. The scratch test (four tops, the sub-carve of 31, A closed, a fifth carve of
32) prints `fifth top carve status 0, top_accounts 4, top_open_limit 128`. The live untrusted accounts hold
128 + 31 = 159 open, and the session keeps 737 rather than ADR-0262's 768. The same holds for buffers and held
replies. INV-0262-3 ("the untrusted accounts together never pass their column") is broken. Its row says
`checked`, and `random_accounts_keep_the_sum_of_bounds_and_free_their_state` cannot see the break: it sums
`top_open_limit`, which leaves B out. `an_account_is_freed_with_its_last_connection` (`:2510`) builds this state
(a dead top with a live sub) and asserts only the account count. A keeper that runs at most four rooms limits the harm
today, but this is the service's own bound (ADR-0262: "every service admits at most four untrusted accounts").
*Fix:* make the column a property of the subtree, not of one record. Either keep a dead account's record (its
connections gone, its leftover returned) until its last descendant ends, so its `top` slot and the column stay
counted. Or record on each account the top account it descends from (`column: Option<usize>`), count a column slot
while any account of that column lives, and sum the column's limits by that field. Add the scenario above to the
tests, and add a dead top with a live sub to the model test's moves.

**2. error: a lost driver leaves a folded `sync` unanswered.** `step` handles `Wake::DriverClosed` with
`mark_lost(); after_engine()`. `after_engine` calls `answer_syncs()` first, while `volume.failure()` is still `None`.
The sync is not answered: no new commit, deadline still set. Only then does it call `self.volume.device_lost()`
(`:967`), which fails the volume. On the next turn `step` sees `failure()` set, waits with no deadline, and never
calls `tick`/`after_engine`. An untrusted program blocked in `sync` hangs until some other client sends `fsd` a
request, or until it closes itself. ADR-0054 says an outage is reported to clients, and `answer_syncs`'s own doc
promises "or a failed volume". Scratch test: carve 8, write, post `Sync`, set `driver_closed`, run. The `DEVICE_LOST`
report goes out, and `take_reply(f)` stays `None` after a further `run()`. *Fix:* call `answer_syncs()` at the end of
`after_engine` as well, after `device_lost()` and the failure report, or only there. Add that test (and one for a
commit failure during `tick`, which works today but has no test).

**3. warning: `NoMemory` closes untrusted connections.** `send` treats `Outgoing::Full` and `Outgoing::NoMemory`
alike, and `may_hold` is false once the account holds one reply. `Full` is the client's own doing: its queue is
full because it does not read. `NoMemory` is the kernel refusing the service's write: the machine, or `fsd`'s job,
is out of memory, which another room's load can cause. Before Q3 such a reply was held and retried after `RETRY_NS`.
Now, in a room with one reply already held (for example a client that is slow to read), the next `NoMemory` on any
other connection of the room closes that connection. Scenario: room B has one slow reader. Room A's activity pushes
the machine to `NoMemory`. B's shell loses its file connection, which ADR-0262's "nothing of any other account
changes" forbids. ADR-0291 Option A reads "as a failed allocation already does", but that sentence covers
`try_reserve`, the service's own allocation, not a refused kernel write. *Fix:* apply the held bound only to `Full`.
Hold a `NoMemory` reply under the existing retry, and bound it by the global `MAX_HELD`, or by a small service-wide
pool of retry slots that no account can fill alone. Test with `Outgoing::NoMemory` from the test transport on a
second connection of an account that already holds a reply.

**4. warning: durable writes are not folded.** `write` with `write_flags::DURABLE` calls `self.volume.sync()` at
once (`:1953`). It needs `SYNC`, which is the same right that makes `Sync` foldable, so any room that may `sync` may
also loop `pwrite(fd, "x", 1, 0)` with `O_SYNC`. That costs one commit per iteration and stalls every client, which
is what ADR-0265 item 9 was written to prevent. ADR-0291 lists it as an open question. But the plan's Q3 row and
INV-0265-5 promise that a room's `sync` loop costs nothing beyond the timed commits, and the gate's service-floods row
relies on it. *Fix:* for an untrusted account, do the write, then fold as `Sync` does. Keep the count in `syncing`
(`Option<(u32, u64, u64)>`) and answer `Done { count }` at the commit. At least, list the durable-write loop in
INV-0265-5's check so the in-QEMU test cannot pass with the hole.

**5. warning: no refusal count for the launcher.** ADR-0262 item 2 ("At the bound ... the service tells the account's
launcher (a count it reads)") and ADR-0263 item 3 ("Services report account refusals to the account's launcher").
`Account` has no refused counter, and `AccountTotals` has none. The report's open items list only `top` and the
launcher's `CloneAccount`. *Fix:* add a per-account count of refusals at each bound (open, buffers, held-close).
`TOO_MANY_OPEN` and `LIMIT_REACHED` from `room`/`attach`/`carve`, and the connection closed in `send`, increment it.
Expose it in `account_totals`, or with a per-account getter, for the Q6a request. Or record the gap in the report's
open items and in ADR-0291.

**6. warning: the cited tests check less than the rows say.**
- INV-0262-2 ("every connection belongs to exactly one account ... closing ... frees its state"): the model test
  makes new connections only by `CloneAccount` and `Open`. `Clone`, `Share`, `Narrow`, `GetToken`/tokens, FIFO opens
  and held replies never occur. The `held -= 1` in `close_connection` (`:807-809`), the token's `release` (`:800`)
  and the held reply's handles have no test that runs them under accounts. No test shows that a budgetless `Clone`
  or `Share` of a carved connection stays in the carved account (the "escape through `Clone`" question). The code
  is right today (`account_of(i)` at `:1880` and `:2543`), but nothing pins it.
- INV-0262-3: see finding 1. The column check is blind to re-parented sub-accounts.
- INV-0291-1: see finding 9. The root connection never closes in the model, so the death of the session's account is
  never exercised.
*Fix:* add moves to the model: budgetless `Clone`, `Share`, `GetToken`, a held reply (set `room` to 0 for a step,
then restore), and closing the account that carved others. After each step, assert that every live connection's
account is live (`account_live(account_index(c))`) and that each account's `open <= open_limit`, not only the sums.

**7. note: deferred FIFO answers bypass the account's room.** `fifo_open` checks `room(i)` for the requester. When a
waiting open is answered later by `fifo_wake` → `fifo_answer(index)`, the dup is charged to the waiting connection's
account with no check. If that account has filled up meanwhile and the reply is held, its `open` passes its
`open_limit` by the held dups. Today this happens only in `fsd --memory`, which ADR-0291 says has one account. Nothing
stops `CloneAccount` there, though, and the model's `t.open <= t.open_limit` is a sum, so it would not notice.
`carve` is safe only because `room(i)` runs first; it subtracts unchecked (`p.open_limit - p.open`). *Fix:* reserve
the dup's place at `fifo_open` time, or refuse `CloneAccount` on a service with FIFOs. Use `saturating_sub` in
`carve`'s comparisons.

**8. note: worst case of the account table.** `total()` adds `per_account * (1 + MAX_UNTRUSTED_ACCOUNTS)`, but every
connection can be its own account: a top account can chain sub-accounts of budget 31, 30, ... (each takes the one held
slot of its parent), and the session can carve any number of non-top accounts. Neither `of_account` (one room with its
sub-accounts) nor `total` counts that. The bytes are small (72 B each, about 64 KB at 896 accounts). The formula
should say `MAX_OPEN * per_account`, or the service should cap sub-accounts.

**9. note: INV-0291-1 is not an equality after the session's account ends.** ADR-0291 itself says that the carve of a
dead session account is not returned. Its top children get `parent: None`, and their limits vanish when they end.
`add_root` is called only once (`user/src/bin/fsd.rs:266`), so no new session account is ever made. *Fix:* state the
invariant as "equal while the session's account lives, at most otherwise", or return the carve to the global pool
(`session_account()` already recomputes the remainder).

**10. note: the sync bookkeeping.** `fold_sync` and `answer_syncs` ignore `interest()`'s result. If the subscription
fails after the unsubscribe, the connection's close is not heard until the commit; this is bounded by
`T_COMMIT`, but it is silent. `answer_syncs` builds a new `Vec` and scans every waiting sync on every
`after_engine`, that is on every request of every client, while any sync waits. It is bounded by the connections, and
cheap enough, but it is an allocation on the hot path. A count of the pending syncs that answers only when `commits()`
changes or failure is set would avoid both.

**11. note: a syncing connection is closed on any wake.** `ready` (`:994-998`) treats every wake of a syncing
connection as its peer's close. In `fsd.rs` the subscription is `Mode::Level` on `PEER_CLOSED` alone, and
`zoraya-events` drops a removed subscription's queued item by serial, so a stale `READABLE` packet should not arrive.
But the service relies on that silently. `flush_held` survives a spurious wake and this path does not. Reading once
(`Incoming::Closed` closes it, anything else re-arms `Closed`) would make it robust.

**12. note (question 6): `SetLimit` and the account.** Proposed ADR-0292 Option A lets any full `WRITE` holder set
limits below its directory, and leaves Option B (a `LIMIT` right) to Q6a. `Account.untrusted` already separates the
connections an untrusted room reached from the session's without an ABI change. "`SetLimit` and `remove` only from a
trusted account" (or "an untrusted account may only lower a limit") is a one-line rule in `set_limit`. It would close
Option A's negative ("any full `WRITE` holder") for rooms before Q6a. Not changed. For the architect, with ADR-0292.

**13. note: ADR-0262's open-question note.** The second added bullet ("The bounds of the table, `fsd` part measured")
is a new bullet holding a result. ADR-0001 rule 6 allows an agent a note under an open question. It would fit as a
note under the existing bullet "The bounds in the table: measure ...".

## Checked and found sound
- Acquire/release pairs: every `charge` (connection, token, token copy, buffer, FIFO dup) has a `release` on each
  exit: sent (`send`/`flush_held` `Sent`), peer closed (`close_connection` frees end, buffer, token, held handles and
  the held count), failed encode (`drop_handles`), held-bound close (`drop_handles` + close), and client killed
  (`PEER_CLOSED` → `close_connection`). The global `open` and the sum of the accounts' `open` stay equal outside the
  temporary service's FIFO pair ends.
- `carve`: budget 0 and >32 from the session are `INVALID_ARGUMENT`. More than the parent's remaining open,
  buffers or held is `TOO_MANY_OPEN`. A fifth top is refused (finding 1 aside). The `room(i)` check before `carve`
  guarantees `open < open_limit`, so its subtractions do not underflow. A huge `u32` budget is refused. Rights only
  decrease (`has(i, CLONE) && has(i, rights)`, shared with `Clone`). An untrusted account carves only from itself
  and never makes a top account. `Clone`, `Share`, `Narrow` and `Open` stay in the caller's account, and tokens
  charge only the requester. The failure paths after `carve` (`reopen` error, `add_connection` refused) free the new
  account and return its carve.
- Cross-account effects through held replies: the held-bound close and `answer_syncs`'s replies act only on the
  connection's own account. A client that never reads cannot block the loop: writes do not block, and its connection
  is either held (bounded) or closed.
- The folded `Sync`: a trusted account's path is unchanged. A waiting `sync` does not delay other accounts, and a
  trusted `sync` or the timed commit answers it. Waiting syncs are at most one per connection, and a closed waiter is
  removed. A commit failure in `tick` answers `STORAGE_FAILED` (finding 2 is the lost-driver order only). The
  re-entrant `answer_syncs` → `done` → `close_connection` → `after_engine` path does not clobber `self.output` in
  the middle of another reply: no handler encodes and then calls an engine function before sending.
- `CloneAccount` decode: a fixed message with one handle, covered by `check`'s truncation and handle-count exercise.
  The service decodes it with the request's handle count, and a missing or non-channel end is `INVALID_ARGUMENT`.
  `fsclient::clone_account` only encodes it and reads a `Done`.
- Statuses match ADR-0263 item 1 and ADR-0262 (`TooManyOpen` at the account's open bound, `LimitReached` for buffers
  as before). Ordinal 40 is unique (`REQUESTS`), and the `zoraya-posix` status test covers 0..18.

## Dispositions

(Main model, after merging `work/q5` 26812b3 into `work/q3`.)

1. Fixed. `free_account` keeps an account that has no connection but has sub-accounts, with its leftover limits, and frees it
   (and then its parent) when the last descendant goes; nothing is re-parented, so a column slot and the column's limits stay
   counted. Test `a_dead_top_account_keeps_its_column_slot_while_a_sub_account_lives` (the review's scenario); the model's column
   sum is now over every untrusted account (`untrusted_*_limit`).
2. Fixed. `answer_syncs` runs last in `after_engine`. Test `a_lost_driver_answers_a_waiting_sync`. (The `tick` commit-failure
   test: `a_failed_timed_commit_reaches_the_supervisor_once` exists for the report; a waiting-sync variant is not added.)
3. Fixed. Only `Full` is bounded; `NoMemory` is held and retried. Test
   `no_memory_does_not_close_a_connection_of_an_account_holding_a_reply`.
4. Fixed. Durable writes from untrusted accounts defer their reply to the next commit (`durable`, `write_reply`); test
   `an_untrusted_durable_write_waits_for_the_next_commit`. ADR-0291 records it. INV-0265-5's in-QEMU test should loop both.
5. Fixed (counter and host getter `account_refusals`, test `refusals_are_counted_per_account`); the launcher's read is Q6a.
6. Fixed. The model test now covers the listed moves, the session's close, and asserts live accounts; the invariant rows say what is covered.
7. Fixed by refusing `CloneAccount` in a service with FIFOs, and `saturating_sub` in `carve`.
8. Fixed in `total()`.
9. Fixed by finding 1's rule (the session's account is a zombie, not dropped).
10. Not changed (bounded by connections; recorded).
11. Not changed (relies on `Level` `PEER_CLOSED` only, as `fsd.rs` subscribes).
12. For the architect, with ADR-0292.
13. Fixed (moved under the existing bullet as a note).
