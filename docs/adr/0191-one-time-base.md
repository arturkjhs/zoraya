# ADR-0191: One time base for every volume and for `CLOCK_REALTIME`

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0014, ADR-0080, ADR-0104, ADR-0107, ADR-0180
- **Evidence:** alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

Zoraya has no wall clock: no device on QEMU `virt` that the kernel reads gives a date (ADR-0080). Until stage 6d
each `fsd` stamped files with its own clock, the monotonic clock in whole seconds, and the engine keeps times rising
by assigning `max(wall, last + 1 ns)` (ADR-0014 rule 3). So the root volume's times went on from the latest time it
had stored, one nanosecond at a time, until this boot's uptime passed that time. `libposix`'s `CLOCK_REALTIME` was
the monotonic clock (ADR-0180 item 13).

Stage 6d adds a second volume: the temporary directory service, `fsd --memory`, on a fresh volume in memory
(ADR-0107 item 12). A fresh volume has no stored time. With each service on its own clock, a file in `/tmp` would be
stamped near the boot's uptime and a file in `/home` near the root volume's stored time, which can be weeks of
earlier uptime later. `make` comparing a target in one with a prerequisite in the other decides by those times, so
it would decide wrongly. `CLOCK_REALTIME` would be below the times of files just written.

What is measured: nothing here is chosen on a measurement. `make` reads `st_mtim` with nanoseconds (GNU make's high
resolution timestamps) and compares a target with each prerequisite.

## Decision drivers

1. One order of time across volumes: a file written after another, on any volume, has the later `mtime` (`make`'s
   correctness, the reason for this ADR).
2. `CLOCK_REALTIME` is never below a time a file service has assigned: a program that stamps something with
   `time()` and compares it with a file's `mtime` (`make`'s own checks for a file "in the future") sees them in
   order.
3. No time may go backwards on the root volume across boots (ADR-0014 rule 3 as the engine keeps it).
4. The least change to accepted interfaces: no new kernel call, no change to the volume format.

## Considered options

### Option A: Each service on its own clock (as before)

- **Description:** Every `fsd` stamps with the monotonic clock; the engine keeps each volume's times rising.
  `CLOCK_REALTIME` is the monotonic clock.
- **Evidence for:** Nothing to build.
- **Evidence against:** Driver 1 fails as the Context says: a fresh volume starts near uptime, the root volume near
  its stored time.
- **Cost:** none.
- **Precedent:** what Zoraya did through stage 6c.

### Option B: A wall clock in the kernel, set once per boot (what Linux does)

- **Description:** Linux keeps `CLOCK_REALTIME` in the kernel, set from the RTC at boot and by `settimeofday`; every
  filesystem stamps from it. Here: a kernel offset added to the monotonic clock, set once by `init` from the root
  volume, read by a new call `clock_realtime`, or by `clock_monotonic` with a flag.
- **Evidence for:** One source for every process with no plumbing; what POSIX programs expect of the system.
- **Evidence against:** A new kernel call and a global mutable kernel value that only `init` may set: a right to
  design (who may set the clock), which target 2 must not hand to a sandbox. Nothing in stage 6 needs the clock to
  change after boot.
- **Cost:** a call, a right, its tests; `libposix` and every `fsd` change as in Option C anyway.
- **Precedent:** Linux, every Unix.

### Option C: One base per boot, from the root volume, handed down by `init`

- **Description:** The root volume's `fsd` takes the base at mount: the latest time the volume holds, less the
  monotonic clock now, plus 1 ns. Its clock is then the base plus the monotonic clock, in nanoseconds, so its times
  go on from what it had. Any connection can ask the base (`time_base`, file protocol ordinal 35, answered in
  `Done`'s count). `init` asks it of the root connection and passes it to the broker (`--time-base`), which passes it
  to the temporary directory service (`fsd --memory ... --base`) and writes it into every bootstrap record it
  builds (`ZPB1`'s reserved 8 bytes become `realtime_base`); `libposix` copies its own into the records of the
  children it spawns, and a `fork` child has it in its copied memory. `CLOCK_REALTIME` is `realtime_base` plus the
  monotonic clock; a record with 0 keeps the monotonic clock (a launcher with no root volume).
- **Evidence for:** Drivers 1 to 3 by construction: every clock reads one base plus one monotonic clock, and the base
  is above every time on the root volume. No kernel change, no format change; the record had the 8 bytes reserved.
- **Evidence against:** A second volume mounted later with stored times above the base (none exists at stage 6)
  would have to raise it, which a value handed down cannot do. The base is a fact every spawner must pass on.
- **Cost:** one request, one record field, a few lines in `fsd`, `init`, the broker, `libposix` and `posixrun`.
- **Precedent:** none; it is Option B's offset kept in user space.

### Option D: A clock service (Fuchsia's UTC clock object)

- **Description:** Fuchsia gives each process a UTC clock handle at start; a time service updates the clock object,
  and readers read it without a call. Here: a clock object, or a service every process asks.
- **Evidence for:** The right shape for a clock that changes (NTP, a user setting the time), with the authority to set
  it held by one service.
- **Evidence against:** A kernel object type or a round trip per read; nothing at stage 6 changes the clock.
- **Cost:** a kernel object, or a service and its protocol.
- **Precedent:** Fuchsia's `zx_clock` objects and the UTC clock handed to processes
  (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).

### Option E: The real-time clock QEMU `virt` has

- **Description:** QEMU `virt` has a PL031 real-time clock in its device tree. A driver reads it at boot and gives
  the base as a real date.
- **Evidence for:** Real dates; the base needs no stored time.
- **Evidence against:** A device the kernel does not map today, a driver, and the question of who holds it; the
  stored times of the root volume can still be above it (a host clock that went back), so the rule of Option C (never
  below the stored time) is still needed. More than this task needs.
- **Cost:** a driver and its tests, plus Option C's chain.
- **Precedent:** Linux's `rtc-pl031`.

## Recommendation

**Recommendation (not a decision): Option C.** It meets the drivers with what exists, and keeps Option B or D for
when a clock must change after boot.

1. **The base.** `zoraya_fs_service::time_base(latest, monotonic)`: the root volume's latest assigned time in
   nanoseconds plus 1, less the monotonic clock at mount, never below 0. The root volume's `fsd` computes it once,
   after mount, and stamps every request with `wall_time(base, monotonic)`.
2. **The request.** `time_base` (file protocol ordinal 35, no handles) answers `Done` with the base in `count`. A
   temporary service answers the base it was given.
3. **The chain.** `init` asks the root connection once the storage stack is up and starts the broker with
   `--time-base N`; the broker starts `tmpd` with `--base N` and writes `N` into the shell's record; `libposix`
   writes its own `realtime_base` into every child's record; `posixrun` asks the root connection as `init` does.
4. **The record.** `ZPB1`'s 8 reserved header bytes are `realtime_base`, a little-endian `u64`; the header size is
   unchanged, and 0 means none.
5. **`libposix`.** `CLOCK_REALTIME` and `time()` are `realtime_base` plus the monotonic clock; the other clocks stay
   the monotonic clock (ADR-0180 item 13 for a record with no base).

## Consequences

- `make` compares times across `/tmp` and `/home` correctly, and a file is never stamped "in the future" of
  `CLOCK_REALTIME` by more than the nanoseconds the engine adds to keep times distinct within one request burst.
- The dates are not real dates: the root volume's first date is the host's clock when `zoraya-mkfs` made the image,
  and each boot goes on from the latest stored time. ADR-0014's open question on a wall clock stays open.
- A volume mounted with times above the base is not handled; the first task that mounts a second persistent volume
  must raise the base (Option B or D) or refuse.

## Invariants

- **INV-0191-1.** A file written on the temporary volume after a file on the root volume has the later `mtime`, and
  the other way round, and `clock_gettime(CLOCK_REALTIME)` read after both is not below either. *Check:*
  `zoraya-fs-service`'s host test `a_file_in_tmp_written_after_one_in_home_is_later`, and the in-QEMU session
  test's `t_posix time`.
- **INV-0191-2.** The base is above every time the root volume holds when it is mounted. *Check:* the host test
  `the_base_goes_on_from_the_latest_stored_time`.

## Open questions

- A second persistent volume, or a clock that must change after boot: Option B or D.
- Whether `CLOCK_MONOTONIC` should also start at the base (Linux's does not; nothing measured needs it).

## Revisit triggers

- A wish for real dates: Option E, with Option C's rule kept as the floor.
- A second persistent volume, or networking (NTP).

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request.
