# TASK-31 part 10h: git (report)

Branch `work/10h`, from `main` at `ea6f656e`. Written by an agent (Sonnet). ADR number used: 0464 (proposed). ADR-0461 (accepted 2026-11-18)
is implemented as decided; ADR-0339 and ADR-0347 are used as they stand, with the deviations ADR-0464 records.

## Done

1. **The port** (ADR-0461, ADR-0347). mbedTLS 3.6.8 pinned in `ports/sources.lock` (SHA-256 `bc54b818...b595b`, the digest its release lists,
   checked), built by `make -f ports/Makefile mbedtls` as a TLS 1.2 and 1.3 client only (no server, DTLS, sockets, timing, threading, assembly
   or NEON), entropy from `getrandom` alone (`ports/patches/mbedtls/0001`). A second libcurl from the curl port's pin and patches, library only,
   on mbedTLS (`make curl-mbedtls`). git 2.56.1 links it: `git-remote-http` is 6,193,088 bytes (9,306,128 on OpenSSL), `git` 6,710,560.
   Patches (`ports/patches/git/`): `0005` the four names (`alarm`, `getnameinfo`, `statvfs`, `execlp`) are patched out and `ports/apps/gitshim.c` is
   deleted; `0006` the transport helper is started by `posix_spawn` with the random service and, when git holds a connection, a duplicate of it;
   `0007` a missing ancestor in `realpath` is a plain directory; `0008` the helper is started as `git-remote-https` itself; `0009` a temporary
   file's name falls back to the clock, the process and a counter where there is no random service. `make apps` puts `/bin/git`,
   `/usr/libexec/git-core/git-remote-http` and `-https`, `/etc/gitconfig` and an `info/exclude` template in the apps root.
2. **The shell** (ADR-0339, ADR-0464). `zoraya-describe`: the keys `repository`, `sub` and `derive`, the kind `create-tree`. `zoraya-shell`:
   `repository::find` (the nearest `.git` directory inside what the shell holds, refusing a `.git` file, the home directory and `/`),
   `Access::CreateTree`, the form of a command chosen by its first word, the clone directory made from the URL, no `--` before git's command.
   `zosh` gives the repository, sets `GIT_CEILING_DIRECTORIES`, gives git an account of three connections, makes `/data` on first use of an
   own directory, and `help git-clone` finds its page. Descriptions: `git.cmd`, `git-clone.cmd`, `git-init.cmd`, `git-ls-remote.cmd`.
3. **libposix** (found by running git in QEMU, ADR-0464 item 3 and 4): a child keeps the write of an entry that lacks `publish` (before, a
   fork or spawn child of any program held a read-only connection to the directories its parent could write); the ancestors of an entry are
   `ENOENT` and are not reported (no hint tells a person to give `/`).
4. **Tests.** `tools/test-httpd` serves a bare repository over smart HTTP with the host's `git http-backend` (`--git-root`, Basic credentials for
   a push with `--git-auth`) through its TLS listeners, which now pass request bodies on. `tests/net/tls-git`: the test PKI's chain with a non-critical
   name constraint, because mbedTLS 3.6 cannot read a critical one (`README.md`). `tools/test-apps-git.sh` (in `make test-apps`) runs
   `tests/shell/apps-git.script` (139 steps) and, when `make ports` has made `/bin/sh`, `tests/shell/apps-git-hooks.script` (43 steps), then
   reads the host's repository and the server's log.
5. **The checks.** INV-0461-1 (`ports/check-early.sh`: by symbol, with a control), INV-0461-3 (`tools/check-dependencies.sh`, in `make check`),
   INV-0461-2, INV-0339-1 to -5 and INV-0341-6 (rows and statuses in `docs/adr/invariants.md`; the two halves not covered are named there),
   INV-0464-1 to -4. `docs/dependencies.md` (mbedTLS with the licence option used, git's line, the second libcurl), `ports/README.md`,
   `ports/patches/README.md`, `docs/design.md` (a git section).

## What was run

- `cargo test -p zoraya-describe -p zoraya-shell -p zoraya-command -p zoraya-posix`: pass (at the commits of steps 1 to 5).
- `make check`: exit 0 at `89df00e5` (and before each commit).
- `make apps`: passes its checks (mbedTLS in the log). `sh ports/check-early.sh`: passed except the known openssh log count.
- `tools/test-apps-git.sh` by hand under HVF: `PASS: 139 steps` (apps-git) and `PASS: 43 steps` (apps-git-hooks, with `make -f ports/Makefile rootfs`
  done), then its host checks (`ok`). Run before the review's fixes below.
- `make test-changed` with `PAR_QEMU=1` was started on `89df00e5` and **had not finished when this was written**: no verdict exists for it.
- **Not done:** the merge. The independent review (`docs/tasks/TASK-31-review-10h.md`, `opus-review`) found four errors at `f1871d1a`:
  (1) the helper is chosen by the basename of a command string, which a `core.pager`-like setting can imitate (fix: a flag set only in
  `transport-helper.c`, `!use_shell`, a path under `/usr/libexec/git-core/`); (2) INV-0464-3 cited a hook script that does not exist (status
  now says partly); (3) the ceiling test used the starting directory itself, which git ignores (script now uses `/home/proj`);
  (4) only the last `-C` was used (a second `-C` is now refused, host test still to add). Findings 5 to 15 (an untrusted-root server for
  INV-0461-2, the ancestor rule's reach, `create-tree` on `~`, doc mismatches, test-map candidate rule) are **open**.

## Measurements

- Binary sizes: `git` 6,710,560 bytes and `git-remote-http` 6,193,088 (linked with libcurl, mbedTLS and zlib), against 9,306,128 for
  `git-remote-http` on OpenSSL in ports-early.
- The 139 steps of `apps-git.script` (init, add, commit, log, a clone of one commit over https, a commit, a push with the prompts, three refused
  certificates) pass under HVF. The handshake time and the speed of mbedTLS against OpenSSL are **not measured**.
- git's network account: three connections (git, its helper, one spare); two failed at the third link of the chain git, `git remote-https`, `git-remote-https`
  with `EAGAIN` before the helper was started directly.

## Decisions taken to continue (ADR-0464, for the architect)

Forms of `git` as descriptions with `sub`; the clone directory by `derive`; only the transport helper has a network connection (hooks have none, which
is narrower than ADR-0339 item 5); the helper has the random service and the helper's own children do not (temporary file names fall back); the shell's
`GIT_CEILING_DIRECTORIES` is the parent of the repository, as ADR-0339 says; the ancestors of an entry are not reported (narrows INV-0153-7);
the child's connection keeps the write of an entry without `publish`; a second test PKI because of mbedTLS.

## Gaps and known limits

- `git commit` with the message typed in `vi`, and `git log` in `less`, are not played by a script (a window test of the tour's chapter, part 10i).
- No ssh transport, submodules, `git daemon`, manual viewer (`git help` finds none); `git-upload-pack` as a server exists in the binary and is not reachable.
- A hook has no network and no random service. A push over 1 MiB (`http.postBuffer`, `Expect: 100-continue`) is not exercised.
- `credential.helper`, `core.askPass` and `SSH_ASKPASS` are ignored by patch 0002 and not exercised by a script; the file open for writing in the own
  directory (ADR-0393 item 13) does not arise with git.
- The hooks script runs only when `make ports` has made `build/rootfs-posix` (it needs `/bin/sh`, `chmod`): CI's `test-apps` job may not have it; the
  step then says so and is skipped.
- `check-early.sh` reports a compile count of 0 for `openssh` when its log was overwritten by a relink (pre-existing, review of ports-early finding 6).
- `Held::Table` has no `link_target`, so the repository search sees a `.git` link as nothing in a room's table (a `.git` file is refused).

## Hand checks for the architect (a clone and a push against a real host)

On the Mac mini, with a repository you own over https and a token (replace `OWNER`, `REPO` and the host):

```
make fetch-ports && make apps            # builds git; build/rootfs-apps has /bin/git
make run-gui                              # or make run-net for the console; a terminal window
~> git config --global user.name 'Ada'
~> git config --global user.email YOUR-ADDRESS
~> grants git clone https://github.com/OWNER/REPO.git
~> git clone https://github.com/OWNER/REPO.git          # a public name needs no +net word
~> cd REPO
~/REPO> echo hello >> README.md
~/REPO> git add README.md
~/REPO> git commit                       # the message typed in vi (:wq)
~/REPO> git --no-pager log --oneline
~/REPO> git push                         # refused: the host is not given (nothing is sent)
~/REPO> +net github.com:443 git push     # git asks "Username for ..." and "Password for ..." on the terminal; the token is not echoed
~/REPO> grants git push
```

Then check on the host that the commit arrived, and that `cat /data/home/git/config` holds your name and address and no token. A host on your LAN needs
`+net HOST:PORT` for the clone too (a private address is not a public name). In a room (`room new r +w ~/r +net github.com:443`) `git push` needs no word.

## Proposed ADRs

- ADR-0464 (what 10h decided): recommends Option A (as built).
