# Backlog

Known small problems, to take up when the area is next worked on. Each id is a row of the fifth (V) or sixth (W) review in
[`research/stage-4/review-notes.md`](research/stage-4/review-notes.md), which has the evidence. Where a review was
limited to the lines it changed, what it finds outside them is added here.

| Id | Where | What |
|---|---|---|
| V3 | ADR-0050 Object | "Copied once more" holds only for a structure read as input. An output structure in one `Buffer` needs no extra copy, so the rejection of that alternative overclaims. |
| V4 | ADR-0057 INV-0057-1 | The reason for treating 875 ns as an upper bound on the rate is weak. A generation takes an install and a close, two system calls, which is the better argument. |
| V6 | ADR-0055 driver 4 | `fs/requirements.md` has an anchor for part of the driver. F7's reason for leaving it uncited is not the whole story. |
| V10 | ADR-0050 `channel_create`, ADR-0054 `device_map`, `dma_pin` | Lower-case `out` beside the `Out` kind. Say that these are `Buffer`, or allow `Out` for fixed records. |
| V11 | ADR-0051 INV-0051-5 | `Out` is in the check, not in the rule, while ADR-0050 says the invariant names it. |
| V12 | ADR-0054 Interrupt object | "ADR-0045's lint is enough" is stale: INV-0051-5 now covers the index. |
| V13 | Depends lines | ADR-0050 lacks ADR-0044, and ADR-0051 lacks ADR-0050 and ADR-0054. |
| V14 | ADR-0057 Option B, INV-0057-1 | Two sets of figures, for 1 microsecond and 875 ns. Use one rate. |
| V15 | ADR-0057 Option C, Consequences | 11 bits hold slots up to 2047. `POWER` keeps its value too, and is not named. |
| W3 | ADR-0051 Calls | "An address kind for the last" now reads as `length`. Name `memory_unmap`'s address. |
| W5 | ADR-0041 note, ADR-0054 Tensions | The exception to INV-0041-1 covers frames only; a quarantined pin also keeps heap use. |
| W6 | ADR-0053 What `init` receives | "`init` keeps them" now reads as giving `Power` to a restarted driver. |
| W8 | ADR-0054 Invariants | INV-0054-7 is out of order against `invariants.md`. |
| - | ADR-0054 Consequences | "Quarantined frames stay lost until a new driver runs" no longer covers a live driver that closes a pin. |
| - | ADR-0051 INV-0051-5 | Its list of payloads, from the sixth review's backlog items. |
| - | `zoraya-events` | The signal table keeps its peak size, so one burst of channels stays as heap until reboot. |
| R-23 | ADR-0062 INV-0062-1 | "Only the first process starts with a `Power` handle" is false now that `init` hands one to `zosh`. It should say that only the kernel makes one. |
| R-27 | ADR-0050 No cycles | The rule on moving an end with handles queued holds for `channel_write`. `spawn` can move such an end, which makes no cycle. Say "through a channel". |
| - | `zoraya-fs-service` | A loss seen during a request is reported by what it lost (`DEVICE_WRITE`), not as `DEVICE_LOST`. `init` treats them alike, so nothing depends on it yet. |
| - | `zoraya-protocol`, `zoraya-fs` | Two name validators, kept alike only by their tests (ADR-0013 open question). |
| R-18 | `zoraya-fs-service` `fs_status` | `ReadOnly`, `BadHandle` and `Internal` all become `StorageFailed`: a read-only mount reports every write as a storage failure. |
| R-6 | `zoraya-virtio` `Driver::new` | Capacity is two 32-bit reads with no `ConfigGeneration` loop; config-change interrupts are ignored; `F_RO` and `F_ACCESS_PLATFORM` are not considered. None of it happens with QEMU's device. |
| R-7 | `blkd` `Queues::publish` | The notify store is ordered by `dmb ish`; Linux uses `dmb oshst` before MMIO. Safe under QEMU, where the store traps. |
| R-8 | `zoraya-virtio` `Kind::Flush` | A flush is queued whether or not `F_FLUSH` was negotiated; unreachable, since the engine mounts read-only without flush. |
| R-9 | `zoraya-virtio` `Shared` | A safe trait whose implementation dereferences any offset: safety rests on the driver passing only its own offsets. |
| R-21 | `t_storage` | No test kills a driver with a request on the device (see design.md, Known limitations). |
| R-26 | INV-0015-4 | The device's `UNSUPP` becomes `FAILED` in `blkd` and `IoError` in the adapter: `DevError::Unsupported` is never produced. |
| R-27 | INV-0013-1 | `zoraya-protocol`'s `Name` has a table of cases and no property test against an independent predicate. |
| R-29 | `blkd` flush | Nothing in `make test` sees a flush reach the device from the real driver. |
| R-36 | `fsd` `copy_from`, `copy_to`, `DriverLink::data` | Copies from memory a client or device may change are data races in Rust's model; a volatile copy would match the SAFETY comments. |
| R-37 | `blkd` `Window` | The kernel checks a transport's size only up to `Status`; a read at 0x104 could leave a short mapping. Not on QEMU `virt`. |
| R-38 | `kernel/src/object.rs` `Object::same` | `Image`, `Interrupt` and `Pin` compare unequal even with themselves. Only tokens use it now. |
| R-39 | `zosh` `slots` | Generation 0 only: a reused slot's handle is not shown. Needs a kernel call to list a table. |
| R-40 | `kernel/src/device.rs` | `base + FRAME_SIZE` is not checked for overflow; only a device tree naming the last page reaches it. |
| R-41 | `tools/test-runner` `o` step | Counts occurrences so far, not over the whole run. |
| W7 | ADR-0054 Quarantine | A pin can also end when a queue is torn down: the text reads as a full list of ways. |
| - | `kernel/src/arch/aarch64/usercopy.rs` | Copying between user memory and the kernel is byte by byte: 3.2 us a KiB under HVF. After stage 4, try word-sized copies through the unprivileged instructions, with a measurement. |

| - | `gpud` under TCG | A virtio-gpu completion's interrupt is sometimes not seen, about once per driver's life, early on (TASK-14). `gpud` polls the queue every 10 ms as a fallback. Not seen under HVF, nor for `blkd` in stage 4's tests. Find out whether the kernel's interrupt object, the GIC model or the driver's order of acknowledgements loses the edge; virtio-mmio is edge-triggered on QEMU `virt`. |
| - | `blkd`, `zoraya-virtio` | `blkd` keeps its own split queue beside the generic one in `queue.rs`. Move it onto the generic one, with its mutation tests, when the blk driver is next worked on. |
| - | GUI, `zosh` | Programs started from `zosh` in the window print to the serial console. Byte streams (stage 6) or a terminal connection handed to each child would fix it (ADR-0096). |
| - | `compd` `Platform::call` | The display driver's end is subscribed on two ports while a call waits (the main port and the call port). It works; a single port with a per-call key would be simpler. |
| - | Fuzzing | `zoraya-term`'s parser and the GUI decoders have deterministic random tests in `make test-unit`; no coverage-guided target exists for them in `fuzz/` (no nightly toolchain on the host in TASK-14). |

## Mutation testing: `zoraya-virtio`

`make mutants-host CRATE=zoraya-virtio` (cargo-mutants 27.1.0), 2026-11-02, branch `tests/virtio-mutants`. Before:
168 mutants, 139 caught, 25 missed, 4 unviable. The `mutant-triage` subagent sorted the 25; new tests in
`crates/zoraya-virtio/src/tests.rs` cover the holes. After: 156 caught, 8 missed, 4 unviable.

- **Killed: 17.** Each was a hole in the tests, not a bug in the driver: every new test passes on the unmutated code.
  The holes were a request with exactly 30 pieces (and a literal 31 refused), each request of a batch reaching its own
  sector, a failure of the second request in a batch, an answer left intact by a later `queue`, the high words of a
  queue address above 4 GiB, `FAILED` kept when the device already shows it, and the queue layout: rings aligned as
  2.7 requires, not overlapping, and with the headers within the four pages `QUEUE_MEMORY` promises.
- **Equivalent: 8**, all left missed on purpose.
  - Six in the layout constants (`lib.rs` 93:34, 93:38, 93:42 twice, 94:44 twice). Each changes a size that is then
    rounded up to 4096 and lands on the same page, so `USED_AT` and `HEADERS_AT` keep their values.
  - 265:36 writes the reserved header word at `header - 4` instead of `header + 4`. The queue memory is zeroed
    (`Driver::new`'s contract) and nothing else writes either place, so the word is 0 either way.
  - 275:36 `|` to `^` on the descriptor flags. `DESC_NEXT` (1) shares no bit with the data flags (0 or `DESC_WRITE`,
    2), so the two give the same value.
- **Left: 0** that are not equivalent.

The triage's one miss: it said the 30-piece test kills `MAX_PIECES` = 16, but a test that takes its count from
`MAX_PIECES` changes with it. The test uses a literal 30.
