# TASK-31, part work/host-ports: independent review

Diff `7361fedf..HEAD` (44026e9b, 72a287ee) of branch `work/host-ports`. Read: `tools/run-qemu.sh`, `tools/net-poke.sh`,
the `test-net` recipe of the `Makefile`, section 7 of `tools/check-net-open.sh`, `tools/test-sshd.sh`, the report and the
queue entry, and the logs `build/before-net.log`, `build/after-net.log`, `build/after-net2.log`. Mutations were run on a
`git archive` copy of `Makefile` and `tools/` in a scratch directory, never on the worktree; no make or QEMU was run.

## Findings

### 1. must-fix: the window of the race is stated as "a few milliseconds" without a measurement, and its cause too narrowly

Where: `tools/run-qemu.sh:66-68`, report "Known limit", `docs/architect-queue.md` entry.

The port is closed by the picker's `perl` and bound again by QEMU's slirp when QEMU creates its netdevs. Between the two
lie the rest of `run-qemu.sh`, the `exec`, the dynamic loading of QEMU and its Homebrew libraries, option parsing and the
accelerator's setup. That is QEMU's start-up time, not a few milliseconds, and it grows on a loaded host (the parallel
lanes this part is for). Nothing in the logs measures it. The report also says two pickers collide "only if one closed it
and the OS reissued it within the window". In fact any `bind(…, 0)` or loopback `connect()` of any process in that
window can get the port: the ephemeral range is shared. The concurrent steps do exactly this (`test-httpd --port 0`,
`test-dns --port 0`, the guestfwd relays connecting to 127.0.0.1, `test-sshd.sh`'s `free_port`). `test-sshd.sh:81`
already guards against two picks in a row returning the same port, which shows that the OS does not avoid a port it has
just released.

Scenario: `test-net` and `test-apps-ssh` start together under `make test-par`. `free_port` gets P just after
`run-qemu.sh` closed it, and sshd binds P. QEMU then fails with "Could not set up host forwarding rule". The failure is
loud, not silent, and that part of the stated limit is correct.

Fix: write the limit as "the window is QEMU's start-up up to its netdev (not measured)", name every competitor, and
either measure it (for example, the time from the picker's close to the moment the port accepts) or drop the number. To
remove the race, look at two options: a retry of `run auto` in the recipe when the log holds "Could not set up host
forwarding rule", or `hostfwd=tcp:127.0.0.1:0-…` with the bound port read back over the QMP socket
(`info usernet`). For the second, check first that this QEMU's slirp accepts host port 0.

### 2. must-fix: the comment in run-qemu.sh points to a check that does not exist

Where: `tools/run-qemu.sh:65`: "Two runs on one machine never name the same port (tools/check-host-ports.sh)".

There is no `tools/check-host-ports.sh`. The check is section 7 of `tools/check-net-open.sh`. The sentence is also
untrue as written ("never"), see finding 1. Fix: cite `tools/check-net-open.sh` section 7 and say "differ unless the
race of …".

### 3. must-fix: `make check` leaves one file in `$TMPDIR` at every run

Where: `tools/check-net-open.sh:38`: `need … --net-forward auto:${TMPDIR:-/tmp}/check-net-open-fwd.$$`.

`cmd` runs `run-qemu.sh` with `QEMU=/bin/echo`. The auto path really picks a port and renames it into that file, and
nothing removes the file. Reproduced: 12 runs of the copy left 12 files `check-net-open-fwd.PID` in the scratch
`TMPDIR`. Fix: put the file under `$tmp` (create `$tmp` and its trap before section 1), or `rm -f` it after the `need`.

### 4. warning: `--net-forward auto` without `:FILE` passes validation and leaves a file in the current directory

Where: `tools/run-qemu.sh:287-295, 382`.

`auto` does not match `auto:?*`, so `net_forward_file` stays empty, and the second `case` accepts `auto`. The script then
picks a port, writes `.$$` (a dot file such as `.8757`) in the current directory (the repository root under make), fails
at `mv … ""` and dies with "cannot write". Reproduced in the copy: the dot file stayed. Fix: in the second `case`, accept
`auto` only when `net_forward_file` is set (`auto) [ -n "$net_forward_file" ] || die "--net-forward auto needs :FILE"`).

### 5. warning: check 7a and 7b catch only the literal forms; the mutations they miss pass

Where: `tools/check-net-open.sh:155-158`.

Mutations run on copies, with the result of `check-net-open.sh`:

| Mutation | Result |
|---|---|
| Makefile `--net-forward 47015` | caught (7a) |
| Makefile `$(HTTPD) --port 8080` | caught (7b) |
| run-qemu.sh auto uses host port 47015 | caught (7c, bind fails) |
| run-qemu.sh guest side = host port | caught (7c) |
| run-qemu.sh writes the wrong port to FILE | caught (7c) |
| net-poke.sh ignores the file form | caught (7d) |
| Makefile `--net-forward "47016"` (quoted) | **passes** |
| Makefile `--net-forward $$port` | **passes** |
| `tools/test-apps-git.sh` `$HTTPD --port "8080"` | **passes** |
| `tools/test-sshd.sh` `free_port` returns a fixed 2222 | **passes** |
| run-qemu.sh writes FILE directly instead of by rename | passes (not observable by this check; acceptable) |

The report says that the check stops a fixed host port. It stops two literal spellings in the `Makefile` and in
`tools/*.sh`. Fix: in 7a, accept only `--net-forward auto:` (`grep -- '--net-forward' | grep -v -- '--net-forward
auto:'`, with the comments excluded), not a list of bad spellings. In 7b, accept only `--port 0` after a server
(`--port ["']*[1-9]` at least). Cover `free_port` in `test-sshd.sh`, or say in the report that it is not covered.
`ports/Makefile` and `tests/` are not grepped at all; today they hold no host listener (see "checked and found sound"),
but nothing keeps it so.

### 6. note: `tools/test-sshd.sh` has the same pick-and-close race, and the report lists it as "OS, unchanged"

Where: report table, row `tools/test-sshd.sh`; `tools/test-sshd.sh:36, 79-81`.

`free_port` binds port 0 in python, prints it and closes it; sshd binds it later. The window is longer than run-qemu's,
because sshd's start and the key files come in between. The report's table puts it on the same footing as `--port 0`
(where the server keeps the socket it got). Say that it shares finding 1's limit.

### 7. note: section 1 no longer checks the numeric `--net-forward PORT` form

Where: `tools/check-net-open.sh:38`.

The numeric form stays in `run-qemu.sh` (`:385-386`), but no caller uses it now, and the only `need` with a forward now
uses `auto:`. Either remove the numeric form, or keep a `need` for it.

### 8. note: the report's numbers can be checked only in part

The second tree is gone, so the two failing logs it cites (the before run with "Could not set up host forwarding rule",
and the after run with `cpu_ceiling_from_a_process … code: 101`) cannot be checked. What is left agrees with the report:
`before-net.log` is from `44026e9b-dirty` (the old files put back) and has RC=0, the run that won the port.
`after-net.log` and `after-net2.log` are from `44026e9b` and both have RC=0, with the pcap check of the ARP for
10.0.2.15 passing, so `net-poke.sh` reached the forward through the file. There is no separate log of the "one
`make test-net` alone" run: the three logs are the three runs of this tree. The section "Verdicts run" ends with
"`make test-changed`: see below", and nothing follows it. Fill it in, or say that the run is still going.

## Checked and found sound

- Makefile: `NET_FWD_FILE = $(BUILD)/net-forward-port.$$$$` becomes the recipe shell's PID. The single-quoted trap
  expands `$fwd` when it runs. The trap is set after the probe's `exit 0` and `exit 1`. `rm -f` in an EXIT trap keeps the
  recipe's exit status: run with `/bin/sh`, a chain ending in `return 3` exited 3 and the files were removed. The
  `( … exit 1 )` subshells and the background `net-poke.sh` do not inherit the trap. The later `$(MAKE)` calls are child
  processes. `run()` removes the file before it starts the poke, so the TCG run cannot read the stale port of the auto
  run. `build/` holds no leftover `net-forward-port.*`. Under dash, a death by signal skips the EXIT trap and leaves a
  small file in `build/` (harmless).
- `run-qemu.sh`: the port is picked after every check that can die, just before the `exec`. The file is written through a
  rename, so `net-poke.sh` never reads part of it. The `||` after `printf … && mv …` dies on either failure. Only the
  test step uses the forward. `--net-subnet` with a forward is still refused.
- `net-poke.sh`: the wait for the file is inside the alarm, a numeric argument keeps the old behaviour, and the poke is
  killed when the run ends, so a QEMU that never starts does not leave it waiting for 240 s.
- Portability: `mktemp -d TEMPLATE`, `grep -A1`, perl's core `IO::Socket::INET` and `$((…))` work on macOS and on the
  Linux runner. The baseline copy passes (`check-net-open: ok`, rc 0).
- Other fixed host ports: every `$(HTTPD)` and `$(TEST_DNS)` in the `Makefile` and in `tools/test-apps-git.sh` uses
  `--port 0`. The Rust listeners in `tools/test-httpd`, `tools/test-dns` and `crates/zoraya-tls/tests` bind port 0 or
  the port they are given. The `127.0.0.1:8000/8001` of `tests/shell/python-serve.script` and the shell's crate tests
  are in the guest or are strings. `make debug`'s `:1234` is not a test step. `ports/Makefile` has no listener. The
  report's claim that `NET_FWD_PORT` was the only fixed host port of a test step holds.
