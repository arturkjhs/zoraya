# TASK-29: coverage holes (two targeted questions)

Read-only audit: grep and reading only. Nothing was built or run, so no claim here is about a test passing or
failing today; every claim is about what the test's source would or would not detect.
"CONFIRMED" = I read the invariant, the check and the enforcement site. "SUSPECTED" = inferred, not fully read.

## 1. Mechanical results (Question A, part 1)

Parsed `docs/adr/invariants.md`: 700 rows; 551 `checked`, 128 `not yet applicable`, 21 `replaced by`.

- Commands named in `checked:` statuses: every `make` target exists in the `Makefile`, every `cargo test -p` crate
  exists, every `--test NAME` has its `tests/NAME.rs`, every `tools/...` script exists. Every back-quoted test
  function name in a status (80 names) exists as a `fn` in the tree (the two misses are C names,
  `pthread_cond_wait`, `pthread_mutex_lock`). No dangling command or test name.
- 349 of the 551 `checked` statuses are bare (a command, no test named). Largest groups: `make test-unit` 112,
  `make test-qemu` 73, `make test` 27. For these the table alone does not say which test to read.
- 130 `checked` invariants whose id appears nowhere in `kernel/ user/ crates/ tools/ tests/ fuzz/ ports/ Makefile
  .github/` (the places to look first; ADR: items):
  0004: 7; 0005: 4; 0011: 3; 0016: 1,2,3; 0020: 1-5; 0021: 1,2,3; 0022: 1,2; 0023: 1-4; 0024: 1,3; 0030: 2,3;
  0031: 1,2,3,5; 0033: 2; 0034: 1,2; 0046: 4; 0051: 5; 0053: 3; 0060: 1,4; 0072: 2; 0091: 1,2; 0093: 6; 0097: 1;
  0110: 1,4; 0140: 1; 0141: 1; 0142: 1; 0144: 1; 0154: 3; 0164: 2; 0169: 1,3; 0170: 2,3,4,5; 0171: 1; 0180: 1,2,3;
  0190: 1-5; 0191: 2; 0200: 1; 0210: 1; 0211: 1,2; 0213: 1,2,3; 0223: 2,4; 0241: 1; 0244: 1; 0245: 1,2,3;
  0246: 1,2; 0247: 1,2; 0281: 1; 0286: 2; 0287: 1,2,3; 0288: 1,2,3; 0289: 1,4; 0290: 1; 0294: 1,2; 0295: 2;
  0297: 1; 0298: 1,2; 0299: 1; 0302: 1,7; 0304: 2,4; 0305: 1; 0306: 2; 0309: 3; 0313: 1; 0314: 1,2;
  0315: 1,2,3,5,7,8; 0316: 1,2,4; 0318: 1; 0319: 1,2; 0320: 1,2,5; 0321: 2; 0322: 1,2; 0330: 2.
  Security-relevant among them: 0091-1/2, 0295-2 (which driver gets which device), 0170-2..5 (clipboard and drag),
  0245-1..3 (chooser wiring, sheet guard), 0289-1/4 (TLS failures, `/system` read-only), 0319-1/2 (record locks),
  0297-1 (`+net` rules), 0298-1 (test hook absent from the product).
- `not yet applicable (stage N)` for a stage whose code is built (finding C1 below): stage 6: 6 rows, stage 7: 5,
  stage 8: 24, stage 9: 20.

## 2. Findings

### C1. Security invariants still marked "not yet applicable" for stages that are built (CONFIRMED)
- Rows: `INV-0172-3`, `INV-0172-4` say `not yet applicable (stage 7)`; the roadmap in `docs/design.md` marks stage 7
  `(done)`. `INV-0220-1`, `INV-0220-2` (the scope rule: no frame for a destination outside the scope),
  `INV-0224-1..5` (`+net` grants), `INV-0228-2`, `INV-0228-3` (roots only with a network grant), `INV-0230-1..4`,
  `INV-0289-3`, `INV-0002-5` say `(stage 8)` or `(stage 6)`, while `netd`, `+net`, TLS and the broker exist
  (`docs/design.md` "Network service", "TCP, names and fetch", `user/src/bin/t_netd.rs`, `tests/shell/net-*.script`).
- Why it matters: for these the table claims no check at all, so nothing ties the tests that do exist to the
  invariant. Some have tests anyway (`INV-0172-3`: `crates/zoraya-compositor/src/tests_drops.rs:214`; `INV-0220-2`:
  `user/src/bin/t_netd.rs:275-363`, `crates/zoraya-net/src/service_tests.rs:392`, `conn_tests.rs:1218`); some have
  none (C2, C3).
- Proposal: `make adr-check` refuses `not yet applicable (stage N)` when the roadmap row N is `(done)`; a pass over
  the 55 rows to name the existing tests or record the gap. No code outside docs/tools.

### C2. `INV-0172-4`: "a room can hold only entries its creator's table covered" has no test at all (CONFIRMED)
- Enforcement: `user/src/bin/zosh.rs:3646-3658` (`room new`: "outside what this shell holds"). It is in the `zosh`
  binary, not in a host-tested crate. `grep -r "outside what this shell holds"` over `tests/ crates/ user/` finds
  only that line: no script expects the refusal and no host test reaches it.
- Violation that passes: delete the `covers` check. A shell inside room `project` (+w ~/project) runs
  `room new wide +w /`, enters it and holds the volume. Every test still passes.
- Proposal: in `tests/shell/rooms.script`, inside `agent-1`: `room new wide +w /` expects
  `outside what this shell holds`, then `room` shows no `wide`. Needs no code outside tests (better: move the check
  into `zoraya-shell` or `zoraya-rooms` with a host test, a small refactoring).

### C3. `INV-0230-1` (the network restrict rule) is unchecked, and most network steps run without `restrict=on` (CONFIRMED)
- Rule: "`make test` opens no connection to any address outside `127.0.0.1` and the guest's SLIRP network";
  method: "`restrict=on` is asserted in the printed command line".
- As built: `tools/run-qemu.sh:286-287` drops `restrict=on` under `--net-open`, and `--net-open` is passed by the
  fetch, DNS, lookup, POSIX, apps, python and NetSurf runs (`Makefile:360, 401, 447, 477, 495, 565, 697, 725`).
  Nothing asserts the command line; no `unshare`/network namespace in `.github/workflows/` or `tools/ci-*.sh`.
- Violation that passes: a script line, a port's default (curl, wget, CPython, NetSurf are third-party code) or a
  `netd` bug that names a real host reaches the Internet through SLIRP from a test run; no step fails.
- Proposal: (a) on Linux CI run the `--net-open` steps in a network namespace with loopback only; or (b) every
  `--net-open` run records a pcap (`--net-pcap` exists) and a runner check fails on a destination outside
  10.0.2.0/24. Needs Makefile/runner changes, no product code.

### C4. `INV-0040-2`: not "every call refuses a handle lacking its right" (CONFIRMED for two calls)
- Method in the ADR: "an in-QEMU test that every call refuses a handle lacking its right". Status: bare `make test`.
- I listed the 50 rights checks of `kernel/src/syscall.rs` and searched `user/src/bin/t_*.rs` and
  `kernel/src/testing.rs` for a call of each with a narrowed handle. No such test for:
  - `device_interrupt` without `INTERRUPT` (`kernel/src/syscall.rs:1098`): `t_devices.rs` makes
    `handle_duplicate(device, Rights::MAP)` (`no_dma`, line 233) but passes it only to `dma_pin` and
    `dma_release_quarantine`.
  - `interrupt_ack` without `WRITE` (`syscall.rs:1165`): only successful acks (`t_devices.rs:177, 183`).
  - `thread_interrupt` without `SIGNAL` (`syscall.rs:1471`): no test, but a thread handle cannot be duplicated
    (INV-0300-1), so a narrowed one may be unreachable; noted, not counted.
- Violation that passes: replace `Rights::INTERRUPT` by `Rights::NONE` at line 1098: a holder of a device handle
  narrowed to `MAP` takes the device's interrupt line.
- The tests are hand-written per object type; nothing compares them with the kernel's list of checks.
- Proposal: two lines in `t_devices.rs`; and a table-driven test from `zoraya-abi`'s signature table (each call,
  each handle parameter, a duplicate without the needed right). Add both mutations to
  `tools/mutants/kernel-objects.toml`. No kernel change.

### C5. The gate's rows describe tests "in an untrusted room" that run in trusted rooms; the check reads only step names (CONFIRMED)
- `tools/check-untrusted-gate.sh` verifies that a row's STEP is a step of `make test` and of CI. It cannot tell
  whether the test of that name exists in the step or does what WHAT says.
- `hostile-programs` ("TASK-21's hostile cases from inside an untrusted room"): `tests/shell/hostile.script:13`
  makes `room new hostile --mem 64M --procs 16 +w /t/w`, a trusted room (its header says "a room with limits").
  `cpu-spinners` ("two untrusted rooms"): `tests/gui/spinners.script:65,68` are trusted rooms with `--cpu`.
  `disk-filler`: `tests/shell/disk-filler.script:24-30`, trusted rooms. They cannot be otherwise today: the shell
  refuses `--untrusted` (`tests/shell/rooms.script:20-21`).
- `INV-0172-2` is one of the invariants the gate requires to be `checked`. Its method: "in-QEMU test that a program
  in an untrusted room cannot reach `~/project` and has no chooser channel to ask with". Its status is
  `checked: make test-unit and make test-gui`; `tests/gui/rooms.script` has only a trusted room (`project`) and
  the refusal line. The untrusted half is a host test of the panel's launch record
  (`crates/zoraya-panel/src/tests.rs:279`).
- Violation that passes: the markers are removed with these rows as they are. The gate opens with no end-to-end
  run of a hostile program, a spinner or a filler in an untrusted room (own `tmpd`, broker in the room's job, no
  chooser, the default limits), and `check-untrusted-gate` says ok.
- Proposal: the commit that removes the markers must switch those three scripts to `--untrusted`; make the check
  enforce it (once the markers are gone, each stage 1 row with a script names it and the script contains
  `--untrusted`), and add to `tests/gui/rooms.script` a program in the untrusted room that prints its slots and
  tries `~/project`. Alternatively a test-only way to create an untrusted room before the gate opens (a decision:
  needs an ADR).

### C6. `INV-0269-2`, third clause: no in-QEMU read of a room program's slots (CONFIRMED)
- Rule: "no program in a room but its shells and `top` holds a keeper channel"; method: "in-QEMU test reading a
  room program's `slots`". Status: `make test-unit` and `make test-qemu`.
- `t_rooms.rs:435-520` covers the first two clauses (no `LIMIT`; a `Room(R)` channel's messages). The third is
  only the panel's host test (`crates/zoraya-panel/src/tests.rs:94,150`). No script runs `probe` or `slots` inside
  a room: `tests/shell/rooms.script` runs `grants probe` (prints the plan, does not run `probe`), and
  `grants.script` runs `probe` in `home` only.
- Violation that passes: `zosh` in a room passes its keeper channel (or the room's launch job with more rights) to
  the commands it starts.
- Proposal: in `rooms.script` after `room agent-1`: `probe`, expecting the exact `slots:` line and the entry line
  for `/t/w`. That also gives INV-0153-1 a run inside a room. No code outside tests.

### C7. `INV-0295-2` and `INV-0091-2`: positive path only (CONFIRMED)
- Rule (0295-2): "On a shared page `init` gives no driver the page, only its transport." Check: `make test-gui`
  with `tests/gui/net.script` (17 lines): it expects `init: gui: gpu on its own page`, `init: device slot`, a
  prompt and `net status` with a lease.
- Violation that passes: `init` (`user/src/gui.rs:98-141`, in a binary's module) hands `ethd` the page's handle as
  well as its transport. Everything boots and the lease arrives. The kernel's own rule (INV-0295-3,
  `t_devices.rs:341-420`) stops two holders at once, so the damage is bounded, but the invariant about what `init`
  gives is not tested. Also the script's first line expects the text "gpu on its own page" on a machine where the
  page is shared; that line is printed regardless (`user/src/bin/init.rs:691`).
- Same shape for INV-0091-2 (`make test-gui` boots and `init` reports).
- Proposal: each driver prints its device slot's type and rights at start in the test image (or `init` prints the
  handle list it gives each driver, from the same list it passes to `spawn`), and `net.script` expects
  `ethd: device: transport` with no `MAP`. Small code in `init`/drivers behind the test build.

### C8. In-QEMU half of `INV-0093-2` is the stage 5 list; the lists are hand-written (CONFIRMED, low)
- `user/src/bin/t_gui.rs:460-510` sends six messages on a client channel: `AddClient` 32, `AttachDisplay` 33,
  `AttachInput` 34, `Stats` 35, an input frame, a `Key` event. Not sent against the real compositor:
  `AddRoomClient` 37, `AttachClientManager` 38, `SetRooms` 39, `SetDock` 40, `AddSystemClient` 42, `ApplySettings`
  43, `SetUsage` 44, nor the system requests 11 and 12 (sheet, `Granted`) from an unmarked client.
- Host side: `no_client_message_changes_a_record_or_its_pixels`
  (`crates/zoraya-compositor/src/tests_rooms.rs:240-320`, INV-0172-1 "every client ordinal") sends 32, 37, 33, 34,
  38, 39, 40 by hand; 43 from a client is in `tests_settings.rs:61`; 44 is sent only on `init`'s manager channel
  (`tests_usage.rs:286`), never on a client's; 42 only on the panel's (`tests_sheets.rs:114`). No test iterates
  `MANAGER_REQUESTS` / `CLIENT_MANAGER_REQUESTS` (`crates/zoraya-proto-gui/src/lib.rs:581-585`; they are used only
  in `zoraya-proto-gui`'s own disjointness test).
- The dispatch is by channel (`lib.rs:716-717` against `client_message`, line 3005), so a break is unlikely by
  accident; but a new manager message accepted in `request()` would not be caught by either list.
- Proposal: one host test that, for every ordinal of the two tables with 0 and 1 handles, sends a well-formed
  message on a client channel and compares records, rooms, dock, usage, settings and client count; and the same
  loop in `t_gui::no_injection`. Tests only.

### C9. `INV-0245-2` / `INV-0167-3` in QEMU: one right probed, and any error counts as the refusal (CONFIRMED, low)
- Rule: "A file the file manager opens reaches the application with `READ | CLONE` only"; check:
  `tests/gui/files.script` ("the write is refused").
- `user/src/bin/t_open.rs:76-79`: `match fs.write_all(..) { Ok => "allowed", Err(_) => "refused" }`. Any failure
  (a closed channel, a storage error) prints "refused"; and only `WRITE` is tried, not `SYNC`, nor a `clone_as`
  with more rights, nor (for Save's narrowed folder) `remove`/`rename`/`list`.
- Violation that passes: the panel passes the file with `READ | SYNC | CLONE`, or the write fails for another
  reason. The exact rights are compared on the host (`crates/zoraya-chooser/src/tests.rs:73`), so this is the
  wiring only.
- Proposal: `t_open` prints the status (`not granted`) and runs `probe`'s request list on what it was given
  (`user/src/bin/probe.rs:36-71`). Tests only.

### C10. `probe` does not try "every request of ADR-0055" (CONFIRMED, low)
- INV-0153-1 method: "tries every request of ADR-0055 through each connection". `probe.rs` tries stat, clone
  (widen), read, write, sync, share, list, create, mkdir, remove, rename, token, narrow, unnamed, publish.
  `zoraya-proto-file` also has `SetSize` 19, `SetAttributes` 20, `MakeLink` 21, `SetFlags` 25, `WriteBuffer` 8,
  `WriteCurrent` 29, `OpenWithMode` 27, `FifoOpen` 34, `SetLimit` 38, `CloneAccount` 40, `CorruptReads` 17.
- Covered on the host by `every_guarded_request_needs_each_of_its_rights`
  (`crates/zoraya-fs-service/tests/service.rs:558`), so the hole is only the in-QEMU claim. I did not check that
  `CorruptReads` (a fault-injection request) is refused in the production `fsd`.
- Proposal: extend `probe` (truncate, set attributes, make link, set limit); check `CorruptReads` on a production
  image. Tests only.

### C11. Record locks: the C boundary is not exercised with two owners (CONFIRMED absence, low)
- INV-0319-1/-2 are host tests of `Posix::fcntl_lock` over a hand-built 32-byte record
  (`crates/zoraya-posix/tests/files.rs:1648-1850`); they do check conflicts, `close` and `fork`.
  `tests/posix/t_posix.c` has no `F_SETLK`/`F_GETLK`; the only C caller is SQLite's `threadtest3`, which the ADR
  itself calls "not evidence for the lock logic (one process, one owner)".
- Violation that passes: the layout of musl's `struct flock` differs from the offsets the Rust side reads, or the
  `fcntl` shim routes `F_OFD_SETLK` to the POSIX table; with one owner nothing conflicts and `threadtest3` passes.
- Proposal: a `t_posix locks` scenario in C: two `open`s, `F_OFD_SETLK` conflict gives `EAGAIN`, `F_GETLK` reports
  the range and pid, `close` of a second fd drops the POSIX lock, a `fork` child holds none. Tests only.

### C12. Statuses that name fewer checks than the invariant's method (CONFIRMED by reading the rows; the missing halves mostly exist)
- `INV-0289-4` (`/system` read-only): status names only `cargo test -p zoraya-fs-service --test service`; the
  method also lists `zoraya-shell`'s `a_write_grant_never_reaches_the_system_directory` and
  `tests/shell/system-ro.script`. Both exist and `system-ro.script` is a thorough negative test (redirection, cp,
  rm, mv in and out, mkdir, `+w` at three levels, `+w /`, `probe`, then the file unchanged). The host test's loop
  "by any spelling of the path" has one spelling (`service.rs:2951`); the seal is decided on the first path
  component (`src/lib.rs:1279-1288`), names `.` and `..` are refused by the engine and links are returned to the
  client (`IsLink`), so I found no bypass; `//system/x` and a root link to `system` would be cheap to add.
- `INV-0228-1`, `INV-0289-1`: status `cargo test -p zoraya-tls` only; the in-QEMU half exists
  (`Makefile:393-395, 601-603`: the expired and wrong-name servers end after 0 request bytes).
- `INV-0167-4`, `INV-0172-6..10`: the method says a GUI screendump test; the status is `make test-unit` (host
  renders). For 0172-10 a GUI test cannot exist while untrusted rooms are refused.
- `INV-0093-1`: the in-QEMU test covers keys (`t_gui.rs:393`); the pointer half is host only.
- Proposal: correct the status cells (the ADR's method is the reference); no test needed except where noted.

## 3. Question B: refusal rules at trust boundaries and their negative tests

| Rule | Negative test found | Verdict |
|---|---|---|
| A call without the right is refused (ADR-0040) | `t_handles`, `t_ports:149`, `t_streams:275`, `t_image:199`, `t_devices`, `t_quota:95`, `t_console:95-107`, `t_snapshot:942` | covered except C4 |
| No duplicate/transfer with more rights; no transfer without `TRANSFER` | `t_channels.rs:181-189`, `t_spawn.rs:72-84`, `zoraya-handle` over all masks | covered |
| Forged handle values, bad pointers | `badptr`, `fuzz` in `hostile.script`; `t_handles` | covered (in a trusted room, C5) |
| `spawn` gives nothing implicitly; personality-only calls | `t_spawn` (`child.rs:17`), `t_snapshot.rs:1199` | covered |
| A command sees only its grants; links do not lead out | `tests/shell/grants.script` | covered in `home`; not in a room (C6); request list partial (C10) |
| A room holds only what its creator held | none | C2 |
| A room's program cannot write outside its grants | `tests/gui/rooms.script:66-69`, `hostile.script` | covered for trusted rooms; untrusted: C5 |
| Keeper: no `LIMIT`, `Room(R)` messages, limit above the session's | `t_rooms.rs:435-520`, `rooms.script` | covered; slots clause C6 |
| `/system` read-only | `system-ro.script`, `service.rs:2921` | covered |
| Chooser gives `Open` read-only, `Save` one name | `tests/gui/files.script` + `t_open` | covered, weakly (C9) |
| A client cannot send manager/input messages, read others' events | `t_gui.rs:393-510`; host tests | covered for the stage 5 ordinals (C8) |
| A client cannot get the trusted mark, a sheet, the tint; cannot report `Granted` | host only: `tests_sheets.rs:93`, `tests_rooms.rs:240` | host only; no attempt against the real `compd` (C8) |
| No input to a sheet within 500 ms; no paste/drop without the gesture | host only (`tests_sheets.rs`, `tests_drops.rs:305`) | host only; the code under test is the crate `compd` runs |
| A driver gets only its own device | host test of `identify`; kernel `t_devices` | `init`'s handing-out untested (C7) |
| A transport handle cannot map or widen | `t_devices.rs:410, 572-612` | covered |
| Network scope: outside the scope is `NotGranted`; no widening by `narrow` | `t_netd.rs:275-363`; `service_tests.rs:392`, `conn_tests.rs:1218` | covered for ping, connect, resolve; rows say "not yet applicable" (C1). No listen or datagram request exists in `zoraya-proto-net` to test |
| Tests reach no outside network | none | C3 |
| TLS: bad certificate fails before any byte; no option turns it off | `net-fetch.script`, `apps-refusals.script`, `Makefile:393, 601, 701` | covered |
| Quota refusals (memory, processes, CPU, disk) | `t_quota`, `t_cpu`, `t_rooms`, `disk-filler.script`, `rooms.script` | covered; service state: `fsd` (`t_storage.rs:257`), compositor (`t_gui.rs:523`), `netd` (`t_netd.rs:156`); the broker and `tmpd`, which ADR-0260 lists as hard, have no flood test, and `tools/check-quota-tests.sh` is satisfied per resource, not per service (INV-0262-1's status admits "the broker" is not checked; its "netd ... not yet checked" looks stale) |
| A POSIX window only in a trusted room (INV-0317-1) | panel host test (`tests.rs:630-660`); the shell's refusal `zosh.rs:1566` is in the binary, untested | host only; not examined how the panel learns the asking shell's room |

## 4. What was sampled and what was not

- Read against their tests (about 40): INV-0040-2/-3, 0054-1/-7, 0062-1/-2, 0091-1/-2, 0093-1/-2/-3, 0094-1,
  0153-1/-2/-7, 0167-2/-3/-4, 0172-1/-2/-3/-4/-10, 0220-2, 0228-1, 0230-1, 0245-2, 0260-1, 0261-3, 0262-1,
  0263-4, 0266-1, 0268-1/-2, 0269-1/-2, 0289-1/-4, 0295-1/-2/-3, 0317-1, 0319-1/-2/-3, 0321-1, 0323-1.
- Found sound on reading: INV-0153-1/-2/-7 (`grants.script`), INV-0289-4 (`system-ro.script`), INV-0321-1,
  INV-0261-3/0269-2 clauses 1-2 (`t_rooms`), INV-0295-1/-3 (`t_devices`), INV-0054-1 (the kernel walks the
  mappings; a Normal mapping would give count 0), INV-0220-2 in `t_netd`.
- Not reached: the 207 `make test-unit` rows outside the security ADRs; the filesystem engine's crash/integrity
  invariants; stage 9 thread invariants; ADR-0264 CPU figures; `fuzz/` targets' content (only that the gate's
  names exist); whether ADR-0321's option list equals the script's; INV-0317-1's panel wiring; INV-0318-2;
  `tests/posix/*.script`; INV-0298-1's string check; ADR-0292 (`SetLimit` rights) and ADR-0293.
