# TASK-31 part: syscall returns long (`work/syscall-long`)

Architect's order, ahead of the queued work: libposix's `syscall()` returned a 32-bit `int` where its C prototype is
`long syscall(long, ...)`; `tee` and `sendmsg` the same for `ssize_t`. On AArch64 the upper half of the result
register is then undefined, so `-1` read as 4294967295 (a success). ADR-0451 item 1 and INV-0451-1 (accepted
2026-11-15). This part does the width fix and its checks only.

## Definitions fixed

Found by the new checker, which compares every exported libposix definition (212 `no_mangle` or `export_name`
functions and the `not_yet!` stubs, 217 names) with the prototype in the sysroot's musl headers. Three were narrower:

| name | was | C prototype | now |
|---|---|---|---|
| `syscall` | `i32` | `long` | `i64` |
| `tee` | `i32` | `ssize_t` | `i64` |
| `sendmsg` | `i32` | `ssize_t` | `i64` |

No other definition differs in width. (`recvmsg`, `readv`, `writev`, `sendfile`, `splice`, `copy_file_range`,
`pread`, `pwrite`, `read`, `write`, `lseek` are not libposix Rust definitions: musl's C or the `zoraya_posix_*`
routes of `ports/musl/zoraya/syscall.h`, which return `i64` for `long` and are in the table.) `not_yet!` takes a kind
per name now (`int`, `ptr`, `long`); the stubs behave as before: `-1` with `errno` `ENOSYS`, now as 64 bits.

What `syscall()` answers is unchanged: it never answered a number (the C macro `syscall` of
`ports/musl/zoraya/syscall.h` routes compile-time calls inside musl; the exported function was the stub), so every
number gives `-1` and `ENOSYS`. The fixed list of ADR-0451 item 1 (futex, getrandom, gettid) is the part `work/rust-std`.

## The checker

`make check-libposix-widths` (`tools/check-libposix-widths.py`, table `tools/libposix-widths.txt`), in
`make check` and `TEST_STEPS`, the CI checks matrix, `tools/test-par.sh`'s check list and `tools/test-map.txt`.

- Offline (always): every definition has a row, and its Rust return type has the row's width class (void, 1, 2, 4, 8,
  f32, f64; sign is not checked).
- With the sysroot built (`build/sysroot/usr/include`) and `clang`: the table is also checked against the headers,
  parsed from clang's AST dump (typedefs resolved).
- `--regen DIR` rewrites the table from the headers; `--selftest` shows a narrowed definition failing.
- 12 names are in no public header: musl's hidden internals and `zoraya_posix_*` (types in the script's `INTERNAL`,
  read from musl 1.2.5 `src/internal` and `syscall.h`) and the `zoraya_text_*`, `zoraya_window_*` names of the ports'
  own sources (class `-`, not checked).

Shown by hand: with `syscall` made `int` and `zoraya_posix_read` made `i32` again, the checker exits 1 naming both;
reverted, it passes.

## Review fixes (TASK-31-review-syscall-long.md, findings 1 to 3)

1. `make test-libposix` now runs `make check-libposix-widths-built` after the sysroot is built: `--headers` with no fallback
   (fails if clang, the headers or the archive are missing), so a wrong hand-written row fails CI.
2. The parser fails on any `no_mangle` or `export_name` it did not consume (`cfg_attr`, `export_name = concat!`, an attribute
   without `extern "C" fn NAME(`), on `link_name`, on a `not_yet!` kind with no arm, and on another macro that emits
   `extern "C"` items; the indented, brace and bracket forms of `not_yet!` are read. `--archive` compares the parsed names with
   the `zoraya_posix` member's global text symbols (`nm`) in both directions (217 equal). Fixtures for each in `--selftest`.
   clang's exit status is read, and a `?` row is an error. Three headers that include missing Linux headers
   (`sys/kd.h`, `sys/vt.h`, `sys/soundcard.h`) are left out of the header scan.
3. The Status row no longer says `t_syscall` covers `sendmsg` (the checker alone does); ADR-0451's open questions record why
   (rule 6). `docs/design.md` names the check; findings 4 to 8 are in `docs/backlog.md`.

## Tests

`tests/posix-core/t_syscall.c` (built by `make test-libposix`, run by `libposix.script`): `syscall` with unanswered
numbers (9999, 4096 with six arguments, -1, 0x7fffffff, `(1<<32)|98`, `(1<<40)+1`) gives the 64-bit `-1` and `ENOSYS`;
`tee` the same (`sendmsg` is hidden from `libc.a`, outside the tier list: the checker covers it); `lseek` to 5 GiB returns it whole; compile-time `sizeof` of the three results is 8.
The invariant's Status row is `checked: make check-libposix-widths` with `partly`: the fixed list, the futex forms and
the host tests per answered number are not built here.

## Run

`make check` (all steps ok, with the table also checked against the built sysroot's headers), `cargo test -p zoraya-posix`
(all pass), `make test-libposix` under HVF (`PASS: 97 steps`, with `t_syscall` syscall, stubs and wide, fsck clean).
The `build/ports/src` tarballs were copied from a sibling worktree.

## Known limits

- The width check is by return type only, not by parameters, and not for a definition written in C.
- `ffi` compiles only for the target, so `cargo test -p zoraya-posix` cannot call the stubs: the checker and
  `t_syscall` are the checks.
