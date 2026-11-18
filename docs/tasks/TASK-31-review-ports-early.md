# TASK-31: review of part ports-early (branch `work/ports-early`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...work/ports-early` at `61dfab5` (merge base `3292aa2`; `origin/main` was `8b09113`): ncurses 6.6
(target archives), less 710, Berkeley DB 1.85 with nvi2 2.2.2, OpenSSH 10.6p2 (`ssh`, `ssh-keygen`) and git 2.56.1
(`git`, `git-remote-http`), C builds only. Read: `ports/Makefile`, `ports/check-early.sh`, the three shims of
`ports/apps/`, `ports/nvi/*`, `ports/db1/build.sh`, `ports/git/*`, every patch under `ports/patches/{less,nvi2,db1,openssh,git}`,
`ports/sources.lock`, `docs/dependencies.md`, the READMEs, `.github/workflows/test.yml`, `tools/test-map.txt`, and
proposed ADR-0390, ADR-0346, ADR-0347 against accepted ADR-0334, 0335, 0339, 0340, 0342 and ADR-0005, 0310, 0421.
The code around the patches was read in the build trees that already existed in the worktree (`build/ports/work/*`,
patched): git's `git-compat-util.h`, `config.c`, `connect.c`, `transport.c`, `http.c`, `credential.c`, `prompt.c`,
`compat/pread.c`, `builtin/index-pack.c`; OpenSSH's `ssh.c`, `readpass.c`, `ssh-sk-client.c`, `sshkey.c`; less's
`cmdbuf.c`, `decode.c`, `defines.h`; nvi2's `common/exf.c`, `ex/ex_preserve.c`; DB 1.85's file headers.

What was run here (no port build, no QEMU):

- `make check` (with `~/.cargo/bin` on `PATH`): `24 of 24 passed`. Without cargo on `PATH`, `check-argv` fails, as the
  report says.
- `make check-workflow-syntax check-host-paths`: passed.
- `shasum -a 256` of the six archives in `build/ports/src/`: each equals its `sources.lock` line. No upstream checksum
  file or signature is available offline, so the pins are **unverified against upstream** (trust on first use, as the
  lock's comment says).
- `posix-surface-check imports` for ADR-0390 (`ncurses=`, `nvi=`, `less=`), ADR-0346 (`ssh=`) and ADR-0347 (`git=`,
  `git=.../compat`) on the existing objects: each `passed`.
- `zoraya-elf-check` on copies of the six binaries: `6 files, 0 refused`. Sizes on disk equal the report's.
- `strings` of `ssh`/`ssh-keygen`: no `SSH_ASKPASS`, `ssh_config` or `.ssh/config`; `SSH_SK_HELPER` and
  `SSH_AUTH_SOCK` are present (finding 9).
- `llvm-nm -u` over every object of the git tree: **40 objects import the real `getenv`, 27 import `zoraya_getenv`**
  (finding 1).
- The build logs: no `undefined symbol`, no ` error:`, no `-D__linux` in `less`, `nvi`, `db1`, `ncurses`, `openssh` and
  `git`'s logs; `less.log` holds only a relink (finding 6).

## Findings

1. **error (must-fix).** `ports/patches/git/0004-variables-not-read.patch:5-13`. **What.** The
   `#define getenv(name) zoraya_getenv(name)` is inserted inside `#ifdef DISABLE_SIGN_COMPARE_WARNINGS ... #endif` at the
   end of `git-compat-util.h`. Only the sources that define `DISABLE_SIGN_COMPARE_WARNINGS` before including the header
   get the macro. `config.c`, `connect.c`, `transport.c`, `environment.c`, `exec-cmd.c`, `setup.c`, `pager.c`,
   `editor.c`, `builtin/config.c` and 30 other objects besides `wrapper.o` call libc's `getenv` (`llvm-nm -u config.o connect.o transport.o`
   shows `getenv`; `http.o`, `diff.o`, `credential.o` show `zoraya_getenv`). So, of the list the patch and ADR-0347
   item 3 claim, these are still read: `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_*`, `GIT_CONFIG_VALUE_*` (`config.c:744`
   and below), `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_PROXY_COMMAND` (`connect.c:1186`, `1420`, `1060`) and
   `GIT_ALLOW_PROTOCOL` (`transport.c:1098`). Accepted ADR-0339 item 8 names `GIT_CONFIG_COUNT and its companions`,
   `GIT_SSH` and `GIT_SSH_COMMAND` as compiled out or ignored; the binary does neither. INV-0347-2's check
   (`check-early.sh:66`, `grep zoraya_getenv`) passes on this binary, so it cannot see the defect. **Scenario.**
   `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0=/tmp/x.cfg git status` makes git read a
   configuration file that no scope of ADR-0339 item 4 names. In a repository whose remote is `ssh://...`,
   `GIT_SSH_COMMAND=/tmp/run git fetch` starts `/tmp/run` through `/bin/sh` with git's table, before any network
   decision. `GIT_ALLOW_PROTOCOL=ext` lets an `ext::` URL run a command. **Fix.** Move the declaration and the macro
   out of the conditional, after the header guard's `#endif` and outside `DISABLE_SIGN_COMPARE_WARNINGS`, so that every
   unit except `wrapper.c` (which `#undef`s it) gets it. Replace INV-0347-2's check with a symbol check: no object of
   the tree other than `wrapper.o` has `getenv` among its undefined names (`llvm-nm -u`, or `posix-surface-check` if it
   can list a directory's imports). At the same time decide, in ADR-0347, three names the list misses that decide
   which program runs or which file is read: `GIT_EXEC_PATH` (`exec-cmd.c:316`: which `git-remote-https` and which
   `git` the subprocesses are), `GIT_TEMPLATE_DIR` (`setup.c:2280`: the hooks `git init` and `git clone` install) and
   `GIT_CONFIG` (`builtin/config.c:934`: the file `git config` reads and writes). `GIT_CONFIG_PARAMETERS` is kept on
   purpose (ADR-0347:70), but a hostile environment can set it directly and it does everything `GIT_CONFIG_COUNT` does;
   either clear an inherited value at git's start (keeping what `git -c` adds) or say in ADR-0347 why the one is
   blocked and the other not.

2. **error (must-fix).** `docs/dependencies.md:165`, `ports/Makefile` (`$(GIT_BINS)`, `CURL_LIBCURL`), ADR-0347
   item 1. **What.** `git-remote-http` links libcurl and the image's OpenSSL statically (the link line in
   `build/ports/logs/git.log` names `-lcurl -lssl -lcrypto`). git is GPL-2.0-only with no OpenSSL exception; OpenSSL 3.5
   is Apache-2.0 (`docs/dependencies.md:156`), which the FSF and Debian treat as incompatible with GPL-2.0-only (Debian
   builds git's HTTP helper against GnuTLS for this reason). The row's justification, "a separate program like GNU
   wget", does not carry over: wget is GPL-3.0-or-later, which is compatible with Apache-2.0, and NetSurf's row needed
   its explicit OpenSSL permission. `git` itself does not link OpenSSL (`NO_OPENSSL`, its link line has only `-lz`
   and the shims). Accepted ADR-0339 item 1 prescribes this combination, so the conflict is in an accepted decision,
   not only in this part. **Scenario.** Part 10h copies `git-remote-http` into the image as the ADRs say; the image then
   distributes a binary whose licences do not allow its distribution as one work. **Fix.** Not a code change in this
   part: correct the row (state the combination and that it is unresolved), add the question to ADR-0347's open
   questions, and raise it with the architect before 10h (ADR-0001 rule 6 note on ADR-0339). The options are a libcurl
   with a GPL-2.0-compatible TLS library for git's helper only (BearSSL is MIT; ADR-0312 already considered it), a
   reading of GPL-2.0 §3's system-library exception (weak when the library ships in the same image), or no HTTPS git.

3. **warning (should-fix).** `ports/Makefile:49` (`ADDED_ADRS`), `docs/adr/invariants.md` rows of INV-0390-1,
   INV-0346-1, INV-0347-1. **What.** The three proposed blocks are in `ADDED_ADRS`, so on merge `libc.a`, and through
   `rootfs` the image's `/usr/lib/libc.a`, exports 33 new names (`strlcpy`, `asprintf`, `getpwent`, `catopen`,
   `syslog`, `memmem`, `getchar` and the rest). INV-0342-1's second half (the named program's objects import the name)
   is checked only by `check-early.sh`, which runs only when the early ports are built; CI builds none of them
   (`ports` and `rootfs` do not depend on them, confirmed: `$(ROOTFS_STAMP)` lists only dash, make, pdpmake, toybox,
   awk, tcc and the sysroot), so in CI the script prints `nothing to check` and passes. The names are exported with
   no program in the image behind them, which is what ADR-0342 driver 1 and CP-27 exclude, and INV-0005-1 reads "in an
   accepted superseding ADR" (ADR-0398's precedent was noted in the poll review, but its program `t_poll` was built and
   checked in CI). **Scenario.** A later change of a block (a wrong name added for `nvi`) passes CI and enters the
   image's `libc.a`, because the only check that ties the name to nvi's objects never runs there. **Fix.** Either build
   the early ports in CI's `ci-build` (they are already inside the ports cache key) and make `check-early.sh` fail when
   they are missing in CI, or leave the three ADRs out of `ADDED_ADRS` until the part that puts the program in the
   image, linking these early builds against a `libc.a` built with them in a scratch sysroot. Write the Status column
   as what CI actually checks (`not yet applicable (stage 10)` is the true status while CI builds none of them).

4. **warning (should-fix).** `tools/test-map.txt:275-277`. **What.** The three ADR lines map to `check test-libposix
   test-posix`. A change of their blocks changes `libc.a`'s exports and with them the `configure` answers of every
   port that is in the image or tested: CPython (`HAVE_GETPWENT` makes `pwd.getpwall` exist; `syslog`), curl, wget,
   NetSurf, toybox. ADR-0398's line (`:203`) maps to `test-posix-threads test-posix-suites test-posix-threadtest3
   test-posix test-apps test-python test-python-regr` for that reason. This branch's own export change has been run by
   no test (the report says so). **Scenario.** `strlcpy` becoming visible makes a port's `configure` stop compiling its
   own copy and use musl's, with a different truncation contract in one caller; a candidate run of this branch does
   not run `test-apps` or `test-python-regr`, so the merge goes in on the smoke set. **Fix.** Give the three lines the
   step list of ADR-0398's line, and run `candidate-full/` for this merge rather than the candidate set.

5. **warning (should-fix).** `ports/nvi/build.sh:13`, `:41`, `:45`. **What.** The object directory is never cleaned,
   compiles run in the background three at a time with their exit status ignored, and the only check is that each
   `.o` exists. **Scenario.** A patch change makes `ex/ex_script.c` fail to compile; `$(NVI)` is unpacked again, but
   `$(OBJ)/nvi/ex_ex_script.o` from the previous build still exists, so the check passes and the old object (with the
   pseudo-terminal code) is linked into `vi` without an error. **Fix.** `rm -rf "$obj"` before `mkdir -p`, and either
   collect the PIDs and `wait "$pid" || exit 1` for each, or compile in the foreground.

6. **warning (should-fix).** `ports/check-early.sh:24`, `:30`, `:46`, `:61`. **What.** (a) INV-0390-2's check is
   `grep -q "$zcc"` on the whole log, which one link line satisfies: `build/ports/logs/less.log` here has five lines and
   no compile command, and it passes. (b) The tree names are written in with their versions (`less-710`,
   `openssh-10.6p2`, `git-2.56.1`); after a version bump in `sources.lock` the `[ -x ... ]` tests are false and the
   ADR-0346 and ADR-0347 checks are silently skipped, and the ncurses/less/nvi group is skipped whole if any one of
   the three is missing. **Fix.** Count the `-c` compile lines that start with `$zcc` and require at least one per
   object (or per source the build lists); pass the work directories from `ports/Makefile` (`$(LESS)`, `$(OPENSSH)`,
   `$(GIT)`), and report each skipped group by name.

7. **warning (should-fix).** `docs/dependencies.md:163`, `ports/README.md:55`, `ports/patches/README.md:21`.
   **What.** DB 1.85's files carry the four-clause University of California licence (clause 3, the advertising
   clause, is in `hash/hash.c` and in at least 50 files of the archive), not BSD-3-Clause. The University withdrew clause 3 for
   its own code in 1999, so it is permissive and allowed by CLAUDE.md, but the rows record a licence the files do not
   state. **Fix.** Write `BSD-4-Clause-UC (clause 3 withdrawn by the University of California, 1999)` in the three
   places.

8. **note.** `ports/patches/git/0003-no-tls-or-proxy-configuration.patch:11`, git's `http.c:1454-1458`. `remote.NAME.proxy`
   and `remote.NAME.proxyAuthMethod` of a repository's own `.git/config` bypass the `http.proxy*` filter and reach
   `CURLOPT_PROXY`. It is harmless only because the pinned libcurl is built `--disable-proxy`. Say so in the patch's
   comment (or filter the remote keys too), so that the one guard is not mistaken for two. The same holds for
   `http.curloptResolve` (a repository can pin a host to an address; TLS still checks the name): worth a line in
   ADR-0347.

9. **note.** OpenSSH, ADR-0340 items 1 and 4. The binary still holds, behind options only the description refuses,
   connection sharing (`-M`, `-S`, ADR-0340 item 1 says "not built"), X11 forwarding (`-X`, `-Y`, which runs `xauth`),
   ProxyJump (`-J`, which runs `ssh -W` through `$SHELL`) and the agent client (`SSH_AUTH_SOCK`). The security-key
   helper (`SSH_SK_HELPER`, `execlp`) is unreachable: without `ENABLE_SK` no sk key type is registered
   (`sshkey.c:123-143`) and `sshsk_sign` returns before `start_helper` (`ssh-sk-client.c:258`). Record in ADR-0346 that
   the port's second guard covers only `-o`, `-F` and options after the host, and that `-J`, `-X`/`-Y`, `-M`/`-S`
   rely on the description alone; 10g can decide whether to fatal them like `-o`.

10. **note.** `ports/apps/{ttyshim,sshshim,gitshim}.c`. Confirmed that the archives cannot reach the image now: they
    are in `build/ports/ttyshim/`, outside `$(SYSROOT)`; the image takes `build/rootfs-posix` (whose rule copies only
    the named binaries and `$(SYSROOT)/usr/include` and `/usr/lib`) and `build/rootfs-apps`, and no rule copies `less`,
    `nvi`, `ssh` or `git` into either. Their code is, however, inside the four programs, and parts 10d, 10g and 10h
    copy those programs. A cheap guard: a marker string in each shim (`zoraya-early-shim`) and a check in
    `check-ports` that no file of `build/rootfs-posix` or `build/rootfs-apps` contains it. Also: `$(SSHSHIM)` is a
    prerequisite of `$(GIT_BINS)` but not linked into git; the OpenSSH rule runs `make -k`, which is harmless (the exit
    status still fails the rule) but has no reason in a build that must stop at the first error.

11. **note.** ADR-0347 open question (`:117`). "Git's emulation holds a lock" is not so: `compat/pread.c` is
    `lseek`, `read_in_full`, `lseek`, with no lock. It is safe in `index-pack` because each thread opens its own
    `pack_fd` (`builtin/index-pack.c:213`), and elsewhere because pack reads are under git's object-read lock. Correct
    the sentence; the question for 10h stands.

12. **note.** `ports/sources.lock`. The six hashes match the archives in `build/ports/src/`; none is verified
    against upstream here. git publishes `sha256sums.asc` next to the tarball on kernel.org and OpenSSH a signed
    release announcement with the SHA-256; checking those two costs little. `nvi2` is a GitHub tag archive, whose bytes
    GitHub does not promise to keep stable (the `awk` pin has the same exposure), and `db1` has one URL only.

## Checked and found sound

- **ssh** (`0002`, `0003`, `0004`): `process_config_files` reads nothing, `-o` and `-F` are fatal in the option loop,
  the `goto again` is gone so nothing after the host is read as an option, and the user name check sits after
  `-l`, `USER@HOST` and `ssh://` parsing and `fill_default_options`. `ssh_askpass` returns before forking; `read_passphrase`
  and `notify_start` read no `SSH_ASKPASS*`, `DISPLAY` or `WAYLAND_DISPLAY`. ProxyCommand, LocalCommand,
  PermitLocalCommand, KnownHostsCommand and `Match exec` are reachable only from the configuration reader, which is
  gone; `StrictHostKeyChecking` falls to OpenSSH's default, `ask`. `verify_host_key_dns` returns at once.
- **git** (`0001`, `0002`, `0003`): the global file is `~/.config/git/config` only (the XDG path is not read),
  `GIT_CONFIG_SYSTEM`/`NOSYSTEM`/`GLOBAL` decide nothing (patched in `config.c` itself, so finding 1 does not reach
  them); `git_prompt` runs no askpass; `credential_do` runs no helper in any scope; the `http.*` filter sees the
  normalized key of `http.<url>.<key>` too (`urlmatch_config_entry` calls `http_options` with `http.<key>`), so
  `http.https://h/.sslVerify` is ignored; `GIT_SSL_NO_VERIFY` and the other `GIT_SSL_*` are read in `http.c`, which has
  the macro. Hooks, `core.fsmonitor`, `core.sshCommand`, `core.pager`, `diff.external` and aliases from a repository's
  configuration still run, as ADR-0339 item 5 accepts.
- **less**: `TAGS` and `USERFILE` are 0 in the generated `defines.h` (which also removes `LESSKEY_CONTENT` and
  `LESSKEYIN`), `LESSHISTFILE` `-` disables the history file at `cmdbuf.c:1519`, `LESSOPEN`/`LESSCLOSE` return at once.
- **nvi**: no system or home start-up file; `NEXINIT`/`EXINIT` and the off-by-default `exrc` stay (ADR-0335 item 6);
  `:script` reports and returns; the recovery stubs never set `F_RCV_ON`, so `:preserve` says preservation is not
  possible rather than claiming it; the buffer is DB 1's unlinked `mkstemp` file. The inert `lock` option is a recorded
  difference from ADR-0335 item 1 (ADR-0390 open question).
- **Build**: none of the new targets is a prerequisite of `ports`, `rootfs`, `apps` or `ci-build`; `ports/git/ar.sh`
  gives distinct member names (297 members, no duplicates, in `libgit.a`) and is right to exist, since `zoraya-ar`'s
  same-name replacement differs from `ar`'s; the cache key and the touch list name the same three ADRs and
  `check-workflow-syntax` passes; the `posix-surface-added` blocks pass `posix-surface-check imports` against the
  existing objects; the sizes and the loaded sizes in ADR-0390 add up; ADR numbers are in the task's reserved ranges.
- **Licences** other than findings 2 and 7: less (Less License, files dual-offered with GPL), nvi2 (three-clause BSD,
  no advertising clause anywhere in the tree), OpenSSH (no advertising clause in any `.c`/`.h`), ncurses (X11).

## Merge recommendation

Not as is. Findings 1 and 2 must be settled first: 1 is a patch that does not do what an accepted ADR requires and
whose check cannot tell; 2 is a licence record that is wrong and a question for the architect. Findings 3 to 7 belong
in the same round, and 3 and 4 decide how this branch's change of `libc.a` is tested before it reaches `main`
(`candidate-full/`).
