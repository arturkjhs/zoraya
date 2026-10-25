# ADR-0010: Filesystem guarantees to clients (atomicity, ordering, durability, errors)

- **Status:** proposed
- **Date:** 2026-10-25
- **Depends on:** ADR-0002, ADR-0005, ADR-0006
- **Evidence:** CP-12, CP-14, CP-19, CP-27, X-14, DM-037, DM-038, DM-045, GR-014, fs/requirements.md#12-linux-syscalls, fs/requirements.md#2-what-the-toolchain-depends-on-for-correctness, fs/requirements.md#21-durability-is-never-requested, fs/requirements.md#22-atomic-rename, fs/requirements.md#23-exclusive-create, fs/requirements.md#24-unlink-while-in-use-and-cleanup-on-interrupt, fs/requirements.md#25-timestamps, fs/requirements.md#5-questions-the-data-cannot-answer, fs/requirements.md#6-requirements-derived-for-the-adrs, fs/survey.md#comparison, fs/survey.md#crash-consistency-as-applications-experience-it, fs/survey.md#ext4, fs/survey.md#f2fs, fs/survey.md#fxfs-and-minfs-fuchsia, fs/survey.md#littlefs, fs/survey.md#plan-9-cwfs-fossil-and-venti, fs/survey.md#testing-crash-consistency, fs/survey.md#zfs, task-02/libc-surface.csv, task-02/syscall-surface.csv

## Context

The filesystem is a user-space service reached through handles (ADR-0002 rule 1; native calls name objects only by
handle and relative path, INV-0002-1). This ADR decides what the service promises its clients: what is atomic, what
order clients can rely on, what "durable" means and how a client asks for it, and how a write-back error reaches the
client. ADR-0011 to ADR-0015 must satisfy it. The IPC protocol between client and service does not exist yet; this
ADR assumes only that every request gets exactly one reply and that the reply comes after the request's effect is
decided `[assumption: to be fixed by the channel/message ADRs]`. Regular files are not waitable (ADR-0006 rule 1),
and completion packets are reserved for later (ADR-0006 rule 7); nothing here depends on them.

**What is measured** (`fs/requirements.md`):

- The measured toolchain **never asks for durability**: no `fsync`, `fdatasync`, `sync`, `syncfs` or `msync` in any
  run of T0–T3, and no compiled call site in any measured program (`fs/requirements.md#21-durability-is-never-requested`;
  `task-02/syscall-surface.csv`; `task-02/libc-surface.csv`). What survives a crash is therefore decided entirely by
  the filesystem's default behaviour.
- Atomic replace by rename is used by toybox `mv`, `sed -i` and `ln -f` in T0–T2 and by clang and lld in T3; tcc
  instead unlinks and rewrites its output in place (`fs/requirements.md#22-atomic-rename`).
- Exclusive create (`O_EXCL` via `mkstemp`, `mkdtemp`) and unlink-while-open (make deleting partial targets on
  interrupt, 36 of 36 runs; FIFO nodes) are relied on (`fs/requirements.md#23-exclusive-create`,
  `fs/requirements.md#24-unlink-while-in-use-and-cleanup-on-interrupt`).
- GNU make decides rebuilds by comparing modification times, so a crash that leaves a partial file with a new time
  breaks incremental builds (`fs/requirements.md#25-timestamps`).

**What is assumed.** No crash was ever injected in the measurements (`fs/requirements.md#5-questions-the-data-cannot-answer`).
Workloads beyond the toolchain (databases, package managers) that *do* call `fsync` often are out of the measured
scope (ADR-0005).

**Forces from TASK-01 and the survey.** POSIX leaves crash behaviour undefined and applications build atomic updates
from folklore; Pillai et al. found 60 crash vulnerabilities in 11 applications (DM-038). Linux reports a write-back
error once and drops the data, so a retried `fsync` succeeds falsely (DM-037); PostgreSQL's answer is to crash and
replay its own log (`fs/survey.md#crash-consistency-as-applications-experience-it`). The ext4 documentation calls the
rename and truncate idioms "broken" and still adds a heuristic to make them work (`fs/survey.md#ext4`). Linux
filesystems do not agree on what a successful `fsync` means (F2FS `fsync_mode`, `fs/survey.md#f2fs`). Multi-file
transactions were tried and abandoned (DM-045, *contested*). CP-19 asks for durability and ordering to be written
contracts; X-14 is its conflict with throughput.

## Decision drivers

1. **Defaults decide the outcome**, because the toolchain never calls `fsync` (`fs/requirements.md#21-durability-is-never-requested`).
   A contract that is only safe for programs that call `fsync` correctly is unsafe for every measured program.
2. **The idioms in use must be correct without `fsync`**: replace-by-rename, exclusive create, unlink while open
   (`fs/requirements.md#6-requirements-derived-for-the-adrs`, R-1 to R-3; DM-038).
3. **A crash must never expose data that was not written under a size or time that says it was** (R-4): stale
   blocks are a security exposure (`fs/survey.md#ext4`) and a too-new modification time silently breaks make.
4. **An error must never be lost or reported as success later** (DM-037, CP-12, CP-19).
5. **Durability costs throughput** (X-14, DM-038: delayed allocation exists because ordering is expensive). The
   contract must leave room for batching.
6. **Keep the interface small** (DM-045: broad transactional APIs did not get used; CP-27 spirit for the native side).
7. **Every promise must be checkable mechanically** by crash-state enumeration against a reference model
   (`fs/survey.md#testing-crash-consistency`).

## Considered options

### Option A: POSIX/Linux semantics (baseline)

- **Description:** Operations are atomic with respect to other processes only (`rename`: "no point at which another
  process ... will find it missing"). After a crash anything may have been persisted in any order unless the
  application called `fsync` on the file and on its directory. Write-back errors are reported once per open file
  description (`errseq_t`) and the data is dropped.
- **Evidence for:** Maximum freedom for the implementation to batch and reorder, which is why delayed allocation
  exists (DM-038). Zero emulation cost for `libposix`. Every POSIX program's expectations are, by definition, met
  (`fs/survey.md#crash-consistency-as-applications-experience-it`).
- **Evidence against:** DM-038 (60 vulnerabilities in 11 mature applications; zero-length files after ext4 shipped),
  DM-037 (lost errors, falsely successful retries). The measured toolchain calls no `fsync` at all, so under this
  option none of its outputs has any crash guarantee (driver 1). Even Linux had to add a heuristic outside the
  contract (`fs/survey.md#ext4`, `auto_da_alloc`).
- **Cost:** *(estimate)* Cheapest to implement; highest cost to every client, paid as folklore and bugs.
- **Precedent:** Linux, BSDs. Works for throughput; its crash-consistency record is DM-038.

### Option B: Ordered data, explicit per-file durability (ext3/ext4 `data=ordered` plus `auto_da_alloc`)

- **Description:** Metadata never refers to data that has not reached the disk; data of a file is written before a
  rename over it commits; otherwise no ordering across operations. `fsync(file)` makes one file durable. Errors are
  sticky per file.
- **Evidence for:** This is what made write-then-rename "work" on ext3 for years, and what ext4 restored with
  `auto_da_alloc` (DM-038, `fs/survey.md#ext4`). It closes the stale-data exposure (driver 3). Per-file `fsync` is
  cheap when only one file is dirty.
- **Evidence against:** It is a set of special cases, not a model: which pairs of operations are ordered depends on
  heuristics that Pillai et al. found to differ by filesystem and mode (DM-038). In-place rewrites (tcc) are still
  torn. It cannot be stated as one checkable rule (driver 7).
- **Cost:** *(estimate)* Moderate implementation cost; testing must enumerate each special case.
- **Precedent:** ext3 (by accident: ordered mode was a security measure, DM-038), ext4 with its heuristic.

### Option C: Every operation atomic, crash state is a prefix, explicit durability barrier (non-Unix precedent: littlefs, Plan 9 fossil, Fuchsia Fxfs)

- **Description:** Every mutating request is atomic. All completed requests on a volume have one linear order. After
  a crash the volume is in the state reached by *some prefix* of that order: no request is half-applied, and no
  request is present while an earlier one is missing. A client asks for durability with `sync(handle)`, which returns
  only when everything completed before it is durable; without it, completed requests become durable within a bounded
  interval. A write-back failure stops the volume and is reported to every later request, permanently.
- **Evidence for:** Makes every measured idiom correct without `fsync` (drivers 1, 2): a rename-over can never
  appear before the writes that preceded it. Rules out R-4 violations by construction (driver 3). One rule, testable
  by comparing each post-crash state with the prefixes of a reference model (driver 7). littlefs makes every
  operation atomic under power loss, including cross-directory rename, in about 7,700 lines (`fs/survey.md#littlefs`);
  fossil keeps the disk "always self-consistent" so no check is needed after a crash
  (`fs/survey.md#plan-9-cwfs-fossil-and-venti`); Fxfs journals logically and reverts torn copy-on-write data to its
  last complete version (`fs/survey.md#fxfs-and-minfs-fuchsia`, search summary only). Copy-on-write designs give
  prefix semantics naturally by committing batches in order (`fs/survey.md#comparison`). The sticky volume-wide
  error answers DM-037 the way PostgreSQL does for itself: stop, never pretend
  (`fs/survey.md#crash-consistency-as-applications-experience-it`).
- **Evidence against:** Prefix order across *all* clients means one client's `sync` forces out every other client's
  completed work, the ext3 "fsync flushes everything" cost that made Firefox's `fsync` slow (DM-038). A volume-wide
  stop on one write error is coarse: one bad sector stops all writers. It is stronger than POSIX, so programs tested
  only here may later be ported to Linux with hidden crash bugs (an argument the other way from DM-038).
- **Cost:** *(estimate)* Requires a mechanism that commits batches atomically and in order (ADR-0011). Throughput is
  bounded by commit batching; `sync` latency is one commit. `libposix` maps `fsync` to `sync` directly.
- **Precedent:** littlefs (embedded; power-loss safe by design), fossil, Fxfs (search summary), ZFS transaction groups
  for non-synchronous writes `[inference from GR-014's copy-on-write description; ZFS's ordering guarantee is not
  stated in the fetched sources]`.

### Option D: Every operation durable before it returns (synchronous semantics)

- **Description:** Option C's atomicity, plus every request is durable when its reply is sent: `O_SYNC` for
  everything, ZFS `sync=always`.
- **Evidence for:** The simplest contract: what a client saw succeed survives. No staleness bound to specify.
- **Evidence against:** ZFS documents `sync=always` as "a large performance penalty" (`fs/survey.md#zfs`). The
  toolchain issues tens of thousands of writes per W4 run (`fs/requirements.md#12-linux-syscalls`), each of which
  would cost at least one device flush (ADR-0015). X-14.
- **Cost:** *(estimate)* Same mechanism as C; throughput limited by flush latency per request.
- **Precedent:** NFS synchronous writes, `O_SYNC` (`fs/survey.md#crash-consistency-as-applications-experience-it`),
  ZFS `sync=always`.

### Option E: Client-defined multi-operation transactions (TxF, TxOS)

- **Description:** Clients group arbitrary operations into a transaction that commits or aborts as a whole.
- **Evidence for:** Fixes crash consistency and TOCTOU at once; TxOS measured about 10 % overhead for a
  transactional install (DM-045).
- **Evidence against:** Two production attempts were abandoned for complexity and lack of users (DM-045,
  *contested*); no measured program would use it (`fs/requirements.md#2-what-the-toolchain-depends-on-for-correctness`).
- **Cost:** *(estimate)* Largest: isolation, conflicts, resource limits across many objects.
- **Precedent:** Windows TxF (deprecated), btrfs transaction ioctls (removed), TxOS (research).

## Recommendation

**Recommendation (not a decision): Option C.** The contract, stated so that ADR-0011 to ADR-0015 can be checked
against it:

1. **Live visibility.** Every request takes effect at one instant between its submission and its reply. After the
   reply, every handle on the volume observes the effect. Requests on one volume therefore have a single linear
   order, the *completion order*.
2. **Atomic requests.** Each of the following is atomic, both for concurrent clients and across a crash: create
   (including exclusive create), make directory, remove a name, remove a directory, rename (within a volume,
   including across directories, with the modes *replace*, *no-replace* and *exchange*), set attributes, set size,
   publish an unnamed object (rule 7), and each write request of at most `MAX_ATOMIC_WRITE` bytes
   `[assumption: 1 MiB; to be fixed with the IPC message size]`. A larger write is split by the client library into
   requests that are each atomic and ordered.
3. **Crash state is a prefix.** After a crash and remount, the volume's state equals the state after some prefix of
   the completion order. A request that had not replied may be in the prefix or not, but never partially. Any
   request that completed before a `sync` that returned is in the prefix.
4. **Durability on request.** `sync(handle)` returns success only when every request that completed before the call
   (on any handle of the volume) is durable. A request may also carry a `durable` flag, meaning "reply only after
   this request is durable", which is a request followed by `sync`. "Durable" means it survives power loss, given the
   device contract of ADR-0015.
5. **Durability without request.** Under no failure, every completed request becomes durable within `T_COMMIT`
   after it completes `[assumption: 5 s, ext4's default commit interval, fs/survey.md#ext4]`. Clients that never
   call `sync` lose at most the last `T_COMMIT` of completed work in a crash, and always a suffix of it.
6. **Errors.**
   - Errors decided at request time are typed results of that request (CP-12): `NotFound`, `Exists`, `NoSpace`,
     `NotEmpty`, `InvalidName`, ... In particular, space is reserved when a request is accepted, so a later commit
     never fails for lack of space (CP-14).
   - **A write-back failure is fail-stop and sticky.** If the device reports an error while the service is making
     completed requests durable, the volume enters the state `Failed`. From then on: every mutating request and
     every `sync`, on every handle including handles opened later, fails with `StorageFailed`; no `sync` ever returns
     success again; the volume object asserts a `FAILED` signal (ADR-0006 rule 1) for supervisors; reads continue to
     be served. Only a remount (which reverts to the durable prefix, rule 3) clears the state. Completed requests that
     were not durable are lost, and every client that could have depended on them sees `StorageFailed` on its next
     mutating request or `sync`.
   - A checksum mismatch on read is the error `Corrupt` for that read; metadata corruption also makes the volume
     read-only (ADR-0012).
7. **Unnamed objects and atomic publish.** A client can create a file with no name in a directory, write it, and
   then give it its one name atomically with `publish(dir, name, mode)` where `mode` is *no-replace* or *replace*.
   An unnamed object that was never published does not exist after a crash. This is the native form of Linux's
   `O_TMPFILE`+`linkat` and of Windows' `ReplaceFile` (`fs/survey.md#crash-consistency-as-applications-experience-it`),
   and serves `mkstemp`, `tmpfile` and `memfd`-like uses (R-9).
8. **Not offered.** Multi-request transactions (Option E), per-file durability that skips earlier requests, and file
   locks (ADR-0005 excludes record locks).

**Personality mapping (`libposix`).** `fsync`, `fdatasync` and `sync`/`syncfs` → `sync` on the handle (or on each
volume handle in the namespace table); `fsync` of a directory → `sync` on it (redundant under rule 3, harmless);
`O_SYNC`/`O_DSYNC` → the `durable` flag on every write; `StorageFailed` → `EIO`, also returned by `close` of a
written fd; `NoSpace` → `ENOSPC`; `O_TMPFILE` → unnamed create, `linkat(AT_EMPTY_PATH)` → `publish`. POSIX gives no
crash guarantee, so every POSIX program's expectations are met (rule 3 is stronger than anything POSIX promises).

**Why C.** Driver 1 decides it: the toolchain never calls `fsync`, so only a default that is consistent by itself
protects it. C is the only option that is one checkable rule (driver 7) and makes the measured idioms correct
(driver 2) without the per-request flush cost of D (driver 5). A is the catalogue (DM-037, DM-038); B is a
collection of heuristics; E has no user.

**Principles this recommendation strains.** X-14: prefix order across clients costs throughput when one client
calls `sync` often; the mitigation is that the measured clients never do, and the revisit trigger below. CP-19's
"locks are owned by handles" is not addressed because no locks are offered.

## Consequences

- **Positive.** Write-then-rename, `O_EXCL` creation and unlink-while-open are correct with or without `fsync`. A
  crash costs at most a suffix of recent work and never consistency. `make` after a crash sees either a complete
  object file or none, *if* the compiler publishes atomically (open question 1); a partial in-place write by tcc is
  still possible but its size and time never claim more than was written. Errors cannot be lost or "fixed" by a
  retry.
- **Negative.** One volume-wide order: a `sync`-heavy client slows the others. A single write error stops all
  writers on the volume. Behaviour stronger than Linux may hide crash bugs in programs later ported back.
- **Follow-up work.** ADR-0011 must provide atomic, ordered batch commits; ADR-0015 must give the flush semantics
  that make "durable" meaningful; the IPC ADR must fix `MAX_ATOMIC_WRITE` and the reply semantics assumed here; the
  `libposix` mapping above; a reference model of this contract in a host-tested crate, used by every invariant below.

## Invariants

- **INV-0010-1.** After a simulated crash at any point of any generated request sequence, the remounted volume equals the reference model's state after some prefix of the completion order, and that prefix contains every request that completed before the last `sync` that returned success. *Check:* a model-based test in the filesystem core crate: random request sequences from several simulated clients run against the core and against an in-memory reference model; the block log is cut at every flush boundary and at random points between flushes, every device state ADR-0015 allows at each cut is remounted, and the result is compared with the model's prefixes.
- **INV-0010-2.** No remounted state shows a partially applied request: in particular a renamed name refers to the complete old or the complete new object, and no file's size or modification time reflects a write whose data is absent. *Check:* the same harness, with assertions per request type, plus a targeted test that crashes between every pair of block writes of a single rename-over and a single maximum-size write.
- **INV-0010-3.** After the device reports a write error during a commit, every later mutating request and every `sync` on every handle of the volume, including handles opened after the error, returns `StorageFailed`, no `sync` returns success, and the volume's `FAILED` signal is asserted. *Check:* a fault-injecting test device that fails the n-th write for each n over a request sequence, with assertions on all later replies.
- **INV-0010-4.** With no `sync` and no failure, a crash taken after the volume has been idle for `T_COMMIT` plus one commit loses no completed request. *Check:* a test with a simulated clock and the crash harness of INV-0010-1.
- **INV-0010-5.** No commit fails for lack of space: a request that cannot reserve the space its commit needs fails with `NoSpace` and leaves the volume unchanged. *Check:* a property test that fills the volume with random request sequences (including renames and overwrites, which need copy-on-write space) and asserts that every commit succeeds and every refusal is a `NoSpace` reply.
- **INV-0010-6.** An unnamed object that was never published is absent after a remount, and its space is free. *Check:* crash-harness test that creates, writes and crashes before `publish`, then checks the namespace and the allocator.

## Open questions

1. Should `libposix` map tcc's pattern (`unlink` then `open(O_CREAT|O_TRUNC)` and write) onto unnamed create plus
   `publish` at `close`? It would make compiler outputs atomic across crashes, but changes what a concurrent reader
   sees during the write (the old file instead of a growing one), which POSIX programs might notice. Unmeasured.
2. `MAX_ATOMIC_WRITE` and `T_COMMIT`: values are assumptions; fix them with the IPC ADR and a measurement of commit
   cost on the prototype.
3. Is volume-wide fail-stop too coarse once real hardware or network storage is in scope? On virtio-blk an error is a
   host failure (ADR-0015), so this is not measured.
4. Do any T3 or future workloads call `sync` often enough that the cross-client prefix order (X-14) matters?

## Revisit triggers

- A workload in scope calls `fsync` frequently and `sync` latency or cross-client cost is measured to dominate:
  consider an intent log (ADR-0011 Option D) or per-object durability with explicit ordering rules.
- A required program depends on multi-file atomic updates that `publish` and rename cannot express: reopen
  Option E.
- Transient device errors that succeed on retry are observed on a supported device: refine rule 6.

## Architect's decision
