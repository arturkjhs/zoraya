# ADR-0226: The wall clock: the PL031 read once at boot as the floor of the time base

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0014, ADR-0023, ADR-0043, ADR-0053, ADR-0054, ADR-0062, ADR-0091, ADR-0107, ADR-0153, ADR-0157, ADR-0180, ADR-0191, ADR-0221
- **Evidence:** CP-20, DM-065, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, network/survey.md#a-real-clock, network/survey.md#what-a-fetch-over-https-needs

## Context

Zoraya has no wall clock. ADR-0191 gave every volume and `CLOCK_REALTIME` one time base per boot, the root volume's
latest stored time less the monotonic clock, so that times stay ordered across volumes; it said plainly that "The
dates are not real dates", kept ADR-0014's wall-clock question open, and named "Option E, with Option C's rule kept as
the floor" and "networking (NTP)" as its revisit triggers. TLS needs a real date: every validator checks the
certificate's validity period against the current time (`network/survey.md#what-a-fetch-over-https-needs`), and a
clock that counts from the image's creation is wrong by weeks or years. QEMU's `virt` has a PL031 real-time clock
driven by the host's time: one 32-bit register, `RTC_DR` at offset 0, counting seconds since the Unix epoch
(`network/survey.md#a-real-clock`); the repository's own device-tree fixtures show its node. SNTP can refine it later
over the network. This ADR decides the source of real time and how it relates to ADR-0191's base.

**What is measured.** The fixture's `pl031@9010000` node (`network/survey.md#a-real-clock`). Nothing on Zoraya.

**What is assumed.** That QEMU's RTC is within a second of the host's clock (its documented default,
`clock=host`), and that the host's clock is right enough for certificates (days of validity).

## Decision drivers

1. **TLS needs the date within the validity period** (`network/survey.md#what-a-fetch-over-https-needs`), so an
   error of minutes is fine and an error of weeks is not.
2. **Time never goes backwards on a volume** (ADR-0014 rule 3, ADR-0191 driver 3; DM-065's record of what a wall clock
   that jumps does to programs that mistake it for a monotonic one).
3. **One base, handed down** (ADR-0191's chain): nothing in it should change for the network.
4. **Who may set the clock is a right** (ADR-0191 Option B's objection; CP-20's monotonic deadlines are untouched):
   nothing a program can do moves another program's clock.
5. **Hardware facts from the device tree, in a process** (ADR-0054, `CLAUDE.md`): the RTC is a device.

## Considered options

### Option A: A kernel real-time clock set at boot and by `settimeofday` (Linux)
- **Description:** The kernel reads the RTC at boot and keeps `CLOCK_REALTIME`; a privileged call adjusts it; NTP
  daemons slew it.
- **Evidence for:** What every program expects; one source.
- **Evidence against:** ADR-0191 rejected a kernel clock for the same stage: a global mutable kernel value with a right
  to design; a kernel driver for the PL031 against ADR-0054; `settimeofday` is ambient authority over everyone's time.
- **Cost:** *(estimate)* A call, a right, a kernel driver.
- **Precedent:** Linux, every Unix.

### Option B: `init` reads the PL031 once and it floors ADR-0191's base
- **Description:** The kernel makes a `Device` object for the RTC's page (compatible `arm,pl031`) as it does for the
  virtio pages (ADR-0091), and gives it to `init`. Before starting the storage stack `init` maps it, reads `RTC_DR`
  once (seconds since the epoch), unmaps it, and passes the value to the root volume's `fsd` as `--time-floor N`
  (nanoseconds). `fsd`'s base becomes `max(latest stored + 1 ns, floor) − monotonic now` (ADR-0191 rule 1 with the
  floor); everything downstream (`time_base`, the broker, `tmpd`, `ZPB1`, `libposix`) is unchanged. ADR-0153's native
  `Startup` record gains `realtime_base` in the same way, so native programs have the clock too.
- **Evidence for:** Drivers 1 to 5 with one register read and no new call: the base is real on every boot, times on
  the volume never go back (the floor only raises), nothing changes after boot so no right is needed, and the device
  is read by a process. ADR-0191's own revisit trigger.
- **Evidence against:** The clock cannot change after boot: a boot with a wrong host clock stays wrong; SNTP cannot
  correct it (Option C later). The RTC is read with the kernel's device window machinery for one word; a page kind
  beyond virtio in ADR-0091.
- **Cost:** *(estimate)* A kind in the kernel's device scan (20 lines), 40 lines in `init`, an option in `fsd`, a field
  in `ZNS1`.
- **Precedent:** ADR-0191's chain, with the RTC as its floor.

### Option C: A clock object or service that programs read and a time service sets (Fuchsia's UTC clock)
- **Description:** Fuchsia gives each process a UTC clock handle maintained by a time service
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, ADR-0191 Option D). Here: a `timed` that reads the
  RTC, later runs SNTP, and hands programs a clock they read without a call, or ask over a channel.
- **Evidence for:** The right shape for a clock that changes; the authority to set it is one handle.
- **Evidence against:** A kernel object or a round trip per read for something nothing at this stage changes after
  boot; SNTP is not needed for TLS when the RTC is a second off.
- **Cost:** *(estimate)* A service, a protocol, and either a kernel clock object or a read-only shared page.
- **Precedent:** Fuchsia.

### Option D: SNTP only, over the network, no RTC
- **Description:** `netd` asks an SNTP server at start and the base comes from the answer.
- **Evidence for:** No device; right on any host.
- **Evidence against:** A machine without a network or without a server has no date, and `make test` has no Internet
  (ADR-0230); a network-set clock before TLS is a chicken-and-egg only when the server is reached by name over TLS,
  which SNTP is not, but an attacker on the path sets the time.
- **Cost:** *(estimate)* An SNTP client in `netd`.
- **Precedent:** Systems without an RTC `[assumption: a general observation, no source in the files]`.

## Recommendation

**Recommendation (not a decision): Option B now, Option C when a clock must change after boot.**

1. **The device.** The kernel, which already makes a `Device` per `virtio,mmio` page from the device tree (ADR-0054,
   ADR-0091), also makes one for the page of the node with compatible `arm,pl031`, and gives it to `init` at a fixed
   slot of its own, `RTC` at slot 5, after `Power` at slot 4 (ADR-0043, ADR-0062), empty when the device tree has no
   such node; ADR-0227's seed takes slot 6 the same way, and the virtio pages then start at slot 7 instead of 5, so
   `init` tells the RTC apart by its slot, not by reading registers (a change to ADR-0053's "`Device` handles from
   slot 5 on", listed below). The
   RTC's page holds nothing else on `virt` (`network/survey.md#a-real-clock`: `0x09010000`, size `0x1000`).
2. **The read.** `init` maps it (`device_map`), reads `RTC_DR` at offset 0 as an aligned 32-bit volatile load, unmaps
   it, and keeps the handle (it may read again for a later ADR). If ADR-0221 item 8's register access by call is
   adopted, the read is one `device_read(rtc, 0)` and no mapping. A value of 0 means the device gave nothing, and
   no floor is passed.
3. **The floor.** `fsd --time-floor N` (nanoseconds since the epoch): `zoraya_fs_service::time_base(latest,
   monotonic, floor)` is `max(latest + 1, floor) − monotonic`, never below 0. ADR-0191's INV-0191-2 ("above every
   time the root volume holds") stays true; a new invariant says it is also not below the floor.
4. **Native programs.** `zoraya_abi::native::Startup` (`ZNS1`) gains `realtime_base`, written by `zosh` and the panel
   from their own, which each gets by asking `time_base` on the root connection `init` gives it, as `posixrun` does
   (ADR-0191 rule 3); `zoraya-rt` offers `realtime()` as the base plus the monotonic clock; a record with 0 keeps the
   monotonic clock, as ADR-0191 rule 5.
5. **Dates.** `CLOCK_REALTIME`, `time()`, `date`, `ls -l` and `stat` show real dates from the first boot with this
   change. A volume whose stored times are *above* the RTC (a host clock that went back) keeps counting from its own
   latest time, as today.
6. **Not now.** Setting the clock after boot, SNTP, time zones (everything is UTC), leap seconds.

**Examples.**

```
~> date
2026-11-04 10:12:31 UTC
~> stat notes.txt
notes.txt  12 bytes  modified 2026-11-04 10:11:58 UTC
```

**Tensions with accepted ADRs.**
- *ADR-0091* makes "one `Device` per page of `virtio,mmio` transports" and has `init` identify devices by their
  registers; item 1 adds the RTC's page as a second kind, told apart by a fixed slot. A superseding note in ADR-0091
  if accepted; ADR-0053's list of what `init` receives gains slots 5 and 6, and its "`Device` handles from slot 5 on"
  becomes "from slot 7 on" (ADR-0054 states no slot; ADR-0053 is where the words are).
- *ADR-0191 rule 1* gains the floor; its Consequences sentence "The dates are not real dates" becomes false on
  purpose; its open question on a wall clock is answered.
- *ADR-0153 rule 8* and *ADR-0107 item 1*: both records gain `realtime_base` (the POSIX record already has it,
  ADR-0191 rule 4; the native one did not).
- *ADR-0053*: one more handle `init` receives.

### Effect of the vision
For target 2 nothing changes: a sandbox reads a base it cannot move. Target 3 wants a clock that can be set and a
time zone: Option C, later. Threads: none. Networking: SNTP joins through Option C, where the authority to adjust is a
handle `netd` would hold.

## Consequences
- **Positive.** Real dates in one register read; TLS can validate; nothing after boot changes.
- **Negative.** The clock is as right as the host's at boot and no better; a second kind of device page.
- **Follow-up.** The device kind; `init`'s read; `fsd --time-floor`; `ZNS1`'s field; `date` shows UTC.

## Invariants
- **INV-0226-1.** The time base is not below the floor `init` passed and not below the root volume's latest stored
  time plus one. *Check:* `zoraya-fs-service`'s host test of `time_base` with a floor above, equal to and below the
  stored time (INV-0191-2's test extended).
- **INV-0226-2.** `CLOCK_REALTIME` is within 2 s of the host's clock at the moment it is read, when QEMU's RTC follows
  the host. *Check:* an in-QEMU test under the runner: a test program prints `CLOCK_REALTIME`, and the runner compares
  it with the host's time at the moment it reads that line (not at launch, which under TCG is many seconds earlier).
- **INV-0226-3.** A native program's `realtime()` and a POSIX program's `time()` in the same session agree within 1 s.
  *Check:* an in-QEMU test running one of each.

## Open questions
- Whether the RTC should be read again (a second handle use) when a second persistent volume mounts (ADR-0191's open
  question), or that case waits for Option C.
- Time zones: a `time.zone` setting (ADR-0157) with the tz data as a file, or UTC for good.

## Revisit triggers
- A need to set the clock after boot (SNTP, a `date` command that sets): Option C.
- A host whose RTC QEMU does not follow (`-rtc clock=vm` under icount): the tests must then set a known date.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request.
