# ADR-0227: Entropy: a random service over virtio-rng and the boot seed, given to every program

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0020, ADR-0052, ADR-0053, ADR-0054, ADR-0091, ADR-0103, ADR-0107, ADR-0153, ADR-0167, ADR-0221, ADR-0222
- **Evidence:** CP-01, CP-15, DM-048, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#genode, network/survey.md#entropy, network/survey.md#what-a-fetch-over-https-needs

## Context

TLS needs unpredictable bytes for every handshake (`network/survey.md#what-a-fetch-over-https-needs`); so do
ephemeral ports, DNS transaction ids and TCP's initial sequence numbers in `netd`, and later ASLR and session keys.
Zoraya has no source. The survey (`network/survey.md#entropy`) finds four on this platform: the `RNDR` register, which
QEMU's `cortex-a72` lacks and whose presence on the architect's M4 is unconfirmed (no `FEAT_RNG` key in `sysctl`, none
in Apple's documented list); virtio-rng, a device QEMU offers on any host with `rng-builtin` as its default backend;
the `rng-seed` property QEMU writes into the device tree, one seed per boot; and timing jitter, which nothing
endorses. Randomness is not authority over anything: holding a source lets a program harm no one. This ADR decides
the source and who serves it.

**What is measured.** The `sysctl` listing and the fixture's `rng-seed` property (`network/survey.md#entropy`).
Nothing on Zoraya.

**What is assumed.** That QEMU's virtio-rng, fed from the host's `/dev/urandom` or QEMU's own generator, is as good
as the host's randomness, which TLS clients everywhere rely on; that a few hundred bytes per handshake and a few
thousand per second system-wide are the demand.

## Decision drivers

1. **No crypto in the kernel's trusted base that a service can do** (CP-15, DM-048, ADR-0054's rule for drivers).
2. **A source on every host**: HVF on the architect's Macs and TCG in CI (`network/survey.md#entropy`).
3. **Per-program unpredictability**: two programs seeded from one seed must not share a stream.
4. **Reach only what you are given** (CP-01) read strictly: even a harmless source is a handle, not an ambient call.
5. **Cheap enough for TLS**: a round trip per handshake is nothing; a round trip per byte is not.

## Considered options

### Option A: A kernel generator and `getrandom` (Linux)
- **Description:** The kernel mixes hardware and interrupt timing into a CSPRNG and every process reads it with
  `getrandom(2)` or `/dev/urandom`.
- **Evidence for:** What every program expects; one pool for the machine; no handle to route.
- **Evidence against:** A CSPRNG and a virtio-rng driver in the kernel, against ADR-0054 and driver 1; an ambient call,
  against driver 4.
- **Cost:** *(estimate)* A driver and a cipher in the kernel.
- **Precedent:** Linux, the BSDs; Zircon's `zx_cprng_draw`, a kernel call with no rights required
  (`network/survey.md#entropy`), is the non-Unix form of the same.

### Option B: A random service, `rngd`, over virtio-rng and the boot seed, reached by a handle every program is given
- **Description:** `rngd` is a virtio-rng driver on ADR-0054's objects and a service with one protocol,
  `zoraya-proto-random`: `bytes(n)` → up to 4 KiB of random bytes. It keeps a deterministic random bit generator
  seeded at start from the device tree's `rng-seed`, which the kernel hands `init` and `init` hands `rngd`, and
  reseeded from the virtio-rng device when there is one (every 64 KiB of output or every 60 s `[assumption]`). Every
  program receives a connection to it: a `Random` kind in ADR-0107's table (`/dev/urandom`, which ADR-0107 deferred)
  and a fixed slot `RANDOM` in ADR-0153's `Startup` record; `getrandom()` and `/dev/urandom` reads in `libposix` are
  requests on it. No surveyed system serves randomness from a user-space service (`network/survey.md#entropy`:
  Fuchsia and Linux serve it from the kernel); the precedent is the shape, not the fact: Genode's and Fuchsia's
  drivers are components a parent wires (`alternatives-postmortem.md#genode`,
  `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`), and this option treats the entropy device as one more.
- **Evidence for:** Drivers 1 to 5: the device and the cipher live in one process; the seed covers a machine without
  the device; every program has a handle it was given; the cost is a round trip per request of up to 4 KiB.
- **Evidence against:** One more process and protocol in `init`'s table; a channel per program; a compromised `rngd`
  gives predictable bytes to everyone (the same is true of a kernel pool). A program that wants randomness before its
  first request must wait a round trip.
- **Cost:** *(estimate)* A 300-line driver on `zoraya-virtio`'s queue, a 200-line generator (ChaCha20-based,
  `[assumption]` written in-house as CLAUDE.md prefers), a protocol of one request.
- **Precedent:** None for a randomness service in the files; the component model of Genode and Fuchsia for the shape;
  QEMU's virtio-rng as the device.

### Option C: Each program seeds its own generator from the boot seed
- **Description:** `init` gives every program the `rng-seed` bytes in its record; the program runs its own generator.
- **Evidence for:** No service, no round trips.
- **Evidence against:** Every program sees the same seed (driver 3 fails unless each launcher derives a child seed,
  which puts the cipher in every launcher); no reseeding from the device; a program's generator state is in its own
  memory, where a `fork` copies it (the classic duplicated-stream bug).
- **Cost:** *(estimate)* Smallest, and the subtlest bugs.
- **Precedent:** None in the files.

### Option D: A kernel call with a generator seeded from `rng-seed` and `RNDR`, no device
- **Description:** `random_bytes(buf)`, a generator in the kernel seeded once from the device tree, `RNDR` when
  `ID_AA64ISAR0_EL1` reports it.
- **Evidence for:** No process; the seed is already in the kernel's hands; `RNDR`, where present, is a register.
- **Evidence against:** Driver 1 and 4; `RNDR` is unconfirmed on the development host and absent in CI
  (`network/survey.md#entropy`), so the kernel would run on one seed per boot with no reseeding unless it also drove
  virtio-rng, which is Option A.
- **Cost:** *(estimate)* A cipher in the kernel and a call.
- **Precedent:** Zircon (`zx_cprng_draw`, `network/survey.md#entropy`), without the reseeding device.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **The seed.** The kernel reads `/chosen/rng-seed` from the device tree (`zoraya-dtb`, the parser the boot already
   uses) into a memory object and
   gives `init` a handle to it at slot 6, after ADR-0226's `RTC` at 5 and before the virtio pages (ADR-0226 item 1's
   layout; the slot is empty when the property is absent); `init` passes it to `rngd` and keeps nothing. A device tree without it
   is reported, and `rngd` starts only if a virtio-rng device exists; with neither, `init` reports "no entropy source"
   and gives no `Random` connections, so `getrandom` fails with `ENOSYS` and TLS cannot start (stated, never silent).
2. **`rngd`.** The virtio-rng driver (device ID 4, `requestq`, `network/survey.md#entropy`) on ADR-0054's objects,
   found by `init`'s scan (ADR-0091), and the generator: seeded from the seed and the first device read, reseeded as
   above, producing `bytes(n)` for `n` up to 4096. Its logic is a `no_std` crate with host tests over a fake device,
   as every driver's is.
3. **The connection.** A channel to `rngd` with one right, `read`; two requests, `bytes(n)` and `share(server_end)`
   (a second connection, for a `fork` child, which inherits no channel, ADR-0050, and for `spawn`: `libposix` carries
   a `share`d connection in the move list and in the record, as it carries control channels, ADR-0225 item 9); one
   connection per program, given at launch by every launcher (`zosh`, the panel, the broker for POSIX processes,
   `init` for services), without a grant word: randomness designates nothing, so there is nothing to show, and `grants` lists it
   on one line with the streams (`random    the random service`).
4. **`libposix`.** `getrandom(buf, n, flags)` is one request (or several for `n` above 4 KiB); `/dev/urandom` and
   `/dev/random` are the `Random` entry kind, readable, never blocking (the generator never blocks). musl's own
   `arc4random` is not in any tier.
5. **`netd`** seeds its own ports, ids and sequence numbers from its connection.
6. **Native programs** read it through `zoraya-rt`'s `random(buf)`; a TLS provider's random source (ADR-0228) is a
   generator in the program, seeded and reseeded from it, so that a handshake costs one round trip, not one per byte.

**Examples.** Nothing is typed; `grants` shows:

```
~/proj> grants fetch https://example.org/ -o page.html
fetch gets:
  in, out, err             the terminal
  random                   the random service
  ~/proj                   read (current directory)
  ...
```

**Tensions with accepted ADRs.**
- *ADR-0153 rule 1* ("nothing else") gains a default item, the random connection; INV-0153-1's and INV-0002-5's lists
  gain the `RANDOM` slot. *ADR-0167 point 2*'s GUI entries gain it too.
- *ADR-0107*'s deferred `/dev/urandom` is resolved as an entry kind `Random`.
- *ADR-0033*: the ramdisk and device-tree parsing gain one property to pass on; its parser is total already.
- *ADR-0053*: one more service in `init`'s table, started before `netd`.

### Effect of the vision
For target 2 a sandbox gets randomness as every program does and gains nothing by it; a supervisor that wanted to
deny it could (it is a handle), which no surveyed system offers and none needs. Target 3 and threads: none.

## Consequences
- **Positive.** One source, one process, every program has it; no crypto in the kernel; works under HVF and TCG.
- **Negative.** A process, a protocol, a channel per program; a round trip for the first bytes.
- **Follow-up.** The kernel's `rng-seed` handle; `rngd` and `zoraya-proto-random`; the `Random` kind and `RANDOM`
  slot; `getrandom` and the device entries; `tools/run-qemu.sh --net` adds `-device virtio-rng-device` (ADR-0230).

## Invariants
- **INV-0227-1.** Two programs started in one boot receive different byte streams, and after `fork` both parent and
  child can read, through the child's shared connection, streams that differ from each other. *Check:* an in-QEMU
  test reading 32 bytes in two programs and in a forked pair, asserting success in the child and inequality.
- **INV-0227-2.** `rngd` reseeds from the device: with a fake device whose output is recorded, the generator's output
  changes after the reseed point and depends on the device's bytes. *Check:* a host test of the generator crate.
- **INV-0227-3.** `bytes(n)` never blocks and never returns fewer than `n` bytes for `n` up to 4096. *Check:* a host
  test of the service loop.

## Open questions
- Whether to use `RNDR` as a third input when `ID_AA64ISAR0_EL1` reports it (the plan's first task prints the register).
- The reseed interval and the generator's construction (a well-known DRBG written in-house, or a crate recorded in
  `docs/dependencies.md`).
- Whether `netd` should be given its own virtio-rng rather than a connection, to shorten its path; not needed by any
  measurement.

## Revisit triggers
- A measured demand that makes the round trip matter (bulk key generation).
- ASLR (ADR-0020's KASLR later) needs randomness in the kernel before any process: the seed is already there; a
  kernel generator for that one use would be a new ADR (Option D's shape for the kernel alone).

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request.
