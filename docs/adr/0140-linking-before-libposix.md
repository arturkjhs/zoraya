# ADR-0140: How the ports link before `zoraya-posix` exists

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0005, ADR-0100, ADR-0109
- **Evidence:** CP-27, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#redox, task-02/libc-surface.csv

## Context

ADR-0100 makes `libc.a` from musl's retained C and `zoraya-posix`, a Rust static library that implements every
OS-facing function of the tier list on native handles. ADR-0109 builds the ports (dash, GNU make, toybox, busybox,
tcc) against that archive. The plan of stage 6 puts the toolchain in 6b and `zoraya-posix` in 6c, after 6a's kernel
objects, so 6b has no Rust half to link against. No accepted ADR says what the ports link against meanwhile; TASK-17
asks for a stub whose OS-facing exports fail with `ENOSYS`, recorded as a proposed ADR.

**What is measured** (TASK-17, on the M4 with Apple clang 21 and the pinned Rust 1.99.1):

- musl's retained files define 131 of the 243 names of the tier list (`task-02/libc-surface.csv` by ADR-0100 item 7's
  rule); the other 112 come from the directories ADR-0100 item 2 gives to Rust, or from the 34 files of the retained
  directories that make a system call the shim does not route (ADR-0141).
- The retained objects also use 31 names they do not define and that are not in the tier list: musl's internal
  `__lock`, `__unlock`, `__lseek`, `__dup3`, `__clock_gettime`, `__map_file`, `__libc_malloc`, `__block_all_sigs`
  and others, and public functions of the replaced directories that retained code calls (`socket`, `connect` and
  `sendmsg` from the `nscd` client under `getpwnam`; `pthread_setcancelstate` from the `passwd` parsers; `pipe2`,
  `execl` and `posix_spawn_file_actions_addclose` from `popen` and `wordexp`).
- musl's `printf` and `strtod` use `long double`, which is IEEE quad on AArch64 and needs the soft-float routines
  (`__addtf3`, `__multf3`, `__extenddftf2`, ...). The host's clang ships none for a bare AArch64 target; the pinned
  Rust toolchain's `compiler_builtins` for `aarch64-unknown-none` has them, and any Rust static library brings it.
- With a stub that defines the 112, the 31 and the shim's targets, `libc.a` has no unresolved reference, and dash,
  GNU make, busybox, tcc and a C hello world link and pass `zoraya-elf-check`.

## Decision drivers

1. **Every port must link now**, so that the build, the patches and the loader's view of each program are checked in
   6b and not discovered in 6c (TASK-17; ADR-0108's open question on tcc's output).
2. **The archive must already have 6c's shape**: musl's objects plus the members of one Rust static library, so that
   the export check, the hiding of ADR-0141 and the `svc` scan are tested on the real layout (CP-27).
3. **Nothing may pretend to work.** A stub that returned success would let a later test pass for the wrong reason.
4. **No surface beyond the tier list** (INV-0005-1, INV-0005-2).

## Considered options

### Option A: Link against musl's own system-call layer for Linux (baseline)

- **Description:** Build musl unchanged for `aarch64-linux-musl` and link the ports with it until 6c, as TASK-02 did
  on Alpine.
- **Evidence for:** No stub; everything links, and the programs would even run under Linux.
- **Evidence against:** The archive would contain `svc` everywhere and Linux system-call numbers, which INV-0100-2
  forbids, and none of ADR-0100's cut, shim or hiding would be exercised; 6c would change the link of every port at
  once. It is the masquerade ADR-0109 rejected.
- **Cost:** *(estimate)* Smallest now; all of 6b's integration risk moves to 6c.
- **Precedent:** Linux distributions; Fuchsia's Starnix runs unchanged Linux binaries instead
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option B: A generated Rust stub library that fails every function (recommended)

- **Description:** `ports/posix-stub/stubs.txt` lists every function the Rust half must provide, each with a kind
  (`int`, `zero`, `ptr`, `sigerr`, `err`, `void`, `raw`, `trap`, or a `u32` variable); `stubs.awk` turns it into a
  `no_std` Rust file that `ports/Makefile` compiles with the pinned `rustc` as a static library `zoraya_posix`. An
  `int` function returns -1 with `errno` `ENOSYS`, a pointer function `NULL`, one that returns an error number returns
  `ENOSYS`, the shim's targets return `-ENOSYS`, and a function that must not return (`_exit`, `abort`,
  `siglongjmp`, `zoraya_posix_exit_group`, `zoraya_posix_start`) executes `brk #0x7a`, which ends the process with a
  fault. Arguments are not declared: on AAPCS64 a callee that reads none may be called with any.
- **Evidence for:** Drivers 1 to 4. The archive is musl's objects plus a Rust library's members, as in 6c, and brings
  `compiler_builtins` the same way. Nothing succeeds. The list is a reviewable file, and the export check of ADR-0100
  item 7 proves it complete: a missing name fails `make check-ports`.
- **Evidence against:** A second, throw-away list of the Rust half's names, which 6c replaces. A few names that need
  no system (`htonl`, `sigemptyset`, the `posix_spawnattr_*` setters) fail too, because they belong to the Rust half
  and the stub does not implement anything.
- **Cost:** *(estimate)* 160 lines of list and generator; 9.5 MB of `core` and `compiler_builtins` members in
  `libc.a`, which the linker leaves out of programs.
- **Precedent:** Redox grew relibc behind a platform layer that started mostly unimplemented
  (`alternatives-postmortem.md#redox`).

### Option C: Stubs in C, generated into the archive

- **Description:** The same list, but as C functions compiled by `zcc`.
- **Evidence for:** One language for the stub and no Rust in 6b's build.
- **Evidence against:** The long double routines would then need another source (LLVM's compiler-rt, a new port, or
  tcc's own `lib-arm64.c`), and the archive's shape would differ from 6c's (driver 2).
- **Cost:** *(estimate)* As B, plus a source for the soft-float routines.
- **Precedent:** None in the files.

## Recommendation

**Recommendation (not a decision): Option B.** The stub is `ports/posix-stub/`; its list has three sections: the
tier list's names that musl does not define (112, `ioctl` among them because `src/misc/ioctl.c` is excluded), the
names retained musl code uses that are not in the tier list (31; ADR-0141 hides the public ones), and the shim's
targets with `zoraya_posix_start` (10). 6c replaces the stub with `crates/zoraya-posix`, built the same way into the
same archive; the list then documents what 6c must provide on day one.

## Consequences

- **Positive.** All ports link in 6b against the archive 6c will have; every host check runs on real objects.
- **Negative.** Nothing built in 6b runs: every program faults in `zoraya_posix_start` before `main`. A function the
  stub fails may hide a port bug until 6c.
- **Follow-up.** 6c deletes `ports/posix-stub/` and builds `crates/zoraya-posix` in its place; `ports/Makefile`'s
  `STUB_LIB` then names that crate's static library.

## Invariants

- **INV-0140-1.** Until `zoraya-posix` exists, every function of `libc.a` that the Rust half of ADR-0100 owns either
  returns a failure or traps; none reports success. *Check:* the generator accepts only the kinds above (a review
  item), and `make check-ports` fails if the stub and musl together do not define the tier list.

## Open questions

- Whether 6c should keep a stub kind for the rows ADR-0100 item 6 stubs on purpose (`tee`, `syscall`, `mprotect`).

## Revisit triggers

- 6c lands: this ADR is superseded by the crate itself.

## Architect's decision

