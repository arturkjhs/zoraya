# ADR-0347: The names git adds to libc.a, the patches that make it ADR-0339's, and what is left for part 10h

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0005, ADR-0100, ADR-0141, ADR-0310, ADR-0312, ADR-0339, ADR-0342, ADR-0346, ADR-0390
- **Evidence:** CP-27

## Context

ADR-0339 items 1, 4, 8 and 9 say how git is built and which files and variables it must not read. TASK-31 part
`ports-early` builds git 2.56.1 ahead of part 10h (no cargo, no QEMU; ADR-0390 says how `libc.a` was relinked), linked
with the library of the pinned curl (ADR-0310, the one already built for `curl` and `wget`), the OpenSSL of ADR-0312 and
zlib. CP-27 (a name needs a program behind it) is the rule for the block. The probe is the linker.

## Decision drivers

1. A name is exported because a program in the image imports it (ADR-0342 driver 1), and a switch of git's own that
   avoids a name is preferred to a new name (ADR-0339 item 1 lists the switches).
2. What decides trust, which program runs and which configuration is read is not left to the environment or to a
   repository's configuration (ADR-0339 items 4, 8, 9; ADR-0310 item 6).
3. `libposix` is not changed by this slot.

## Considered options

### Option A: switches first, musl-own names in the block, shim and list the rest, four patches (the recommendation)
- **Description:** below.
- **Evidence for:** drivers 1 to 3.
- **Evidence against:** the shimmed names are present and unreachable; one patch changes `getenv` for the whole program.
- **Cost:** *(estimate)* none beyond the block.
- **Precedent:** ADR-0343 (probe and block), ADR-0346.

### Option B: export `pread`, `getpagesize`, `sync_file_range`, `sysinfo`, `alarm`, `statvfs`, `getnameinfo` as a Linux libc has them
- **Evidence against:** each needs a meaning in `libposix` that no accepted ADR gives (`alarm` is ADR-0342 item 6's
  exclusion), for programs that have a switch that avoids it.
- **Precedent:** every Linux distribution.

### Option C: patch each use out of git
- **Evidence against:** a patch per release for code the switches already guard (ADR-0310 option B's argument).
- **Precedent:** none here.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **What was built** (`make git`): `git` (6516272 bytes: `.text` 3.49 MB, `.rodata` 0.64 MB, `.bss` 0.42 MB, debug sections
   1.4 MB from `libc.a`'s `-g`) and `git-remote-http` (9306128 bytes: `.text` 4.9 MB; also `git-remote-https`), `-O2`, static.
   The switches: `NO_PERL NO_PYTHON NO_TCLTK NO_GETTEXT NO_ICONV NO_EXPAT NO_MMAP NO_UNIX_SOCKETS NO_SETITIMER`
   (ADR-0339 item 1) and, found by the probe, `NO_RUST` (its Rust code is optional in this release), `NO_OPENSSL` (git's own
   SHA code; OpenSSL is in `libcurl`'s link), `NO_PREAD` and `NO_GETPAGESIZE` (git's emulations, which avoid `pread` and
   `getpagesize`), `HAVE_SYNC_FILE_RANGE=` and `HAVE_SYSINFO=` empty (Linux-only calls) and `NO_REGEX=NeedsStartEnd`
   (musl's `regexec` has no `REG_STARTEND`); git's own `compat/regex` is used. The host answers are `uname_S=Linux`
   (git's Makefile reads the build host's `uname`, which is Darwin); `zcc` defines nothing of Linux, which the log shows.
2. **`zoraya-ar` names a member by its base name** (ADR-0109 item 3) and replaces a member of the same name. `libgit.a`
   holds `fsck.o` and `tree.o` twice (the top directory's and `reftable/`'s), so the archive lost one of each and the link
   left git's own `fsck_*` and `tree_*` functions undefined. `ports/git/ar.sh` gives each object a name that holds its path
   before archiving. The tool's silent replacement is a hazard for any port with such names (to the architect).
3. **Patches** (`ports/patches/git/`, GPL-2.0-only as git's):
   - `0001-configuration-paths-and-variables.patch` (item 4): the global configuration is `~/.config/git/config` always, never
     `~/.gitconfig`; `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM` and `GIT_CONFIG_NOSYSTEM` decide nothing (the system file is
     `/etc/gitconfig`, `ports/git/gitconfig`: no detached garbage collection, no guessed identity).
   - `0002-no-askpass-no-credential-helper.patch` (items 8 and 9): `git_prompt` runs no askpass program and
     `credential_do` runs no helper, in any scope: the prompt is git's own, on the terminal.
   - `0003-no-tls-or-proxy-configuration.patch` (item 8): `http.ssl*`, `http.proxy*`, `http.cookiefile`, `http.savecookies`,
     `http.pinnedpubkey` and `http.extraheader` are ignored, and `remote.NAME.proxy` sets no proxy (libcurl's own `--disable-proxy` also stops the proxy options; the patch makes git not ask).
   - `0004-variables-not-read.patch` (item 8): `getenv` is `zoraya_getenv` in every object (the macro follows `compat/posix.h`, outside every `#ifdef`), which answers "not set" for `GIT_SSL_*`,
     every `GIT_CONFIG*` (`GIT_CONFIG_GLOBAL`, `_SYSTEM`, `_NOSYSTEM`, `_COUNT`, `_KEY_n`, `_VALUE_n`, the legacy `GIT_CONFIG` and `GIT_CONFIG_PARAMETERS`: a line's environment carries no configuration; the consequence is that `git -c k=v` sets nothing, in process or in a child), `GIT_EXEC_PATH` and `GIT_TEMPLATE_DIR` (the compiled-in directories are used: a variable must not name where git's programs or a new repository's hooks come from), `GIT_ASKPASS`, `SSH_ASKPASS`, `GIT_SSH*`, `GIT_PROXY_COMMAND`, `GIT_HTTP_PROXY_AUTHMETHOD`,
     `GIT_CURL_*`, the proxy variables, `CURL_CA_BUNDLE`, `SSL_CERT_*`, `GIT_EXTERNAL_DIFF` and `GIT_ALLOW_PROTOCOL`.
     `EDITOR`, `VISUAL`, `PAGER`, `GIT_EDITOR`, `GIT_PAGER`, `HOME` and `GIT_DIR` stay (item 5, ADR-0335 item 10).
4. **Names.** The block holds the six names that are musl's or `libposix`'s own code, hidden by the tier list: `getchar`
   (the terminal prompt), `getpgid` (progress), `memmem` (bundle), `mkdtemp` (refs), `strcasestr` (mailinfo) and `utime`
   (copy). The names `libc.a` still lacks, linked from `ports/apps/gitshim.c` in this build and each **left for part 10h**:

   | name | program | what it needs |
   |---|---|---|
   | `alarm` | `git` (`upload-pack.c`) | the timeout of a served fetch; git is built `NO_SETITIMER`; ADR-0342 item 6 keeps `alarm` out: 10h patches the call out |
   | `getnameinfo` | `git` (`connect.c`) | the printed form of the address of a connection; ADR-0342 item 11's numeric form (libposix) |
   | `statvfs` | `git` (`diagnose.c`) | `git diagnose`'s disk figures; ADR-0343 left `statfs` out: 10h patches it out |
   | `execlp` | `git` (`help.c`) | `git help` starting a manual viewer; patch it out |
   | `tcsetattr`, `tcflush`, `cfgetospeed`, `cfsetospeed`, `select` | `git` (`compat/terminal.c`) | the terminal names of ADR-0342 items 4 and 5 (10c): the password prompt |

```posix-surface-added
getchar git
getpgid git
memmem git
mkdtemp git
strcasestr git
utime git
```

5. **Not built here.** ADR-0339 item 7 (a child's network connection and random service), the `repository` key, the
   `create-tree` operand, `git.cmd`: parts 10h and the planner's. A child process (`git-remote-http`) therefore cannot
   connect in this build.

## Consequences

- **Positive.** `git` and `git-remote-http` build and link from the pinned source on the pinned curl library with the
  rules of ADR-0339 items 4, 8 and 9 in the program.
- **Negative.** Untested: no QEMU ran. `zoraya_getenv` is a blunt instrument: a variable git needs later has to be taken
  off its list by a patch. The binaries are 6.5 and 9.3 MB. git's Rust requirement (`NO_RUST` is a switch of this release)
  is a revisit trigger.
- **Follow-up.** Part 10h: the four patched-out names, `getnameinfo`, the child's network, `/etc/gitconfig` in the image
  (`ports/git/gitconfig`), `git.cmd`, the tests against the runner's servers.

## Invariants

- **INV-0347-1.** Every line of the block names a program, and that program's objects import the name. *Check:*
  `posix-surface-check imports docs/adr/0347-surface-for-git.md git=DIR` in `make check-ports` (`ports/check-early.sh`).
- **INV-0347-2.** No object of git imports `getenv` but `wrapper.o` (which holds `zoraya_getenv`) and `compat/regex`'s locale
  lookups, and `git` and `git-remote-http` pass `zoraya-elf-check`. *Check:* `ports/check-early.sh` in `make check-ports`, by symbol (`nm -u` of every object).

## Open questions

- Which git release when git's Rust becomes mandatory (`NO_RUST` is removed)? Pin an earlier release or accept a Rust
  step in the port.
- Is `NO_PREAD`'s emulation safe for git's threaded pack reads? `compat/pread.c` saves the offset, seeks, reads and seeks back with no lock, so two threads reading one descriptor can interleave: 10h measures, or sets `NO_PTHREADS`.
- Git is GPL-2.0-only and `git-remote-http` links Apache-2.0 OpenSSL statically (through libcurl): not combinable. `docs/architect-queue.md` D has the options. Until decided git does not enter the image. Decided by the architect, 2026-11-16 (recorded in ADR-0461, proposed): `git-remote-http` links a libcurl built with mbedTLS 3.6 LTS, its GPL-2.0-or-later option, for git only; the pin is made by 10h's part. Until ADR-0461 is accepted and that part builds it, git does not enter the image.
- Pins: the git archive and the hashes are of the first download (trust on first use).

## Revisit triggers

- The Rust of git becomes mandatory; `libposix` offers `pread`.

## Architect's decision

(Left empty for the human.)
