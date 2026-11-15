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

## Tests

`tests/posix-core/t_syscall.c` (built by `make test-libposix`, run by `libposix.script`): `syscall` with unanswered
numbers (9999, 4096 with six arguments, -1, 0x7fffffff, `(1<<32)|98`, `(1<<40)+1`) gives the 64-bit `-1` and `ENOSYS`;
`tee` and `sendmsg` the same; `lseek` to 5 GiB returns it whole; compile-time `sizeof` of the three results is 8.
The invariant's Status row is `checked: make check-libposix-widths` with `partly`: the fixed list, the futex forms and
the host tests per answered number are not built here.

## Run

See the commit message and the report of the session for the results of `make check`, `cargo test -p zoraya-posix`
and `make test-libposix`.

## Known limits

- The width check is by return type only, not by parameters, and not for a definition written in C.
- `ffi` compiles only for the target, so `cargo test -p zoraya-posix` cannot call the stubs: the checker and
  `t_syscall` are the checks.
