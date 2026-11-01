# Backlog

Known small problems, to take up when the area is next worked on. Each id is a row of the fifth review in
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
