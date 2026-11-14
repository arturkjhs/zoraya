# TASK-31 review: part work/test-fixes-2

Independent review by the `opus-review` subagent of `git diff origin/main...work/test-fixes-2` at ccad298 (2026-11-14), read
against `docs/tasks/TASK-31-part-test-fixes-2.md`, the audit (`docs/research/test-audit/report.md`, T1, V40, C2, C3) and the
architect's decisions on this part in `docs/backlog.md`. The reviewer ran `make check` on ccad298 (rc 0), probed `adr-check`
with altered copies of `invariants.md` (below), ran a three-rule model of the `libc.a` stamp under the host's GNU make 3.81,
and read the candidate run 43610301170's `build` job log. No QEMU was run.

Severities: **must-fix** (wrong before merge), **should-fix** (fix in this part or record it), **note**.

## Findings

1. **must-fix** `docs/adr/invariants.md` INV-0230-1, `tools/check-net-open.sh`, `tools/net-open-allowed.txt`: the row says
   `checked:`, but the steps of `make test` that use the network run without `restrict=on`, so nothing stops them reaching
   outside `127.0.0.1` and SLIRP's network. What the script checks is that each open run is listed. It does not check that
   the invariant holds. ADR-0230's Decision item 2 has a test's network as `restrict=on` with `guestfwd` to the runner's
   `127.0.0.1` ports. `NET_FETCH_RUN`, `NET_POSIX_RUN`, `APPS_RUN`, `NS_RUN`, `test-net-lookup` and `test-python` (all
   steps of `make test`) instead pass `--net-open`. The guest then has the whole host NAT: a script that fetches a public
   address, or a port that resolves a real name, connects out and every check stays green. Two more gaps:
   - `test-hand-run` (a step of `make test`) runs `RUN_NET_CMD` (`Makefile:180`, `:879`), which is person mode with no
     `--test`, so it has no `restrict=on` and no `--net-open` either. The awk in `check-net-open.sh:17-18` never sees it,
     so the list reads as complete when it is not.
   - Allow-list line 8 says `test-net-fetch` is "the same" (against the test server). Its line (`Makefile:435`) is the
     `net-dns` leg. That leg asks SLIRP's forwarder, which asks the host's resolver, and `nothing.invalid` is normally
     sent on to an outside DNS server. It runs locally by default and on CI with `NET_DNS=1`.

   The audit's C3 said this needs "a decision on what `--net-open` may reach". No decision is recorded. The allow-list
   makes that decision without one.
   *Fix:* put INV-0230-1 back to `not yet applicable (until the network steps run under restrict=on with guestfwd, ADR-0230
   item 2)`, or move the runs to `restrict=on` plus `--net-guestfwd`/`guestfwd` to the runner's ports and keep the check.
   Add `RUN_NET_CMD` (or any `run-qemu.sh --net` call without `--test`) to what the script looks for. Correct line 8 of the
   allow-list. Ask the architect for C3's decision. If the open runs stay, that is a deviation from ADR-0230 item 2 and
   needs a proposed ADR.

2. **should-fix** `tools/adr-check/src/main.rs:507-511`, `:503`: the V40 rule still accepts statuses that check nothing.
   The reviewer gave INV-0041-3 each status below in a copy of `invariants.md` and ran `adr-check`. Every one passed with
   0 errors:
   - `` checked: `tools/` ``: a directory.
   - `` checked: `tools/net-open-allowed.txt` ``: a data file.
   - `checked: by review, see tools/zcc`: prose. A path anywhere in an un-backticked status counts. This is the same
     wording as INV-0042-1's old row with a path added, so V40's case passes again.
   - `` checked: `cargo test` ``: no `-p`. `j == i + 2` counts it as a command.

   `make test-gone` and the old INV-0042-1 text are refused, as claimed.
   *Fix:* a `tools/` or `.github/` word counts only when it is an executable regular file, or a workflow file under
   `.github/workflows/`. Count a bare `cargo test` only when it is in backticks. Add the four cases above to
   `checked_names_a_command_that_exists`.

3. **should-fix** INV-0042-1 (`docs/adr/invariants.md:123`) and ADR-0001 rule 6: rule 6 lists "an `abi` type" as a check,
   and the new rule refuses any `checked:` status that does not name a command. That makes INV-0001-4's check stricter
   than ADR-0001 without saying so. The row's new status, `not yet applicable (until ...)`, is also not true: the
   invariant applies today and is unchecked. Rule 6 also requires the reason to go into the ADR's open questions, and
   ADR-0042's open questions are unchanged.
   *Fix:* the better fix is a check that fails when the invariant breaks. For example, a `check-*` script asserting that
   `kernel/src` has exactly one user-copy read of the spawn buffer, and that `SpawnRequest::decode` takes `&[u8]`. Failing
   that, add the rule 6 open-question note to ADR-0042, and either say in a proposed ADR that INV-0001-4 now requires a
   command or ask the architect whether an `abi`-type status may name the type's file.

4. **should-fix** `tools/test-map.txt:181`, `:175`: the narrower map leaves out changes that do change libc++'s tests:
   - `tools/zc++` is a symlink to `zcc` (`git ls-files -s`: mode 120000). A change to the C++ driver is a change to
     `tools/zcc`. Line 181 no longer lists `test-cxx-libcxx`, and line 182 only fires when the link itself changes.
   - `ports/Makefile` (the LLVM unpacking, `CXX_HEADERS_STAMP`/`CXX_STAMP`, `libc.a`, crt) falls to `ports/` (line 175).
     So do `ports/sources.lock` (the LLVM version), `ports/crt/` (every test's `crt1.o`) and `ports/include/`.

   An LLVM bump, or a change to the `-std`/link flags of `zc++`, no longer runs libc++'s own tests on the candidate. They
   run only in the nightly or `candidate-full/` set, after the merge. The architect's list ("ports/cxx, ports/musl,
   libposix and tests/cxx") did not deal with the symlink.
   *Fix:* add `test-cxx-libcxx` back to `tools/zcc`, and add lines for `ports/Makefile`, `ports/sources.lock`,
   `ports/crt/` and `ports/include/` (ask the architect if that widens the decision). `kernel/` (the futex libc++'s wait
   polls) and `crates/zoraya-rt/` have never run `test-cxx-libcxx`. That gap was there before this part and should be
   recorded.

5. **should-fix** `Makefile:1056` (`CXX_LIBCXX_TCG`) with `tests/cxx/libcxx-tcg-excluded.txt`, INV-0360-2: on any host
   that is not a Mac, the `-tcg` script is used whatever the accelerator, and CI is Linux. So `thread.semaphore`,
   `thread.barrier` and `thread.latch` now run in no CI job, nightly and `candidate-full/` included. They run only when an
   agent's `make test-changed` on the Mac picks the job. The report's "HVF runs everything" is true only on the Mac. The
   nightly still runs `make test-cxx-libcxx` (`tools/ci-tests.txt:52`), but without these groups, and INV-0360-2 stays
   `checked: make test-cxx-libcxx` with nothing to say so.
   *Fix:* record the limitation in INV-0360-2's row (or ADR-0360's open questions) and in `docs/design.md`'s libc++
   paragraph, next to the backlog item on libc++'s atomic wait over the futex (ADR-0301). Better, keep the three groups in
   the nightly or full set under TCG with their own timeout, so that some CI job still runs them.

6. **note** `ports/Makefile:170-184`, the `libc.a` stamp: the reviewer found it correct.
   - **The model.** The reviewer ran a three-rule model under GNU make 3.81 (FORCE'd library, stamp rewritten only on a new
     `cksum`, consumer). The first run relinks, a second run does not, and `-j4` does not either: make reads the stamp's
     time again after its recipe.
   - **Ordering.** `libc.a` waits for the stamp, which waits for cargo, so `-j` cannot read a half-written library through
     this rule.
   - **A failed relink.** If the relink fails after the stamp is written, the stamp stays newer than the old `libc.a`, and
     the next run relinks.
   - **The cache.** The stamp sits in `build/ports/obj`, the same cache entry as `libc.a`, so they are restored together.
     `actions/cache` saves only on success. A `zoraya-posix` change is not in the key: the cksum differs and the relink
     happens anyway, which is right.
   - **What is not shown.** The candidate run missed the ports cache, because the key changed with `ports/Makefile`
     (log: "Cache not found ... ports-build-v3-container-fae3..."; "libzoraya_posix.a changed: 2092420297 11953294";
     "libc.a: hid 476 ..."). So the cache-hit path on CI, and whether cargo writes the same bytes from one job to the
     next, are not shown yet. A build that is not byte-for-byte reproducible would only lose the speed-up, not
     correctness. Check the first run after merge that hits `ports-build-v3-container-<new key>`: its log should have no
     `libc.a: hid` line.
   - **Before this part.** Two sub-makes of `ports/Makefile` running at once would race on the stamp and on `libc.a`, but
     that race was there before.

7. **note** `user/src/bin/t_quota.rs:59-66`, `:438-457`, `:466-470`: the handshake cannot deadlock. Both sides poll with
   `eventually`'s 10 s bound and panic, so a lost message fails the test instead of hanging it. The code is test-only.
   The channel end moves into the child at slot 9, and the parent reads usage after it has consumed `ready`. One
   weakness: `channel_read(...).is_ok()` also retries when the peer has closed. A child that dies early costs 10 s and
   reports "no message" instead of the child's exit. Waiting for `READABLE | PEER_CLOSED` and asserting on the reason
   would make that failure clear.

8. **note** `tools/check-net-open.sh:27-28`: only `tools/test-httpd` is checked. `tools/test-dns` (the `test-net-lookup`
   server) binds `127.0.0.1` (`main.rs:121`), but nothing would catch a change to it. The pattern for a wide bind also
   misses `bind("0.0.0.0:N")` and `Ipv4Addr::UNSPECIFIED`. Part 1 of the script is sound: with QEMU set to `/bin/echo`,
   it fails on a test network without `restrict=on`, and on `restrict=on` together with `--net-open`.

9. **note** INV-0172-4 (`user/src/bin/zosh.rs:3646-3660`, `:4896-4902`): the console's shell creating
   `room new X +w /t` from inside an entered room follows ADR-0172. It is not a trust-boundary defect. Point 3 of the
   Decision says each grant "is resolved against the shell's own table, so a room holds only what its creator held". Point
   4 bounds what a *program in a room* holds. Entering a room (`enter_room`, `zosh.rs:3117`) leaves the shell's own
   `root` unchanged: the room's grants are added to each line it launches, not taken from the shell. `room new` is a
   builtin, run by the person's shell with the person's authority, and the console's shell holds the volume's root (slot
   5), as INV-0168-2 allows. The reviewer found no way to run a builtin as a room's program would: no `source` builtin,
   and a script runs as a child, which gets a table. The refusal guards the case that matters: a shell that holds a table,
   such as a room's own terminal or a `zosh` launched as a line inside an entered room. The audit's C2 wording ("a shell
   in a room can run `room new wide +w /`") does not hold for the console's shell, and a case in `rooms.script` at the
   console cannot test the refusal.
   *What should happen:* test the refusal where it applies.
   - Launch `zosh` as a line in the entered room, so that it starts with the room's table, and type `room new wide +w /etc`
     there, expecting `outside what this shell holds`. That runs under `make test-files` at the console. The test should
     first confirm that a launched `zosh` reads its table through `namespace::take`.
   - Or add the host tests the ADR names, once the planner moves to `zoraya-shell` (open question at ADR-0172 line 114).

   Whether entering a room should also narrow what the person's own `room new` may give is a UX question for the
   architect, not a defect. If the answer is yes, it is a change to ADR-0172 point 3.

## Checked and found sound

- **`libcxx-build.sh`.** The directory matching is right in both places. The "listed but not built" check accepts a
  directory when at least one test was built from it. The comment lines of the list have `$1 = "#"` and never match. The
  TCG script is taken only where `CXX_LIBCXX_TCG` is true, so on the Mac HVF runs the full script.
- **The CI job.** `test-cxx-libcxx` took 4 min 49 s in run 43610301170.
- **Wiring.** `make check-net-open` is wired into `CHECK_STEPS`, `TEST_STEPS`, `test-par.sh`, the CI checks matrix and
  the map. `make check` passes at ccad298, with `check-test-lanes` and `check-ci-plan` among its steps.
- **`make_targets`.** It skips variable assignments, including `X ?= a:b`, and recipe lines. Every target named by an
  existing `checked:` row resolves.

## Recommendation

Do not merge as it is.
- **Must-fix:** finding 1. INV-0230-1's `checked:` claims something that is not true.
- **Should-fix before merge:** findings 2 to 5. They are small. Findings 3 and 4 touch the architect's decisions, so ask
  first.
- **Ready to merge:** parts 1 (T1) and 3 (`t_quota`) could go now.
