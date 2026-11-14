# TASK-29: serial-line scripts that can pass without testing what they claim

Read-only audit; nothing was run (no QEMU, no build). "CONFIRMED" means confirmed by reading the runner
(`tools/test-runner/src/script.rs`) and the code that prints the string; no finding was confirmed by a run.

## Runner semantics the findings rest on (script.rs)

- `x TEXT` looks only at `text[before..from]`: from the end of the match before the last one to the end of the
  last match (`Seen::between`, line 346). It is evaluated at once, it does not wait.
- `>` does not move the match position, and the shell echoes typed keys (`jobs.script:84-87` expects `/> typed`),
  so a `<` after a `>` can match the echo of the command.
- `~ MS TEXT` and `! TEXT` are NOT unescaped: `Step::Quiet(ms, text.to_string())` (line 190) and
  `Step::Forbid(rest.to_string())` (line 214), while `<`, `^`, `x`, `o`, `w` go through `text_of`. A `\r\n` in
  them is four literal characters and can never arrive.

## Confirmed findings

### S1. `~` with escapes never fires (runner bug, two scripts)
- `tests/shell/console.script:117` `~ 400 \r\n/>`: claims "The shell waits for the job, not its first process:
  no prompt while a child lives."
- `tests/shell/files-write.script:174` `~ 300 \r\n/>`: INV-0152-2, no prompt while `echo $x | sleep 60` runs.
- Break that passes: the shell prints the prompt at once. In console.script the `< tick` before it also matches
  the echo (S4), so the whole claim of lines 114-120 rests on `~ 300 tick` after the Ctrl-C.
- Fix: `text_of(text)` in the `~` arm of `parse` (and in `!`), with a unit test.
- Adjacent, outside my scope: `tests/posix/zosh.script:77` `! tstp: continued\r\ntstp: continued` has the same
  defect through `!`.

### S2. `x` placed before the answer has arrived (three scripts)
- `tests/shell/net-settings.script:17-18` `x net.gateway`, `x net.ip` right after `> cat /config/settings\r`.
  Claim (INV-0229-1): "a value outside its type is refused and the file is unchanged". The range examined is
  line 14's message to line 15's prompt; the output of `cat` is not in it. Break: the refused value is written.
  Fix: move both `x` lines after the `< \r\n~>` of line 19.
- `tests/shell/exit.script:27` and `tests/shell/jobs.script:114` `x [1]` right after `> jobs\r`: "A new shell:
  no jobs". The range is the banner to the prompt, before `jobs` answers. Fix: move after the next prompt.

### S3. `x` whose range ends before the text could appear
- `tests/shell/console.script:27-28` `< hello` then `x hello.cmd` (INV-0061-2, "no descriptions among the
  programs"). The range ends at the end of the first `hello`; `hello.cmd` contains `hello`, so it can never lie
  inside. Fix: after the prompt of line 29, `x .cmd`.
- `tests/shell/jobs.script:89-90` `< [1]  running ...` then `x [2]`: a `[2]` row is printed after the `[1]`
  row, outside the range. Fix: move `x [2]` after the prompt of line 91.

### S4. Expected text is in the echo of the typed command
- `tests/shell/disk-filler.script:47-48` `> cat /t/home/note.txt\r` / `< home` (INV-0265-3, "Home writes").
  Break: the write is refused and `cat` says not found. Fix: `< \r\nhome\r\n`.
- `tests/shell/system-ro.script:109-110` `> cat /home/fine.txt\r` / `< fine` ("The rest of the volume is still
  writable"). Same break. Fix: `< \r\nfine\r\n`.
- `tests/shell/net-fetch.script:94-95` `> +net nonsense: fetch ...\r` / `< +net nonsense:`. The real message is
  `zosh: +net nonsense:: PROBLEM` (`crates/zoraya-shell/src/netgrant.rs:127`). Break: the malformed word is
  accepted and the page fetched. Fix: `< zosh: +net nonsense:: ` and a `status` step.
- `tests/shell/files-write.script:131-132` `> echo hi there | cat\r` / `< hi there` (a builtin into a program).
  Break: nothing comes through the pipe. Fix: `< \r\nhi there`.
- `tests/shell/console.script:108-109` and `115-116` `> fanout N tick...\r` / `< tick` (`ticker` prints `tick`
  every 50 ms). The Ctrl-C that follows can be sent before any child ticks, so "ticking children go silent" may
  be checked on children that never ticked. Fix: `< tick\r\ntick`.
- `tests/shell/system-ro.script:34-35`, `52-53` `> echo sync\r` / `< sync`: asserts nothing, and it is not the
  `sync` command; probably `sync` was meant, so that the refusals are checked after a commit.
- Harmless (the next step covers them): `net-fetch.script:69-70` `< cut.bin` (the `exit status 1` after it is
  the check), `net-fetch.script:131`, `hostile.script:51`, `hand-run.script:12`, `console.script:190,220` (`up`).

### S5. Expected text that the failure also prints
- `tests/shell/rooms.script:75-76`, `77-78` and `tests/shell/keeper.script:75-78`: `> room --leave\r` /
  `< /t/w> `. The prompt inside the room is `agent-1 /t/w> `, which contains it. Break: `--leave` does nothing.
  Fix: `< \r\n/t/w> `.
- `tests/shell/rooms.script:79-80` and `tests/shell/hostile.script:55-56`: `> room\r` / `< home` after
  `room remove ... --kill`. `zosh.rs:3566` prints the `home` row always, before the rooms. Break: the room is
  not removed. hostile.script's header claims "removing the room with --kill brings its use to zero"; nothing
  reads the use. Fix: after the prompt, `x agent-1` / `x hostile`.
- `tests/shell/net-settings.script:29` `< net.dns = 1.1.1.1` after the refused `+10.0.2`: also matches
  `net.dns = 1.1.1.1 10.0.2`. Fix: `< net.dns = 1.1.1.1\r\n`.
- `tests/shell/disk-filler.script:52-54` "the other room [writes] up to its own limit" (INV-0265-3):
  `< diskfill: stopped after` / `< refused (16)` also match `stopped after 0 KiB: create refused (16)`
  (`user/src/bin/diskfill.rs:95-99`). Break: the first room's fill exhausts the second's. Fix: expect the KiB
  figure of a 256K limit, or forbid `stopped after 0 KiB`.
- `tests/shell/console.script:123` `< slots: 1` ("A child holds only what the shell lists"): `handles.rs`
  prints `slots: 1 2 ...`, so any table that starts with slot 1 passes. Fix: the whole line with `\r\n`.
- `tests/shell/python.script:41` `< ok ssl`: `smoke.py:32` prints `verify_mode == CERT_REQUIRED` and
  `check_hostname` instead of asserting them, and the script does not read them. Fix: assert in smoke.py.

### S6. Forbidden text no code prints
- `tests/shell/console.script:7` `! no such command: hello`: the message is `zosh: hello: no such command`
  (`crates/zoraya-shell/src/eval.rs:1487`). Stale since the native shell; redundant with line 44.

### S7. Steps that assert nothing where the comment claims a check
- `tests/shell/console.script:105-106` `> jobs\r` / `< \r\n/>`: "and the job is gone". Fix: `x [1]` after.
- `tests/shell/files-write.script:12-13` `> ls\r` / `< \r\n~> `: "The shell starts in an empty home". Fix:
  `> count $(ls)\r` / `< \r\n0`.
- `tests/shell/rooms.script:29-37`: "the shell says so once", "a new hint for the new line, still one": no
  `x`/`o` counts the hints. Fix: `x hint: room` after each prompt (the range then starts after the first hint).
- `tests/shell/apps.script:180-181` `grants curl --help`: "A line without a URL gives no network and no roots".
  The roots are covered by the two adjacent lines, the `net:` line (printed after them, as line 178 shows) is
  not. Fix: `x net:` after the prompt.
- `tests/shell/exit.script` and `tests/shell/jobs.script:109-115` (INV-0158-3, "the second exit ends both"):
  no step observes that the old shell's jobs ended; a new shell has an empty job table either way.
- `tests/shell/files-write.script:207-210` `settings --reset` then `settings` with only the prompt awaited;
  `net-settings.script:31-32` `settings --check` likewise.

### S8. test-crash passes with nothing durable to check
- `tools/test-runner/src/crash.rs:172-236`: a round whose kill comes before the first `crash: durable N` line
  has `durable = -1`, and `crashcheck -1` passes on an empty prefix. Nothing requires
  `totals.durable_files > 0` (it is only printed). Break: the `crash: durable` line changes or is never seen;
  every round is then vacuous. Locally there are 2 rounds with delays of 0-2000 ms.
- Fix: fail in `all()` when `durable_files == 0`.

### S9. Skips (class 6)
- `Makefile:400` `[net-dns] SKIP` when `CI` is set and `NET_DNS != 1`; no workflow sets `NET_DNS`, so
  `tests/shell/net-dns.script` never runs on CI (documented in the script's header). It prints `SKIP`, not the
  `[LABEL] skipped: ` form `tools/test-summary.sh` counts.
- `Makefile:572` (test-apps) and `Makefile:678` (test-python, test-python-regr): `--probe-net >/dev/null 2>&1
  || { skipped; exit 0; }` gives a green job on CI for a missing network and also for the probe's exit 2 (a
  broken QEMU). Both are CI jobs of their own (`tools/ci-tests.txt`). Only test-net (`Makefile:308-312`) fails
  on CI. The same line is in test-net-fetch (368), test-net-lookup (429), test-net-posix (498), which run on CI
  only under test-net, after its strict probe. (test-netsurf and test-hand-run: reported by others.)
- `Makefile:853` `EL2_OR_SKIP` on boot-auto-el2: a local skip under HVF before the M3; `FULL_ONLY` lines
  (854-857, 1247) and `[ "$(FULL)" != 1 ] ||` (339-340) skip TCG locally, by design; CI sets FULL=1.
- `test-python-long` is in no CI list (by design, `regr.py`).

## Suspected, not verified

- `tests/shell/python-regr.script:11` `< Result: SUCCESS`: CPython's regrtest also says SUCCESS when modules
  are skipped (a missing `_sqlite3` or `zlib` skips the module). Not verified against the regrtest source
  (not in the repository). Fix if so: expect `All 16 tests OK.` or forbid `skipped`.
- `tests/shell/grants.script:50` `x tried to read /t/w/` ("A missing path ... never reported"): the range ends
  at the hint for `/t/secret.txt`; a report for `missing.txt`, tried last, would follow it. Also "reported
  once" has no `o`. Order of hints not read in zosh. Fix: move the `x` after the prompt of line 51.
- `tests/shell/grants.script:60` `x \nsecret\r` and `console.script:63` `x not run\r\n`: both ranges end at the
  refusal message; output printed after it would be missed.
- `tests/shell/console.script:225` `x bytes dropped` ("The count is reported once"): the range is one command;
  `o bytes dropped` after the last prompt would say it.
- `tests/shell/net.script:30-35` (INV-0296-1, "`Crash` ends nothing"): `net status` after `net crash` may pass
  after a restart too; `init` would print `init: the network service ended` (`init.rs:958`), which is not
  forbidden. The Makefile's check of the binaries' strings is the real check. Fix: `! init: the network`.
- `tests/shell/net-lookup.script:30-35`: "answers at its lookup bound (8 s)": no time is measured.
- `x power off` before `> poweroff\r` (most scripts) looks at one gap only and proves next to nothing.

## Makefile recipes: read and found sound

- The TLS log checks of test-net-fetch (391-397), test-apps (599-605), test-python (699-704): a missing or
  empty log fails them (`! grep -q '^tls valid: request of'`); `\(expired\|wrong\)` works with this host's
  grep (checked with a one-line `printf | grep`).
- `rc=$?` is carried to `exit $$rc` in every recipe read; the `for` loops of NET_CRASH_MARKS `exit 1` inside.
- test-net-fetch 366-367 checks that the INV-0296-1 strings exist in the test image, so test-net's inverse
  grep is not blind.

## Scripts read in full and found sound

conventions.script (generated, exact `^` lines), leak.script (`zosh: exit-with: ` is the real form,
`zosh.rs:4452`), reboot.script, storage-down.script, device-error.script, clock.script, net-posix.script,
net-dns.script, hand-run-net.script, apps-launches.script, apps-refusals.script, keeper.script (but S5),
tests/python/*.py, tests/apps/tlsthreads.c (the refusal reason is compared).

## Not read

`tests/apps/cryptothreads.c`, `tests/posix-net/zfetch.c`, the `--check-pcap` code of `tools/test-dns`,
`tests/shell/net-bench.script`, `tests/shell/hand-run-gui.script`, `tools/ci-plan.sh`.

## Helpers read

- `tools/test-httpd/src/main.rs:395-412` (`tls_serve`): `tls NAME: ended after N request bytes` is printed
  whenever the read ends before a full request head, so also for a TCP connection that closes with no
  handshake, or a handshake that completed with nothing sent after it. The line therefore shows "no request
  byte" (INV-0228-1's claim), not "the handshake was refused"; the refusal itself is shown only by the guest's
  message or exit status in the scripts. Sound for the invariant as worded.
- `tools/net-poke.sh`: bounded by its own alarm; its failure is not read, the pcap check in test-net is.
