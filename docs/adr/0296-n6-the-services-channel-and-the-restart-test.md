# ADR-0296: N6, the services channel and the test of the network's restart

- **Status:** accepted
- **Date:** 2026-11-12
- **Depends on:** ADR-0053, ADR-0080, ADR-0221, ADR-0222, ADR-0224, ADR-0230
- **Evidence:** CP-12, CP-17, alternatives-postmortem.md#minix-3

## Context
ADR-0221 item 5 has `init` restart `ethd` and `netd` together, at most three times per boot, and item 6 has `init` tell
each launcher of the restart on a *services channel* with `NetworkRestarted { root }`, a launcher whose root connection
shows `PEER_CLOSED` refusing grants until it comes. ADR-0230 item 4 asks for a test with "a dead `netd` (killed through a
test-only request) and the restart". `init` already restarted the network (N2) and sent nobody the new root. N6 builds
item 6 and the test. The ADRs leave open the message shapes, who the launchers are today, and what the test-only request is.

## Decision drivers
1. A restart that no launcher learns of leaves the network dead for the rest of the boot (ADR-0221 driver 3; CP-12, failure is a value and a restart; CP-17, a driver restarts alone).
2. No registry and no new authority: a launcher gets a root connection only on a channel `init` gave it at start
   (ADR-0053; ADR-0221 item 6 says so).
3. The test must run the real `init` path, with nothing in a normal boot that can end `netd`.
4. Small.

## Considered options

### Option A: the services channel as the ADR says, one message kind per event, and a test-only `Crash` honoured by one flag
- **Description.** Below.
- **Evidence for.** Drivers 1 to 4.
- **Evidence against.** The test hook is code in `netd` that a normal boot never runs.
- **Precedent.** MINIX 3's reincarnation server tells dependants of a restart (`alternatives-postmortem.md#minix-3`).

### Option B: the launcher polls `init` for a new root
- **Evidence against.** A launcher would need a channel to `init` that answers any asker: the registry ADR-0221 item 6
  avoids.

### Option C: kill the job from the test (a kernel-side test hook)
- **Evidence against.** The qemu-test kernel is not the kernel a person runs, and `init`'s restart is user code that the
  normal kernel runs: the test must run there.

## Recommendation
**Recommendation (not a decision): Option A.** Built in N6.

1. **Messages** (`zoraya-proto-net`): `NetworkRestarted` (ordinal 32, one handle: the new root connection) and
   `NetworkDown` (33, no handle: the restarts of this boot are used up). They are not requests; `netd` never sees them.
2. **Launchers today.** The console shell (which is also the first terminal window's shell) holds a root connection and,
   since N6, the services channel at slot 13. The panel and the broker hold no root connection today (a shell started by
   the panel has no network: ADR-0224 item 4's desktop half), so they get no services channel; they get both with the
   `+net` words of rooms. Recorded in the report as not done.
3. **The launcher.** Before a line that needs the network runs, the shell reads the channel without waiting. A
   `NetworkRestarted` replaces the root connection (the old one is closed); `NetworkDown` is remembered. If the root's far
   end is closed the line is refused: `zosh: fetch: the network is not available (restarting)`, or `(...)` left out after
   `NetworkDown`. A shell that holds the services channel and no root (none was made) refuses the line the same way, never
   runs it without a connection (N6 review, finding 7; `zoraya_shell::netgrant::available`). Running programs are not
   told: their connections are closed and they see `PEER_CLOSED`.
4. **`init`.** On the end of either process it ends both and starts them again as before, then makes a new root with
   `narrow` from its own control connection and sends `NetworkRestarted`; if the write fails it closes the new root
   (N6 review, finding 9). After the third restart it sends `NetworkDown` and starts nothing. Its end of a shell's
   services channel is closed when that shell ends (finding 3).
5. **The test-only request** `Crash` (ordinal 40) ends `netd` with exit code 3. It exists only in the test image: `init`,
   `netd` and `zosh` build its path only with the `zoraya-user` feature `net-test`, which only `make test-net-fetch`'s
   image (`build/zoraya-kernel-net-test.elf`, its own ramdisk and target directories) has; every other kernel embeds a
   ramdisk without it, so no volume can turn it on (N6 review, finding 2: the first cut honoured a file in `/etc`, which
   anything that writes `/etc` could create, and `Crash` from any connection). In the test image `netd` honours it only
   if it was started with an event handle at slot 8, which `init` gives only if the volume holds `/etc/zoraya/net-crash`,
   and only on `init`'s control connection, not on a connection made from it with `narrow`. `net crash` makes the shell
   ask `init` on its services channel (after the program's own connection is made); `init` sends `Crash` on its control
   connection; the `net` program sends its own `Crash`, which `netd` answers as an unknown ordinal, and prints its line.
   The production `netd` answers every `Crash` as an unknown ordinal.

**Tensions with accepted ADRs.** *ADR-0053* "a process has what it was handed": the new root arrives on a channel the
shell was handed at start. *ADR-0080* restarts the shell with the storage stack and the network without it; both stand.

## Consequences
- **Positive.** A restart is visible to the person and healed for the next line; the bound of three is tested through
  `init`.
- **Negative.** A second image to build for one test (`make ci-build` builds it); the panel's windows are not covered yet.

## Invariants
- **INV-0296-1.** No image but `make test-net-fetch`'s has a way to end `netd` on request, whatever its volume holds: its
  `netd` answers `Crash` as an unknown ordinal; in the test image only `init`'s control connection can send one.
  *Check:* `make test-net`: the image's `netd` and `init` hold none of the strings of the test image's `Crash` paths
  (`NET_CRASH_MARKS` in the `Makefile`: `netd`'s end on request, `init`'s request on its control connection and its look
  for `/etc/zoraya/net-crash`), `make test-net-fetch` checks that its own programs hold each (so the check is not blind),
  and on the image's boot (`tests/shell/net.script`) `net crash` leaves the service up and `net status` answers after it.
  The shell's `net crash` path has no string; it only asks `init`, whose path the check covers (re-review N6 finding 6).
- **INV-0221-3 and INV-0221-4** (ADR-0221): the first for `init` and the shell. *Check:* `tests/shell/net-fetch.script`:
  three `net crash` with a fetch after each, the fourth leaves the network down and the next line is refused.

## Open questions
- The panel's and the broker's services channels, with the rooms' `+net` words.
- Whether ADR-0080's restart of the shell should become the same notification (ADR-0221's open question).

## Revisit triggers
- A launcher that holds the network and is not told of a restart.

## Architect's decision
Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.
