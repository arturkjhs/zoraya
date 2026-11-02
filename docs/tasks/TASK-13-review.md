# TASK-13 review

Independent review of `main..stage-4b` (main `3075a7e`, HEAD `30bda1a`), by a reviewer that did not write the code.
Every result below was produced in this session on the development host (M4, macOS, QEMU 11.1.2), in clones under
the session's scratchpad. The repository's working tree was not modified. Anything not verified
by running or by reading the named code is marked **unverified**.

Severity: **error** is wrong now, with a concrete failure; **weakness** is a gap in a check, a claim that is stronger
than its evidence, or a failure that needs an unusual condition; **note** is minor or for the record.

The Disposition column is for the author.

**Summary.** `make test` passed from a fresh clone. 47 findings: 6 errors, 19 weaknesses, 22 notes. The errors:
R-1 (no read barrier between `used.idx` and the used entries), R-2 (used length not validated), R-11 (`open` with
`DIRECTORY` creates or truncates and then refuses), R-12 (`DURABLE` write syncs without the `SYNC` right), R-33 (the
shell leaks two handles on every refused path, and runs out after 509), R-47 (INV-0055-3 marked checked, with nine
surviving rights mutants). Of 66 mutants, 33 were caught and 33 survived; most survivors are listed against their
invariant in section 5.

## 1. `make test` from a fresh clone

`git clone -b stage-4b` of HEAD `30bda1a` into a new directory, then `make test` under an alarm of 1500 s.

- Result: **passed**. `make test: all checks passed`, exit status 0, 6 min 32 s (08:03:19 to 08:09:51).
- Host tests: 68 `test result: ok` lines, none failed.
- In-QEMU tests: `pass=95 fail=9` in each of the seven configurations; the nine are the `--expect-panic` tests.
- `test-files`: files-write, files-read, storage-down and device-error passed, `zoraya-fsck` clean after each.
- `test-crash`, seed `1793603346476420001`: 10 rounds and the no-sync round; "10 rounds, all clean; 50160 durable files
  all intact; 2 rounds with files past the last sync".
- **Not observable here:** the CI path (`.github/workflows/test.yml`, Linux, TCG only, where the three `auto`
  configurations, `test-files` and `test-crash` also run under TCG). It has not been run by this review.

## 2. The driver against the virtio specification

Sections are from the virtio 1.2 specification as I know it, quoted from memory. I am confident of the rules; section
numbers may be off by one level, and are marked "(from memory)".

Checked and found correct (no finding):
- Transport v2 initialisation (4.2.3.1, 3.1.1): magic, version 2 and device ID read before anything else; reset by
  writing 0 and polling Status to 0; ACKNOWLEDGE, DRIVER; features read with `DeviceFeaturesSel`; only offered
  features accepted (`VERSION_1` required, `BLK_F_FLUSH` only if offered); FEATURES_OK set and re-read; FAILED set on
  refusal. Mutants V13 and V14 were caught.
- Queue setup (4.2.3.2): `QueueSel` 0, `QueueReady` must read 0, `QueueNumMax` checked, `QueueNum` 256, the three
  64-bit addresses low word first, `QueueReady` 1, then DRIVER_OK.
- Layout and alignment (2.7, table of alignments): descriptors at 0 (16), available ring at 4096 (2), used ring at
  8192 (4), request headers at 12288, all inside one contiguous, zeroed, pinned 16 KiB object. `% N` with N = 256 is
  correct across the 16-bit wrap because 65536 is a multiple of 256.
- Descriptor chains (5.2.6): a 16-byte device-readable header, data pieces with `NEXT` (device-writable for a read,
  readable for a write, mutant V15 caught), a 1-byte device-writable status; at most 32 descriptors, a fixed slot per
  request so the head identifies the slot; the status byte primed with `0xff`.
- Avail index publication (2.7.13): ring entry, barrier, `idx`, barrier, notify. The disassembly of `Blk::finish`
  shows `dmb ish` before the `idx` store and before the notify store.
- Interrupt acknowledge (4.2.2): `InterruptStatus` read and written back to `InterruptACK`, then `interrupt_ack` to
  unmask the line, then the used ring is read again at the top of the loop, so no completion is lost.
- Reset at stop: Status 0 written and polled; the data pin is ended only after a reset that completed.

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-1 | error | `crates/zoraya-virtio/src/lib.rs` `collect`; `user/src/bin/blkd.rs` `Queues::acquire` | The read barrier is before the load of `used.idx`, and there is none between that load and the loads of the used entries and the status bytes (2.7.14 and Linux's `virtio_rmb` after reading `used->idx`, from memory). Verified in the disassembly of `Blk::finish` (built with symbols): `dmb ish` at `0x4003d8`, `ldrh` of `used.idx` at `0x4003e0`, then `ldr` of the entry at `0x4004c8` and `ldrb` of the status at `0x400504` with no barrier between; a control dependency does not order loads on AArch64. | Under HVF the vCPU is a real, weakly ordered core and QEMU's block completion runs on another host thread. The CPU may satisfy the entry or status load before the `idx` load: it sees the new `idx` with an old entry (an id no longer in flight) or the primed `0xff`. `collect` returns `Error::Device`, `blkd` prints "the device broke the protocol" and exits, the volume fails and `init` restarts the stack. Fail-stop, never silent: a stale status is `0xff`, never 0. Not observed in this session (**unverified at run time**). The fake device's `acquire` is a no-op, so no host test can see it (mutant V8 survived). | Fixed: `collect` calls `acquire` after reading `used.idx`, before the entries and statuses. |
| R-2 | error | `crates/zoraya-virtio/src/lib.rs` `collect`, test `a_used_length_is_not_trusted` | The used length is ignored: a request is accepted on status 0 whatever `len` says, and the test asserts that a length of `u32::MAX` is accepted. ADR-0015 rule 8 and the task say the driver validates every length the device returns, and 2.7.8 (from memory) says the driver must not assume anything about device-writable data beyond the first `len` bytes. | A device (or a QEMU bug) that completes a 4 KiB read with status 0 and `len` 1 leaves the previous transfer's bytes in `fsd`'s data object, and `blkd` reports `OK`. The engine's CRC32C of every block catches it as `Corrupt`, so the damage is contained, but the driver's rule is not kept. A read should require `len >= data + 1`. | Fixed: the driver keeps each slot's writable bytes; a length above them, or a successful read below them, is `Error::Device`. The fake now reports what a device writes (1 for a write). Test rewritten: `a_used_length_is_not_trusted` covers both. |
| R-3 | weakness | `user/src/bin/blkd.rs` `transfer` | When a range in a `Write` batch fails `check` (or `queue`), the function returns `BAD_RANGE` with the earlier ranges already queued but not notified. They stay `in_flight` and are sent to the device by the next `notify`, after the client was told the batch was refused. | A service bug or a compromised service sends a batch whose third range is out of bounds: the reply is `BAD_RANGE`, and the first two writes reach the disk with the next unrelated request. Only in-range writes, but against ADR-0054's "treats every message from the service as hostile" and the block protocol's "a batch failing whole". Verified by reading. | Fixed: `transfer` checks every range before queuing any; a refused batch writes nothing. |
| R-4 | weakness | `crates/zoraya-virtio/src/tests.rs` | The tests never pass the 16-bit index wrap: `a_batch_completes_together_and_the_rings_wrap` stops at 12,000 requests. Mutant V9 (`wrapping_add` replaced by `+`, which panics in a debug test at 65,536) survived. Device ids out of range (V3) and a repeated id on its own (V4) are also never tested: the lying-device test only uses id 1 and a repeat that the `new > outstanding` check catches first; V1 also survived because later checks catch the same device. | A regression in the wrap or in the id-range check passes `make test`. Without the range check, an id of 256 indexes `in_flight[8]` and panics `blkd` (bounded, not unsafe). | Fixed: the wrap test runs 68,000 requests; `IdOutOfRange` and `RepeatsFirst` added to the lying-device test. |
| R-5 | weakness | `user/src/bin/blkd.rs` (no host test) | `blkd`'s own checks of service messages (`check`: capacity, attached size, sector overflow) have no test: mutant Q6 (no capacity check) survived `make test-qemu-one`. INV-0054-4's check names only `zoraya-virtio` and `zoraya-proto-block`, and neither runs this code. | A broken `check` lets the service read or write past the device's end; QEMU then fails the request, a real device might not. | Fixed: the check moved to `zoraya_virtio::place`, with `a_range_is_placed_only_inside_the_device_and_the_memory`. |
| R-6 | note | `crates/zoraya-virtio/src/lib.rs` `Driver::new` | The 64-bit capacity is read as two 32-bit reads with no `ConfigGeneration` loop (4.2.2 and 2.5.1, from memory: fields wider than 32 bits are not atomic). A config-change interrupt is acknowledged and ignored. `VIRTIO_BLK_F_RO` and `VIRTIO_F_ACCESS_PLATFORM` are not considered. | A device whose capacity changes during negotiation gives a torn value. None of this happens with QEMU's virtio-blk-device. | Backlog. |
| R-7 | note | `user/src/bin/blkd.rs` `Queues::publish` | `fence(SeqCst)` is `dmb ish` (verified in the disassembly). The notify is a store to Device-nGnRnE memory, for which Linux's `writel` uses `dmb oshst`. Under QEMU the MMIO store traps, so this is safe there; on hardware with a non-coherent or outer-shareable device it might not be. No DSB is used anywhere, which is correct for this design. | None under QEMU `virt`. | Backlog. |
| R-8 | note | `crates/zoraya-virtio/src/lib.rs` `Kind::Flush` | `queue` sends a flush whether or not `BLK_F_FLUSH` was negotiated (5.2.6.2 forbids it, from memory). The engine never flushes a device without flush, since it then mounts read-only, so this is unreachable today. | None today. | Backlog. |
| R-9 | note | `crates/zoraya-virtio/src/lib.rs` `Shared` | `Shared` is a safe trait whose `blkd` implementation dereferences `base + offset` for any offset; safety rests on `Driver` only passing computed offsets. Every offset the driver computes is inside `QUEUE_MEMORY` (checked by reading). | A future caller of the trait could read outside the mapping without `unsafe`. | Backlog. |
| R-10 | note | `user/src/bin/blkd.rs` `serve` | A message with one handle and an ordinal other than `Attach` gets `UNSUPPORTED` or a refusal, and its handle is never closed. | A buggy service leaks handles into `blkd`'s table. | Fixed: a handle on any message but `Attach` is closed. |

What a hostile device can do, as built: anything by DMA (no IOMMU, ADR-0054 says so). Through the queue it can make
`blkd` exit (any bad index, id or status), hang `blkd` forever (`finish` waits with no deadline; `fsd`'s 30 s deadline
then reports the loss and `init` kills the stack), or return stale data with status 0 (R-2, caught by checksums).
An interrupt storm is limited by the masked line until `interrupt_ack`.

## 3. The service loop

Blocking: one thread; the only blocking wait inside a request is `DriverLink::call`, with a 30 s deadline. A client
whose queue is full gets one held reply and is not read again until it drains (INV-0055-6, host-tested and mutants
S17/S18 caught). Starvation between clients: the port's ready list re-queues a level item at the tail
(`zoraya-events`), so clients are served round robin.

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-11 | error | `crates/zoraya-fs-service/src/lib.rs` `open` | The `DIRECTORY` flag is checked after the engine's `open` has run with `CREATE` or `TRUNCATE`. A request that replies `NotDirectory` has already created a file or truncated one. ADR-0010 rule 2: a request fails and changes nothing, or succeeds. | Verified with two host tests added in a scratch clone: `open("new", CREATE|DIRECTORY)` replies `NOT_DIRECTORY` and `new` then exists; `open("f", TRUNCATE|DIRECTORY)` on a file holding "precious" replies `NOT_DIRECTORY` and the file is then empty. | Fixed: `DIRECTORY` with `CREATE`, `UNNAMED` or `TRUNCATE` is `InvalidArgument` before the engine runs; test `an_open_as_a_directory_never_creates_or_truncates`. Recorded in ADR-0080. |
| R-12 | error | `crates/zoraya-fs-service/src/lib.rs` `write_flagged` | A write with the `DURABLE` flag calls `Volume::sync` with no check of the `SYNC` right. ADR-0010 rule 4 defines `durable` as the request followed by `sync`, and INV-0055-3 says no request succeeds with a right its connection lacks. | Verified with a host test: a connection opened with `WRITE` only gets `ACCESS_DENIED` for `Sync`, and its `Write` with `DURABLE` commits the whole volume (two flushes counted). | Fixed: a durable write needs `SYNC` too (`write_handle`). Recorded in ADR-0080. |
| R-13 | weakness | `crates/zoraya-fs-service/src/lib.rs` `step` | `T_COMMIT` and the reply retry are driven only by `Wake::Deadline`, and the kernel's `port_wait` returns ready packets before it looks at the deadline (`kernel/src/port.rs` `wait_masked`). While any connection has a request ready at each wait, `tick` never runs. | Two clients that keep requests queued (or one that pipelines) keep dirty data uncommitted past ADR-0010 rule 5's `T_COMMIT` bound, and a `NoMemory` held reply is never retried. With single synchronous clients on one CPU the queue empties between requests, which is why the crash test's no-sync round passes. Verified by reading; sustained starvation **not demonstrated** in QEMU. Calling `tick` when `now >= deadline` after every request fixes it. | Fixed: after each request the loop runs `tick` and the retry if due; test `a_busy_loop_still_commits_in_time`. |
| R-14 | weakness | `crates/zoraya-fs-service/src/lib.rs` `step`, `user/src/bin/fsd.rs` | Two busy loops once the volume has failed. (a) A commit that fails leaves `oldest_uncommitted` set, so `commit_deadline()` stays in the past and every `port_wait` returns `DeadlineExceeded` at once; verified by a host probe: after a failed timed commit `commit_deadline()` is `Some` and past. (b) After the driver dies, the `PEER_CLOSED` level subscription on `DRIVER_KEY` returns a packet on every wait. The host fake hides both (`wait` ignores the deadline, and `driver_closed` is taken once). | `fsd` uses a whole CPU from the failure until it is killed. Under `init` that is short; under `t_storage` it lasts until `stack.end()`, and on a single CPU it competes with the client. | Fixed: (a) a failed volume gives no commit deadline, test `a_failed_volume_does_not_wake_the_loop`; (b) `fsd` unsubscribes from the driver after its first `PEER_CLOSED`. |
| R-15 | weakness | `crates/zoraya-fs-service/src/lib.rs` `send`, `get_token` | A token copy handed to `send` is not closed when the write fails with `Closed` (or when holding the reply fails), and a held reply's handle is not counted in `open`. | Verified with a host probe: a client that sends `GetToken` and closes leaves the duplicated token handle in `fsd`'s table, uncounted. Repeated, `fsd`'s real table passes `MAX_OPEN`, which INV-0055-7 forbids, and fills to the kernel's 1024. | Fixed: a token copy needs room and is counted until sent; closed with its reply if that is not sent. |
| R-16 | weakness | `crates/zoraya-fs-service/tests/service.rs` `the_table_never_passes_its_limit_under_random_use` | INV-0055-7's model test checks the service's own counter, not the handles the fake transport says are held, so an undercount cannot fail it. Mutants S19 (tokens not counted) and S20 (buffers not counted) survived. | A regression that forgets to count a held handle passes; the real table then exceeds `MAX_OPEN`. | Fixed: the fake counts the handles the service holds, and the model test compares the two at every step; tokens and closing clients added. S19, S20 and two copy mutants now caught. |
| R-17 | note | `user/src/bin/fsd.rs` `Kernel::report` | A report whose `channel_write` fails (`NoMemory`) is dropped, and `take_failure_report` has already been consumed. `add_connection` ignores a failed `port_subscribe`, so the client gets `OK` for a connection that is never served. | Under memory pressure a failure is never reported and `init` does not restart; a new connection hangs its client. | Fixed: an unsent report ends `fsd`, which `init` hears; a connection whose subscription fails is refused (`LimitReached`). |
| R-18 | note | `crates/zoraya-fs-service/src/lib.rs` `fs_status` | `ReadOnly`, `BadHandle` and `Internal` all become `STORAGE_FAILED`. `attributes` computes `secs * 1_000_000_000` in `u64`, which wraps for times past the year 2554 (`user` profile: no overflow checks) and panics in a debug host build. | A read-only mount reports every write as a storage failure; a far-future `mtime` prints a wrong time. | Times saturate. The error mapping is in the backlog. |
| R-19 | note | `user/src/bin/fsd.rs` `main` | The `Attach` reply's status is read and ignored. | If the pin fails, every block request gets `BAD_RANGE`, and the mount fails with a misleading reason. | Fixed: `fsd` checks the `Attach` reply. |

Lost failures: apart from R-17, every engine call in the loop is followed by `after_engine`, so a `Failed` volume is
reported. Mutant S14 (no `after_engine` after `dispatch`) survived only because each handler also calls it. The 30 s
deadline is not tested by anything: mutant Q12 (`fsd` waits for the driver with no deadline) survived
`make test-qemu-one`. Nothing in the tests makes a driver hang, so ADR-0054's "Hangs" rule is unchecked (it has no
invariant row either).

## 4. `init`, restart and rights

ADR-0080 as written is implemented: on any event (driver or service `TERMINATED`, supervisor channel readable or
closed) `init` ends the shell, the service and the driver, in that order, and starts all three again, three times at
most; then the shell runs with no volume. Device reset before remount: the new `blkd` calls `dma_release_quarantine`
(the kernel resets the transport if any pin was quarantined) and `Driver::new` resets; `fsd`'s first block request
sits in the channel until `blkd` has done both, so the order of ADR-0054 holds.

Rights (ADR-0053, ADR-0054): `blkd` gets the console (`WRITE`), the device with `MAP | INTERRUPT | DMA` (no
`DUPLICATE` or `TRANSFER`) and its channel, no job. `fsd` gets the console and three channels. The shell gets what it
had at TASK-12 plus the root connection with every file right. Each process gets what it uses and no device handle
reaches the shell. No finding.

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-20 | weakness | `docs/adr/invariants.md` INV-0080-1, `user/src/bin/t_storage.rs` | The check cannot see whether the device was reset before the remount. Mutant Q7 (`Driver::new` does not write Status 0) and Q8 (`blkd` does not reset when its client goes) both survived `make test-qemu-one`; Q7 also survived `make test-files`. The kernel's quarantine release resets the device anyway when a pin was left. | A regression that removes either reset passes; with both removed and a clean unpin, an old request could land after the remount. | Partly fixed: host test `a_new_driver_resets_the_device_first`. `blkd`'s reset on exit stays checked by reading; recorded in ADR-0080's open questions. |
| R-21 | weakness | `user/src/bin/t_storage.rs` `the_driver_dies` | "During a commit" and "during a read" do not put a request on the device. On one CPU with a non-preemptive kernel, `t_storage` runs on through `job_kill` and blocks in `wait_exit`; `fsd` then sends its first block request, and `blkd`, woken with its exit pending, ends at its return to EL0 without reading it. So every case is "driver dead before the request reached it", and the assertion accepts three reasons. | The case ADR-0054 worries about, a device with writes in flight when its driver dies, is not exercised. Verified by reading the code and the scheduler rules in `design.md`; not instrumented. | Recorded as a limitation (design.md) and in the backlog. |
| R-22 | weakness | `user/src/bin/init.rs` `power_down` | `let _ = root.sync();`: a failed sync at shutdown is not reported. | A person types `exit` after writing with a failing device: nothing says the last writes are lost, and the machine powers off. `poweroff` in the shell does report. | Fixed: `init` prints that the last writes may be lost. |
| R-23 | note | `user/src/bin/init.rs` `main` | If `clone_as` or `start_shell` fails while the stack is healthy, `init` waits on a port that only storage events reach. If `start_stack` returns `None`, it prints "no more restarts" without having restarted. | A machine with no shell and nothing to wake `init`. Not reachable with the current ramdisk. | Fixed: no shell with a healthy stack ends `init` with a message; a stack that does not start says so and runs the shell without a volume. |
| R-24 | note | `user/src/bin/init.rs` | Races between shell exit and storage failure are benign: one packet decides. If the failure comes first, the shell is ended and restarted, so a typed `exit` is lost and a new prompt appears; if the exit comes first, the sync fails (R-22) and the machine powers off. | | No change needed. |

## 5. Invariants, against their checks, with mutants

Every row whose status became `checked` in this range. "Caught" means the named check failed on the mutant.

| Invariant | Named check exists and tests the rule? | Mutants (result) |
|---|---|---|
| INV-0052-4 | yes: host test and `t_storage` | S24 unknown ordinal closes the connection: caught |
| INV-0053-3 | partly: `storage-down` passes through two other paths | Q1 `init` stops watching the service: **survived**; Q2 also stops seeing the supervisor channel close: **survived** (the driver's exit still wakes `init`); Q3 restarts forever: caught |
| INV-0054-4 | partly (R-4, R-5) | V2, V5 caught; V1, V3, V4 **survived** (R-4); Q6 **survived** (R-5) |
| INV-0054-5 | yes (with R-21's limit) | S25 `DriverClosed` ignored: caught; Q4 `fsd` not subscribed to the driver's close: caught by `t_storage` |
| INV-0054-6 | partly: one file compared | Q5 `blkd` replies OK to a flush without sending it: **survived** `test-qemu-one`, `test-files` and 12 crash rounds |
| INV-0055-1 | partly: no model comparing completion order with reply order exists; `test-crash` has one client | S12 reply before effect: caught |
| INV-0055-2 | yes | S10 `sync` without commit: caught; S11 `durable` ignored: caught |
| INV-0055-3 | **no**: 6 of the 15 guarded requests are tested, and `clone` only from a connection without `CLONE` | S1 `clone` adds rights: **survived**; S2 open creates without `CREATE`: **survived**; S3 truncate without `WRITE`: **survived**; S4 set_attributes, S5 make_link, S6 read_directory, S7 read_buffer, S8 write_buffer without their rights, S9 token target's right not checked: all **survived**. Also R-12. |
| INV-0055-4 | yes | S13 no report after the timed commit: caught; S15 report repeated: caught |
| INV-0055-5 | weakly: counts copies, does not change the buffer during a request | S16 second copy: caught |
| INV-0055-6 | yes | S17, S18: caught |
| INV-0055-7 | **no** (R-16) | S21 off by one: caught; S19, S20 undercounting: **survived** |
| INV-0055-8 | yes | S21, S22: caught |
| INV-0055-9 | yes | S23 a record per connection: caught |
| INV-0080-1 | **no** (R-20) | Q7, Q8: **survived** |
| INV-0080-2 | yes | S27 four restarts: caught; Q3: caught |

The four rows said to be "checked whole" now:

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-25 | weakness | `docs/adr/invariants.md` rows INV-0010-3, INV-0013-1, INV-0013-5, INV-0015-4; ADR-0010, ADR-0013, ADR-0015 notes | The ADR notes say these are now checked whole, but the Status column of all four still names only the TASK-07 engine tests, so the table does not say what checks them (CLAUDE.md: keep the Status column true). | `make adr-check` cannot see the claim; a reader of the table sees TASK-07's checks. | Fixed: the Status column names the new checks for INV-0010-3, INV-0013-5 and INV-0015-4. |
| R-47 | error | `docs/adr/invariants.md` INV-0055-3 | The row says `checked`, but its test covers 6 of the 15 requests a right guards, and `clone` only from a connection with no `CLONE` right, so the clause "`clone` never adds one" is untested. Nine mutants that remove a rights check (S1-S9) all survived, and R-12 is a live bypass. | Mutant S1 lets a connection with `CLONE | READ` clone itself with every right, including `WRITE` and `REMOVE`, and `make test` passes. | Fixed: `every_guarded_request_needs_each_of_its_rights` covers 16 requests, each without each needed right, and `clone`/`open` widening; six rights mutants, including clone-adds-rights, are now caught. |
| R-26 | weakness | INV-0015-4 | "Each virtio status mapped to the trait's error type": `UNSUPP` becomes `status::FAILED` in `blkd` and `DevError::IoError` in the adapter; `DevError::Unsupported` is never produced. The mapping is lossy, so the row is not whole. Mutant V17 (UNSUPP treated as OK) was caught. | An unsupported request is reported as a write-back failure. | Accepted: ADR-0015's note now says INV-0015-4 is still checked in part. Backlog. |
| R-27 | weakness | INV-0013-1 | The row is about the `abi` `Name` constructor, with boundary and property tests against an independent predicate. There is no `abi::Name`; `zoraya-protocol`'s `Name::is_valid` has a table of 12 cases and no property test. Its rule is right (`char::is_control` is exactly U+0000-U+001F and U+007F-U+009F). | | Accepted: ADR-0013's note says INV-0013-1 is not checked whole. Backlog. |
| R-28 | note | INV-0013-5, INV-0010-3 | Judged whole: every service request's names go through `Name` or `Path`, `t_storage` sends `..`, absolute and control-character paths; mutant P1 (`Path` accepting dots) was caught by `zoraya-protocol`'s test, though not by the service's tests, since the engine refuses the name too. INV-0010-3: the service forwards the report, and mutant Q11 (`fsd` never reports) was caught by `t_storage` and would hang `device-error` (in `device-error` `fsd` stays alive, so only the report can wake `init`). | | No change needed. |

## 6. The crash test

What it covers: a writer that creates, writes and closes files in a fixed order and syncs every 8 (or 512) files, killed
by SIGKILL at a random time in 0-2 s; then `zoraya-fsck` on the image, a second boot, and `crashcheck`, which requires
that files up to the last durable line are whole, that what exists is a prefix of the writer's order, and that only the
last may be empty. One more round checks `T_COMMIT` without a sync.

What it cannot cover, as the author states: killing QEMU loses nothing the guest wrote, because QEMU has already
handed every write to the host. So no write is lost or reordered, and a missing flush or a wrong order inside a commit
is invisible unless the kill falls between the two halves.

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-29 | weakness | `tools/test-runner/src/crash.rs`, `docs/adr/invariants.md` INV-0055-1, INV-0054-6 | The test is cited as a check of durability, but nothing in `make test` sees whether a flush reaches the device from the real driver: mutant Q5 (`blkd` answers `Flush` with OK and sends nothing) survived the whole storage part of `make test`. Mutant C2 (the engine skips the flush between the data and the root record) survived `make test-crash` with 12 rounds; the engine's host crash test catches it (H-C2), so the gap is only in the QEMU path. | A driver or adapter change that drops flushes passes every test; durability on a real power loss is then gone. | Recorded as a limitation (design.md) and in the backlog. |
| R-46 | weakness | `tools/test-runner/src/crash.rs` | `Guest` has no `Drop`, and every error path returns with `?` after `Guest::boot`, so a failing round leaves QEMU running, holding the pipe of whoever started the test. | Seen twice in this session: after mutants Q13 and C3 failed, `zoraya-crash-test` exited and its QEMU kept running (one for 10 minutes, until I stopped it by PID). In CI an orphan holding stdout can keep the job alive until its timeout. | Fixed: `Guest` kills its QEMU when dropped. |
| R-30 | note | `user/src/bin/crashcheck.rs` | The prefix check is sound for this writer: a gap ends the loop and the listing count then disagrees; a short file fails the content compare. It does not check the files that came from `rootfs/`, and the writer only creates; rename, unlink and overwrite are not crash-tested in QEMU. | | No change needed; the limitation is in design.md. |
| R-31 | note | `tools/test-runner/src/crash.rs` | Whether kills land inside commits is not measured. With `crashwriter 8` the writer commits about 500 times a second (6352 files in 1.5 s in this run), each with two flushes, so a fair share of kills should fall inside one. Mutant C1 (root record written and flushed before the data) was caught in round 3 of 12 (`zoraya-fsck`: checksum mismatches), which shows that kills do land between the two halves of a commit. | | No change needed. |

## 7. Stack depth

Built with `CARGO_PROFILE_USER_STRIP=false CARGO_PROFILE_USER_DEBUG=1` into a separate target directory, and
analysed from `xcrun llvm-objdump -d`: frame sizes from the prologues, a call graph from `bl` and tail `b`, every
`blr` given an edge to every closure and vtable shim, panic and formatting paths left out, and every recursive
component counted `MAX_DEPTH` (8) times.

- `fsd`: worst path about **16.6 KiB** of 64 KiB, through `main` (1,808 B), the loop, the engine and a stable sort
  (4,160 B). The engine's recursive functions are `insert_rec` (592 B a level), `remove_rec` (544), `first_from`
  (480), `write_child` (400), `count_dirty` (32) and `Rc` drop (32); at 8 levels the largest is 4.7 KiB. Recursion
  is bounded: `tree_height` is refused above `MAX_DEPTH` when the root record is decoded, and each level descends by
  `level.checked_sub(1)`. `select_root` (mount) has the largest single frame, 5,344 B.
- `blkd`: about 9.9 KiB (`main` 8,656 B, which holds `attach`'s 8 KiB of runs). `init`: 1.6 KiB. `zosh`: 9.0 KiB
  (`main` 6,464 B).

So depth is bounded with a wide margin. Remaining blocks on the stack in the engine are in `check.rs` (fsck, not in
`fsd`) and `mkfs`'s zero block.

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-32 | note | build | Nothing checks stack depth: the fix holds for this compiler and `opt-level = 1`. No test reaches tree height 8 in QEMU. | A larger frame from a compiler or profile change overflows in a deep tree only. | No change needed: recorded in design.md's engine section. |

## 8. Hostile clients, the shell, the heap and the kernel changes

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-33 | error | `user/src/files.rs` `File::open`, `clone_as`, `call` | `open` and `clone_as` create a channel and then call `call`; when `call` fails, both ends leak. `call` fails before sending for every path the encoder refuses (`..`, too long, control characters), and also leaks `server` when `channel_write` fails. | Verified in QEMU with `make run`'s kernel: 509 times `cat ../x` (each `InvalidArgument`) and the 510th prints `zosh: cat: VolumeGone`; the shell's table is full, so every file command and program start fails until reboot. | Fixed: `call` closes the handles it was to move when the request never goes, and `open`/`clone_as` hold the client end in a `File` from the start. |
| R-34 | weakness | `user/src/bin/zosh.rs` `Volume::cat` | The text is printed in 4,096-byte chunks, and a chunk that splits a multi-byte character fails `from_utf8` and is skipped silently. | `cat` of a UTF-8 file longer than 4 KiB with a non-ASCII character across a 4,096-byte boundary leaves out a whole chunk. Verified by reading. | Fixed: `cat` cuts between characters. |
| R-35 | note | `user/src/files.rs` `read_all`, `File::read` | `read_all` has no limit and `read` panics if the server returns more than asked. | `cat` of a file larger than the shell's heap can grow (a 60 MiB file on a 128 MiB machine) panics the shell, and `init` then powers the machine off. | Fixed: `read_all` stops at 16 MiB with `LimitReached`; `read` refuses an over-long reply instead of panicking. |
| R-36 | note | `user/src/bin/fsd.rs` `copy_from`, `copy_to`; `DriverLink::data` | The SAFETY comments are true about the range, but `copy_nonoverlapping` from memory a client can change concurrently, and a `&mut [u8]` over memory a device may write, are data races in Rust's model; "reads it only once" is what the code intends, not what `copy_nonoverlapping` promises. A volatile or `ptr::read_volatile`-based copy would match the comment. | None observed. | Backlog. |
| R-37 | note | `user/src/bin/blkd.rs` `Window` | "`offset` is a register": `CONFIG + 4` is 0x104, and the kernel only checks that the transport fits in its page up to `REG_STATUS + 4`. A transport declared with a size under 0x108 at the end of a page would make this read leave the mapping. | Not on QEMU `virt` (transports are 0x200 bytes, 0x200-aligned). | Backlog. |
| R-38 | note | `kernel/src/object.rs` `Object::same` | `Image`, `Interrupt`, `Pin` (and anything else not listed) compare unequal even with themselves. | `object_same(h, h)` is false for an interrupt handle. Only tokens use it now. | Backlog. |

Every `unsafe` block in the range (`blkd`, `fsd`, `heap.rs`, `stack.rs`, `fsbench`) has a `// SAFETY:` comment, and
`heap.rs`'s are true (one thread; `Heap` does not allocate; each `grow` maps a fresh object that is never unmapped).
`event_create` and `object_same` take no right and copy nothing from user memory; `object_same` reveals only whether
two handles the caller holds name one object.

## 9. TASK-12's `7d22484` and `2beb534`, again

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-39 | weakness | `user/src/bin/zosh.rs` `slots` (7d22484) | `slots` calls `handle_type(Handle::new(slot))`, a handle of generation 0. A slot that has been reused has a higher generation, so its handle is not shown. ADR-0057 made generations per slot; `design.md` calls this "the shell's own handles, by type", and INV-0053-2's boot test reads it. | Verified in QEMU: after 400 `stat` commands and `cd docs`, `pwd` prints `/docs` but `slots` shows no current-directory channel. A `Device` handle in a reused slot would also be invisible. | Recorded as a limitation (design.md) and in the backlog: listing a table needs a kernel call. |
| R-40 | note | `kernel/src/device.rs` (7d22484) | `end` is now `checked_add`, but `base + FRAME_SIZE` is not: a transport in the last page of the address space overflows (a panic in the debug kernel). | Only with a device tree naming such an address. | Backlog. |
| R-41 | note | `tools/test-runner/src/script.rs` `Once` (7d22484) | `o TEXT` counts occurrences so far, at that step: a second occurrence later in the run is not seen. In `console.script` it is placed after the first prompt, where that is enough. | | Backlog. |

The rest of the two commits is correct as far as I can see: `Path::is_valid` makes every component a `Name`
(`a_path_has_no_dots_or_empty_parts`), `handle_type` maps an unknown row to `BadHandle`, the `t_channels` message with
three handles closes each, `run_first` is the path the normal kernel takes, and making `spawn_first` and `Power`
unconditional only adds a `Power` handle to the test kernel's `hello`.

## 10. Documentation

| id | severity | location | what is wrong | failure scenario | Disposition |
|---|---|---|---|---|---|
| R-42 | weakness | `crates/zoraya-proto-file/src/lib.rs`, ADR-0052 "Evolution" | `Attach` gained a `size` field, `Rename`'s mode changed type and gained `EXCHANGE`, `Publish` gained a `mode`, and `open_flags` gained `UNNAMED`, all in place. ADR-0052 says new fields mean new ordinals, and ADR-0080's list of small choices names only `Failure`, `event_create` and `object_same`. Nothing is deployed, so the cost is nil, but the change of an accepted ADR's rule is not recorded. | | Recorded in ADR-0080 as an exception, with the reason. |
| R-43 | note | `docs/design.md` | Stale lines: "the in-QEMU tests ... with an empty virtio-blk device attached" (they now get `build/test-disk.img`); "The servers come with TASK-13"; "`exit` alone ends the shell, and the kernel then powers off" (`init` syncs and powers off); "Three processes, each in a sub-job" lists `blkd`, `fsd` and `init`, and `init` is not in a sub-job. | | Fixed. |
| R-44 | weakness | `docs/design.md` measurements table | Partly reproduced. Two sets of three `fsbench` runs per accelerator (`make run`'s 64 MiB image, `--test --keys`), the first while mutants were building, the second with only another QEMU job on the host (load 2.1). Last run of the second set, KiB/s: HVF inline 4 KiB write 119,253 / read 45,977; inline 1 MiB 256,058 / 74,447; memory object 4 KiB 269,668 / 55,587; memory object 1 MiB 2,463,267 / 104,301; sync 369 us after 4 KiB inline, 6,740 us after 1 MiB inline, 3,586 us after 1 MiB through memory; boot 0.105 s. TCG: 2,629 / 2,547; 23,259 / 8,121; 6,279 / 3,964; 327,365 / 11,665; sync 3,858 / 14,466 / 10,524 us; boot 0.194 s. Most HVF rows match the table within run-to-run spread, as does boot time; the 1 MiB memory-object write (table 4,061 HVF, 502 TCG; seen 1.9 to 2.5 GB/s and 284 to 327 MB/s over six runs each) and the TCG 4 KiB writes (table 5.1 inline and 8.3 memory; seen 2.5 to 2.7 and 5.3 to 6.3) are well below it. The table also says MB/s where `fsbench` prints KiB/s without saying how it converted, and does not say which `sync` (inline or memory object) the 3.3 ms is. | | Accepted: design.md says the numbers vary by up to a half between sessions. |
| R-45 | note | ADR-0080 | It replaces ADR-0054's "Service death with a live driver", and ADR-0054's "The driver is a process holding ... a supervisor channel from `init`" goes with it (`blkd` has none) without being named. The task's step 9 asks for a review before the ADR is accepted; the implementation already follows the proposal, which CLAUDE.md allows as the least committing option. | | Fixed: ADR-0080's tensions name the supervisor channel. |

CLAUDE.md: no external crate was added (`Cargo.lock` gains only workspace crates); every new decision I found is in
ADR-0080 except R-42; known limitations are recorded in `design.md`, but R-11, R-13, R-14 and R-33 are not.

## Mutants tried

Host mutants ran `cargo test -p zoraya-fs-service --test service` (S) or `cargo test -p zoraya-virtio` (V). QEMU
mutants ran the target named. Each mutant was applied alone to a clean checkout of HEAD.

| id | change | command | result |
|---|---|---|---|
| S1 | `clone` does not check the asked rights | service tests | survived |
| S2 | `open` with `CREATE` does not need `CREATE` | service tests | survived |
| S3 | `open` with `TRUNCATE` does not need `WRITE` | service tests | survived |
| S4 | `set_attributes` without `WRITE` | service tests | survived |
| S5 | `make_link` without `CREATE` | service tests | survived |
| S6 | `read_directory` without `READ` | service tests | survived |
| S7 | `read_buffer` without `READ` | service tests | survived |
| S8 | `write_buffer` without `WRITE` | service tests | survived |
| S9 | token's connection right not checked | service tests | survived |
| S10 | `sync` replies OK without a commit | service tests | caught |
| S11 | `DURABLE` ignored | service tests | caught |
| S12 | write replies before its effect | service tests | caught |
| S13 | no report after the timed commit | service tests | caught |
| S14 | no `after_engine` after `dispatch` | service tests | survived (each handler also calls it) |
| S15 | failure reported on every call | service tests | caught |
| S16 | buffer copied twice | service tests | caught |
| S17 | held reply keeps the `Requests` subscription | service tests | caught |
| S18 | reply dropped when the queue is full | service tests | caught |
| S19 | tokens not counted in `open` | service tests | survived |
| S20 | buffers not counted in `open` | service tests | survived |
| S21 | `room()` off by one | service tests | caught |
| S22 | `TooManyOpen` closes the connection it came on | service tests | caught |
| S23 | `attach_second` makes a new record | service tests | caught |
| S24 | unknown ordinal closes the connection | service tests | caught |
| S25 | `DriverClosed` ignored | service tests | caught |
| S26 | outage reads counted as corruption | service tests | survived |
| S27 | four restarts | service tests | caught |
| S28 | adapter's `flush` returns Ok without sending | service tests | caught |
| S29 | adapter ignores a write's `Done` status | service tests | survived |
| S30 | adapter ignores a read's `Done` status | service tests | survived |
| V1 | no `new > outstanding` check | virtio tests | survived (later checks catch the same device) |
| V2 | no id alignment check | virtio tests | caught |
| V3 | no id range check | virtio tests | survived |
| V4 | repeated id accepted | virtio tests | survived (V1's check catches the test's device) |
| V5 | unknown status accepted | virtio tests | caught |
| V6 | status byte not primed with `0xff` | virtio tests | survived |
| V7 | no barriers around the `avail.idx` store | virtio tests | survived (fake barriers are no-ops) |
| V8 | no `acquire` | virtio tests | survived (as V7) |
| V9 | 16-bit indexes with `+` | virtio tests | survived (no test passes 65,535) |
| V10 | `Driver::new` does not write Status 0 | virtio tests | survived |
| V11 | `QueueReady` not checked | virtio tests | survived |
| V12 | DRIVER_OK before the queue is set up | virtio tests | survived |
| V13 | FEATURES_OK not re-read | virtio tests | caught |
| V14 | interrupt not acknowledged | virtio tests | caught |
| V15 | write data marked device-writable | virtio tests | caught |
| V16 | IOERR not recorded | virtio tests | caught |
| V17 | UNSUPP treated as success | virtio tests | caught |
| Q1 | `init` does not watch the service process | `make test-files` | survived |
| Q2 | Q1, and the supervisor channel's close not watched | `make test-files` | survived |
| Q3 | `init` restarts without a limit | `make test-files` | caught (`storage-down`) |
| Q4 | `fsd` not subscribed to the driver channel's close | `make test-qemu-one` | caught (`t_storage`) |
| Q5 | `blkd` answers `Flush` without sending it | `make test-qemu-one test-files test-crash` (12 rounds) | survived |
| Q6 | `blkd` does not check the capacity | `make test-qemu-one` | survived |
| Q7 | `Driver::new` does not write Status 0 | `make test-qemu-one test-files` | survived |
| Q8 | `blkd` does not reset the device when its client goes | `make test-qemu-one` | survived |
| Q9 | `blkd` replies OK when the device reported an error | `make test-files` | caught (`device-error` times out) |
| Q10 | the block adapter ignores a write's `Done` status (S29 in QEMU) | `make test-files` | caught (`device-error`) |
| Q11 | `fsd` never sends its report | `make test-qemu-one test-files` | caught (`t_storage`) |
| Q12 | `fsd` waits for the driver with no deadline | `make test-qemu-one` | survived |
| Q13 | `fsd` ignores the commit deadline in its wait | `make test-crash` | caught (the no-sync round) |
| C1 | commit writes and flushes the root record before the data | `make test-crash` (12 rounds) | caught in round 3 (`zoraya-fsck`) |
| C2 | commit skips the flush between the data and the root record | `make test-crash` (12 rounds) | survived |
| C3 | `sync` replies OK without a commit (S10) | `make test-crash` | caught |
| H-C1 | C1 | `cargo test -p zoraya-fs --test crash` | caught |
| H-C2 | C2 | `cargo test -p zoraya-fs --test crash` | caught |
| P1 | `Path::is_valid` only refuses a leading `/` | `cargo test -p zoraya-protocol -p zoraya-fs-service` | caught by `zoraya-protocol` only |

Probes added in a scratch clone (not committed): the five host tests behind R-11, R-12, R-14 and R-15, and three
scripted QEMU sessions behind R-33 (509 refused `cat`s), R-39 (`slots` after `cd`) and R-44 (`fsbench`).

## Check of the fixes

A second subagent, which wrote neither the code nor the review, checked commit 642862a only.

### Report (commit 642862a)

Scope: only the fixes in 642862a, against the dispositions in `docs/tasks/TASK-13-review.md`. Clone at
`scratchpad/check` (branch stage-4b, HEAD 642862a). Each mutant was applied alone and reverted.

Commands run on the unmodified fix: `cargo test -p zoraya-virtio` (16 passed), `cargo test -p zoraya-fs-service --test
service` (39 passed), `make adr-check` (0 errors, 17 warnings, all "not yet applicable (stage 6)"), `make test-files`
(pass, including `device-error`, with `zoraya-fsck` clean).

| id | severity | location | finding | evidence | Disposition |
|---|---|---|---|---|---|
| C-1 | error | `crates/zoraya-fs-service/tests/service.rs` `every_guarded_request_needs_each_of_its_rights`; `docs/adr/invariants.md` INV-0055-3; review R-47 | R-47 is marked Fixed and INV-0055-3 `checked`, but the test still leaves out `read_buffer`, `write_buffer` and the right checked on a token's target directory (`token_directory`). Three of the review's nine rights mutants (S7, S8, S9) still survive, and so does R-12's fix where it applies to `write_buffer`. ADR-0080 now lists `read_buffer` and `write_buffer` as guarded. | Mutants, each run with `cargo test -p zoraya-fs-service --test service`. S7 (`read_buffer` without its READ check) survived. S8 (`write_buffer` takes `handle_of` when `write_handle` refuses, skipping both WRITE and SYNC) survived. S8b (`write_buffer` uses `writable_handle`, so a durable write needs no SYNC) survived. S9 (`token_directory` without `has(target, right)`) survived. S1, S2, S3, S5, `read_link` without READ, and durable `write` without SYNC were caught. | Fixed: the test now covers `read_buffer`, `write_buffer`, a durable `write_buffer`, `publish`, and the token's directory for `rename` and `publish`. S7, S8, S8b and S9 are caught. |
| C-2 | weakness | `crates/zoraya-fs-service/src/lib.rs` `flush_held`, `close_connection` (held handles); review R-15/R-16 | The count is correct for a held reply that carries a handle, but no committed test covers it. The random model never makes a client queue `Full` (`room` is never set), so a token copy is never held. | Mutants that drop `self.open -= sent` in `flush_held`, or drop `self.open -= 1` for held handles in `close_connection`, both survived the service tests. I added a probe test (scratch only, not committed): a `GetToken` with room 0, then either room is given or the client closes, and the fake's `owned` is compared with `open_count`. It passes on the fix and catches both mutants. | Fixed: `a_held_token_copy_is_counted_until_it_goes`, for a client that reads and one that closes. Both decrement mutants are caught. |
| C-3 | note | `lib.rs` `get_token`, the `room()` check before the copy | With the check removed (S-copyroom), nothing fails. Without it, `open` can go one past `MAX_OPEN`: only for a moment if the reply is sent, but it stays there while the reply is held. | The mutant survived the service tests and my probe, because the probe's reply is sent at once. | Fixed: the limit test gives the root a token first, then asks for a copy at the limit. The mutant is caught. |
| C-4 | note | `lib.rs` `add_connection`; `user/src/bin/fsd.rs` `main` | No test covers the new refusal path when `interest` fails, because the fake's `interest` always returns true. `fsd` ignores the `bool` from `add_root`. If the root's subscription fails, the root end is closed and `fsd` waits with no connection. | Mutant S-interest (`return Some(index)` instead of closing) survived. `fsd.rs:291` `service.add_root(root);`. I found this by reading and did not reproduce it. | `fsd` ends if `add_root` fails. The refusal path stays untested: the fake's `interest` always succeeds. |
| C-5 | weakness | `user/src/files.rs` `call`, `File::open`, `clone_as`; review R-33 | The fix is correct and works in QEMU, but no committed test fails without it. `files-write.script` sends a single `cat ../hello.txt`. | QEMU probe script, 1300 × `cat ../x` then `cat hello.txt`. On the fix (normal kernel, `--keys`) all 1300 were refused with `InvalidArgument`, then it PASSED. With `handle_close` in `unsent` removed, `VolumeGone` started after 1015 refusals and the run FAILED (timed out waiting for `hello from the filesystem`). | Fixed: `files-write.script` sends 1100 refused `cat ../x` and forbids `VolumeGone`. With the fix removed it fails. |
| C-6 | note | `user/src/files.rs` `call` | No double close. On any failure the kernel's `channel_write` checks and allocates everything before it moves a handle (`kernel/src/syscall.rs` `channel_write`; ADR-0050: "the caller keeps every handle"), so the handles to move still belong to the caller when `unsent` closes them. Only `Transfer::Move` entries are closed. The callers (`open`, `clone_as`, `rename`) do not close the moved handle themselves. | Read the code. | No change needed. |
| C-7 | note | `crates/zoraya-virtio/src/lib.rs` `collect`, `queue`; review R-2 | The length check matches QEMU's virtio-blk. From memory of QEMU's source (not verified), the used length is `req->in_len = iov_size(in_iov)`, taken before the status byte is stripped: data + 1 for a read, 1 for a write or a flush, on success and on error alike. This is the same as `writable`. An error status with a short length is accepted. In QEMU 11.1.2, `make test-files` passes with the check (reads, writes, sync and a device write error). | `make test-files` PASS. Mutants in `cargo test -p zoraya-virtio`: no `length > writable` check caught; no short-read check caught; `writable = data + 1` for writes caught (3 tests). R-4's V9 (`+` for `wrapping_add`) and V3 are now caught. `place` mutants (no capacity check, no empty check, off by one) were all caught by `a_range_is_placed_only_inside_the_device_and_the_memory`. | No change needed. |
| C-8 | note | `zoraya-virtio` `collect`; review R-1 | The `acquire` is now between the `used.idx` load and the entry and status loads. As the review expected, no host test can catch its removal, because the fake's `acquire` is a no-op. | Mutant without the second `acquire`: survived. Read the code: `blkd`'s `acquire` is `fence(SeqCst)`. | Accepted: a barrier cannot be tested with the fake device. |
| C-9 | note | `crates/zoraya-fs-service/src/lib.rs` handle accounting | I found no double close, underflow, or decrement for a handle that was never counted. Each decrement has a matching increment. `send`: Sent moves the handles, Closed or a failed reservation goes through `drop_handles`, and held handles stay counted. `flush_held` Sent subtracts what it sent, and Closed goes to `close_connection`, which closes and subtracts the held handles. `get_token` counts the copy before `send` and drops it on an encode error. `send` is never called with handles on a connection that already holds a reply, because `ready` returns early when one is held. | Read the code. The probe in C-2 asserts `owned == open_count` after a held reply is sent, and after a held reply's client closes. | No change needed. |
| C-10 | note | `user/src/bin/init.rs` `main`; review R-23 | The health probe holds today. `fsd` mounts only read-write (`MountOptions::default()`), and a device without flush fails the mount, so `start_stack` fails. A `sync` therefore fails only on a failed or dead service. A failed service sends a report, or exits if it cannot (R-17); a dead one sets TERMINATED. Either wakes `init`. A read-only mount would bring the hang back (`ReadOnly` → STORAGE_FAILED with no report), but none exists. | Read `crates/zoraya-fs/src/volume.rs` (`mount`, `sync`, `writable`) and `fsd.rs:269`. | No change needed. |
| C-11 | note | `crates/zoraya-virtio/src/tests.rs` `a_used_length_is_not_trusted` | The last block's comment says "A short read that failed wrote nothing", but the block queues a write under `ShortRead`. The assertion message is right. | Read the code. | Fixed: the comment. |
| C-12 | note | R-3, R-10, R-14(b), R-17, R-19, R-22, R-23, R-34, R-35, R-46 | These fixes are correct as far as I could see, but no test fails without them, and the dispositions do not claim one. | Read the code only. | Accepted: these are fixed by reading, as the dispositions say. |
| C-13 | note | review rows marked Backlog or recorded | The records are accurate. `docs/backlog.md` has R-6, R-7, R-8, R-9, R-18, R-21, R-26, R-27, R-29 and R-36 to R-41. design.md's known limitations cover R-21, R-29 and R-39. ADR-0080 records R-20's open question, R-42's exception and R-45's tension. ADR-0013 and ADR-0015 now say INV-0013-1 and INV-0015-4 are not checked whole. The invariants rows that changed name tests that exist and pass. INV-0055-3 is the exception (C-1). | `git show 642862a -- docs/`, `make adr-check`. | No change needed. |
