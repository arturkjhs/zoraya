# TASK-02 method

How every number in `docs/research/task-02/` was produced, and the known limits of each method. Everything runs from
one entry point:

```sh
tools/posix-surface/run.sh            # all stages, from the Mac; about 25 minutes
tools/posix-surface/run.sh analyze    # selected stages only
```

`run.sh` copies `tools/posix-surface/` into the VM (`~/psurf/tools`) and runs each stage there over SSH. It then copies
`~/psurf/results/` back to `tools/posix-surface/results/`, runs the M3 report locally (it reads the committed reviews
in `tools/posix-surface/review/`), and copies `libc-surface.csv`, `syscall-surface.csv` and `callsites.csv` into
`docs/research/task-02/`. Raw traces, about 490 MB, stay in the VM under `~/psurf/raw` and `~/psurf/raw-m4`; every
table can be regenerated from them.

Stages, in order: `env fetch build static interposer dynamic minbuild strace callsites m4 analyze`, one script each in
`tools/posix-surface/remote/`.

## Environment

| Item | Value | Source |
|---|---|---|
| Host | Apple Silicon Mac, Darwin 25.4.0 | `uname -sr` on the host |
| VM | Lima 2.1.1 instance `alpine-posix`, Apple Virtualization.framework, 4 vCPU, 4 GiB of RAM, 20 GiB of disk, no host mounts | `limactl start --name=alpine-posix --cpus=4 --memory=4 --disk=20 --set '.mounts=[]' template:alpine` |
| Guest | Alpine 3.23.3, Linux 6.18.7-0-virt aarch64, musl 1.2.5 | `results/env.txt` |
| Toolchain for building the measured programs | gcc 15.2.0, binutils 2.45.1 | `results/env.txt` |
| Tracing | strace 6.17, util-linux `unshare` | `results/env.txt` |
| T3 | clang 21.1.2, lld 21.1.2 (Alpine packages) | `results/env.txt` |

**Deviation from the task text.** The task names a UTM/QEMU VM on `localhost:2222`. None was installed, and the human
asked for the VM to be created from the command line, so Lima was used. Lima's SSH port is dynamic; `run.sh` reads
`~/.lima/alpine-posix/ssh.config`. The hypervisor does not affect any measurement here, because all results are about
the guest's userspace and kernel ABI.

## Pinned inputs

`tools/posix-surface/versions.env` pins the versions; `tools/posix-surface/sources.lock` records the SHA-256 of every
downloaded archive, and `fetch.sh` refuses a mismatch.

| Program | Tier | Version |
|---|---|---|
| dash | T0 | 0.5.13.5 |
| oksh | T0 (comparison) | 7.9, built with `--enable-small` |
| mrsh | T0 (comparison) | git a858396b79ba217760b0982dd6f45c91c5192c3b |
| toybox | T0 | 0.8.15 (defconfig, plus per-tier minimal builds) |
| busybox | T0 fallback and comparison | 1.38.0 (defconfig without `tc`, plus per-tier minimal builds) |
| TinyCC | T1 | mob branch, e23dabed154a8495f3ac1ff3cc9d04fa1c207893 (0.9.28rc) |
| pdpmake | T1 (comparison) | 2.0.4, 0faaf258c9b3eaa9f5e4f6842e2484cbea6f23f8 |
| GNU make | T1 | 4.4.1 |
| Lua (W2 input) | — | 5.5.1 |
| SQLite amalgamation (W3 input) | — | 3540100 |
| GNU hello (W4 input) | — | 2.12.3 |

Every measured program is built by gcc from source as a dynamically linked PIE against musl, with
`-O2 -g -U_FORTIFY_SOURCE` (`build.sh`). Build-only fixes are listed in `research-log.md`; the only source change is
`patches/build/tcc-aarch64-wchar.patch`.

## Tiers, runs and sets

- **Sets.** A set is a directory of symlinks that forms a workload's whole `PATH` (`remote/workloads.sh: mkset`):
  - `sh` and `dash` point to dash, and `make` points to GNU make (`t1g`) or pdpmake (`t1p`);
  - `tcc` and `cc` point to tcc;
  - every toybox defconfig applet points to toybox.

  A fallback directory with the busybox applets toybox lacks sits last on `PATH`. Every use of it is reported. T3 runs
  with `/usr/bin:/bin`.
- **Clean environment.** Every workload command runs under `env -i PATH=<set> HOME=<run dir> TERM=dumb SHELL=<set>/sh`.
- **Namespace.** Each command also runs in a user and mount namespace (`ns_exec.sh`). There, `/bin/<name>` resolves to
  the measured binary when the set provides `<name>`, and to the original otherwise. The process keeps its real uid
  and gid through a nested user namespace. Supplementary groups are not mapped.
- **Run kinds** (`analysis/runs.py`):
  - *core* runs define the tiers.
  - *comparison* runs use alternative programs: pdpmake in W1, W2 and W4p; oksh, mrsh and busybox sh in W5.
  - *probe* runs are the deliberately failing configure/make attempts without `ld` and without `ar`.
- **Tier membership comes from the data.** A tier's programs are the measured programs observed in that tier's core
  runs, cumulative for T0→T1→T2. T0 is W5 with dash, T1 adds W1–W3, T2 adds W4, and T3 is W6 alone. The lists are
  printed at the top of `results/summary-tables.md`.

Workloads (`remote/workloads.sh`; outputs are checked in the `*-check` runs):

| ID | What runs | Check |
|---|---|---|
| W1 | `sh ./configure --cc=tcc --config-musl` then `make` (GNU make) in a copy of the tcc tree: tcc compiles itself and libtcc1 | stage-2 `./tcc -v` and compiles and runs a test program |
| W2 | Lua `make CC=tcc AR="tcc -ar rcu" RANLIB=true posix`, once with pdpmake and once with GNU make | `lua -e 'print(...)'` prints `lua ok 42` |
| W3 | `tcc -c sqlite3.c; tcc -c shell.c; tcc -o sqlite3 ... -lm` from a dash script | a SQL query prints 42 |
| W4 | GNU hello `sh ./configure CC=tcc LD=tcc AR="tcc -ar"` + `make`, with GNU make and with pdpmake. Probes without `LD` and without `AR` | `./hello` prints `Hello, world!` |
| W5 | an interactive session on a pty (`remote/w5_session.py`): pipe, redirections, glob, environment export, background job with `jobs`, Ctrl-C of a foreground `sleep 30`, `cd` and `cd -`, `wait`, `exit`; for dash, oksh, mrsh and busybox sh | every prompt seen; `$?` after Ctrl-C |
| W6 | `clang -O2 -c hello.c`, `clang -fuse-ld=lld -o hello hello.o` | `./hello` |

## M1: libc API surface

**Static upper bound** (`analysis/static.py`).
- For each binary: the undefined dynamic symbols (`nm -D --undefined-only`) that musl's `libc.so` defines. musl ships
  libc, libm, libpthread, libdl and librt as one library, so this is the whole libc boundary. Calls musl makes to
  itself never show up, by construction.
- musl's four `environ` aliases are counted as one symbol.
- crt startup references (`__libc_start_main`, `__cxa_finalize`, `_ITM_*`, ...) are tagged `crt` and excluded.
  So are the compiler-inserted stack-protector symbols `__stack_chk_fail` and `__stack_chk_guard`, because Alpine's
  gcc defaults to `-fstack-protector-strong`. The independent review found them counted as API; that had inflated
  every upper bound by 2.
- Cross-check: for dash, the object-file method (undefined across all `.o` files minus defined across them) gives
  exactly the same set as the import table. See `results/static-crosscheck.txt`.
- For T3, `clang+libs` and `ld.lld+libs` are the union over the binary and every non-musl shared library it loads:
  libclang-cpp, libLLVM, libstdc++, libgcc_s, and so on. The clang binary alone imports very little.
- **Two upper bounds per tier.**
  - *Full binaries*: whole toybox defconfig and whole busybox defconfig.
  - *Minimal multi-call builds*: `minbuild.sh` builds toybox and busybox per tier with only the applets that tier's core
    runs executed (`results/applets-by-tier.tsv`), from `allnoconfig`. For busybox, the shell choice is forced to
    "none".

  The full binaries' shared startup code calls a few symbols the minimal builds do not import: busybox's setuid
  handling and long options, toybox's root checks. So the minimal bound is defined as minimal imports plus observed
  dynamic calls, and the size of that gap is printed.

**Dynamic lower bound** (`interposer/`, `remote/interposer.sh`).
- **Generation.** An `LD_PRELOAD` library generated from the union of all static imports gives one trampoline per libc
  function (`results/interposed.txt`). Data objects (`environ`, `stdout`, `optind`, ...) cannot be interposed and are
  listed in `results/not-interposed.txt`; their static evidence stands alone.
- **Trampoline.** Each trampoline is aarch64 assembly. It saves x0–x8 and q0–q7, calls the logger, restores the
  registers and tail-branches to the real musl function. Arguments are never re-marshalled, so variadic functions and
  stack-sensitive ones (`vfork`, `setjmp`, `longjmp`) behave exactly as without the interposer.
- **Self-test.** `snprintf` with mixed integer and float varargs, `open`/`fcntl`/`dprintf`, `setjmp`/`longjmp`,
  `vfork`+`execl`, `posix_memalign`, `malloc`/`strcpy`/`printf`/`free`/`strcmp`. The stage fails unless every call is
  logged and the results are correct.
- **What a hit means.** The first call per symbol per process image whose return address lies outside musl's mapping
  is logged, with pid, `argv[0]` and `/proc/self/exe`. Calls from inside musl, such as internal `malloc`, are never
  logged. The logger uses raw syscalls only and calls no libc function except `dlsym`, which is never interposed.
- **Attribution.** `/proc/self/exe` plus `argv[0]` give program names such as `toybox:sed` and `busybox:awk`.
  Products of the workloads (stage-2 tcc, lua, conftest, ...) are `built:` and are not counted.
- **Validation.** Every workload has the same exit status with and without the interposer (`results/dynamic-status.tsv`
  vs `results/strace-status.tsv`).
- **Consistency check.** Every dynamic hit of a program is in that program's static import table, or in its `+libs`
  closure for clang and lld. The check is printed in `summary-tables.md`.

**`tier` column of `libc-surface.csv`.** It holds one of three kinds of value:
- `T0`–`T3`: the first tier whose minimal upper bound or lower bound contains the symbol.
- `comparison`: no tier contains the symbol, but a comparison program or probe run called it.
- `unused-applets`: no tier contains it and nothing called it. Only the full toybox/busybox binaries import it.

**Categories** (`analysis/categories.py`). Each symbol's header is found by scanning musl's installed headers for a
declaration. The header is mapped to a category, with per-symbol overrides for headers that mix concerns (`unistd.h`,
`stdlib.h`). This is a classification choice, not a measurement; `results/symbol-headers.tsv` lists every assignment.

## M2: syscall surface

`strace -f -qq -s 256` runs each workload with no interposer, so the interposer's own syscalls never appear
(`remote/strace.sh`). `analysis/analyze.py` parses the traces in two passes:
1. Collect `execve` results and `clone`/`clone3`/`vfork` parent links. vfork children are logged before the parent's
   call returns.
2. Attribute every syscall to the program image that issued it.

Argument patterns are extracted for the following:
- `clone`/`clone3` flags, classed as thread, vfork-like or fork-like;
- `openat` and other path syscalls: dirfd, absolute or relative, plus `/tmp`, `/dev`, `/proc`, `/etc` prefixes; open
  flags;
- `fcntl` commands and `ioctl` requests;
- `rt_sigaction` signal and handler kind;
- `mmap` prot and flags, `wait4` options, `kill` signals, `rt_sigprocmask` how;
- identity and process-group calls.

Absolute-path `execve`s chosen by the program itself (not resolved through `PATH`) are tabulated separately.

**Children of clones** (`analysis/fork_children.py` → `results/fork-children.md`):
- Every non-thread clone outside `*-check` runs is classed by what its child does itself: `exec` if the child calls
  `execve` successfully, otherwise `no-exec`, meaning it keeps running the parent's code.
- A `no-exec` child that creates processes of its own is also counted.
- The parent program is the image running in the parent at the clone line.
- Pids are not reused within a run: `pid_max` is 32768, and no clone return value repeats in any trace (checked).
- The core-run fork-style total, 1,708, plus the 2 in the excluded `W3-check` run, equals the 1,710 in
  `syscall-surface.csv`.
- **Limit:** a script run through its shebang is attributed to the script's name (for example
  `built:vc-list-files`), not to the shell that interprets it.

**Signals delivered** (`analysis/signals_delivered.py` → `results/signals-delivered.md`). The script counts strace's
signal-delivery lines (`--- SIGx {si_code=…}`), `+++ killed by SIGx` lines, and syscalls that returned `EINTR` or
`ERESTART*`, per run. Under ptrace a delivery is logged even when the disposition is default or ignore, so the counts
are signals sent, not handlers run. Which handlers exist is in the `rt_sigaction` table of `summary-tables.md`.

## W4 configure checks

`analysis/configure_checks.py` pairs every `checking …` line of hello's `config.log` with its `result:` line. It does
this for the tcc run (`raw/W4-tcc-config.log`) and for an unmeasured `CC=gcc` baseline configure of the same tree
(`raw/W4-baseline-config.log`, made by `remote/workloads.sh`). It then lists the checks whose result differs, with the
first diagnostic logged under the tcc check, in `results/configure-checks.md`. Checks that only one run performs,
such as linker detection, are listed separately.

## M3: call sites

1. **Candidates** (`analysis/callsites.py`). Regular expressions match calls to the feature APIs, including
   toybox/busybox wrappers such as `XVFORK`, `xfork`, `xpopen*`, `xrun` and `spawn_and_wait`. They also match
   absolute path literals and `getenv("PATH")`, plus the TASK-01-flagged APIs (DM-023, -031, -036, -037, -038, -039,
   -053, -060, -065, -085, -097).
2. **Compiled code only.** A match counts only if its source line appears in the DWARF line table of the measured
   binary at an address inside an executable section. This drops unconfigured code and code removed by
   `--gc-sections`, and keeps inlined code. For toybox and busybox the binary is the T2 minimal build, so only the
   applets the workloads executed are covered. Exception: the shared startup files of the full binaries that the
   workloads actually ran (toybox `main.c`, busybox `libbb/appletlib.c`) also accept lines from the full binaries'
   line tables. The independent review found that this executed set-uid code was missing (`callsites.py: STARTUP`).
   The enclosing function comes from universal-ctags.
3. **Classification.**
   - A rule gives a first guess.
   - Four reviewers then classified every site by reading its context in the VM, following
     `tools/posix-surface/review/M3-BRIEF.md`. Each reviewer covered different programs, and none wrote the
     extraction scripts. Their output is in `review/m3-*.tsv`.
   - The coordinator classified the 23 sites that appeared after the extractor fix and the 13 startup sites added
     after the independent review (`review/m3-coordinator.tsv`).
   - Classes: `direct`, `restructure` and `emulate` as defined in the task. `n/a` marks a match that is not a use of
     the feature: a string comparison of `/dev/stdin`, a match in a comment, or a feature not reached on Linux.
   - Reviews are merged by (program, file, line, api), ignoring the feature. A line listed under two features shares
     one review. Review entries for lines the extractor no longer emits are ignored; most are the dead-code rows
     that the extractor fix removed.
   - `analysis/callsite_report.py` merges the sites and reviews into `callsites.csv` and `results/callsite-tables.md`.

Classes, efforts and "what it does" are estimates by reading code, not measurements.

## M4: validation patches

The patches are in `tools/posix-surface/patches/m4/`. `remote/m4.sh` copies the pristine sources, applies the
patches, builds the patched binaries into `prefix/m4/`, and creates sets in which only those binaries differ. It then
reruns W1, W2 and W4 in both modes into `raw-m4/`. `analysis/m4_compare.py` checks that exit statuses, workload
outputs, `config.h` and `config.mak` are identical, and reports the change in calls and syscalls. Upstream trees are
never modified in place: `src/` is read-only after fetching.

## Known limits

| Method | Limit | Effect |
|---|---|---|
| Static imports | Symbols reached only through `dlsym` are invisible. GNU make (`load` directive) and tcc (`-run`) import `dlopen`/`dlsym`, but no workload calls `dlopen` (empty dynamic column in `libc-surface.csv`; `dlsym` is never interposed, so its empty column means nothing) | upper bound could miss plugin-only symbols; not exercised here |
| Static imports | Include dead code, error paths and features no workload uses | overestimate; this is why the lower bound exists |
| Minimal builds | Built from the applets observed, so they are bounded by the workloads too | they are a tighter upper bound for *these* workloads, not for "any build" |
| Interposer | First hit per symbol per process image; a vfork child shares the parent's memory, so its first hits may be suppressed if the parent already logged them | attribution is by program, not process; process counts in M4 are counts of images |
| Interposer | Data symbols are not interposable; `dlsym` is excluded | those symbols have static evidence only |
| Interposer | Only exercised paths | underestimate; depends on W1–W6 |
| strace | Calls served by the vDSO (`clock_gettime`, `gettimeofday` on aarch64) usually do not enter the kernel and are not traced. 64 `clock_gettime` syscalls still appear, through the vDSO fallback | time syscall counts in M2 are partial |
| strace | Argument decoding is strace's symbolic rendering; patterns are regex-extracted | rare formats could be mis-bucketed |
| Namespace | Supplementary groups are not mapped inside the user namespace; `/usr/bin/*` remains Alpine's | any `/usr/bin` absolute use shows up as a leak (none in core runs) |
| M3 regex | Calls via function pointers or unlisted macros are missed. File basenames are used to match the line table, so a line from an uncompiled file that shares a basename with a compiled one could be admitted | false negatives possible (reviewers caught `XVFORK` this way); false positives possible, none found by the review |
| M3 `signals` column | Signal names are regex-matched on the call line only, and most signal sites name none. A line like `!= SIGKILL` is tagged with that signal | the per-signal table is indicative |
| Reruns | Workloads are deterministic in their outputs, but a few syscall counts vary with timing between full reruns: ld.lld `futex` (threads), clang `mmap`, W4 `readv` (a few per thousand) | syscall counts are exact for the committed run, not invariant; no libc symbol set, class or category changed between reruns |
| Environment | `SHELL` is the GNU make set's `sh` in every run; both sets' `sh` is the same dash | no effect on results |
| M3 classes | Expert estimates by reading code, by one reviewer per program | effort numbers are order-of-magnitude; M4 calibrates two of them |
| Categories | Header scan plus a hand-written override table | a symbol's category is a choice; per-category totals inherit it |
