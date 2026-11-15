# TASK-31 part ports-early: ncurses, less, nvi2 (with DB 1.85), OpenSSH's client and git, built ahead of their parts

Branch `work/ports-early` (from `origin/main` 3292aa2). Written by the implementation session (Sonnet). No review yet.

## Limits of the slot and how the build was set up

C port builds only, niced, `make -j3` at most, no cargo build, no QEMU. The tools and the sysroot came from sibling
worktrees' `build/` and `target/` (copied into this worktree's ignored `build/` and `target/`): `zoraya-ar`,
`posix-surface-check`, `zoraya-elf-check`, `zoraya-adr-check` (10e's), `libzoraya_posix.a` (the `poll` branch's, which is
in main), the sysroot and musl's objects (10e's), OpenSSL, zlib and curl (10e's `build/ports/deps` and `work/curl-*`).
`ports/Makefile` was run as `make -f ports/Makefile -o build/ports/obj/sysroot.stamp -o build/ports/bin/.stamp CARGO=true TARGET`
(and for git also `-o` on `libssl.a`, `libz.a`, `libzshim.a` and `src/curl`), so nothing rebuilt the libc or the libs.
`libc.a` was relinked, without cargo, after each ADR block was added to `ADDED_ADRS`:
`make -f ports/Makefile -o ...sysroot.stamp -o ...musl/objects.txt CARGO=true build/sysroot/usr/lib/libc.a`.
Disk: 47 GB free at the start, 42 GB at the end (the three trees copied: about 1 GB). `libposix` was not changed.

## Results

| port | make target | builds | size (bytes) | names left (see the ADR) |
|---|---|---|---|---|
| ncurses 6.6 | `make ncurses` | yes | `libncursesw.a` 344030, `libtinfow.a` 267374 | `iswprint wmemcpy wcsrtombs tdelete tfind tsearch` in the block; `sleep`; terminal names |
| less 710 | `make less` | yes | 2872992 (`.text` 897972) | `creat` (10d) |
| DB 1.85 + nvi2 2.2.2 | `make db1`, `make nvi` | yes | `libdb1.a` 98926; `vi` 3247216 (`.text` 1168112) | `select` (10c), terminal names, 17 musl-own names in the block |
| OpenSSH 10.6p2 | `make openssh` | yes | `ssh` 6823648, `ssh-keygen` 6382280 | `getservbyname socketpair getifaddrs freeifaddrs execlp madvise gethostbyaddr initgroups`, terminal names |
| git 2.56.1 | `make git` | yes | `git` 6516272, `git-remote-http` 9306128 | `alarm getnameinfo statvfs execlp`, terminal names |

Binary sizes include `libc.a`'s `-g` (debug sections 1.3 to 1.4 MB each); the loaded size of `less` is about 1.14 MB and of `vi` 1.34 MB
(ADR-0334's open question). None was run: nothing was tested beyond linking and `zoraya-elf-check`.

## Commands run

- `tools/fetch-ports.sh --verify ports/sources.lock build/ports/src NAME` for each new pin (hashes from the first download).
- the five `make` targets above, each rebuilt from a fresh unpack after its patches changed.
- `target/debug/posix-surface-check imports docs/adr/0390-*.md ncurses=... nvi=... less=...` (and 0346, 0347): passed.
- `sh ports/check-early.sh ...`: `check-early: passed` (compile logs run `zcc` and no `-D__linux`; ELF check of the 7 binaries;
  strings of `ssh` and `ssh-keygen` have no `SSH_ASKPASS` or configuration file; `zoraya_getenv` in both git binaries).
- `target/debug/zoraya-adr-check docs` (the 10e binary): 0 errors. `make check-workflow-syntax`, `make check-host-paths`,
  `check-fetch-ports`, `check-ci-jobs`, `check-ci-plan`, `check-test-lanes`, `check-python-cache`: passed.
- NOT run (no cargo): `make check` as a whole (fmt, clippy, argv, test-caps and the other cargo steps; no Rust file changed),
  `make check-ports` from a clean tree, `make test-changed`, any QEMU test, the candidate CI. Not verified that a CI run
  shows jobs after the workflow edit (only the syntax check ran).

## What the parts must still do

- 10c: `tcsetattr tcflush cfgetospeed cfgetispeed cfsetospeed cfsetispeed select` in libposix; delete `ports/apps/ttyshim.c`.
- 10d: `creat`, `sleep` in libposix; copy `build/ports/curses/usr` into the sysroot; wire `vi`, `ex`, `view`, `less` into the
  rootfs, `.cmd` files, tests; choose the DB 1 copy; decide on nvi's `lock` option (ADR-0390).
- 10g: the eight ssh names (export or patch out), `getnameinfo`, description, test server (ADR-0346).
- 10h: `alarm`, `getnameinfo`, `statvfs`, `execlp` (patch out), the child's network, `/etc/gitconfig` (`ports/git/gitconfig`),
  `git.cmd` (ADR-0347). Delete `gitshim.c`, `sshshim.c`.
- Everyone: the three shim archives are not in any image; `ports` and `rootfs` do not depend on the new targets, so the
  ports cache key changed (everything under `ports/`) and CI's `make ports` builds none of them yet.

## For the architect

Proposed ADR-0390, 0346, 0347 (rows in `docs/adr/index.md` and `invariants.md`, Total 786); recommendations: accept A in each.
`zoraya-ar` replaces same-base-name members silently (git's `fsck.o`, `tree.o`): consider an error or path-keeping names.
Decisions taken to continue: DB 1.85 from Oracle's archive; nvi's `lock` option stays inert; ncurses installed into
`build/ports/curses` not the sysroot; hashes trusted on first download; git `NO_RUST`.
