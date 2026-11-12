# ADR-0288: What N5 decided where ADR-0226 and ADR-0227 are silent: the clock, the seed, the generator and the random service

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0043, ADR-0050, ADR-0052, ADR-0053, ADR-0054, ADR-0091, ADR-0153, ADR-0191, ADR-0220, ADR-0226, ADR-0227, ADR-0228, ADR-0287
- **Evidence:** CP-15, CP-20, CP-27, DM-048, DM-065, network/survey.md#a-real-clock, network/survey.md#entropy, network/survey.md#what-a-fetch-over-https-needs

## Context

N5 of TASK-26 builds the wall clock (ADR-0226) and the random service (ADR-0227) because TLS (ADR-0228) validates
a certificate against the date and draws its secrets from randomness. Both ADRs are accepted and are followed; the
code needed choices they leave open, and one thing is built in part. **Measured:** in QEMU the RTC follows the host
(`date` agrees with the runner's clock within 2 s, `tests/shell/clock.script`), and QEMU's device tree carries
`/chosen/rng-seed` (the kernel prints `device: entropy seed of N bytes`). **Assumed:** a generator seeded from the
device tree is enough entropy for a session key on a QEMU guest whose host fills the seed from its own source; a
virtio-rng device would add a second source, not replace the first.

The evidence this rests on, as the two accepted ADRs cite it: CP-15 (what a program sends over TLS is readable by it
and its peer, so the secrets must be its own), CP-20 (monotonic deadlines stay untouched when the wall clock is
added), CP-27 (the surface grows only by decision), DM-048 (a small trusted base), DM-065 (what a wall clock that
jumps does to programs that take it for a monotonic one), `network/survey.md#a-real-clock` (the PL031 at 0x09010000),
`network/survey.md#entropy` (`/chosen/rng-seed` and virtio-rng) and `network/survey.md#what-a-fetch-over-https-needs`
(the date and the entropy a validator needs).

## Decision drivers

1. ADR-0226 and ADR-0227 are binding; nothing here may contradict them (`CLAUDE.md`).
2. A device page that holds no virtio transport is still a `Device` object (ADR-0054): no new kernel object kind.
3. A secret is never sent over a channel in the clear to a program that has not asked for it, and randomness
   designates nothing (ADR-0227 item 3).
4. The cryptography of the system is `ring`'s (ADR-0228); a hand-written primitive is small, has published test
   vectors and is named here for the reviewer.

## Considered options

### Option A: the choices below (the recommendation)
### Option B: a kernel `getrandom` call and a kernel clock
Rejected by ADR-0226 and ADR-0227 already (a global mutable kernel value; ambient authority).
### Option C: a library generator in each program seeded from the time and the MAC (netd's stand-in until now)
Predictable; rejected for anything that guards a secret.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The RTC is a `Device` with one range and no interrupt.** `device::init_rtc` makes it from the first
   `arm,pl031` node (`zoraya_dtb::platform::rtc`); its range has `intid` 0, which `interrupt` refuses. `init` holds it
   at `RTC_SLOT` = 5, `device_map`s range 0 and reads `RTC_DR` once per start of the storage stack
   (`stack::read_rtc`), so a restarted stack reads the clock again and the floor only rises. `FIRST_DEVICE_SLOT`
   becomes 7 (ADR-0226 item 1); every user of the constant follows it.
2. **`--time-floor N` is decimal nanoseconds** (a `u32` of seconds times 10^9); `time_base(latest, monotonic, floor)`
   is `max(latest + 1, floor) - monotonic`, saturating at 0 (INV-0226-1).
3. **`ZNS1` grows to a 32-byte header**: bytes 24 to 31 are `realtime_base` (u64, little endian); byte 22 is the
   index of the random connection and byte 23 stays reserved. `Startup::decode` stays total. `namespace::realtime()`
   is the base plus the monotonic clock, or `None` for a record with 0 (ADR-0191 rule 5). The base's lowest bit says that the real-time clock gave the date (a floor was applied); `zoraya-abi`'s `checked_wall_seconds` is `None` without it, so TLS never checks a certificate against the volume's latest time. `date` prints it in UTC.
4. **The seed** is `/chosen/rng-seed` copied by the kernel into one memory object (a `u32` length, then at most 4,092
   bytes), given to `init` at `SEED_SLOT` = 6 with read, map and transfer rights. `init` moves it to `rngd`, which maps
   it, hands the bytes to the generator, unmaps and closes it, and wipes its own copy. The kernel takes its reference
   out of its table when it hands the object to `init`, so the object (and its frame) ends with `rngd`'s handle. The
   device tree blob still holds the same bytes in the kernel's memory; wiping it is not done (a limitation: only the
   kernel maps it). `rngd` refuses a seed under 32 bytes or all zero ("no entropy source") and does not serve.
5. **The generator** (`zoraya-rng`) is ChaCha20 (RFC 8439's block function, tested against its vector) with fast key
   erasure: a request's output comes from the block stream under the key, and the first 32 bytes of that stream replace
   the key before any output leaves. Absorbing input XORs 32-byte chunks into the key, each followed by one block
   function round trip (a counter and a domain word in the nonce), then the length the same way. This is not a
   standard DRBG; it is 40 lines whose properties are tested (same seed, same stream; any input byte changes the
   output; trailing zeros differ; successive requests differ). **The reviewer should decide whether to replace it with
   a library DRBG.**
6. **`rngd`** starts when the seed exists (else `init` says `no entropy source`, no `Random` connection is given and
   `random` fails). One channel to `init`, then up to 256 connections made by `Share` (a message carrying the server
   end); `Bytes(n)` answers exactly `n` bytes for `n` up to 4,096 at once (INV-0227-3); a message that does not
   decode ends the connection, an unknown ordinal gets `ErrorReply`. Only `init`'s connection and the shell's (depth 0
   and 1) may `Share`, each for at most 64 live children (freed when a child closes), so a program cannot use up the
   table.
7. **The shell** holds a connection at slot 10 (from `init`) and shares a fresh one with each native program it
   starts, in the new `random` slot of `ZNS1`; `random [COUNT]` prints bytes. A program started without a shell has
   none. `zoraya_user::random::random(buf)` is the call; `zoraya-tls` seeds its own generator with 32 bytes from it
   at each `fetch`.
8. **Not built:** reseeding from a virtio-rng device (ADR-0227 item 2: `Generator::mix` is the entry and is tested
   with a fake source; the driver is not written), `getrandom` and `/dev/urandom` in `libposix`, the connection of
   a `fork` child (INV-0227-1's forked pair), the panel's and the broker's launches, `netd`'s own seeding (it still
   mixes the clock and the MAC), a restart of `rngd`, and the `grants` line `random the random service`.

## Consequences
- **Positive.** Real dates from the first boot; a program can ask for randomness; TLS has both.
- **Negative.** One generator of our own; the seed also lives in the kernel's copy of the device tree; native programs
  from the panel's terminals and POSIX programs have no random connection yet.
- **Follow-up.** The virtio-rng driver and its reseed, `libposix` `getrandom`, the wiring of the other launchers.

## Invariants
- **INV-0288-1.** A device page with `intid` 0 never makes an interrupt object. *Check:* the kernel's
  `device::Device::interrupt` filters it; `make test-qemu` (t_devices) runs with the RTC at slot 5.
- **INV-0288-2.** `Generator::fill` gives exactly the length asked for and `Service::handle` never panics on any
  message. *Check:* `cargo test -p zoraya-rng` (all lengths to 4,096; 20,000 random messages).
- **INV-0288-3.** `date` and a native program's `realtime()` come from the same base. *Check:*
  `tests/shell/clock.script` (`make test-files`).

## Open questions
- Replace the generator with a library DRBG (item 5).
- Whether the seed in the kernel's blob should be zeroed after `init` has it.

## Revisit triggers
- A virtio-rng device in the QEMU command line: build the reseed path.
- A program that needs randomness without a shell: give `init` and the panel the `Share` too.

## Architect's decision

(Left empty for the architect.)
