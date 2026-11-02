# Patches to third-party programs

These are diffs against other people's programs, made for the TASK-02 measurements (`docs/research/task-02*`). Each
patch modifies, and quotes lines of, files that are under the license of the original program. **They are not covered
by the BSD-2-Clause license in the repository's `LICENSE`**: each one is distributed under the license of the program
it patches, given below. The versions are the ones pinned in `../versions.env` and `../sources.lock`.

| Patch | Program and version | Files it changes | License of the original |
|---|---|---|---|
| `build/tcc-aarch64-wchar.patch` | TinyCC (tcc), commit `e23dabed154a8495f3ac1ff3cc9d04fa1c207893` | `include/tccdefs.h` | LGPL-2.1 (tcc's `COPYING`; `tccdefs.h` has no notice of its own) |
| `m4/dash-vforkexec-to-posix_spawn.patch` | dash 0.5.13.5 | `src/exec.c`, `src/exec.h`, `src/jobs.c` | BSD-3-Clause (dash's `COPYING` and the notice at the top of each file) |
| `m4/pdpmake-system-to-posix_spawn.patch` | pdpmake 2.0.4, commit `0faaf258c9b3eaa9f5e4f6842e2484cbea6f23f8` | `make.c` | Public domain (the Unlicense text in pdpmake's `LICENSE`) |

Each license was read from the program's source at the pinned version on 2026-11-02. dash's tarball matched its
SHA-256 in `../sources.lock`. tcc's files were read from the GitHub mirror `TinyCC/tinycc` at the same commit, because
`repo.or.cz` served a bot check. dash's `COPYING` also lists `mksignames.c` as GPL, but no patch here touches it.

`build/` holds fixes needed to build a program for the measurements. `m4/` holds the M4 validation patches, which
replace a `fork`-family call with `posix_spawn`; each patch's header names the call site.
