# ADR-0230: Testing the network without the Internet: user-mode networking, the runner as the far end, a capture on failure

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0001, ADR-0054, ADR-0091, ADR-0097, ADR-0109, ADR-0110, ADR-0220, ADR-0221, ADR-0222, ADR-0223, ADR-0225, ADR-0226, ADR-0227, ADR-0228, ADR-0229
- **Evidence:** CP-12, DM-047, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, network/survey.md#fuchsias-socket-provider, network/survey.md#other-stacks, network/survey.md#qemu-networking-for-development-and-ci, network/survey.md#the-device-virtio-net-under-qemu

## Context

`make test` must not need network access once the toolchain is installed (`CLAUDE.md`), and `tools/run-qemu.sh` is the
only thing that invokes QEMU. QEMU's user-mode networking runs without privilege on every host, blocks inbound
connections, forwards guest connections to a chardev or a spawned command with `guestfwd`, isolates the guest from
the host with `restrict=on` while keeping explicit forwards, and dumps traffic to a pcap with `filter-dump`
(`network/survey.md#qemu-networking-for-development-and-ci`); Ubuntu's `qemu-system-arm` has it, and macOS's `vmnet`
needs root (same section). It offers one queue pair (`network/survey.md#the-device-virtio-net-under-qemu`). ADR-0110
drives the POSIX workloads over the serial line from the runner. Fuchsia calls its netstack "extremely load bearing"
and tests it with industry conformance tools and against its old stack and Linux (`network/survey.md#other-stacks`). This ADR
decides how the network is tested in `make test` and CI without the Internet, what a network test may assume, and
how a failure is captured.

**What is measured.** Nothing on Zoraya; CI runs `make test` under TCG today.

**What is assumed.** That a test server in the runner (HTTP, then HTTPS with a test root) covers the stack, the
service, the grants and the library for the programs in scope; that QEMU's SLIRP is deterministic enough for
functional tests and too slow and host-dependent for performance numbers.

## Decision drivers

1. **No Internet in `make test`** (`CLAUDE.md`, INV-0109-3's rule for the ports).
2. **One place knows QEMU's command line** (`tools/run-qemu.sh`).
3. **Hostile input is tested on the host first** (ADR-0054's INV-0054-4 pattern, CP-12): the stack's and the driver's
   parsers against fakes, before QEMU.
4. **A failure leaves evidence**: what crossed the wire (DM-047's lesson that a failed driver must be diagnosable).
5. **The runner's memory rules** (`CLAUDE.md`): bounded output, bounded captures.
6. **Each invariant of ADR-0220 to ADR-0229 has a check** (ADR-0001 rule 6).

## Considered options

### Option A: Tests against real servers on the Internet (what most projects do)
- **Description:** `curl https://example.org/` in the guest, through the host's connection.
- **Evidence for:** Tests the real thing; nothing to build.
- **Evidence against:** Driver 1; flaky and slow; CI's egress policy decides the result; TLS roots and dates depend on
  the day.
- **Cost:** *(estimate)* None, and `make test` becomes nondeterministic.
- **Precedent:** Everywhere, and a known source of red builds `[assumption: general experience, no source in the files]`.

### Option B: User-mode networking with `restrict=on`, the runner as the far end through `guestfwd`, a pcap kept
- **Description:** Below.
- **Evidence for:** Drivers 1 to 6: no privilege, no Internet, deterministic peers under the runner's control,
  capture built in; it works on HVF and in CI (`network/survey.md#qemu-networking-for-development-and-ci`).
- **Evidence against:** SLIRP is a user-space stack of its own between the guest and the runner, so what is tested is
  "Zoraya against SLIRP"; one queue; no ICMP, no UDP forwarding (UDP is tested on the host only); the runner grows an
  HTTP and a TLS server.
- **Cost:** *(estimate)* 300 lines in the runner (an HTTP server and a TLS wrapper over `std`), options in
  `tools/run-qemu.sh`, scripts.
- **Precedent:** QEMU's own test suites `[assumption]`; Fuchsia's test topologies with an isolated netstack and
  resolver (`network/survey.md#fuchsias-socket-provider`, the offers shard; the system is
  `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option C: Two guests on a socket network (`-netdev socket`)
- **Description:** Two QEMUs connected by QEMU's socket backend, one serving, one fetching.
- **Evidence for:** Tests `netd` as a server too; no SLIRP in the path.
- **Evidence against:** Twice the memory and time per test under the host's memory rules; the server side needs a
  Zoraya HTTP server first.
- **Cost:** *(estimate)* A second image and runner support.
- **Precedent:** QEMU's documentation of inter-instance networking.

### Option D: Host tests only, over a fake device
- **Description:** `zoraya-net`'s logic with smoltcp's loopback and a scripted peer; no QEMU test.
- **Evidence for:** Fast, deterministic, where most bugs are found (ADR-0054's experience with the block driver).
- **Evidence against:** Never exercises the driver, the device, the handles, the grants through `zosh`, or
  `libposix`'s sockets.
- **Cost:** *(estimate)* Part of B's cost already.
- **Precedent:** Every host-tested crate in the repository.

## Recommendation

**Recommendation (not a decision): Option B, with Option D as its first layer.**

1. **Host tests** (`make test-unit`): `zoraya-net`'s logic over smoltcp's loopback and over a scripted frame peer
   (TCP connect, data both ways, FIN, RST, a datagram, a listener, resolution against recorded DNS answers, the
   scope matcher, the DHCP exchange); `ethd`'s queue logic against the fake device of `zoraya-virtio` with
   out-of-range lengths; `zoraya-posix`'s sockets through the fake kernel against `zoraya-net`; deterministic random
   mutation tests of every decoder (frames, DNS, the protocol crates). These carry most of ADR-0220 to ADR-0229's
   invariants.
2. **`tools/run-qemu.sh --net`** adds, in one place: `-netdev user,id=net0,restrict=on` with
   `guestfwd=tcp:10.0.2.100:80-tcp:127.0.0.1:HTTP_PORT` and `guestfwd=tcp:10.0.2.100:443-tcp:127.0.0.1:TLS_PORT`
   (the ports passed by the runner, which listens on them), `-device virtio-net-device,netdev=net0,bus=virtio-mmio-bus.16`
   (page 2, alone on its page as ADR-0091 requires) and `-device virtio-rng-device,bus=virtio-mmio-bus.8` (page 1,
   ADR-0227), which is why `--net` and `--gui` cannot be combined until ADR-0221 item 8 is decided and built (deferred by
   the architect; `run-qemu.sh` refuses the pair until then), and `-object
   filter-dump,id=dump0,netdev=net0,file=PCAP,maxlen=256` with the file under `target/test-logs/`;
   `--net-capture-all` lifts `maxlen` for `make bench-net` and is never used by `make test`. `make run --net` gives an interactive
   machine the same backend without `restrict`, so a person can fetch from the Internet by hand.
3. **The far end.** The runner serves HTTP on `HTTP_PORT` (fixed files from `tests/net/www/`, a path that echoes the
   request, a path that closes mid-body, a path that never answers) and HTTPS on `TLS_PORT` with a certificate for
   `test.zoraya` signed by a test root committed in `tests/net/ca/` (a test key in the repository, said so in its
   README), plus a second server with an expired certificate and one with a wrong name for INV-0228-1. The guest's
   image sets `net.hosts = 'test.zoraya=10.0.2.100'` (ADR-0229) so that names work without DNS; DNS itself is tested on
   the host (item 1) and in QEMU only against SLIRP's forwarder when the host has a resolver (a step that is skipped,
   not failed, without one, as `make test` skips EL2 without HVF).
4. **Workloads** (ADR-0110's form, scripts the runner plays): W11, `+net test.zoraya curl -O http://test.zoraya/w11.txt`
   and a byte-exact compare; W12, the same over HTTPS with `fetch` and with curl; W13, wget; `t_net`, a native test
   program checking the invariants that need the machine (INV-0220-1, INV-0221-1, INV-0221-3, INV-0221-4, INV-0225-1,
   INV-0225-4, INV-0226-2, INV-0227-1, INV-0228-3); the hostile cases: `+net other.example` refused, a listener on a
   port not granted, a resolution outside the scope, a dead `netd` (killed through a test-only request) and the
   restart. Under HVF and TCG, as `make test-posix` runs.
5. **What a network test may assume**: SLIRP's addresses (`10.0.2.15/24`, gateway `10.0.2.2`, DNS `10.0.2.3`), the
   DHCP offer, one queue pair, no ICMP, TCP only across the boundary, the runner's servers at `10.0.2.100`, no
   Internet, a real date from the RTC (ADR-0226; the test root's validity is 2026 to 2126 so a wrong host clock within
   that range still passes), and timing that is arbitrary under TCG (every wait has a deadline of seconds, as
   ADR-0110's scripts have).
6. **On failure** the runner keeps the pcap with the run's logs and prints its path and the last 20 frames decoded by
   a small reader in the runner (Ethernet, IP, TCP flags, lengths), within the output bound. The runner bounds the
   file: `filter-dump` bounds bytes per frame only, so after the run the runner truncates a pcap above 64 MiB to its
   first 64 MiB and says so, as it bounds the serial output; with `maxlen=256` and the tests' sizes the bound is not
   reached.
7. **CI** runs the same under TCG: `libslirp0` is a dependency of Ubuntu's `qemu-system-arm`
   (`network/survey.md#qemu-networking-for-development-and-ci`); the workflow changes only if the runner's package lacks
   it, which the first CI run shows.
8. **Not measured by `make test`**: throughput and latency, which `make bench-net` reports for `docs/design.md`
   (a 16 MiB fetch under HVF and TCG) and no test asserts.

**Examples.** What the runner prints on a failed W12:

```
[W12] FAIL: fetch https://test.zoraya/w12.bin: certificate expired on 2026-01-01
  capture: target/test-logs/test-net-hvf.pcap (142 frames)
  last frames: 10.0.2.15:49152 > 10.0.2.100:443 [S] 0; 10.0.2.100:443 > 10.0.2.15:49152 [S.] 0; ... [F.] 0
```

**Tensions with accepted ADRs.**
- *ADR-0097*'s `tools/run-qemu.sh` options gain `--net`; its placement rule for devices is reused.
- *ADR-0110*: W11 to W13 join the workloads; `make test-net` joins `make test`.
- *ADR-0109 INV-0109-3*: unchanged; the test root and the test pages are committed, nothing is fetched.

### Effect of the vision
For target 2 the hostile cases of item 4 are the tests that matter, and they run without the Internet. Target 3: the
GUI test can fetch through a desktop program the same way. Threads: none.

## Consequences
- **Positive.** Deterministic, unprivileged, on every host and in CI; failures leave a pcap; the runner is the peer.
- **Negative.** SLIRP sits between the guest and the runner; UDP and ICMP are host-tested only; a test key in the
  repository; the runner grows two servers.
- **Follow-up.** `--net` in `tools/run-qemu.sh`; the runner's servers and pcap reader; `tests/net/`; `make test-net`
  and `make bench-net`; the W11 to W13 scripts.

## Invariants
- **INV-0230-1.** `make test` opens no connection to any address outside `127.0.0.1` and the guest's SLIRP network
  while the network tests run. *Check:* the runner's servers bind to `127.0.0.1` only, `restrict=on` is asserted in
  the printed command line, and on Linux CI the step runs under a network namespace with only loopback
  `[assumption: the runner allows `unshare -n`; otherwise the printed command line is the check]`.
- **INV-0230-2.** A failed network test leaves a pcap under `target/test-logs/` that the runner has bounded to 64 MiB,
  and the runner's output stays within its bounds. *Check:* a runner host test with a failing script and a capture
  file larger than the bound, asserting the truncation and the message.
- **INV-0230-3.** The network step is skipped, not failed, on a host whose QEMU lacks user-mode networking, and
  `make test`'s summary lists the skip. *Check:* a runner host test with a fake `qemu` that rejects `-netdev user`.
- **INV-0230-4.** W11 to W13 and `t_net` pass under HVF and TCG from a clean checkout. *Check:* `make test-net`, a step
  of `make test`.

## Open questions
- Whether SLIRP's behaviour under TCG on a loaded CI runner is deterministic enough, or W12 needs longer deadlines.
- The GUI test with the network (plan N6) waits on ADR-0221 item 8's decision about the virtio-mmio pages. *Note, 2026-11-12 (N6):* decided and built (proposed ADR-0295); `tests/gui/net.script` boots `--gui --net` and reads `net status` from the screen. The `+net` pill waits on the rooms' words.
- DNS in QEMU: a UDP `guestfwd` does not exist, so testing `netd`'s resolver against a controlled server in QEMU
  needs the runner to answer through SLIRP's forwarder (host-dependent) or a Zoraya DNS test server in a second guest
  (Option C).
- Where the test root's key lives and how it is rotated (it is a test secret, not a system one).

## Revisit triggers
- A network server program in scope: Option C, or `hostfwd` into a Zoraya listener from the runner.
- CI's QEMU without SLIRP: build QEMU in CI or use `passt`.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request. `--net` and `--gui` stay refused together until ADR-0221 item 8 is decided (deferred).
