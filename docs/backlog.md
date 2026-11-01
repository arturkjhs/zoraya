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
| W7 | ADR-0054 Quarantine | A pin can also end when a queue is torn down: the text reads as a full list of ways. |
