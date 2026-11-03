# TASK-17 review: stage 6b, cross toolchain, sysroot and ports

Two independent reviews by subagents that did not write the code (on the main model, as `CLAUDE.md` asks), both
read-only: the first of `9b58034..e48b761`, the second, after its fixes, of `9b58034..47aa000`. The reviewers ran only
already-built tools on copies, and crafted inputs (finding B4). Then mutation testing of the three new tools,
sorted by the `mutant-triage` subagent. Fixes are in `10e6a04` (first review), `50bccb9` and `bd8c6ea` (second
review), and `1faad3a`, `f4110f8` and `b6d6313` (mutation holes); the check of the lines changed after the second review is the last section. The host ran out of memory once
during the task (a parallel mutation run of 6a); since then the new tools run under a capped allocator (`47aa000`)
and builds are limited to four jobs.

## Findings and dispositions: first review

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| A1 | error | toybox was never built on the M4 (no GNU sed), so `make ports` and `make check-ports` never completed there; design.md and dependencies.md said otherwise. | Docs corrected (design.md, dependencies.md, invariant statuses). GNU sed is still missing on the M4 at the end of the task; see "Open" below. |
| A2 | error | INV-0100-1 and INV-0100-2 were `checked` although the archive meets them only under the proposed ADR-0141's reading (`zoraya_posix_*` internal; the shim's list differs from item 2's). | Statuses now name that dependence. |
| A3 | error | `config.site` gave a false reason for `ac_cv_func_gettimeofday`. | Fixed: it states the run test's real condition. |
| A4 | error | The tier list named a make target that does not exist. | Fixed: it names `make test-unit` and `make check-ports`. |
| A5 | warning | musl objects stayed stale when the shim or musl changed. | Fixed: a new musl tree removes the objects. |
| A6 | warning | A new `libc.a` did not relink the ports, and the `touch` hid it; a changed tool did not rebuild `libc.a`. | Fixed: each binary is removed before its build; `libc.a` depends on the tools' binaries. |
| A7 | warning | The `__linux__` log check saw no compile commands for most ports and passed on no logs; `zcc`'s refusal had gaps (`-Wp,-D`, `-Xclang`, a caller's `--target`, `-D__gnu_linux__`, `-static-pie`). | Fixed: builds log their commands (`V=1`), a log without commands fails the check, `zcc` refuses those forms. Remaining gaps are in ADR-0143's open questions (B10). |
| A8 | warning | A name defined by two members was not reported. | Fixed: two strong definitions fail the check (a fifth broken archive); weak twins (`compiler_builtins`' `memcpy`) are allowed. |
| A9 | warning | Mutations the tests would not catch: entries of the exclusion table, `zoraya_posix_` widened to `zoraya_`, the command line, the fetch path through `ports/Makefile`, `--verify` of an unknown name, the index count. | Fixed: tests for each; `--verify` of an unknown name fails; `test-fetch.sh` corrupts the cache and runs `ports/Makefile`. |
| A10 | warning | INV-0109-3's status claimed more than its check (the network is not disabled). | Status says so. |
| A11 | warning | `zoraya-elf-check`'s `USER_LOW` is a copy of the kernel's; dangling links in `/bin` went unreported. | Dangling links now fail the check. `USER_LOW` stays a copy: moving it into `zoraya-abi` would touch a crate of TASK-16 (backlog). |
| A12 | warning | tcc's `tccdefs.h` has two more `__linux__` branches (huge-value and NaN builtins, the bounds checker's `malloc`) that ADR-0143 did not record. | Recorded in ADR-0143; musl's `INFINITY`, `HUGE_VAL`, `HUGE_VALL` and `NAN` compiled by the host tcc have the right values (checked once). |
| A13 | warning | "GitHub states that these archives are stable" had no source. | Softened in `ports/README.md`. |
| A14 | note | The shim named `ports/musl/files.mk`. | Fixed. |
| A15 | note | 750 hidden names are 673 from musl and 77 from the Rust members; `rust_begin_unwind` is mangled. | ADR-0141 and design.md corrected. |
| A16 | note | `zcc` did not resolve symbolic links to itself. | Fixed. |
| A17 | note | `stubs.awk`'s `allow` had no reason. | Fixed. |
| A18 | note | musl is not MIT throughout. | `dependencies.md` says so. |
| A19 | note | Two statuses were not commands. | Fixed. |
| A20 | note | `fcntl`'s empty variadic in the shim; an interrupted download's `.part` stays in the cache. | Left: clang accepts the first; the second is removed by the next fetch and never verified as an archive. |

## Findings and dispositions: second review

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| B1 | error | Statuses said `checked: make check-ports`, which had not passed since `10e6a04` and cannot pass on the M4 without toybox. | Each status now says that on the M4 the steps were run one by one. They were, after the fixes: the list, the archive, the excluded files, five broken archives, `zoraya-elf-check` over dash, make, busybox, tcc and tcc's `a.out`, both compilers' predefines, the logs, tcc without `-run`. |
| B2 | error | A second `make ports` would fail its own log check (musl's log truncated to "nothing to be done"). | Fixed: musl's log is appended to, and restarted with a new musl tree. |
| B3 | warning | `-ffile-prefix-map=$root/=` left `DW_AT_comp_dir`, so the home path was in every object (1,452 times in `libc.a`). | Fixed (`$root=.`, and the stub's `--remap-path-prefix`): no output contains the path now. |
| B4 | warning | The ELF readers panicked on a section header offset near the top of the address space, contrary to "total". | Fixed and tested in both readers and `rename`. |
| B5 | warning | `objects.txt` did not follow a change of the selection rule. | Fixed. |
| B6 | warning | design.md did not say that `make ports` stops on the M4. | Fixed. |
| B7 | warning | INV-0140-1 ignored the stub's functions without a result, and its status overclaimed. | Invariant reworded; status says the kinds are a review item. |
| B8 | note | `--os-release=` kept the host's `uname -r` in `config.h`. | Fixed (`--os-release=0`); ADR-0143 corrected. |
| B9 | note | `check-excluded` accepted any compile failure. | Fixed: the file must exist and fail on the shim. This found that `ioctl.c` fails on constants of musl's own `syscall.h`, not on an unrouted call; ADR-0141, ADR-0140 and design.md now say so. |
| B10 | note | `zcc` did not refuse `--define-macro` or response files; `-dumpmachine` printed clang's triple. | Fixed. `-include` and a port's own `#define` remain, recorded in ADR-0143. |
| B11 | note | The index check compared names only. | Fixed: each entry must point at the header of a member that defines it, once per definition. |
| B12 | note | "four broken copies" (there are five); ADR-0141 item 7 lacked the duplicate check. | Fixed. |
| B13 | note | design.md's LICENSE line was out of date. | Fixed. |
| B14 | note | tcc's files say LGPL version 2 or later; `COPYING` is 2.1. | Stated as such. |
| B15 | note | `fetch-ports.sh` with an unreadable lock exited 0. | Fixed. |
| B16 | note | No review file; no recorded reproducibility outcome; ADR-0143's finding 8 not reproducible from the tree. | This file; reproducibility below; the host tcc's `a.out` was rebuilt and checked after the fixes. |

## Reproducibility

After the fixes, two clean builds (`make clean-ports`, then `make -j4 -f ports/Makefile sysroot dash make busybox
tcc`) gave byte-identical files for all 17 outputs compared: `libc.a`, the crt objects, the eight empty libraries,
`libtcc1.a`, dash, make, busybox and tcc. toybox is not in the comparison (not built on the M4). Builds in different
checkout paths were not compared; no output contains the checkout's path.

## Mutation testing

cargo-mutants 27.1.0 on the three new host tools, two jobs at a time with a 120 s timeout, every test process under
the tools' capped allocator (1 GiB). The `mutant-triage` subagent sorted the first run's survivors of
`posix-surface-check`; those of the other two were sorted by reading them.

| Crate | First run | Holes, now tested | Last run | Left, all equivalent |
|---|---|---|---|---|
| `posix-surface-check` | 229 mutants: 45 missed, 182 caught, 2 unviable | 41: the index of a short archive, a symbol's type, `rename`'s magic and string-table bounds and alignment, CSV quotes, `parse_list`, `Report::lines`, and the command line (writing the list, a stale list, `--allow-svc`, misuse, the summary's count) | 264 (after the reviews' additions): 3 missed, 259 caught, 2 unviable; then `main.rs` alone after the new test: 27 mutants, 27 caught | `rename.rs:44` (the guard `end <= elf.len()` made `true`): the next read refuses the same header |
| `zoraya-ar` | 174: 38 missed, 134 caught, 2 unviable | 37: the long-name boundary, non-relocatable members, a bad string-table link, error messages, and every operation of the command line with its errors and `ranlib` | 179: 1 missed, 176 caught, 2 unviable | `main.rs:149` (`\|\|` to `&&` in the refusal of `.` and `..`): writing either name fails anyway, as it is a directory |
| `zoraya-elf-check` | 22: 9 missed, 12 caught, 1 unviable | 9: the range's upper end, the refusal messages, and the command line (a refused, unreadable or missing file, no file, a dangling link) | 28: 0 missed, 27 caught, 1 unviable | none |

The last full run of `posix-surface-check` left three; the two in `main.rs`'s count of exports (line 87) were holes,
killed by `the_summary_counts_defined_non_internal_names_only`, as the run of `main.rs` alone shows.

## After the architect's decisions (2026-11-03)

The architect accepted ADR-0140, ADR-0141 and ADR-0142 (with dash's job-control patches written in 6b) and installed
GNU sed on the M4. Then:

- **toybox** builds without `__linux__` with one patch, `ports/patches/toybox/0001-zoraya-portability.patch`: toybox
  stops an unknown OS with `#error` at seven places of `lib/portability.c` and leaves `struct statfs` incomplete; each
  gets a Zoraya branch. Its link needs `atoll` and `getdelim` (T2 rows, pure musl), which proposed ADR-0144 adds.
- **`make ports` completes on the M4**, ending in the full `make check-ports` (`check-ports: all passed`), twice in a
  row over the same tree and once from clean. `libc.a` exports 245 names, the CSV's 240 plus ADR-0142's three and
  ADR-0144's two; with the three `_` rows the list has 248.
- **Reproducibility with toybox:** two clean `make -j4 ports` runs (about 31 s each) gave the same bytes for all 18
  outputs compared (`libc.a`, the crt objects, the eight empty libraries, `libtcc1.a`, dash, make, busybox, tcc and
  toybox).
- **ADR-0143**, for the architect: no program of W1 to W3 nor any port tests `__unix__`, so both compilers keep
  `__zoraya__` alone; `zcc` and tcc now predefine the same 26-name identity, compared by `ports/compare-predefs.sh`.

## Open

- ADR-0143 and ADR-0144 await the architect.
- Linux CI has not run.
- `USER_LOW` is copied in `zoraya-elf-check` (A11).

## Check of the changed lines

One check, by a third subagent that wrote none of it, of `git diff 47aa000..b6d6313` (the fixes after the second
review): no errors; nine warnings and notes, all fixed in the commit after `b6d6313`.

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| C1 | warning | The index-offset check said nothing when a second member defining a name had no entry, if the name was indexed at another member (98 names of `libc.a` have such weak twins; a crafted copy without one entry passed). | Fixed and tested; the crafted copy now fails. |
| C2 | warning | `check-excluded` matched clang's notes ("expanded from macro `__zoraya_sys_...`"), so a file failing inside a routed call would pass. | It now matches the errors themselves (undeclared function or identifier). |
| C3 | warning | `-Wp,-D,__linux__` (separate fields) was not refused. | Refused. |
| C4 | warning | The table's last run of `posix-surface-check` was not one command's output. | Corrected: 3 missed in the full run, then the run of `main.rs` alone. |
| C5 | note | This file named a commit that did not exist. | Corrected. |
| C6 | warning | Statuses began `checked: make check-ports`, a target that has not passed on the M4. | They now begin "step by step on the M4". |
| C7 | note | Not every tcc file carries the LGPL notice (`arm64-gen.c` is permissive). | `ports/README.md` and `dependencies.md` corrected. |
| C8 | note | design.md pointed to a README that `ports/musl/zoraya/` lacks. | Corrected. |
| C9 | note | `check-broken-archives.sh`'s header listed four cases. | Corrected. |

The task allows one more check if this one found errors; it found none, so none was run.
