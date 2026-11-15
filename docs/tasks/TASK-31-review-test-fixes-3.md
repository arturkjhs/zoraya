# TASK-31 review: part work/test-fixes-3

Independent review by the `opus-review` subagent of `git diff origin/main...work/test-fixes-3` at 7689510 (base f969cf4,
2026-11-15). It was read against `docs/tasks/TASK-31-part-test-fixes-3.md`, ADR-0230 and its rule-6 notes,
`docs/tasks/TASK-31-review-test-fixes-2.md` and the architect's requirement: the network test steps may reach only the
runner's own test servers (and the QEMU host address they use), never the Internet, and INV-0230-1 is `checked` again.
The nightly TCG step for libc++'s semaphore, barrier and latch groups was also in scope.

What the reviewer ran, on the Mac mini:
- `make check` on 7689510: rc 0.
- `make test-net-dns`: PASS, 20 steps. `target/test-logs/test-dns-names.log` shows the guest's queries reaching
  `tools/test-dns`.
- `tools/confine-loopback.sh python3 -I PROBE` with a wider probe than the check's (below).
- `tools/check-net-open.sh` on five altered copies of the tree, made with `git archive` into a scratch directory, never in
  the worktree.
- The log of the branch's CI run 43662588759, job `make check-net-open`.

No other QEMU step was run.

Severities: **must-fix** (wrong before merge), **should-fix** (fix in this part or record it), **note**.

## Findings

1. **must-fix**: `test-hand-run` still runs unrestricted and unconfined. Where: `Makefile:942` (`$(call RUN_NET_CMD,...)`),
   `Makefile:180`, `tools/net-open-allowed.txt` (the `RUN_NET_CMD` line), `docs/adr/invariants.md` INV-0230-1.
   `test-hand-run` is a step of `make test` (`TEST_STEPS`, `Makefile:1704`) and of CI's full list. Its network leg boots
   person mode (`--net` without `--test`), so it gets no `restrict=on`, no `--net-open` refusal and no confinement. The
   guest gets SLIRP's whole NAT, a router and the DNS forwarder 10.0.2.3, which asks the host's resolver (the script
   itself expects `gateway 10.0.2.2` and `dns 10.0.2.3 (dhcp)`). Nothing but the script's content keeps it on 10.0.2.2.
   The architect ruled that kind of guarantee insufficient. Review test-fixes-2, finding 1, named this exact gap. This
   part lists `RUN_NET_CMD` as "person mode" in the allow-list, and `check-net-open` accepts any number of callers of
   it. Altered copy B4 below shows a new test step that calls `RUN_NET_CMD` and still passes the check. ADR-0230's new
   "Done" note and the INV-0230-1 row do not mention `test-hand-run`. The `checked:` claim is therefore false for
   `make test`.
   *Scenario:* a change to `hand-run-net.script` (or to `netd`, `fetch`, or `curl`'s defaults) makes the guest connect to
   a real name or address. CI and `make test` stay green, and the host has reached the Internet.
   *Fix:* run the hand-run network leg through `CONFINED`. It tests person mode's command line, so it must stay
   unrestricted. Put it in its own `test-hand-run-net-run` target, so that the wrapper confines it and its server.
   Then make `check-net-open` require that every caller of `RUN_NET_CMD` in a test recipe, and every `$(RUN_QEMU) --net`
   line without `--test`, is reached only through `CONFINED`, as it already requires for `--net-open`. `run-net` is
   the only exception. Alternatively, record `test-hand-run` as an exception the architect accepts, and leave
   INV-0230-1 not `checked`.

2. **must-fix**: `check-net-open` does not fail when the confinement is broken. Where: `tools/check-net-open.sh:28-30`,
   `:55-63`. INV-0230-1's status is `checked: make check-net-open`. The reviewer applied five breaks, each to its own
   scratch copy. Four of them pass the check with rc 0 and the message `ok (... every guestfwd is a relay to 127.0.0.1;
   loopback wrapper: proved to block ...)`:
   - **B1** `CONFINED = ZORAYA_LOOPBACK_ONLY=1 $(MAKE) ...`, in place of `tools/confine-loopback.sh $(MAKE) ...`
     (`Makefile:449`). Both UDP steps then run unrestricted and unconfined, and `run-qemu.sh` accepts the marker. The
     check looks for the text `CONFINED,net-dns,test-net-dns-run)` but never reads `CONFINED`'s definition, and the probe
     calls the wrapper directly, not `CONFINED`. Result: rc 0.
   - **B2** a fourth NetSurf rule, `--net-guestfwd "$(NS_NET).12:443=$(NS_RELAY_OUT)"` with
     `NS_RELAY_OUT = nc 198.51.100.7 443`. Line 61 drops every line that contains the substring `NS_RELAY`, and the
     `NS_RELAY` definition check (line 63) still matches. Result: rc 0.
   - **B3** in `tools/run-qemu.sh:334`, `restrict=,restrict=on` only when `[ -z "$net_guestfwd" ]`. Every relay step
     (fetch, posix, apps, python, netsurf) then runs unrestricted. The check prints only the command line of
     `--test --net` with no other option, so it never sees a guestfwd run. Result: rc 0.
   - **B4** a new step `test-net-extra` that runs `$(call RUN_NET_CMD,...)` (finding 1). Result: rc 0.
   - Control: removing the `--net-open` refusal from `run-qemu.sh` fails the check, as the report says.

   The author's four hand breaks (report, "What was run") are real but narrow. The three breaks the task asked for (B1
   to B3) all get through.
   *Fix:*
   - (a) Check the printed `-netdev` of every network command line the Makefile actually builds, not of a hand-made
     one. For example, run `make -n` of each network step with `QEMU=/bin/echo`, or call `cmd` with each recipe's options
     (`--net-guestfwd`, `--net-forward`, `--net-pcap`, `--net-subnet`). Require `restrict=on` unless the definition is in
     the allow-list.
   - (b) Check that `CONFINED`'s definition starts with `tools/confine-loopback.sh`, and that no Makefile line sets
     `ZORAYA_LOOPBACK_ONLY`.
   - (c) Check each `--net-guestfwd` argument's command after `=`, after expanding `NS_RELAY`/`NET_RELAY`, instead of
     grepping the line for a substring. The command must be exactly `$(CURDIR)/$(HTTPD) --relay 127.0.0.1:PORT`.
   - (d) Optionally, have `zoraya-test-httpd --relay` refuse a target that is not `127.0.0.1` (`tools/test-httpd/src/main.rs:557`).

3. **should-fix**: on CI both confined steps are always skipped, and the skip passes silently. Where: `Makefile:449-451`
   (`CONFINED`), `tools/check-net-open.sh:83-93`, the workflow's `container:` (`.github/workflows/test.yml:293`, `:348`).
   - **Evidence.** The CI log of `make check-net-open` in run 43662588759 says `loopback wrapper: not on this host (the
     steps skip)`. CI's jobs run in the image container, where Docker's default seccomp profile denies
     `unshare(CLONE_NEWUSER)` without `CAP_SYS_ADMIN`. On the bare fallback, Ubuntu 24.04's AppArmor
     `apparmor_restrict_unprivileged_userns` is expected to refuse `unshare -rn` as well. The reviewer did not run
     Linux. Either way the wrapper exits 75, and that is fail-closed: nothing runs unconfined.
   - **Consequences.** INV-0230-1 still holds on CI, because a skipped step opens nothing. But:
     - (i) `test-net-lookup` ran on CI before this part. Now it never runs there, so INV-0316-1, -2 and -3, whose status
       cites `make test-net (test-net-lookup)`, are no longer checked by CI, and nothing records that.
     - (ii) The wrapper's probe never runs on Linux, so its Linux branch (`unshare -rn`, `ip link set lo up`) is
       unproved anywhere.
     - (iii) The skip is an `echo` inside the `test-net` step, which ends `ok`. `check-test-lanes`' rule (a step listed
       and not run prints `SKIPPED` or fails) works on whole steps in `test-par`'s summary, and it never sees a leg
       skipped inside one. So the skip appears in no summary, only in the step's log.
     - (iv) This differs from `NET_OR_SKIP` (`Makefile:318-320`), which fails on CI instead of skipping.
   - *Fix:* exit 75 should fail on CI (`[ -n "$$CI" ]`), like `NET_OR_SKIP`, and CI should be given a way to confine.
     Lowest-risk options, for the architect:
     - (A) For the `test-net` job only, add `options: --cap-add=SYS_ADMIN` to the container through a matrix
       expression, and let the Linux branch use `unshare -n` when it runs as root. The container is root already, and
       the capability ends with the job.
     - (B) On the bare runner, run `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0` in that job only. The
       VM is discarded after the job.
     - (C) Until one of these is chosen, record the CI skip and INV-0316's lost CI coverage in INV-0230-1's and
       INV-0316-x's status, and in `docs/backlog.md`. Do not leave it as "may" in the report.

     Either (A) or (B) also makes `check-net-open`'s probe prove the Linux branch. Then `confined="not on this host"`
     should fail on CI too.

4. **should-fix**: `run-qemu.sh` trusts an environment variable instead of checking the confinement. Where:
   `tools/run-qemu.sh:267`. `--net-open` is accepted whenever `ZORAYA_LOOPBACK_ONLY=1`, whoever set it: break B1, a
   person's shell, or a CI job's `env:`. Nothing confirms that the process is really confined.
   *Fix:* in `run-qemu.sh`, before starting QEMU with `--net-open`, verify the confinement. One way is a datagram to
   192.0.2.1 that must fail with EPERM or ENETUNREACH; `tools/confine-loopback.sh` could provide a `--verify` mode
   that runs inside the confinement. Refuse when the datagram is sent. Keep the variable only as the reason given in
   the message.

5. **should-fix**: `test-net-dns` does not prove that its answers come from `tools/test-dns`. Where:
   `Makefile:456-471`. The script passes on "the name has no address" for `nothing.invalid` and on the local refusal for
   `localhost`. If netd used the lease's 10.0.2.3, the host's real resolver would give the same answers. That happens
   when `net.dns` is not read, and under the reviewer's run `netd` printed `1 dns server(s) (dhcp)` with
   `net.dns = 10.0.2.2` on the disk. So the step would pass with an unconfined wrapper (macOS without a sandbox) as long
   as the host can resolve.
   *Fix:* after the run, require the lines `query localhost` and `query nothing.invalid` in
   `target/test-logs/test-dns-names.log`. `test-net-lookup` does the equivalent through its pcap check. Also check why
   `netd` labels a `net.dns` server `(dhcp)`. That is not this part's code, but it reads as if the setting was not taken.

6. **note**: one assertion of `check-net-open` is dead. Where: `tools/check-net-open.sh:30`.
   `env -u CI cmd --test --net --net-internet` calls a shell function through `env`, which cannot run it: `env: cmd: No
   such file or directory`, on stderr, on every run. The substitution is empty, so the `--net-internet has no
   restrict=on` assertion never runs. The `CI=1` refusal on line 34 does work. A related weakness: every
   `case $(cmd ...) in *restrict=on*) fail` passes vacuously when `run-qemu.sh` dies and prints no `-netdev`.
   *Fix:* `(unset CI; cmd ...)`, and require a non-empty `-netdev` value in each case.

7. **note**: under `--net-open` the guest reaches every loopback port of the host, not only the runner's servers. Where:
   `tools/confine-loopback.sh:28-30` (`(remote ip "localhost:*")`). The reviewer's probe inside the macOS sandbox:
   outside TCP4, UDP4, TCP6, UDP6 and ICMP (`SOCK_DGRAM`, `IPPROTO_ICMP`) all fail with EPERM; `127.0.0.1` works;
   `127.0.0.2` is denied. That is sound. But SLIRP maps 10.0.2.2:ANY to the host's 127.0.0.1:ANY, so the two steps'
   guests can reach any other service on the host's loopback. That includes another worktree's test server, and a local
   DNS forwarder or proxy on `127.0.0.1` (dnsmasq, Docker Desktop, some VPN clients), which runs outside the sandbox and
   asks the Internet itself. INV-0230-1's wording (127.0.0.1) allows this; the architect's wording ("only the runner's
   own test servers") does not. In Linux's empty namespace nothing else is on the loopback, so the gap is the Mac's.
   *Fix:* record it as a known limit in `docs/design.md`'s network-testing note, or narrow the profile to the servers'
   ports (they are known before QEMU starts; the profile could be built per run).

8. **note**: IPv6 under `restrict=on` is not ruled out. Where: `tools/run-qemu.sh:339`. `-netdev user` has IPv6 on by
   default. The guest's libposix refuses `AF_INET6`, but a test program with the NIC (ethd, `net-poke`) could send any
   frame. The reviewer did not verify libslirp's handling of restricted IPv6 TCP and UDP.
   *Fix:* add `ipv6=off` to a test's `-netdev`, and assert it in `check-net-open`. It costs nothing.

9. **note**: `CONFINED` reads exit status 75 from the inner `make` as "cannot confine". Where: `Makefile:449`. Today
   `make` exits 0, 1 or 2, so a recipe's 75 cannot reach it, and the mapping holds. If the wrapper ever runs something
   other than `make`, a command's own 75 becomes a silent pass. A comment on the definition would keep that true.

10. **note**: the `--relay` flush. Where: `tools/test-httpd/src/main.rs:568-586`. The loop is correct: `write_all`, then
    `flush` on each read; a 60 s read timeout or EOF ends it; EPIPE ends it, because Rust ignores SIGPIPE. Upward
    `copy` from `StdinLock` returns partial reads. No host test covers a reply without a newline. A unit test that
    relays a 5-byte reply without a newline through a socketpair-backed stdout is not easy with `stdout()`. The
    QEMU legs (`test-net-fetch`'s TLS) cover it, so this is recorded only.

11. **note**: the static address in `test-netsurf` is sound. Where: `Makefile:841`. libslirp's BOOTP leaves out the
    router and DNS options when restricted, so the change is needed. `.15/24` and gateway `.2` are SLIRP's defaults for
    that subnet, and the names come from `net.hosts`. The cost: `test-netsurf` no longer covers the web rule's refusal
    of a lease-given gateway. Under `restrict=on` no test step has a lease with a router now, since `test-hand-run`
    and the two confined steps are unrestricted. Nothing in the report or `docs/design.md` says which test still covers
    ADR-0223 item 4 for a DHCP gateway.

12. **note**: the nightly step. Wiring checked and sound:
    - `tools/ci-plan.sh` adds `tools/ci-nightly-tests.txt` only for `FULL=1 NIGHTLY=1`.
    - The workflow sets `nightly=1` only for `schedule`. A schedule run on `refs/heads/main` takes the `else` branch,
      because the push-to-main branch needs `push`.
    - `timeout-minutes` tests `test-cxx-libcxx-slow` before `test-cxx-libcxx` (150).
    - No `hashFiles(...)` list was touched. Every one there is comma-separated.
    - The stamp depends on `libcxx-slow-groups.txt`. `build/cxx-libcxx/lct` reaches the tests job through the build
      artifact, not the ports cache, so `script-slow` is always current.
    - `check-ci-jobs` checks the nightly plan, the other plans, the workflow's `NIGHTLY=$nightly` and `TEST_STEPS`.
    - `libcxx-build.sh` refuses a slow group that is not TCG-excluded or not built, and an empty `script-slow`.

    Timing: `target/test-logs/cxx-libcxx-slow-tcg.log` in the author's worktree was written from 02:17:15 to 02:19:22,
    so the Mac's TCG run of the 38 tests took about 2 minutes. 7200 s is 60 times that. That is plausible even for CI's
    x86 TCG, if generous: a hang costs 2 hours of a nightly runner. The report's "58 steps" are script steps, not tests.
    Gaps:
    - `candidate-full/` does not run it, so a stage tag does not gate on INV-0360-2's slow groups. That is a choice for
      the architect, and the report should say so.
    - The run's duration should be in the report. "The Mac's took the time INV-0360-2 gives" is not a measurement.

13. **note**: the report's CI evidence. Run 43662588759 ("27 jobs, succeeded") ran only the check jobs; `build` and the
    test matrix were skipped. No network step and no NetSurf run under `restrict=on` has run on CI yet. `bench-net` was
    edited and not run (stated).

## Checked and found sound

- Under `restrict=on`, libslirp drops all guest UDP except BOOTP and TFTP to the host address, which matches the
  report's measurement. It refuses TCP that matches no guestfwd and does not forward ICMP outside. Every relay is
  `--relay 127.0.0.1:PORT` to a server that binds `127.0.0.1`. Neither test server makes an outward connection (the
  TLS front connects only to `127.0.0.1:backend`). `--net-forward` (hostfwd) is inbound only, on `127.0.0.1`.
- Every `$(RUN_QEMU) ... --net` line of the Makefile is accounted for: `test-net`'s boots and `tests/gui/net.net` are
  `--test` (restricted); fetch, posix, apps, python and netsurf use relays; dns and lookup use `--net-open` through
  `CONFINED`; `hand-netsurf-wikipedia` uses `--net-internet`; `run-net` and `test-hand-run` are person mode (finding 1).
  No other script under `tools/`, `tests/` or the workflow starts QEMU with `--net`.
- `tools/confine-loopback.sh` fails closed. A missing `sandbox-exec` or `unshare`, a profile that cannot be applied, or
  a namespace that cannot be made all give 75 before anything runs. `exec` leaves no unconfined fallback. A `lo` that
  does not come up makes the step fail, not run open. The macOS profile was probed as in finding 7.
- `--net-internet` is refused when `CI` is set and together with `--net-open`, and it is used only by `hand-` targets.
- `make check-ci-jobs` and `make check-net-open` pass on the tree as it is (part of `make check`, rc 0).

## Merge recommendation

Do not merge as is. Findings 1 and 2 make INV-0230-1's `checked:` claim untrue: one step of `make test` still reaches
SLIRP's whole NAT, and three of the three requested breaks pass the check. Either fix 1 and 2 (and 6, which is small),
or set INV-0230-1's status back to `not yet applicable` with the remaining gap stated, and merge the rest. The nightly
step, the relay flush and the NetSurf static address can merge as they are. Finding 3 needs the architect's choice of
(A), (B) or (C) before the next nightly. Until then, INV-0316's CI coverage is lost silently.
