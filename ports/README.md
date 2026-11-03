# Ports: the cross toolchain's third-party inputs

Stage 6b (ADR-0109, TASK-17). `make fetch-ports` downloads the archives of
[`sources.lock`](sources.lock) into `build/ports/src/` and checks each SHA-256; it
is the only step that uses the network. `make ports` builds, from that cache only,
`build/sysroot/` (musl's headers, `libc.a`, the crt objects, the empty libraries,
tcc's `libtcc1.a` and headers) and `build/rootfs-posix/` (the programs and the
sysroot as `/usr`), then runs `make check-ports`. No third-party source is
committed: only the lock, the patches and the build files below.

| Path | What |
|---|---|
| `sources.lock` | name, version, SHA-256, cache file name and URL of each archive |
| `Makefile` | the whole build, run from the repository's root by the top Makefile |
| `config.site` | answers for autoconf checks a cross build cannot run, each with its reason |
| `target-signals.sh` | a header that gives host-built helpers Zoraya's signal numbers (dash's `mksignames`) |
| `musl/musl.mk`, `musl/excluded.txt` | which musl files `libc.a` retains (ADR-0100, ADR-0141) |
| `musl/zoraya/` | musl's replaced `src/internal/syscall.h` (the shim), the empty `crt_arch.h` for `crt1.c`, and musl's patch |
| `crt/crt1.S` | `_start` (ADR-0109 item 4) |
| `posix-stub/` | the ENOSYS stub of `zoraya-posix` until 6c (ADR-0140) |
| `patches/<program>/` | patches to each program, applied in name order |
| `tests/hello.c` | the program tcc links in the host check (ADR-0143) |
| `check-broken-archives.sh` | the export check on broken copies of `libc.a` |
| `test-fetch.sh` | the fetch step's host test (INV-0109-3, INV-0109-4), a step of `make test` |

## Programs and licences

Each licence was read from the program's source at the pinned version on 2026-11-02.
A patch to a program, and a file derived from a program's file, is distributed under
that program's licence, not under the repository's BSD-2-Clause `LICENSE`.

| Program | Version (`sources.lock`) | Licence of the original | Patches here |
|---|---|---|---|
| musl | 1.2.5 | MIT (`COPYRIGHT`) | `musl/zoraya/0001-no-secure-start.patch`; `musl/zoraya/syscall.h` replaces musl's file of that name and is under musl's MIT licence |
| dash | 0.5.13.5 | BSD-3-Clause (`COPYING`; it also lists `mksignames.c` as GPL, which no patch touches) | `patches/dash/0001-vforkexec-to-posix_spawn.patch`, TASK-02's M4 patch unchanged |
| GNU make | 4.4.1 | GPL-3.0-or-later (`COPYING`); `build-aux/config.sub` GPL-3.0-or-later with the Autoconf exception | `patches/make/0001-config-sub-zoraya.patch` (one line of `config.sub`) |
| toybox | 0.8.15 | 0BSD (`LICENSE`) | none |
| busybox | 1.38.0 | GPL-2.0-only (`LICENSE`) | none |
| TinyCC | mob, commit `e23dabed154a8495f3ac1ff3cc9d04fa1c207893` | LGPL-2.1-or-later (`COPYING`, the notice in each file) | `patches/tcc/0001-zoraya-target.patch`, `patches/tcc/0002-skip-shf-exclude-sections.patch` |

TASK-02 pinned tcc by commit and fetched it with git. The cache holds the archive
GitHub generates for that commit from the mirror `TinyCC/tinycc` (TASK-02 read tcc
there too, because `repo.or.cz` served a bot check); its pax header names the
commit, and its SHA-256, taken by TASK-17, is recorded like any other. Whether
GitHub keeps such generated archives byte-identical was not checked here; if one
ever changes, `make fetch-ports` refuses it and the lock must be renewed by an
ADR-0109 revisit.

## How each program is built

- **musl**: `musl/musl.mk` compiles the retained directories with `tools/zcc` and the
  shim; `libc.a` is those objects and the stub's, with every name outside the tier
  list renamed `__zoraya_*` (ADR-0141).
- **dash**: `./configure --host=aarch64-unknown-zoraya`; its host helpers are compiled
  with `-include build/ports/obj/target-signals.h`.
- **GNU make**: `./configure --host=aarch64-unknown-zoraya --disable-load --without-guile
  --disable-nls`.
- **toybox**: `make allnoconfig`, then the applets of `TOYBOX_APPLETS` in
  `Makefile`; on a macOS host it needs GNU sed (`brew install gnu-sed`).
- **busybox**: `make allnoconfig`, the shell choice set to none, then `tr` and `diff`,
  as TASK-02's minimal builds.
- **tcc**: `./configure --cpu=aarch64 --targetos=Zoraya ...` (ADR-0143), `tcc` and
  `libtcc1.a` (`arm64-libtcc1-usegcc=yes`, so by `zcc`); a second build of the same
  source by the host's compiler, rooted at `build/sysroot`, links `tests/hello.c` for
  the host check.
