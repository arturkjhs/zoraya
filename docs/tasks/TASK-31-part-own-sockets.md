# TASK-31 part own-sockets: socket files in a program's own directory

Branch `work/own-sockets` from `origin/main` fe1feacd. ADR-0465 (proposed; the architect's decision of 2026-11-17 allowed the place, this
part builds it). Review: `docs/tasks/TASK-31-review-own-sockets.md`.

## What was built

- `bind` works in an `Own` entry as in `Tmp` (`unix::bind_place`, `unix_bind`); a child of a program with an own directory holds
  `READ | CLONE` there, so the service refuses its `bind`.
- `fsd` keeps socket nodes on every service (`user/src/bin/fsd.rs`), so the volume's `/data/ROOM/NAME` can hold a node (an empty file; type
  and listener in the service's memory).
- `+sock PATH`: a literal word of the line (`syntax.rs`, `eval.rs`, `Line.socks`), planned as the file's directory narrowed to the name
  with `Access::Connect` (rights of a change of one name; `grants` says `connect to NAME`, the window pill `+sock PATH`). Saved with
  `install` and `programs grant` (`zoraya_settings::apps::Word::Sock`, `sock_place`), applied by `installed::apply` if the shell holds the
  file, never in an untrusted room.
- Tests: `crates/zoraya-posix/tests/socketfile.rs` (own-directory bind, a narrowed grant meets the server, a sibling, no grant,
  `unlink`/`rename`/`bind` refused, read-only grant, a volume without nodes), shell and settings host tests, and
  `tests/posix/zosh-sockets.script` (QEMU: `sockd` and `sockc` installed, started on separate lines, meet over
  `/home/.config/sockd/s`; a grant to `s` is refused on `t` with EACCES, a program with no grant is refused with ENOENT, `unlink` and
  `rename` refused, a typed `+sock` works for its line only, `programs revoke` takes it away, a server rebinds after a stale file).
- Mutant list: `tools/mutants/sockets-f7.toml` (the `bind_place` mutant follows the new text, one added).

## Decisions where the ADRs were silent (ADR-0465)

The node is a plain file on the volume with its type in memory (stale after a stop); `+sock` as the one saved word that names a file (a
stated exception to ADR-0397 item 1); the client names the real path, the server sees its view path; a `+sock` in a room is not built.

## Known limits

Two path strings for one file; the volume's 256 nodes and 64 listeners are shared by every room; `readdir` shows a plain file; a stale
node after a crash is `EADDRINUSE` until unlinked; `room new` takes no `+sock`; a typed `+sock` whose target is not a socket fails at `connect`.

## What was run

See the final report of the part (head, `make check`, `make test-changed`, the review's verdict).
