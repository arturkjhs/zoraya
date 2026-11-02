# ADR-0108: Running programs from the filesystem

- **Status:** accepted
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0003, ADR-0012, ADR-0013, ADR-0014, ADR-0031, ADR-0033, ADR-0040, ADR-0042, ADR-0043, ADR-0051, ADR-0055, ADR-0103, ADR-0107
- **Evidence:** CP-01, CP-03, CP-16, DM-009, DM-032, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, fs/requirements.md#22-atomic-rename, fs/requirements.md#32-symbolic-links, fs/requirements.md#4-metadata-read-and-written, stage-6/sources.md#other-programs-and-inputs, stage-6/sources.md#posix-semantics, stage-6/sources.md#tinycc, task-02/posix-surface.md#absolute-paths-chosen-by-programs, task-02/posix-surface.md#linux-syscall-surface-m2

## Context

`spawn` takes an image handle (ADR-0003), which today comes only from `directory_lookup` on the embedded ramdisk
(ADR-0043); the stage 4 plan's step 11, "spawn from a filesystem image: image handle from a file", was optional and
was not done. ADR-0033 loads static `ET_EXEC` AArch64 ELF only, with W^X checked at load, and ADR-0043 asked whether an
image handle should stay valid as a read-only memory object. Stage 6 must run `/bin/sh`, `make`, `tcc`, the toybox
applets and, above all, the executables tcc writes to the volume (`tcc hello.c && ./a.out`). This ADR decides how an
image handle is made from a file, how `execve` of a file, a `#!` script and a `$PATH` name works, and what a program
tcc produces must look like.

**What is measured.**

- 3,002 successful `execve` calls in the core runs, 2,922 by absolute path after a user-space `$PATH` walk; 7 failed
  (`task-02/posix-surface.md#linux-syscall-surface-m2`). `/bin/sh` is run 143 times by make for recipes and 92 times
  through `#!/bin/sh` in W4 (`task-02/posix-surface.md#absolute-paths-chosen-by-programs`).
- toybox is a multicall binary dispatching on the name it was called by, installed as symlinks
  ([B3], `stage-6/sources.md#other-programs-and-inputs`), and follows the symlink chain with `readlink` to find the
  applet name when needed (`fs/requirements.md#32-symbolic-links`).
- tcc unlinks its output and recreates it with `O_WRONLY|O_CREAT|O_TRUNC` (`fs/requirements.md#22-atomic-rename`);
  W1 runs the stage-2 tcc it built, W3 runs `sqlite3`, W4 runs `hello` and every configure test program, so the
  execute bit on fresh files is load-bearing (`fs/requirements.md#4-metadata-read-and-written`). The mode of a create
  reaches the service through ADR-0103 item 1's `open_with_mode`; the stage 4 `Open` message carried none.
- Linux `execve`: `#!interpreter [optional-arg]`, up to 255 characters, four levels of interpreter scripts; `ENOEXEC`
  for an unrecognised format; the pid unchanged ([P7], `stage-6/sources.md#posix-semantics`).
- tcc has its own linker, writes ELF executables directly, adds `crt1.o`, `crti.o`, `crtn.o`, `libc.a` and
  `libtcc1.a` unless `-nostdlib`, and defaults to a shared executable unless `-static` ([T2], [T7],
  `stage-6/sources.md#tinycc`).
- The stage 4 service transfers data inline up to 60 KiB or through an attached memory object up to 1 MiB per request
  (ADR-0055); the kernel copies image segments into fresh frames at load (ADR-0033).

**What is assumed.** Image sizes: dash, make and tcc as static musl binaries are a few hundred KiB each `[assumption]`;
copying one at the measured memory rates is below a millisecond under HVF.

**Forces from TASK-01.** Executing a file must grant nothing (CP-03); no global name in the kernel (CP-01, DM-009);
W^X from day one (CP-16, DM-032); the loader is a parser of hostile bytes (ADR-0012's discipline, ADR-0033). Zircon has
no `exec`: a launcher reads the executable and builds the process from memory objects
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`; the loader service is
`[outside evidence: fuchsia.dev program loading, not fetched]`). Plan 9 kept `fork` and `exec` and gave each process
its own namespace (`alternatives-postmortem.md#plan-9--inferno`); that its `exec` resolves the binary through that
namespace is the author's inference from the two.

## Decision drivers

1. **The kernel names no file** (ADR-0043 driver 1): an image comes from bytes a process already holds, by handle.
2. **The loader stays the one parser of program bytes, total and W^X-checking** (ADR-0033, INV-0033-1, INV-0033-2).
3. **The filesystem service stays a file service** (ADR-0012 rule 4, INV-0055-9): it should not learn ELF.
4. **Cost per `exec` is bounded by one copy of the file** plus the load (3,002 `execve`s, mostly of small images).
5. **tcc's output runs unchanged**: whatever shape the loader accepts, tcc's linker must produce it with the sysroot's
   crt objects (ADR-0109).
6. **No executable mapping from user space** (ADR-0051: "no right maps anything executable; user code comes from
   `spawn` alone").

## Considered options

### Option A: The kernel reads the executable from a path (Linux `execve`)

- **Description:** `spawn` takes a path; the kernel opens and reads the file through a VFS and loads it.
- **Evidence for:** The simplest call for callers; one copy, from the page cache.
- **Evidence against:** A kernel filesystem client and a global name (CP-01, ADR-0043's rejection); the engine is a
  user-space service (ADR-0055), so the kernel would speak its protocol.
- **Cost:** A kernel IPC client; rejected by accepted ADRs.
- **Precedent:** Unix.

### Option B: The process reads the file into a memory object and the kernel makes an image from it

- **Description:** A new call, `image_create(memory, length) → Image`: the kernel parses the ELF in the memory
  object's frames with `zoraya-elf`, keeps the validated program headers and a reference to the object, and returns an
  image handle with `READ | DUPLICATE | TRANSFER`. `spawn` loads such an image as it loads a ramdisk one: fresh frames,
  segment bytes copied from the object, permissions from the headers, instruction-cache maintenance. `libposix`'s
  `execve` opens the file, reads it into a memory object of the file's size (through `attach` and `read_buffer` in
  1 MiB requests), calls `image_create`, and spawns.
- **Evidence for:** Drivers 1 to 3 and 6: no name in the kernel, one parser, the service unchanged. Driver 4: one copy
  service-to-object, one object-to-frames, both at memory speed. An image handle is duplicable, so a launcher may
  cache one (the open question of ADR-0043 is answered: the image *holds* a memory object rather than *is* one).
- **Evidence against:** A new kernel call and a `Memory` reference inside the image object. The creator keeps a
  writable mapping of the object: it can change the bytes after validation, so the kernel copies at `spawn` what is
  there then; the headers it validated are fixed, so W^X and the ranges hold whatever the bytes say, and a creator that
  rewrites them harms only the child it spawns. Two copies of each image per `exec`.
- **Cost:** *(estimate)* 150 lines in the kernel (parse once, keep headers, load from an object instead of a slice);
  `libposix`'s loader path of 100 lines.
- **Precedent:** Zircon's launcher builds a process from VMOs the launcher filled; ADR-0033's loader, moved one step.

### Option C: The filesystem service makes the image

- **Description:** A new request, `executable → Image`, on a file connection: the service copies the file into a
  memory object, calls `image_create`, caches the image by object id and modification time, and replies with a
  duplicate.
- **Evidence for:** One copy fewer for a cached image (the 143 `/bin/sh` runs share one); the client never holds the
  bytes.
- **Evidence against:** Driver 3: the service learns which files are programs, holds image handles and a cache with an
  invalidation rule, and the trusted base of ADR-0012 widens ("holding only the block handle and client channels").
  A rights question follows: who may ask for an image, which INV-0055-3's table would gain. The copy saved is a few
  hundred KiB per `exec`.
- **Cost:** *(estimate)* Option B's kernel call plus a cache in the service and a right.
- **Precedent:** None in the files; Fuchsia's loader is a separate service, not the filesystem.

### Option D: A user-space loader mapping segments directly (Zircon's `vmar_map` with execute)

- **Description:** `libposix` parses the ELF, creates memory objects per segment and maps them into a new, empty
  process with execute permission where the headers say; the kernel knows no image type.
- **Evidence for:** No image object, no loader in the kernel; a dynamic loader could use the same path later.
- **Evidence against:** Driver 6: an executable mapping right does not exist and ADR-0051 says user code comes from
  `spawn` alone; cross-process mapping into an empty process is ADR-0003's rejected Option D as ABI; the parser would
  leave the kernel, so W^X at load (INV-0033-2) would rest on the library.
- **Cost:** *(estimate)* The largest ABI change: cross-process construction and executable mappings.
- **Precedent:** Zircon (with a launcher service and job policy around the raw primitives).

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`image_create(memory, length)`** needs `READ` on the memory object; `length` is the file's size, at most the
   object's. The kernel runs `zoraya-elf` over the bytes (total, INV-0033-1), rejecting what ADR-0033 rejects
   (anything but little-endian AArch64 static `ET_EXEC`; a segment that is not readable; a segment both writable and
   executable; segments outside the user range or overlapping; `memsz < filesz`) with `InvalidArgument`, and returns
   an `Image` with
   `READ | DUPLICATE | TRANSFER`. The image keeps the validated headers and an `Arc` of the object (ADR-0041); the
   object's frames live while any image handle, mapping or handle to it does (ADR-0051's rule extended by one holder).
   `spawn` with such an image copies each segment's `filesz` bytes from the object through the direct map into frames
   it allocated and zeroed, as the ramdisk path does today; `memsz` beyond `filesz` stays zero. Images from the
   ramdisk are unchanged. `INV-0051-5`'s lint allows `Size` for `length`.
2. **`execve(path, argv, envp)`** in `libposix`: resolve `path` (ADR-0107); `open` it read-only; `fstat`; refuse
   without an execute bit for the owner (`EACCES`, ADR-0014 rule 6's advisory mode, which is what make's and dash's
   `X_OK` probes test); read the first bytes: `#!` → item 3; `\x7fELF` → read the whole file into a memory object
   (`attach` then `read_buffer` in requests of at most 1 MiB, ADR-0055), `image_create`, and on `InvalidArgument`
   return `ENOEXEC`; anything else → `ENOEXEC`. Then the `exec` of ADR-0103 item 9 and ADR-0104 item 6: `spawn` into
   the caller's process job with the bootstrap record of ADR-0107 (its `Image` entry is the new image, its executable
   path the resolved one), `exec_commit`, exit. `execvp` and the shells walk `$PATH` through the table and, on
   `ENOEXEC`, run `/bin/sh` with the file as its argument, as `execvp` and dash's `tryexec` do.
3. **Scripts.** A first line `#!interpreter [optional-arg]` of at most 255 bytes `[Linux 5.1's limit]` is split into
   the interpreter path and at most one argument; the interpreter is resolved and executed with
   `argv = [interpreter, optional-arg, path, argv[1..]]`, where `path` is the script as the caller named it; at most
   four levels of interpreters that are themselves scripts, then `ELOOP`. The script's execute bit is required, the
   interpreter's too.
4. **`/proc/self/exe`.** The bootstrap record's executable path (ADR-0107 item 1); `readlink` returns it; `open` of
   it opens that path. The `Image` entry of the table is the process's own image handle, which is what a
   self-re-executing program (toybox's `/proc/self/exe` re-exec, T2) could `spawn` directly without reading the file
   again; at T1 nothing does.
5. **What tcc must produce** (ADR-0109 makes it so): a static `ET_EXEC` for AArch64 with a read-execute text segment
   starting at or above 4 MiB (`USER_LOW`, ADR-0031; tcc's default text start for ARM64 is 0x400000, which is that
   address `[outside evidence: tcc's arm64-link.c, not fetched]`), a read-write data segment, no writable-executable
   segment, no `PT_INTERP`, entry `_start` from the sysroot's `crt1.o`. The loader ignores program headers that are
   not `PT_LOAD` (`PT_GNU_STACK`, `PT_NOTE`, `PT_TLS` for musl's TLS template, which `__init_tls` reads from the
   program headers through the auxiliary vector `AT_PHDR`: the startup block of ADR-0109 therefore carries the
   program header table too).
6. **Symlinks.** `execve("/bin/cat")` where `cat` is a link object to `toybox` (ADR-0013): the resolver follows it
   (ADR-0107 item 7) and the image is toybox's, with `argv[0]` = `cat`, which is how toybox chooses the applet
   ([B3]). The installer of ADR-0109 writes those links on the volume.
7. **Caching.** None at stage 6. A later launcher-side cache keyed by (volume id, object id, `mtime`) is the revisit
   if `exec` cost shows on W4; images are duplicable handles, so it needs no new kernel call.

**Tensions with accepted ADRs.**

- *ADR-0043*: `directory_lookup` stays for the ramdisk; its open question is answered as "an image holds a memory
  object" rather than "is one".
- *ADR-0051*: a new holder of a memory object (an image) and a new creator of mappings' content; "no right maps
  anything executable" holds, since the kernel copies into frames it maps itself. `INV-0051-5` gains one `Size` use.
- *ADR-0033*: the loader reads from an object's frames as well as from the ramdisk slice; the parser is the same.
  "Dynamic linking and `ET_DYN` are outside" stays true: tcc is made to link statically.
- *ADR-0012 rule 4* is kept: the service learns nothing about programs (Option C would have strained it).
- *ADR-0042*: the startup block gains the program header table (item 5), a change to its layout that ADR-0109 lists.

### Effect of the vision

For target 2 Option B is the right shape: a sandbox can only make images from bytes it was given, the kernel's parser
is the one place program bytes are trusted, and a launcher can hand a sandbox pre-made image handles with `READ` only.
Caching by a launcher (item 7) fits target 3's many `exec`s. Threads and networking: no effect. A dynamic loader later
would be a user-space program that reads `ET_DYN` files and needs an executable mapping right, a separate ADR.

### Deferred to a later stage

The image cache; `ET_DYN`; `#!` with more than one argument; executing from the ramdisk by path (`/zoraya/bin`
could be a table entry of kind `Ramdisk` later).

## Consequences

- **Positive.** One loader, one parser, no name in the kernel; the service unchanged; tcc's static output runs through
  the same path as everything else.
- **Negative.** Two copies per `exec`, a memory object per image, no sharing of text between processes of one image.
- **Follow-up.** `image_create` and its tests; `zoraya-elf` reading `PT_TLS` and exposing the header table; the
  `execve` path and script handling in `zoraya-posix`; the installer's symlinks.

## Invariants

- **INV-0108-1.** `image_create` accepts exactly what `zoraya-elf` accepts for the ramdisk (INV-0033-2's set), and a
  rejected object leaves no image and no held reference. *Check:* host tests of `zoraya-elf` with the same mutation
  set over an object-backed input, and an in-QEMU test with INV-0041-1's counters around a refused call.
- **INV-0108-2.** A program spawned from an image made of a file's bytes runs with the same behaviour as the same
  bytes in the ramdisk. *Check:* an in-QEMU test that places `hello`'s ELF on the test volume, makes an image from
  it and spawns it, expecting the ramdisk `hello`'s output and exit code.
- **INV-0108-3.** `execve` refuses a file without the owner's execute bit with `EACCES`, a non-ELF file without `#!`
  with `ENOEXEC`, and nests interpreters at most four deep. *Check:* in-QEMU `libposix` tests with such files on the
  volume.
- **INV-0108-4.** An executable linked by tcc inside Zoraya from the sysroot's crt objects loads and runs:
  `tcc hello.c && ./a.out` prints its line. *Check:* the stage 6 roadmap test (ADR-0110).

## Open questions

- Whether tcc's ARM64 ELF output needs a patch for the loader's range or alignment checks: the first toolchain task
  finds out with a host-side `zoraya-elf` check of tcc's output before anything boots.
- The cost of the two copies on W4's 3,002 `exec`s: ADR-0110.

## Revisit triggers

- Measured `exec` cost on W4: the launcher cache (item 7).
- A program in scope needing `ET_DYN` or a dynamic loader.

## Architect's decision

Accepted as recommended, 2026-11-02, with the architect's six changes already in the text of ADR-0100 to ADR-0110: the 10 % threshold in ADR-0102, `KILL` for `job_suspend` and `job_resume`, `make test-posix-long`, `make fetch-ports`, `aarch64-unknown-zoraya` with the T2 cost of `config.sub`, INV-0004-6 until exception channels, and INV-0106-4. Decided by the architect; recorded by an agent at the architect's request.
