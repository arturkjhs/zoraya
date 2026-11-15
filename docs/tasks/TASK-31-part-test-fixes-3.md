# TASK-31, part work/test-fixes-3: report

Branch `work/test-fixes-3`, from `origin/main` f969cf4. Machine: Mac mini, `CARGO_BUILD_JOBS=3`, one QEMU at a time.

## What changed

1. **INV-0230-1 (the architect's decision of 2026-11-15): the network test steps reach only the runner's own test servers.**
   - The TCP steps (`test-net-fetch`, `test-net-posix`, `test-apps`, `test-python`, `test-netsurf`, `bench-net`) no longer use
     `--net-open`. They are `restrict=on` and reach `tools/test-httpd` through `--net-guestfwd` rules (`NET_RELAY` in the
     Makefile): a connection to `10.0.2.2:PORT` (`203.0.113.2:PORT` in `test-netsurf`) runs `zoraya-test-httpd --relay
     127.0.0.1:PORT`. Measured: a guestfwd on the host address works under `restrict=on`; a connection to any other port is refused.
   - `zoraya-test-httpd --relay` wrote to the line-buffered standard output, so a TLS flight (no newline) waited in the buffer:
     it now flushes every chunk (the first run of `test-net-fetch` under `restrict=on` stopped at the TLS legs for that reason).
   - A UDP query to the host cannot pass `restrict=on` (measured: `test-net-lookup` under `restrict=on` fails at the first
     lookup). The two steps that need it, `test-net-lookup` and the DNS leg, run unrestricted but inside the new
     `tools/confine-loopback.sh` (macOS: `sandbox-exec` denying every outbound address but the loopback; Linux: `unshare -rn`
     with `lo` up). `tools/run-qemu.sh --net-open` refuses to start outside it (`ZORAYA_LOOPBACK_ONLY=1`); a host that cannot
     confine (exit 75) skips the two steps with a printed `SKIP`, it never runs them unconfined.
   - The DNS leg of `test-net-fetch` asked SLIRP's forwarder, which asks the host's resolver. It is now its own step,
     `make test-net-dns` (called by `test-net`), answered by `tools/test-dns` (`localhost` = 127.0.0.1, every other name
     NXDOMAIN) through `net.dns = 10.0.2.2` on the test image; `tests/shell/net-dns.script`'s comments say so. It no longer needs
     `NET_DNS=1` on CI.
   - `--net-internet` is new, for the hand run that reaches the real Wikipedia (`hand-netsurf-wikipedia`); refused on CI.
   - `test-netsurf`: with `restrict=on` SLIRP's DHCP lease has no router, so the web rule did not know the gateway and a click on
     the gateway link got "Could not connect" instead of the refusal page (found by the first run, step 134 of 186). The test disk
     now gives `net.address = static`, `net.ip = 203.0.113.15/24`, `net.gateway = 203.0.113.2` (the values the lease gave).
   - `make check-net-open` (and `tools/net-open-allowed.txt`) now check: `restrict=on` in the printed command line unless
     `--net-open`/`--net-internet`; `--net-open` refused outside the wrapper and `--net-internet` refused on CI; every
     Makefile line without `restrict=on` listed with its reason; `--net-internet` only in `hand-` targets; `--net-open`
     definitions started only through `CONFINED`; every `--net-guestfwd` a relay to 127.0.0.1; the servers bind 127.0.0.1; and
     the wrapper really blocks an outside TCP connection and UDP datagram (a probe through `python3`; said so, and skipped, where
     the wrapper cannot confine).
   - `docs/adr/invariants.md`: INV-0230-1 is `checked: make check-net-open`. ADR-0230: one line under the rule-6 note. `docs/design.md`
     and `docs/backlog.md` updated.
2. **libc++'s semaphore, barrier and latch groups under TCG, nightly.** `make test-cxx-libcxx-slow` runs `OUT/script-slow` (new,
   from `tests/cxx/libcxx-build.sh`; the groups of `tests/cxx/libcxx-slow-groups.txt`, each of which must also be in the TCG
   exclusion list) under `--accel tcg`, runner timeout `CXX_LIBCXX_SLOW_TIMEOUT` 7200 s. `tools/ci-nightly-tests.txt` lists it;
   `tools/ci-plan.sh` adds that list only with `FULL=1 NIGHTLY=1`; the workflow's plan step sets `NIGHTLY=1` on a schedule run,
   and the tests job's timeout is 150 minutes for it. It is not a step of `make test`, so it is not in `TEST_STEPS`, the lanes or
   `tools/test-map.txt` (a candidate or push must not run it). `make check-ci-jobs` checks that the nightly plan has it, the
   other plans do not, the workflow sets `NIGHTLY`, and it is not in `TEST_STEPS`. INV-0360-2's limit, `docs/design.md` and the
   backlog item say so. No `hashFiles` list was touched.

## What was run (all on this machine, HVF unless noted)

- `make check`: all checks passed (before each commit).
- `make test-net-lookup`, `test-net-dns`, `test-net-fetch`, `test-net-posix`, `test-apps`, `test-netsurf`, `test-python`: each PASS
  with the new settings. (`test-net-posix` first failed to build `libc.a` from a musl tree copied from another worktree; a fresh
  unpack fixed it.) `bench-net` was edited the same way and not run.
- `make test-cxx-libcxx-slow`: PASS, 58 steps, TCG, 450 tests built (full libcxx build in this worktree), `fsck` clean.
- By hand, the check fails when: an unrestricted `--net-open` recipe is added (not in the allowed list); `restrict=on` is
  removed from `run-qemu.sh`; the wrapper is changed to run the command unconfined (the probe reaches the outside); a
  `--net-guestfwd` with a command other than the relay is added. `make check-ci-jobs` fails when `ci-plan.sh` ignores `NIGHTLY`
  and when the workflow does not set it.
- `gh run list --branch work/test-fixes-3 --workflow test.yml`: the run of the second commit (43662588759) has 27 jobs and
  succeeded.

## Known limits, not done

- Linux CI: `tools/confine-loopback.sh` uses `unshare -rn`, which Ubuntu 24.04 may refuse (AppArmor restricts unprivileged user
  namespaces); then `test-net-lookup` and `test-net-dns` print `SKIP` on CI and do not run there. Not run on Linux here. The
  architect decides whether CI should lift that restriction or run the two steps under `sudo`.
- `make test-net` as one chain, the candidate run and the nightly were not run; the first nightly after the merge shows whether
  7200 s and 150 minutes are enough for `test-cxx-libcxx-slow` under CI's TCG (the Mac's TCG run is the only measurement).
- The macOS sandbox does not stop `getaddrinfo` through the system's resolver daemon; QEMU's SLIRP does not use it for a guest's
  traffic (its forwarder sends UDP itself, which the sandbox denies).
