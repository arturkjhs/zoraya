# TASK-26: review of the system call surface after stage 8 (ADR-0268 point 6)

An independent review by the `opus-review` subagent, which wrote none of the code, of every kernel call a program in a
room can make as stage 8 left them: `git diff stage-7h..stage-8-q7 -- kernel/src crates/zoraya-abi` (`stage-7h` is the
last stage-7 tag; `stage-8-q7` is `main` at `4250211`), read with the code around it and the crates the kernel calls
(`zoraya-charge`, `zoraya-events`, `zoraya-handle`, `zoraya-heap`, `zoraya-channel`, `zoraya-cpu`). It was checked
against ADR-0040, ADR-0261, ADR-0262, ADR-0263, ADR-0268, ADR-0270, ADR-0295 (proposed) and the earlier reviews of
TASK-26 (Q1's open warnings above all). The user-space halves the task names were read where they decide a kernel
question: the keeper's launch and its job rights (`user/src/bin/rooms.rs`, `crates/zoraya-rooms`), the refusal
markers (`user/src/bin/zosh.rs`, `crates/zoraya-panel`), the `+net` word path (ADR-0297), the services channel
(ADR-0296), `SetUsage` (ADR-0294) and `tools/check-untrusted-gate.sh`. The protocols of `fsd`, the compositor and
`netd` were not re-reviewed: they have their own reviews (Q3, Q6b, N3 to N6) and fuzz targets (Q7).

Nothing was built or run for the findings below: they come from reading, and the numbers in them are estimates,
marked as such. No kernel code was changed.

## The calls

*Rights* is what the handle must carry. *Payer* is the job charged (ADR-0261). *Masked work* is the bound on what one
call does with IRQ masked between two step points (ADR-0270; "O(1)" means a fixed amount). *Reach* says whether a
program in a room can make the call with what a launcher gives it (its launch job without `LIMIT`, or `CREATE_PROCESS`
only, its streams, channels to services, its own objects), and what it could do with it. *Test* names the test that
covers the stage-8 behaviour.

| # | Call | Rights | Payer | Masked work | Refusals | Reach and risk | Test |
|---|---|---|---|---|---|---|---|
| 0, 1, 3, 10, 13, 14 | exit, yield, clock, exit with reason, disposition, take request | none | none | O(1) | n/a | yes; nothing new | earlier stages |
| 2 | `console_write` | `WRITE` on the console | none | up to 4 KiB polled, about 60 ms (its own comment) | size, UTF-8 | only a holder of the console (note 3) | earlier stages |
| 4 | `sleep_until` | none | none | O(1); deadline queue of 288, 2 kept for the kernel | `NoMemory` when full | yes; 288 > 255 processes, so no starvation of the queue | earlier stages |
| 5 | `handle_close` | none | refund | one object; a memory object over 64 pages goes to the reaper; a channel chain closes in steps; a port frees all its subscriptions at once (error 2) | `BadHandle` | yes | `t_bound`, `t_quota` |
| 6 | `handle_duplicate` | `DUPLICATE`, rights a subset | handle table prepaid | O(1) | `AccessDenied` | yes; no type without `DUPLICATE` in its rights is duplicable, so ports, channel ends, pins and interrupts stay single | `t_handles` |
| 7 | `job_create` | `CREATE_PROCESS` on the parent | the new job (its record) | O(1), plus the parent's `children` growing (uncharged, note 1) | depth 32, `BadState` after kill, `OverQuota` | yes, with `CREATE_PROCESS` only; the new handle has every right, `LIMIT` too (ADR-0261 point 12) | `t_quota` |
| 8 | `spawn` | `CREATE_PROCESS`, `READ` on the image, `TRANSFER`/`DUPLICATE` per entry | the target job: slot, image frames, tables, record, 1024-slot table, thread and stack | 64 pages per step | process and memory limits before any page; `Cancelled` | yes; the image may be a memory object (note 4) | `t_quota`, `t_bound` |
| 9, 11, 12 | resume, exit record, `process_signal` | `RESUME`, `WAIT`, `SIGNAL` | none | O(1) | n/a | own children only | earlier stages |
| 15, 47, 48 | `job_signal`, `job_suspend`, `job_resume` | `SIGNAL` / `KILL` | none | **the whole job subtree** (error 1) | n/a | yes, on any job it made | `t_signals`, `t_suspend` (small trees) |
| 16 | `job_kill` | `KILL` | none | **the whole job subtree** (error 1) | n/a | yes; and the keeper's `room remove --kill` | `t_bound`, `t_quota` (small trees) |
| 17 | `port_create` | none | the caller: record, event entry, 64 packets | O(1) | `OverQuota` | yes | `t_quota` |
| 18 | `port_subscribe` | `WRITE` on the port, `WAIT` on the source | the caller, 128 bytes | **O(subscriptions of the port)** for the sorted insert and the ready list's growth (error 2) | `KeyInUse`, `OverQuota` | yes; no bound on subscriptions per port or per source but memory | `t_ports`, Q1 delta test not |
| 19 | `port_unsubscribe` | `WRITE` | refund | O(log n) search, O(1) removal | `NoSuchKey` | yes | `t_ports` |
| 20 | `port_queue` | `WRITE` | the port's creator, 64 packets at a time, never given back before the port ends | O(1); at most 1024 user packets | `Full`, `OverQuota` | yes; a moved port makes its creator pay for another's packets, at most 1024 × 24 bytes | `t_ports` |
| 21 | `port_wait` | `WAIT` | none | at most 64 packets | `BadAddress` before any packet leaves | yes | earlier stages |
| 22, 25 | `directory_lookup`, `directory_entry` | `LOOKUP` | handle only | O(name) | n/a | only with the boot directory | earlier stages |
| 23, 24 | `console_read`, `console_dropped` | `READ` | none | 256 bytes | n/a | console holders | earlier stages |
| 26, 50 | `power_off`, `power_reset` | `POWER` | n/a | n/a | n/a | the console shell only, without `TRANSFER` | earlier stages |
| 27 | `memory_create` | none | the caller: frames, record, list | 64 pages per step; a contiguous run's scan 48 words per step, at most 17 rounds | size, `OverQuota` before any frame, `Cancelled`, 4 MiB contiguous | yes (note 5) | `t_quota`, `t_bound` |
| 28 | `memory_map` | `READ`, `+WRITE` to write | the mapper: page tables only | 64 pages per step, the frame list's growth mapped ahead | `LimitReached` (256 mappings), `OverQuota`, `Cancelled`; partial work undone | yes | `t_quota`, `t_bound` |
| 29 | `memory_unmap` | none (own space) | refund | 64 pages per step, never cancelled | not an exact mapping | yes | `t_bound` |
| 30 | `channel_create` | none | the caller: pair, queue slots, sources | O(1) | `OverQuota` | yes | `t_quota` |
| 31 | `channel_write` | `WRITE`; `TRANSFER`/`DUPLICATE` per handle | the writer, while queued | 64 KiB, 64 handles; the reader's subscriptions are signalled (error 2) | every check before the charge and before a handle moves | yes | `t_channels`, `zoraya-channel` host tests; the in-QEMU half of INV-0261-7 is open |
| 32 | `channel_read` | `READ` | refund to the writer | 64 KiB, 64 handles | table room reserved before the message leaves | yes | `t_channels` |
| 33, 34, 35, 36, 37, 38 | device map, interrupt, ack, pin, unpin, release | `MAP`, `INTERRUPT`, `WRITE`, `DMA` | none (devices, ADR-0261 point 6) | a release waits for a reset, up to 100 ms (ADR-0295) | holders and mappings exclude each other | drivers only; no room gets a device handle | `t_devices` |
| 39, 41, 49 | `handle_type`, `object_same`, `object_get_signals` | none / `WAIT` | none | O(1) | n/a | yes; nothing new | earlier stages |
| 40 | `event_create` | none | the caller | O(1) | `OverQuota` | yes | `t_quota` |
| 42 | `stream_create` | none | the caller: 64 KiB ring, records, sources | 16 pages; **a step point with the handle table's lock held** (warning 4) | `OverQuota` | yes | `t_quota`, `t_streams` |
| 43, 44 | `stream_read`, `stream_write` | `READ` / `WRITE` | none (ring prepaid) | 64 KiB | n/a | yes | `t_streams` |
| 45 | `snapshot_process` | `CREATE_PROCESS` on a POSIX job; a POSIX caller | the child's job: slot, copy, objects, tables | 64 pages per step | `OverQuota` before the copy for the slot and objects, page by page for own pages; `Cancelled`; a device mapping | POSIX programs | `t_snapshot`, `t_bound` |
| 46 | `image_create` | `READ` on the memory | the caller: record and header table | O(table) | ELF checks | yes (note 4) | `t_image` |
| 51 | `job_set_limits` | `LIMIT` | the job's walk under `WALK` | depth ≤ 32 | above the parent's effective limit, CPU out of range; all three or none | not on the room or the launch job (the keeper strips `LIMIT`); yes on jobs it made, only narrower | `t_quota`, `t_cpu`, `t_rooms` |
| 52 | `job_get_usage` | `WAIT` | none | O(1) | buffer under 88 bytes | its own subtree; `CLEAR` clears `OVER_QUOTA` of a job it can wait on, never the room's | `t_quota`, `t_rooms` |
| 53 | `device_transport` | `MAP` on the page | none | O(1) | a transport asked for a transport, an index with no range | drivers only (warning 5: the new handle's rights do not come from the caller's) | `t_devices` |
| 54 | `device_read` | `READ` on a transport | none | one register | width 1/2/4, aligned, inside the block, 32 bits below 0x100 | drivers only | `t_devices`, `overlapping_transports_are_left_out` |
| 55 | `device_write` | `WRITE` on a transport | none (claim, at most one per transport) | one register | as 54; a second holder or a mapped page is `BadState`; checks come before the claim | drivers only | `t_devices` (`a_transport_and_its_page_keep_holders_apart`, `the_hold_ends_with_a_move`) |

## Findings

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | error | `kernel/src/job.rs:275-353`, `:369`, `:124-126` | `job_kill`, `job_signal`, `job_suspend` and `job_resume` walk the whole job subtree with IRQ masked; a limited room can make the subtree as large as its memory limit allows. INV-0270-1 does not hold. |
| 2 | error | `crates/zoraya-events/src/lib.rs:340-394`, `:429-457`, `:222`, `:259`; `kernel/src/syscall.rs:488-511` | A port may hold any number of subscriptions and a source any number of subscribers; subscribing, signalling and freeing are linear in them, with IRQ masked, in the caller's call or in a service's. INV-0270-1 does not hold. |
| 3 | warning | `tools/check-untrusted-gate.sh:83-86`, `user/src/bin/zosh.rs`, `docs/adr/invariants.md` (INV-0262-4) | There is no `+net` refusal to keep: removing the stage-1 markers opens `+net` in untrusted rooms at once, with INV-0262-4 unchecked and the per-line `+net` words (ADR-0297) unrefused in a room. |
| 4 | warning | `kernel/src/syscall.rs:1069-1071`, `kernel/src/memory/object.rs:107`, `kernel/src/memory/heap.rs:239` | `stream_create` holds the handle table's spinlock across `heap::prepare`'s step point. |
| 5 | warning | `kernel/src/syscall.rs:941-951` | `device_transport` gives the transport every device right but `MAP`, whatever the caller's page handle carries. |
| 6 | warning | `tools/check-untrusted-gate.sh:63-80`, `docs/adr/invariants.md` | The gate's check passes rows that say part is open, leaves INV-0268-3 out, and counts files, not markers (Q1 warning 7, still open). |
| 7 | warning | `kernel/src/memory/heap.rs` (the window never shrinks), ADR-0261 point 10 | Charges count live heap bytes, but the heap keeps every frame it ever took: a burst of kernel objects in the untrusted rooms moves their ceiling's worth of frames into the heap for good. |
| 8 | note | `kernel/src/memory/user.rs:72-82`, `kernel/src/job.rs:124-126`, `kernel/src/reaper.rs:128-152` | Uncharged growth on call paths (Q1 note 9, still open), now with the reaper's queue. |
| 9 | note | `crates/zoraya-events` (drops under `EVENTS`), `kernel/src/job.rs:356-372` | A `Paid` dropped under `EVENTS` must never hold the last `Arc<Job>` (Q1 warning 5): still unstated and unasserted. |
| 10 | note | `kernel/src/syscall.rs:293-304` | `console_write` keeps IRQ masked for about 60 ms at 4 KiB; no room program may hold the console. |
| 11 | note | `kernel/src/syscall.rs:340-408`, `:1186-1231` | Any process can run code from a memory object; "only code from the system image" is a rule of procedure, not of the kernel. |
| 12 | note | `crates/zoraya-rooms/src/lib.rs:707-850`, `user/src/bin/zosh.rs:2902`, `:3363`, `crates/zoraya-panel/src/lib.rs:541` | The refusal lives only in the clients; the keeper creates and launches into untrusted rooms for any `Manage` holder. |
| 13 | note | `kernel/src/memory/frames.rs:60-144`, `kernel/src/syscall.rs:617-620` | Contiguous memory is open to every process. |
| 14 | note | `kernel/src/process.rs:734-745`, `docs/adr/0295-*.md`, `docs/adr/0298-*.md` | Smaller items: the RTC's rights, the call numbers 53 to 55 ship under a proposed ADR, `held` counts the job's own record. |

### 1. error: the job tree walks are unbounded with IRQ masked (INV-0270-1)

`Job::walk` (`kernel/src/job.rs:275`) visits every job of a subtree, recursively, under `TREE`, and `signal` (`:303`),
`suspend` (`:320`), `resume` (`:331`) and `kill` (`:341`) call it from their system calls (`syscall.rs:465`, `:475`,
`:1259`, `:1264`), which run with IRQ masked and make no step point. The work is one visit per job, two locks of the
job's state, a `Weak::upgrade` and a recursion, for every job in the tree.

How many jobs a limited room can keep alive is bounded only by its memory limit. `job_create` needs only
`CREATE_PROCESS`, which `zosh` gives every program it starts (`zosh.rs:1730`, `:1735`), and the new handle carries
every right, `SIGNAL` and `KILL` included. A job keeps its parent alive (`parent: Option<Arc<Job>>`) while the parent
holds only a `Weak` of it, so one handle to a job at depth 31 keeps its whole chain: a process holding 1024 such
handles keeps about 26,000 jobs below its launch job (the launch job is at depth 5: root, `init`, session, untrusted,
room, launch), and handles in queued messages keep more. Each job is charged `arc_cost::<Job>()`, a few hundred
bytes (three counters of four words, the CPU account, two vectors, the parent link; estimated, not measured).
*Estimate:* at the default `--mem 89M` of a 512 MiB guest, 200,000 to 300,000 jobs; at 21 MiB on 128 MiB, 50,000 to
70,000.

*Scenario:* a program in an untrusted room builds chains of jobs up to its memory limit, then loops
`job_suspend(top); job_resume(top);` on its own top job. Each call walks every job with IRQ masked: at tens of
nanoseconds a visit under HVF, 5 to 20 ms a call (estimate), against INV-0270-1's 2 ms, and about ten times that under
TCG. Timer, keyboard, network and the compositor's interrupts wait that long, once per call, as often as the room's
25% CPU ceiling lets it call. `room remove agent-1 --kill` makes the keeper's `job_kill` do the same walk once.
Smaller effects of the same fact: `Job::drop`'s `children.retain` (`:369`) is linear in the parent's children, and
the `children.push` growth (`:124-126`) copies the whole vector in one masked interval.

ADR-0270 point 1 does not list these calls as sites, `t_bound` measures none of them with a large tree, and
INV-0270-1's status is `checked`. The gate's `masked-time` row therefore passes while the invariant fails.

*Fix:* either bound the tree, or walk it in steps. Bounding is simplest: a per-job limit on live descendants (for
example 1024 below any limited job, counted with the record's charge, refused as `LimitReached`), which keeps every walk
a fixed amount. Walking in steps needs the walk to drop `TREE` between steps and stay correct while the tree changes
(a cursor over children by index already exists; `kill` is safe to repeat since `closed` is set per job). Make
`children` a list that removes in O(1), or record each child's index. Then add to `t_bound` a room that fills its limit
with jobs and measures `job_suspend`, `job_signal` and `job_kill` of it, and add these calls to ADR-0270's sites (a
proposed ADR or the open-question note of rule 6).

### 2. error: subscriptions are unbounded per port and per source, and their lists are walked with IRQ masked (INV-0270-1)

`port_subscribe` (`syscall.rs:488`) charges 128 bytes and calls `Events::subscribe_paid`
(`crates/zoraya-events/src/lib.rs:326`). Keys are any `u64`, so one port takes any number of subscriptions to one
source. Under `EVENTS` (a spinlock, IRQ masked) the call does a sorted `Vec::insert` into the port's list (`:394`,
a move of the whole tail), grows the port's ready list to the number of subscriptions (`port.reserve`, `:123-126`,
a reallocation and a copy), and grows the global slab (`subscriptions`). `Events::set` (`:429`) visits every
subscription of the source whose bits rise; `free_port` (`:222`) and `remove_matching` (`:259`) visit all of a port's
or a source's, and each removal drops a `Paid`, which takes `WALK` and walks the job's limited ancestors.

At 128 bytes each, a room at `--mem 89M` holds about 700,000 subscriptions (estimate); at 21 MiB, 170,000.

*Scenario:* a program in an untrusted room opens a file service connection (a channel end it holds with `WAIT`) and
subscribes one port to that end's `READABLE` 700,000 times with distinct keys. Then it sends a request. When `fsd`
answers, `fsd`'s own `channel_write` raises `READABLE` on the room's end and `Events::set` visits 700,000 subscriptions
with IRQ masked, inside `fsd`'s call: several milliseconds per reply (estimate), more under TCG, on every reply. The
last subscriptions alone each moved about 11 MB in `insert`. Closing the port frees them all in one masked interval
in the room's own `handle_close` or exit, none of it in steps.

The Q1 delta test (`t_quota::usage_matches_the_heap_and_the_frames`) does not cover `port_subscribe`, and no test makes
many subscriptions.

*Fix:* bound subscriptions per port (64 is ample for every program in the tree; `zosh` and the services use a handful)
and per source, refused with `LimitReached`, and keep a port's list short enough that the sorted insert is a step.
Alternatively make the source's list walk a step at a time, which `set` cannot (it runs in interrupt handlers too), so
bounding is the practical answer. Add a test that subscribes to the bound and checks `MASKED_MAX_UNITS` around a
signal and a close.

### 3. warning: opening stage 1 would open `+net` in untrusted rooms (ADR-0268 point 7, the architect's decision)

The architect's decision on ADR-0268 is that the gate opens "in two stages, first without `+net`". The check reads
the `+net` refusal from lines marked `untrusted-gate: net-refusal` (`check-untrusted-gate.sh:83`), and no file has one:
`grep -rn 'net-refusal' user crates` finds nothing. So when the stage-1 markers are removed, the script requires only
that stage 2's rows are not `pending` (`ready 2`), which they are not (`net-account-flood`, `net-fuzz`), and passes.
Nothing then refuses `+net` in an untrusted room: the room words for `+net` are not built (ADR-0297 point 4), and a
per-line `+net any fetch …` typed in a shell that holds the root connection (the console shell after it enters the
room) is accepted by `zoraya_shell::netgrant` with no check of the room (INV-0224-3, the creator's-scope check, is not built). INV-0262-4 (a connection's stream rings charged to the
client's job) is `not yet applicable`, and ADR-0268 point 2 exempts it only "until point 7's second stage", which the
script never checks either.

*Scenario:* the architect, satisfied with stage 1, removes the three stage-1 markers. `make check-untrusted-gate`
passes. In `agent-1`, `+net any fetch https://…` connects through `netd` with the root connection's reach.

*Fix:* add the refusal now, as ADR-0268 point 7 words it: `room new --untrusted` and `room set` refuse `+net` and
`+listen` in an untrusted line, and `zosh` refuses a per-line `+net` word in an untrusted room
(`room: agent-1: +net in an untrusted room needs the network's quota tests`), each marked `untrusted-gate:
net-refusal`. Make the script's stage 2 require INV-0262-4 `checked`. Until then, stage 1 cannot open alone.

### 4. warning: `stream_create` makes a step point with a spinlock held (ADR-0270 point 2)

`stream_create` takes `self.process.handles` (`syscall.rs:1069`) and calls `Stream::create` under it (`:1071`), which
calls `MemoryObject::new(64 KiB, scattered)` (`stream.rs:93`), which calls `heap::prepare(128)` (`object.rs:107`).
When the 128-byte class is empty and no free span of a chunk is left, `prepare_in_steps` maps pages ahead and calls
its step point, `steps::pause()` (`heap.rs:239`), which unmasks IRQ and may switch threads, with the table's lock
held. In the test kernel `pause` asserts that no lock is held (`steps.rs:85-89`) and panics; in the production kernel
the switch happens with the lock taken. No other thread takes another process's table today, so nothing deadlocks on
one CPU, but the rule every step point relies on is broken, and SMP would deadlock on it.

*Scenario:* a program creates streams in a loop while other allocations keep the heap at a chunk boundary; one
`stream_create` lands when the class is empty. The test kernel panics "a spinlock is held across a step"; the
production kernel preempts the caller holding its table's lock.

*Fix:* in `stream_create`, reserve the two slots, release the lock, create the stream, and take the lock again to
insert (the room is kept by the reservation, as `job_create` does: `syscall.rs:335-337`). Grep for other callers of
`MemoryObject::new` under a lock (none found besides this one).

### 5. warning: `device_transport` widens rights (ADR-0040)

`device_transport` reads the page handle with `self.object(device, Rights::MAP)` (`syscall.rs:942`), which returns the
object and drops the entry's rights, and inserts the transport with `DeviceObject::RIGHTS.without(Rights::MAP)`
(`:947-950`): `READ`, `WRITE`, `DMA`, `INTERRUPT`, `DUPLICATE` and `TRANSFER`, whatever the page handle had.
ADR-0040: "Rights only decrease." ADR-0295 item 1 says `init` gives a driver a transport with the driver's rights less
`MAP`, which `init` does, but the kernel does not hold the line.

*Scenario:* a process given a page with `MAP` alone (for example, to identify devices by reading, as `init` does) makes
a transport and gets `WRITE` and `DMA` on it, then claims the transport and pins memory for it. Only `init` holds pages
today, so no room reaches this.

*Fix:* read the entry and give `entry.rights.without(Rights::MAP)`; add a `t_devices` case with a page handle that lacks
`WRITE`.

### 6. warning: the gate's check accepts what ADR-0268 point 2 does not

`invariants_ok` (`check-untrusted-gate.sh:63`) passes any row whose text contains `checked:`. These rows say in their
own status that a part is open: INV-0261-6 ("an object held from outside past the kill is open"), INV-0261-7 ("the
in-QEMU half is open"), INV-0262-1, -2 and -3 ("`netd` and the broker are not yet checked", "the other services are not
yet checked"), INV-0263-1 ("disk comes with Q5", stale), and INV-0261-5, whose in-QEMU half covers memory objects,
ports, events, channels and streams but not `job_create`, `port_subscribe`, `channel_write`, `image_create`, `spawn`,
`snapshot_process` or `memory_map` (error 2 is a direct consequence). The pattern
`INV-026[0-7]|INV-0269|INV-0270|INV-0172-2|INV-0172-3` leaves out INV-0268-3, which point 4 requires. And the refusal
count is per file: `zosh.rs` has two markers, so removing the `room new --untrusted` refusal alone (`:3363`) keeps the
file counted and the check green, with `room new agent-1 --untrusted` then accepted (entry stays refused at `:2902`).

*Fix:* require the status cell to begin with `checked:` and to contain no `open` or `not yet`; add INV-0268-3 to the
pattern; count markers (`zosh.rs` 2, panel 1), not files. Then fix the rows' texts or their tests.

### 7. warning: the heap keeps its high-water mark, the charge does not

A job is charged for the heap bytes its objects use and refunded when they go (ADR-0261 point 5). The heap takes
frames and never gives them back (`docs/design.md`, Memory: "The heap never shrinks, and memory in a size class stays
there"; `HEAP_BACKING`'s comment). So the machine pays a room's peak, while the counters pay its current use.

*Scenario:* four untrusted rooms each fill their memory limit with queued 64 KiB channel messages (charged to the
writer, held in the heap), then close the channels. The untrusted job's ceiling worth of frames is now heap, for good,
and a refunded room may take its user memory again from the frames: the untrusted job's ceiling is spent twice, once
as heap that only kernel objects can use. INV-0261-8's promise to `home` ("up to the session's ceiling less the
untrusted job's") no longer holds afterwards; the kernel's 1/16 reserve absorbs only part of it. Not measured.

*Fix:* measure it first (`HEAP_BACKING` and free frames after the scenario, on 512 MiB). Then either charge rooms for
heap growth they cause (a heap ceiling for the untrusted job, refused as `OverQuota`), or let the heap return whole
free chunks to the frames, or record the bound in ADR-0261's open questions with the measurement.

### 8. note: uncharged growth (Q1 note 9, still open)

`UserMemory.frames` (`user.rs:72-82`) grows by 8 bytes for every user frame and table, doubling (0.2 to 0.4% of a
space's charge); `Job.children` (`job.rs:124-126`), 8 bytes a child; `AddressSpace.mappings`, at most 256 entries;
the reaper's queue (`reaper.rs:128-152`), doubling with the number of large objects waiting, each still charged to its
job. All are bounded by charged objects, so none is a hole by itself; with error 1's job counts `children` reaches
megabytes. ADR-0260's table does not list them yet; INV-0261-5 says "every allocation".

### 9. note: `Paid` dropped under `EVENTS` (Q1 warning 5, still open)

Subscriptions keep a `Paid` with an `Arc<Job>`, dropped inside `EVENTS` by `free_port`, `free_source`,
`remove_matching` and `unsubscribe`. If it were the last `Arc<Job>`, `Job::drop` would call `port::free_source`, take
`EVENTS` again and panic. The reviewer found no path: a subscription ends before its owner's handle does, and the owner
process holds its job. The invariant is still written nowhere and asserted nowhere; Q1's fix (return the payers and
drop them after the lock) stands.

### 10. note: `console_write` and the console

`console_write` accepts 4 KiB and prints it on a polled UART with IRQ masked, about 60 ms by its own comment
(`syscall.rs:41-46`), 30 times INV-0270-1's bound. Programs `zosh` starts with a startup block get no console
(`zosh.rs:1733-1736`), and the room keeper gives none, but `init` gives the POSIX broker the console with `WRITE`
(`init.rs:157-161`). INV-0268-3 (the broker in the room's job, holding only what the line grants) is the condition
that keeps it from a room: its check must include "no console".

### 11. note: code from outside the image

`image_create` and `spawn` run any ELF a process holds in a memory object; nothing in the kernel restricts that to
the image. "Never run code from outside the system image as untrusted" is therefore held by the refusal in the
clients (note 12) and by procedure, not by the kernel. That is ADR-0268's design (it bounds harm, it does not filter
code), and it is why errors 1 and 2 matter: once the gate opens, any program in a room can make those calls.

### 12. note: the refusal is in the clients only

`room new --untrusted` is refused in `zosh` (`:3363`), entering an untrusted room in `zosh` (`:2902`) and launching in
one from the dock (`zoraya-panel/src/lib.rs:541`). The keeper itself creates an untrusted room's job and hands out
launch jobs in it (`zoraya-rooms/src/lib.rs:707-850`) for any `Manage` channel, from a line in `/config/rooms` that
anything able to write that file can add. Today only `zosh` and the panel hold `Manage` channels, so nothing reaches
it; the reviewer found no path. A refusal in the keeper too (marked for the gate's check) would make the gate one
place instead of three.

### 13. note: contiguous memory for everyone

`memory_create` with `CONTIGUOUS` is open to every process, up to 4 MiB a run. A refused search costs up to 17 scans of
the frame map, in steps (bounded, charged to the caller's CPU), and held runs fragment memory for drivers that need
runs at restart (`fsd`'s 2 MiB data area). Bounded by the room's limit; worth a right (or `dma_pin`'s rule) later.

### 14. note: smaller items

- `insert_devices` gives `init` the RTC with every device right (`process.rs:734-745`), `DMA` and `INTERRUPT` included;
  `init` needs `MAP` and `READ`.
- Calls 53 to 55 are ABI, but ADR-0295 is `proposed`, as is ADR-0298 for the gate's tests: the architect's queue.
- `held` in `job_get_usage` (`job.rs` `usage`) counts the job's own record (Q1 note 12, open).
- `LAST_REFUSED` (`charge.rs:39`) is still one global; the reaper drops charges but never refuses one, so it cannot
  overwrite it today (Q1 note 10).

## Checked and found sound

- **The charge path.** Every converted constructor charges before it allocates and refunds once on failure (port,
  pair, stream, memory object, thread, event, image, job record, subscription, queued message, process with its
  1024-slot table, which `exit_current` now gives back: Q1 warning 8 is fixed). `charge::arc` and `prepaid_*` drop
  the value, and so the `Paid`, when the heap fails. A refused `spawn` frees its event source (Q1 error 1 fixed).
- **Who pays.** A spawn and a snapshot are paid by the target job, page tables by the mapper, queued messages by the
  writer, a memory object by its creator for its life, a port's packets by its creator in steps of 64, bounded by
  1024 user packets. The reaper carries the `Paid` with the frames, so `held` is honest.
- **Refusal without side effects.** `channel_write` checks every handle and charges before removing any moved
  handle and before ending a hold; `channel_read` reserves table room and writes user memory before the message
  leaves; `spawn` changes nothing of the caller's until the child exists; `snapshot_process` reserves the parent's
  slot first and moves handles only after the child exists; `device_write` checks the register before it claims;
  `job_set_limits` sets all three limits or none.
- **Rights.** `handle_duplicate`, `spawn`'s and `channel_write`'s handle lists and the snapshot's table never widen
  rights; non-duplicable types carry no `DUPLICATE`; a moved handle may not appear twice (the decoders refuse it, and
  `channel_write` checks it). `job_set_limits` needs `LIMIT`, refuses above the parent's effective limit, and the
  keeper hands launch jobs without `LIMIT` (`rooms.rs:379`), so nothing in a room raises its room.
- **The transport hold.** One holder per transport, none while the page is mapped; the page neither maps nor releases
  while a transport has a holder or a pin; the hold ends with the last writable handle on close, on `Move` at
  `spawn`, `channel_write` and `snapshot_process`, and with the space at exit; lock order page, then transport,
  everywhere (`claim`, `begin_mapping`, both releases); register offsets are bounded by the block, which `init` keeps
  from overlapping.
- **Steps (ADR-0270).** `memory_create`, `memory_map`, `memory_unmap`, `spawn`'s image, `snapshot_process`, a channel
  chain's close and an exit's handles and mappings make their step points with no lock held (but error 1's walks,
  error 2's lists and warning 4), and `between` cancels a built object without leaving a charge. Frames the reaper
  hands out unzeroed (`frames::alloc`) are either zeroed (`alloc_zeroed`) or fully overwritten before anything can read
  them (`copy_page`, `new_for_copy`), so no page of a dead process leaks.
- **The ABI.** `JobLimits::decode` and `UsageFlags` refuse unknown bits; `job_get_usage` writes before it clears;
  `device_write` refuses a value wider than its access; calls 51 to 55 are not personality-only, and
  `snapshot_process` still is (INV-0002-2).
- **User-space edges the task named.** `SetUsage` is accepted only on the client manager's channel
  (`compositor/src/lib.rs:2393`, `CLIENT_MANAGER_REQUESTS`); the services channel and `NetworkRestarted` go only to the
  console shell; `Crash` and `room crash-keeper` exist only in the net-test image (ADR-0296 item 5, ADR-0298 item 2).

## Verdict: the gate stays closed

`room new --untrusted` is refused, and must stay so. Opening it now would let a program in an untrusted room hold the
machine's interrupts masked for milliseconds at will (errors 1 and 2), and would open `+net` with it (warning 3).

What blocks opening, beyond the architect's go-ahead:

1. Errors 1 and 2 fixed, with `t_bound` cases for a room full of jobs and of subscriptions, and ADR-0270's site list
   extended.
2. The `+net` refusal built and marked (warning 3), so that stage 1 can open alone; stage 2 also needs INV-0262-4.
3. The gate's check made strict (warning 6), and then every row it reads truly `checked:`. Open today:
   INV-0172-3, INV-0260-1, INV-0264-3 (the GUI bound of ADR-0298 item 4, the architect's), INV-0265-3's `/tmp` half,
   INV-0265-4, INV-0268-3 (the broker and `tmpd` in the room's job), and the half-checked rows listed in warning 6.
4. The gate's pending row `disk-filler-tmp` (an untrusted room's `/tmp` and its `tmpd`).
5. Warnings 4, 5 and 7 fixed or measured and recorded; notes 8 to 14 recorded where the report or the ADRs keep them.
6. A full CI run (`candidate-full/`) of the commit that removes the refusal, then `main`'s, green (ADR-0268 point 5).
