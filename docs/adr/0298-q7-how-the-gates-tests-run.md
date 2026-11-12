# ADR-0298: Q7, how the gate's tests run where ADR-0268 is silent

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0001, ADR-0262, ADR-0264, ADR-0265, ADR-0268, ADR-0269, ADR-0293, ADR-0296
- **Evidence:** CP-14

## Context
ADR-0268 names the tests that end the refusal of `room new --untrusted` and says each one runs in `main`'s full CI.
It leaves open how a test reaches an untrusted account while `room new --untrusted` is itself refused and no code from
outside the system image may run as untrusted (TASK-26: "Never run code from outside the system image as untrusted"); how
`fuzzing`'s "10 minutes without a finding" runs in a CI whose `make test` needs no network; and how a test makes the
room keeper end, which no program can ask. Q7 builds the rows the gate file lists as `pending`; this ADR records the
choices.

## Decision drivers
1. The gate's tests must run the production code of the service they bound (CP-14): a model of `fsd` shows nothing about `fsd`.
2. No untrusted code runs, and the refusal stays: a test that needs an untrusted account plays the room's client itself.
3. `make test` needs no network once the toolchain is installed (CLAUDE.md); a fuzz step needs a second toolchain.
4. A test hook is code that must not be in the production image (INV-0296-1's lesson).

## Considered options

### Option A: untrusted rooms for the tests only (a switch that lifts the refusal in the test image)
- **Evidence for:** The tests would be the real flow. **Evidence against:** Driver 2: an untrusted line, built and run by
  the shell, is the thing the gate withholds; a test-only switch is a way round it, and the first thing a later agent would
  copy. **Precedent:** none.

### Option B: the test plays the room's client, against the production service
- **Description:** The accounts exist in the services (`clone_account` in `fsd`, `NarrowAccount` in `netd`, an untrusted
  `AddRoomClient` record in the compositor); a test program makes the account the way the launcher does and floods through
  it, then checks that home and a second account are whole. Trusted rooms with limits (`--mem`, `--procs`, `--cpu`,
  `--disk`) carry the tests of the job's limits, from the system image's own programs. Drivers 1 and 2.
- **Evidence against:** The shell's untrusted line reader and an untrusted room's `/tmp` are not exercised (ADR-0268 point 7 and
  INV-0265-4 wait for them).

### Option C: host models only
- **Evidence against:** Driver 1; the rows say "in QEMU".

## Recommendation
**Recommendation (not a decision): Option B**, and:
1. **Rows and steps.** `keeper-restart`, `keeper-reconnect`, `prompt-rebind`: `make test-keeper`
   (`tests/shell/keeper.script`). `disk-filler` and `hostile-programs`: `make test-files` (`tests/shell/disk-filler.script`,
   `tests/shell/hostile.script`; `diskfill` is the filler and the timed sync loop). `service-floods`: `make test-qemu`
   (`t_storage`: an `fsd` account; `t_gui`: an untrusted room's surfaces). `cpu-spinners` and the GUI form of INV-0269-1:
   `make test-gui` (`tests/gui/spinners.script`). `net-account-flood`: `make test-net` (`t_netd`). `fuzzing` and
   `net-fuzz`: `make test-fuzz` and `make test-fuzz-net`.
2. **The keeper's end.** On the test image (feature `net-test`, as ADR-0296's) `room crash-keeper` writes `crash` on the
   shell's reconnect channel and `init` kills the keeper's job. INV-0296-1's check (`NET_CRASH_MARKS`) covers the two
   strings; no other image has the path.
3. **Fuzzing.** `tools/run-fuzz.sh` builds `fuzz/` with the `nightly` toolchain and LLVM's coverage flags (what cargo-fuzz
   does; cargo-fuzz itself is not needed) and runs libFuzzer for `FUZZ_SECONDS` a target, in parallel. The full CI list runs 600 s
   (ADR-0268's 10 minutes) in two jobs, 30 minutes each; a local `make test-fuzz` runs 20 s. The workflow installs
   `nightly` and runs `cargo fetch --locked` in `fuzz/` before the step, outside `make test`, as the pinned toolchain is; the
   build is offline. New targets: `spawn_request`, `keeper_messages`, `broker_tty`, `compositor_protocol`,
   `file_protocol` (their inputs are messages with valid headers around fuzzed bodies, `fuzz/fuzz_targets/wire.rs`); the
   network's are `net_rx`, `net_stack` and `net_dns_http`.
4. **A bound for the GUI half of INV-0264-3.** The runner's `latency N MS` step fails if the median keystroke is more than MS
   over one screendump read. The invariant's 50 ms cannot be read through a screendump; the script uses 300 ms to catch
   starvation, and the 50 ms of the kernel half stays in `t_cpu`.

## Consequences
- The gate file has no `pending` row of stage 1 or 2 except those that need the untrusted line reader (see Open questions).
- Two CI jobs of 30 minutes (fuzz) and one more `test-gui` pair, one `test-keeper`.
- The test image grows one hook (item 2), covered by INV-0296-1's marks.

## Invariants
- **INV-0298-1.** The image of `make test-net` (the production programs) holds neither string of the keeper's test hook.
  *Check:* `make test-net` (`NET_CRASH_MARKS`, the two marks added to ADR-0296's) and `make test-keeper`.
- **INV-0298-2.** Every fuzz target of `fuzz/` that a gate row names builds and runs for its time without a finding.
  *Check:* `make test-fuzz`, `make test-fuzz-net`.

## Open questions
- An untrusted room's `/tmp` (`--tmp`, INV-0265-4), the broker and `tmpd` in the room's job (INV-0268-3) and the untrusted line
  reader are not built; the rows that need them are not in the gate file and the invariants stay `not yet applicable`.
- The fuzz targets reach few states of the keeper (valid names and ordinals are rare in random bytes); a structure-aware
  generator would help. The 10 minutes of ADR-0268 point 3 is an `[assumption]`.
- A longer `make test-fuzz` on a bigger machine before the gate opens.

## Revisit triggers
- The architect lets a test-only switch into the shell; cargo-fuzz becomes available in the CI image.

## Architect's decision
(Left empty for the human.)
