# TASK-31: review of the part syscall-long (branch `work/syscall-long`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...work/syscall-long` at `5f49dfe`: `crates/zoraya-posix/src/ffi.rs` (`not_yet!`),
`tools/check-libposix-widths.py`, `tools/libposix-widths.txt`, `tests/posix-core/t_syscall.c`,
`tests/posix-core/libposix.script`, the `Makefile`, `.github/workflows/test.yml`, `tools/test-map.txt`,
`tools/test-par.sh`, `docs/adr/invariants.md` and the part's report (`docs/tasks/TASK-31-part-syscall-long.md`), read
against ADR-0451 item 1 and INV-0451-1. The report's claims were re-run, not taken.

## What was run

In the worktree of `work/syscall-long` (a scratch copy of the script and `crates/zoraya-posix/src` for the breaks),
`CARGO_BUILD_JOBS=3`:

| Command | Result |
|---|---|
| `make check` | 25 of 25 pass; `check-libposix-widths` ran with the built sysroot: "ok (217 definitions, table checked against the headers)" |
| `cargo test -p zoraya-posix` | 186 pass, 0 fail |
| `make test-libposix` (HVF) | `PASS: 97 steps`; `t_syscall: syscall passed (18 checks)`, `stubs passed (3 checks)`, `wide passed (6 checks)`; fsck clean |
| `nm` of the built `libzoraya_posix.a`, the global text symbols of the `zoraya_posix` member (the `_R` names left out) against the table's names | the same 217 names; `zoraya-rt`'s member exports none (its `__user_main` is behind the `entry` feature) |
| disassembly of the stub in `libc.a`'s `zoraya_posix` member | `__zoraya_real_syscall`, `__zoraya_real_tee` and `__zoraya_sendmsg` are one folded body: errno 38, `mov x0, #-1` |
| the old stub's shape (`fn int(Err(38)) -> i32`, `#[unsafe(no_mangle)] extern "C" fn syscall() -> i32`) compiled by the pinned `rustc` for `aarch64-unknown-none -O` | `mov w0, #-1; ret`: x0 is `0x00000000ffffffff` |
| a parameter-width scan (clang's AST of the sysroot's headers against the Rust parameter types, a scratch script) | finding 5 |

Breaks, in the scratch copy:

| Break | Result |
|---|---|
| A. `not_yet!(long syscall tee sendmsg)` back to `int` | **fails**, naming `syscall`, `tee`, `sendmsg` (`i32` (4) against `long`/`ssize t` (8)) |
| B. `#[unsafe(no_mangle)] extern "C" fn brand_new() -> i32` appended, no row | **fails**: "brand_new: no row" |
| C. `mod x { not_yet!(int foo_new); }` appended (with B) | **not reported**: only `brand_new` is named (finding 2) |
| D. the parser alone, on a `macro_rules!` that emits `#[unsafe(no_mangle)] extern "C" fn $n() -> i32`, on `not_yet! {int x}`, on `#[cfg_attr(all(), unsafe(no_mangle))]` and on `export_name = concat!(..)` | each gives no definition (finding 2) |

## Findings

### 1. should-fix: CI never compares the table with the headers; the table is what binds a definition to its prototype

- **Where:** `Makefile` (`check-libposix-widths`, the `if [ -d $(BUILD)/sysroot/usr/include ] ...` branch);
  `.github/workflows/test.yml` (`checks` job: "Steps that need nothing built"); `test-libposix` does not call the check.
- **What:** Offline the check compares each Rust return type with `tools/libposix-widths.txt`, a file anyone edits by
  hand. The comparison of the table with the sysroot's prototypes runs only where `build/sysroot/usr/include` exists. In
  CI the check is a job of the `checks` matrix, which has no sysroot, so in CI (candidate, nightly, `candidate-full/`)
  the header half never runs. The invariant's clause "the width its C prototype declares" is then checked only on the
  author's machine, and only if that machine's sysroot is built and current.
- **Scenario:** the `work/rust-std` part adds `getauxval` (ADR-0451 item 3) as
  `extern "C" fn getauxval(t: u64) -> u32` and writes the row `getauxval unsigned_int 4` by hand (or regenerates from
  a sysroot built before its headers changed, or has none built). `make check` in CI passes; the prototype in
  `<sys/auxv.h>` is `unsigned long getauxval(unsigned long)`, and a C caller reads garbage in the upper half: the
  defect this part exists to stop.
- **Fix:** run `python3 -I tools/check-libposix-widths.py --headers $(SYSROOT)/usr/include` where the sysroot exists
  in CI, without the fallback: in the `test-libposix` recipe after the sysroot is built (or in the build job), and fail
  there if the directory or `clang` is missing. Keep the offline form in the `checks` matrix.

### 2. should-fix: a definition the parser does not recognise escapes the check silently

- **Where:** `tools/check-libposix-widths.py:96-113` (`definitions`).
- **What:** An exported function is found only as `#[unsafe(no_mangle)]` or `#[unsafe(export_name = "x")]` followed
  by `extern "C" fn NAME`, or as `not_yet!(int|ptr|long ...);` at the start of a line. Anything else gives no definition
  and no error: a `macro_rules!` that emits exported functions (its `fn $name` is not `\w+`, so `continue`), an indented
  or brace-form `not_yet!` (break C, break D), a fourth `not_yet!` kind, `cfg_attr(.., unsafe(no_mangle))`,
  `export_name = concat!(..)`. The report's claim that "a new `extern "C"` without a table entry fails" holds for the
  direct form only. Today the parser's 217 names equal the archive's 217 exports, so nothing escapes yet.
- **Scenario:** the `work/rust-std` part writes the answered numbers' helpers or a second stub family with a macro
  (as `not_yet!` is), for example `stub!(i32 gettid)` emitting `extern "C" fn $name() -> $ty`, or puts
  `not_yet!(int pipe2);` inside a `#[cfg(...)] mod`. The definition is in `libc.a`, absent from the table, and
  `make check` passes.
- **Fix:** both of: (a) offline, fail on any `no_mangle`/`export_name` attribute or `not_yet!` occurrence that the
  parser did not consume into a definition, and on a `not_yet!` kind with no arm; (b) with `--headers` (the sysroot is
  built, so `target/posix/.../libzoraya_posix.a` is too), compare the parsed names with the defined global text symbols
  of the archive's `zoraya_posix` member (`zoraya-ar`/`llvm-nm`), and fail on a difference in either direction. (b)
  closes the class; (a) keeps the offline form honest.

### 3. should-fix: the Status row claims a run-time check of `sendmsg` that does not exist, and the deviation from the invariant's check is not recorded in the ADR

- **Where:** `docs/adr/invariants.md` (row INV-0451-1, "by `make test-libposix`'s `t_syscall` for `syscall`, `tee`,
  `sendmsg`"); `tests/posix-core/t_syscall.c:31-33`; ADR-0451's open questions.
- **What:** `t_syscall` calls `syscall` and `tee`. For `sendmsg` it has only `_Static_assert(sizeof(sendmsg(0, 0, 0))
  == 8)`, which checks the header's declaration, not libposix's definition (a narrow `__zoraya_sendmsg` passes it, as
  its own comment says). `sendmsg` cannot be called by a C program at all: `libc.a` hides it as `__zoraya_sendmsg`
  (`build/ports/obj/libc/hidden.txt`), so the invariant's check "calls ... `tee` and `sendmsg` through their prototypes
  and asserts each result is `-1L`" cannot be built as written until ADR-0412's part exports `sendmsg`. Covering it by
  the checker alone is acceptable; the disassembly shows the definition is right today. But ADR-0001 rule 6 (CLAUDE.md:
  "If a check cannot be implemented yet, record why in that ADR's open questions") asks for the note, and the Status
  row should not say `t_syscall` covers `sendmsg`.
- **Scenario:** a reader of the invariants table takes `sendmsg`'s width as checked at run time; a later change to the
  hiding step or to `__zoraya_sendmsg` is assumed covered by `test-libposix`, which never calls it.
- **Fix:** Status row: "by `t_syscall` for `syscall` and `tee` and an off_t past 4 GiB; `sendmsg` (hidden from
  `libc.a`, ADR-0141) by the checker only". Add the rule-6 note to ADR-0451's open questions: the `sendmsg` call of the
  check waits for ADR-0412's part, which exports it.

### 4. note: 48 rows are checked against no C declaration, though the declarations exist

- **Where:** `tools/check-libposix-widths.py:48-60` (`INTERNAL`), the 12 `-` rows of `tools/libposix-widths.txt`.
- **What:** `INTERNAL` (36 names: 21 written out, musl internals and `zoraya_*` entry points, and 15
  `zoraya_posix_*` routes) and the table's rows for them are compared only with each other; the 12 `zoraya_text_*` and `zoraya_window_*` rows are class `-`, skipped. Their
  prototypes are in musl's `src/internal` and in `ports/musl/zoraya/syscall.h`, `attr.c`, `cancel.c`, `ports/netsurf/zoraya_text.h` and
  `zoraya_window.h`. Read by hand against the Rust definitions: all agree today (`zoraya_window_scale` `uint32_t`/`u32`,
  `zoraya_text_position` and `_split` `size_t`, the routes `long`/`i64`, the two `_Noreturn void`).
- **Fix:** in `--headers` mode, also parse those files (one translation unit with musl's `src/internal` on the path, or
  the five files alone) instead of the hand-written `INTERNAL` dictionary, and drop class `-` for names a repository
  header declares.

### 5. note: parameter widths are not checked, and one definition reads an `int` parameter as 64 bits

- **Where:** `crates/zoraya-posix/src/ffi.rs:2269` (`ioctl(fd: i32, request: i64, argument: i64)`),
  `crates/zoraya-posix/src/ffi/procs.rs:1186` (`match request` on the whole `i64`).
- **What:** `<sys/ioctl.h>` declares `int ioctl(int, int, ...)`: `request` is a named `int`, so under AAPCS64 the upper
  half of x1 is unspecified, and `ioctl_tty` compares all 64 bits with `TIOCGWINSZ`, `TCGETS` and the rest. It works
  with today's callers because a constant is materialised with a `w` write, which clears the upper half; a request held
  in an `int` and passed after a sign-extending load (`ldrsw x1`), or any request above `INT_MAX`, gets `ENOTTY`. This
  is pre-existing and outside INV-0451-1's result-only wording, but it is the same defect class, and ADR-0451 item 2
  (`FIONBIO`, the next part) adds a request to exactly this match. The scan found nothing else: `fcntl`, `open`, `openat`
  read their variadic argument as musl's own C does, and `inet_ntoa`'s `u32` is `struct in_addr` by value.
  (The report's "Known limits" names the parameter gap; this is the instance.)
- **Fix:** in the `work/rust-std` part, `request: i32` (matched as `request as u32`, as Linux takes `unsigned int`), and
  extend the checker to named parameters (the clang AST already has the `ParmVarDecl`s).

### 6. note: robustness of the header parse

- **Where:** `tools/check-libposix-widths.py:176-185, 138-161, 230`.
- **What:** clang's exit status is not read: a header that fails to parse still yields a partial AST, and only a
  missing name is caught (it fails closed as "the headers declare none"), not a declaration clang recovered with an
  `int` type. A return type `c_class` does not know becomes class `?`, which `compare` skips silently offline.
- **Fix:** fail on a non-zero clang status, and treat a `?` row as an error.

### 7. note: what the tests prove

- `t_syscall syscall` and `stubs` would fail against the old definitions: the old stub returned `mov w0, #-1`
  (x0 `0xffffffff`, measured above), so `ENOSYS64`'s `r_ == -1LL` fails on the first call. Sound.
- `t_syscall wide` guards `lseek` (`ffi.rs:1800`, already `i64` before this part): a regression guard against a
  narrowed `lseek`/`__lseek` (5 GiB would come back as 1 GiB), not evidence for the fix. Fine as such.
- `syscall((1L << 32) | 98)` is a test for the next part (a dispatcher that truncates the number to 32 bits would
  answer futex); with today's stub it is trivially `ENOSYS`. Good to have.

### 8. note: `docs/design.md` and item 1's remaining clauses

- `docs/design.md`'s list of what `make test` runs does not name `make check-libposix-widths` (CLAUDE.md: keep
  `docs/design.md` true).
- ADR-0451 item 1 also asks for a `REPORT` line once per unanswered number (except the five `std` probes); the part
  report and the Status row defer the fixed list and the futex forms to `work/rust-std` but do not name this clause.
  Name it in the deferral.
- ADR-0451 items 4 and 5 put `sendmsg` and others in `libzoraya-rust.a`, written in C (`ports/rust/zoraya-rust.c`):
  the checker reads only Rust definitions, so those stubs' widths will need their own check in that part.

## What was checked and found sound

- Every libposix definition is now the width of its prototype's result: the table's 217 names are exactly the
  archive's exports; `--headers` against the built sysroot passes; the three changed stubs are `i64` and the folded
  body sets x0 to `-1`. The call-log wrapper `zw-syscall.o` tail-calls `__zoraya_real_syscall` (`b`), so the width
  passes through.
- `syscall()` answers no number today: musl's `src/misc/syscall.c` is excluded (`ports/musl/excluded.txt:25`), the C
  macro `syscall` of `ports/musl/zoraya/syscall.h` routes musl's own calls at compile time, and the exported `syscall`
  is the stub. "Every number: `-1` and `ENOSYS`" is today's behaviour, and the fixed list of item 1 is rightly the later
  part.
- `not_yet!` kinds: `setgroups` (`int`) and `setsid` (`pid_t`) stay `int`, `__map_file` `ptr`, `syscall`/`tee`/`sendmsg`
  `long`; `not_yet_long` sets errno as `int()` does.
- No caller expects the old `int`: no C, Rust, kernel or port source calls `syscall`, `tee` or `sendmsg`;
  `ports/apps/pyshim.c` defines its own `ssize_t sendmsg`; dash is configured `--disable-tee`.
- Wiring: `CHECK_STEPS`, `TEST_STEPS`, the CI `checks` matrix (with `check-ci-jobs` and `check-workflow-syntax`
  passing), `tools/test-par.sh`'s checks, `tools/test-map.txt` (the script, the table, and `check` for
  `crates/zoraya-posix/`); `python3` and `clang` are in `.github/ci/Dockerfile`. The candidate run on the branch
  (`candidate/work-syscall-long-a`) was queued, not finished, at the time of this review.
- The Status row "checked (partly)" is honest about the fixed list, the futex forms and the host tests; it overstates
  `sendmsg` (finding 3) and, while finding 1 stands, what CI checks of the width clause.

## Recommendation

Mergeable after findings 1 to 3, which are small (a Makefile line and a recipe call, a parser guard and an `nm`
comparison, a Status row and a rule-6 note). The width fix itself is correct and tested; findings 4 to 8 can go to the
`work/rust-std` part.
