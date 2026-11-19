# TASK-31 review: the BIND right (`work/bind-right`, ADR-0466)

Independent review by the `opus-review` subagent of `git diff origin/main...HEAD` (head `c2449a13` plus the follow-up commit and the
documents). It ran `cargo test -p zoraya-fs-service --test socket` and `-p zoraya-proto-file` (all passed) and read the code; it ran no QEMU.
The dispositions are the implementer's.

| # | Kind | Finding | Disposition |
|---|---|---|---|
| 1 | error | `rename` (also `exchange`) and `publish` moved a socket node, or a directory holding one, out of the place that made it into a directory without `BIND`, so a program with an own directory and a `+w` grant could put a listener in the shared one. | Fixed in `crates/zoraya-fs-service/src/lib.rs`: a socket node or a directory moving to another directory needs `BIND` on the target connection (and on the source for an exchange); `publish` of an unnamed node needs it on the target. Test `a_socket_node_does_not_leave_the_places_with_bind`. ADR-0466 item 2 says so. |
| 2 | error | `namespace::record` asked for `BIND` on every write hold, so the `files` application's record had it. | Fixed: `record(.., bind)`; `init` and the panel's `zosh` record pass `true`, every other program `false` (`p.image == "zosh"`). The ADR's item 4 and the `bind_sites` reason say it. No host test of the panel table: the list of callers is the check (see 4). |
| 3 | warning | INV-0466-1/-2's check command ran only one test binary. | Fixed in `invariants.md`: two commands. The QEMU half is `t_storage`'s `socket_nodes_need_bind`, run by `make test-changed` (below). |
| 4 | warning | The `bind_sites` scan matched only `rights::BIND`/`WITH_BIND`; `MASTER` or a bare `BIND` got past it. | Fixed: whole-word match of `BIND`, `WITH_BIND` and `MASTER`. It lists files, not callers; the callers of `record` are a known limit, stated here. |
| 5 | warning | Missing cases: `share`, `Open` asking for `BIND`, `UNNAMED` create, the status of a plain create. | Added `bind_is_not_gained_by_share_narrow_names_or_open` (`share`, `open`), the unnamed publish in the rename test, and the status check of the plain create. `narrow_names` has no new test: its per-name rights are a subset of the connection's by the code the reviewer read, and `bind_is_never_gained_by_cloning_sharing_or_narrowing` covers `narrow`. |
| 6 | note | A program without `BIND` saw `LIMIT_REACHED` on a full table. | Fixed: the bound is checked only for a connection that holds `BIND`. |
| 7 | note | `posixrun -t` gives `WITH_BIND` on any path. | Recorded in ADR-0466 item 4 (a test launcher that holds the root). |
| 8 | note | A cwd clone of `/tmp` has no `BIND`. | A comment in `jobs.rs`: `bind` resolves through the table's entry, never the cwd. |

The reviewer also checked as sound: roots get `WITH_BIND` only at `add_root`; `clone`, `share`, `narrow`, `open` need `has(requested)`; per-name
rights of `narrow_names`; the create is detected only in ordinal 27; no other service decodes `Rights`; grants are `ALL & !PUBLISH`; an
own-directory child has `READ | CLONE`; the broker to shell `/tmp` hand-off; the fallbacks in `line_tmp` and `own_connection`.
