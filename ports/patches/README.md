# Patches to the ported programs

Applied by `ports/Makefile` in name order after unpacking. Each patch modifies, and
quotes lines of, files under the licence of the original program, given in
[`../README.md`](../README.md); **they are not covered by the repository's
BSD-2-Clause `LICENSE`**.

| Patch | Program and version | Files | Why |
|---|---|---|---|
| `dash/0001-vforkexec-to-posix_spawn.patch` | dash 0.5.13.5 (BSD-3-Clause) | `src/exec.c`, `src/exec.h`, `src/jobs.c` | TASK-02's M4 patch, unchanged: a non-interactive simple command starts with `posix_spawn` (ADR-0109 item 9). |
| `dash/0002-fork-group-hint.patch` | dash 0.5.13.5 (BSD-3-Clause) | `src/jobs.c` | `forkshell` calls `zoraya_fork_group` with the group `forkparent` and `forkchild` will `setpgid` to (ADR-0104 item 4, ADR-0142). |
| `dash/0003-vforkexec-setpgroup-tcsetpgrp.patch` | dash 0.5.13.5 (BSD-3-Clause) | `src/exec.c`, `src/exec.h`, `src/jobs.c` | `vforkexec` always uses `posix_spawn`, with `SETPGROUP` and `POSIX_SPAWN_TCSETPGRP` under job control and `SETSIGDEF` for the signals `forkchild` resets (ADR-0104 item 5, ADR-0142); the `vfork` path stays as the fallback. |
| `make/0001-config-sub-zoraya.patch` | GNU make 4.4.1, `build-aux/config.sub` (GPL-3.0-or-later with the Autoconf exception) | `build-aux/config.sub` | `zoraya*` as an operating system, so that `--host=aarch64-unknown-zoraya` is accepted (ADR-0109 item 1). |
| `toybox/0001-zoraya-portability.patch` | toybox 0.8.15 (0BSD) | `lib/portability.h`, `lib/portability.c` | a Zoraya branch where toybox stops an unknown OS with `#error`: `struct statfs` from musl, `dev_t` in musl's (Linux's) encoding, extended attributes through the tier's `f*xattr` (the path forms refuse with `ENOTSUP`), and no mount table, file notification or file system type names (ADR-0109 item 6). |
| `tcc/0001-zoraya-target.patch` | TinyCC `e23dabed` (LGPL, version 2 or later) | `configure`, `conftest.c`, `include/tccdefs.h`, `lib/Makefile`, `libtcc.c`, `tcc.c`, `tcc.h`, `tccpp.c` | the `Zoraya` target OS (ADR-0109 item 7, ADR-0143). |
| `tcc/0002-skip-shf-exclude-sections.patch` | TinyCC `e23dabed` (LGPL, version 2 or later) | `tccelf.c` | skip `SHF_EXCLUDE` input sections, which the Rust members of `libc.a` carry (ADR-0143). |
