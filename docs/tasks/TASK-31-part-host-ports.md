# TASK-31, part work/host-ports: report

Branch `work/host-ports`, from `origin/main` 7361fedf. The architect's decision of 2026-11-19: parallel runs of the test steps
collided on host ports (`net-auto` could not set up the host forward on 127.0.0.1:47015).

## The host ports of the test steps

Found by grepping `Makefile`, `tools/`, `tests/` and `ports/Makefile` for numbers, `hostfwd`, `--net-forward`, `--port`, `bind`,
`listen`:

| Port | Where | Before | Now |
|---|---|---|---|
| host side of the net step's forward | `test-net` (`NET_FWD_PORT`) | fixed 47015 | the OS: `--net-forward auto:FILE` |
| `tools/test-httpd` (every net step, `test-apps*`, python, netsurf, hand) | `--port 0` | OS | unchanged |
| `tools/test-dns` (`net-dns`, `net-lookup`) | `--port 0` | OS | unchanged |
| `tools/test-sshd.sh` (`test-apps-ssh`) | python `bind(("127.0.0.1", 0))` | OS | unchanged |
| guestfwd relays (`NET_RELAY`, `NS_RELAY`, test-apps-*.sh) | the port of the OS-given server | OS | unchanged (the guest address `10.0.2.x:PORT` is in QEMU's own network, not a host port) |
| `make debug` gdb stub `:1234` | `--debug`, by hand only | fixed | unchanged (not a test step) |
| QMP and pcap | unix sockets and files under `build/`, `target/test-logs/` | per tree | unchanged (per worktree) |

So `NET_FWD_PORT` was the only fixed host port a test step listens on.

## What changed

- `tools/run-qemu.sh`: `--net-forward auto:FILE`. The host port is picked by binding 127.0.0.1 port 0, reading it back and
  closing it just before `exec` of QEMU, and written to FILE by rename. The guest side of the hostfwd is the fixed 47015
  (the guest does not listen there: the connection only makes SLIRP ask who has 10.0.2.15).
- `tools/net-poke.sh`: its first argument may be a file, which it waits for (within SECONDS) and reads the port from.
- `Makefile` `test-net`: `NET_FWD_FILE` (`build/net-forward-port.$$`, per process), removed on exit.
- `tools/check-net-open.sh` (`make check-net-open`, already in `CHECK_STEPS` and CI): checks 7a to 7d. No numeric
  `--net-forward` in the Makefile or `tools/*.sh`; no test server with a fixed `--port`; twelve `run-qemu.sh` at once, each with a
  stub QEMU that binds the hostfwd port it is given and holds it, all succeed with twelve distinct ports and the guest side 47015;
  `net-poke.sh` waits for the file and connects. Mutations shown to fail: a numeric `--net-forward` in the Makefile and a fixed
  `--port` on a test server.

## Known limit (the race)

The port is free when chosen and closed before QEMU binds it: a window of a few milliseconds. Holding the socket is not possible
across `exec` (QEMU binds its own). If another process takes the port in it, QEMU fails to start ("Could not set up host
forwarding rule") and the step fails; a retry loop would need `run-qemu.sh` not to `exec` QEMU, which changes how the runner
kills it. The OS hands out ephemeral ports at random, so two pickers get the same port only if one closed it and the OS reissued
it within the window.

## Numbers

Two `make test-net` at once, from two trees (one worktree and a `cp -Rc` copy of its `target/` and `build/`), HVF, with two
other QEMU runs on the machine:

- Before (7361fedf's Makefile and tools in both): one run RC=0, the other RC=2 with `qemu-system-aarch64: ... hostfwd=tcp:127.0.0.1:47015-10.0.2.15:47015: Could not set up host forwarding rule` and `[net-auto] FAIL: QEMU exited with status 1`.
- After, first pair: one RC=0, the other RC=2, `[net-auto] FAIL: cpu_ceiling_from_a_process failed: ... code: 101` (a timing test
  of the guest under host load, not a port: no forwarding error in either log).
- After, second pair: both RC=0.
- One `make test-net` alone, after: RC=0.

## Verdicts run

`make check` RC=0 (before the commit). `make test-changed`: see below.
