# ADR-0464: What part 10h decided where ADR-0339, ADR-0347 and ADR-0461 are silent: forms of `git`, the clone directory, the transport helper's network, the ceiling

- **Status:** proposed
- **Date:** 2026-11-19
- **Depends on:** ADR-0153, ADR-0156, ADR-0224, ADR-0310, ADR-0311, ADR-0339, ADR-0341, ADR-0347, ADR-0393, ADR-0411, ADR-0433, ADR-0461
- **Evidence:** CP-01, CP-05, CP-27, CP-28, DM-009, stage-10/survey.md#how-git-starts-other-programs, stage-10/survey.md#files-git-touches-unasked

## Context

Part 10h builds git on the decisions of ADR-0339 (accepted: Option C, the `repository` key, `create-tree`, a typed URL gives its
host, a child's network), ADR-0347 (the patches and names) and ADR-0461 (accepted: libcurl on mbedTLS for `git-remote-http`).
The code needed choices those ADRs leave open: how one description says that `git clone URL DIR` and `git init DIR` have
operands and `git status` has none; how `git clone URL` names its directory; what a child of git gets of the network now that
`libposix` has `posix_spawn` with grants (ADR-0411, ADR-0433) and a forked child still cannot be given a channel end (ADR-0287
item 3); where git's own search for `.git` ends. This ADR records them, with the least committing choice in each case, and the
two places where the build is narrower than ADR-0339. **Measured:** `git-remote-http` on mbedTLS 3.6.8 is 6,193,088 bytes
(9,306,128 on OpenSSL, ADR-0347); `git` is 6,710,560 bytes. **Not measured:** a TLS handshake's time against OpenSSL's.

## Decision drivers

1. **The shell, not git, names what git is given** (ADR-0339 driver 3, CP-01, CP-28, DM-009; what git reads and runs unasked is `stage-10/survey.md#files-git-touches-unasked`, how it starts children `stage-10/survey.md#how-git-starts-other-programs`): nothing that a repository's files say makes a grant.
2. **A description is data the shell reads** (ADR-0156): no code per program in the shell, and a description a person cannot
   write for their own program (only the image's designate, ADR-0153's Decision).
3. **A child gets what the parent gives it and no more** (CP-05, ADR-0433 driver 1).
4. **Fail closed**: a word the shell cannot place is refused with the way out, never guessed.

## Considered options

### Option A: forms as descriptions of their own, a spawn grant for the helper only, the shell's ceiling (the recommendation)

As below.

### Option B: one description for git with a table of subcommands inside it

Evidence for: one file. Against: a new syntax in `zoraya-describe` for nested operand lists, and `help` and the planner each learning
it; the first three commands that name something (`clone`, `init`, `ls-remote`) are all there is today.

### Option C: the shell knows git's command line

A function in `zoraya-shell` that reads `git`'s words. Against: driver 2; a second place that must agree with git's own parsing, and the
`repository` and `create-tree` rules become code, not data. Precedent: none in this system.

### Option D: fork and `exec` for the helper, give the child a connection by a request to the shell

Against: ADR-0411 Option C (a request path from a running program to the shell) is the hardening it keeps for later; `posix_spawn` with
`POSIX_SPAWN_GRANT_NET` already exists and is built (ADR-0433).

## Recommendation

**Recommendation (not a decision): Option A** (CP-27: git is ported as it is; only what must differ is patched).

1. **Forms.** A description may carry `sub WORD` (a lower-case word): it is the form of its `program` when the line's first word is
   `WORD` (`git clone`), and the shell parses the words after it. `user/describe/posix/git-clone.cmd`, `git-init.cmd` and
   `git-ls-remote.cmd` are forms of `/bin/git`; `git.cmd` is the program's own and has the `repository` key, the optional `-C DIR` (a
   `read` operand: it moves the search's starting point) and a `rest` operand for the command's words, which are the program's. A
   `-C` before the command selects the program's own description, so a `clone` after it gives no network and no directory: refused by
   the program with the shell's hints, not widened (driver 4). Only the image's descriptions are chosen (INV-0153-6): a copy of the program
   gets none. `git` reads long options, so `userland_options` does not refuse them as it does for `ssh` (ADR-0393 item 7).
2. **The clone directory.** `derive TARGET FROM CUT` (a description key): when the optional `create-tree` operand TARGET is not written,
   the shell designates the directory that `url_dir_name(FROM, CUT)` gives: the last segment of the address ([`url_file_name`]'s plain
   rule, ADR-0311) with a final `.git` cut, not empty and not hidden. A name it cannot be sure of refuses the line, with the hint to
   write a directory. It is the rule git's own naming gives for such an address; where git would name it otherwise git's write is
   refused, never widened.
3. **A child's network and random service (ADR-0339 item 7, narrowed).** ADR-0339 says a fork and a spawn each get a connection of the
   parent's scope. Built here: only the transport helper is started by `posix_spawn` with `POSIX_SPAWN_GRANT_NET |
   POSIX_SPAWN_GRANT_RANDOM` (`ports/patches/git/0006-*`): a duplicate of git's own connection, with its scope, and a share of the random
   service. If git holds no connection (no `+net`, no typed URL) the grant of the network is refused with `EACCES` and the helper is
   started with the random service alone: libcurl cannot start without it (`curl_global_init failed`), and the helper then fails to
   connect, which is the network refusal the person sees. The helper is started as `git-remote-https` itself, not through `git
   remote-https` (`0008`): the random service is shared one level below a program's own connection only (ADR-0435: depth 2 to 3, a
   leaf), and the network account counts each holder, so the chain git, `git remote-https`, `git-remote-https` failed at its third
   link with `EAGAIN` (measured). **Hooks, the editor, the pager and every other child are forked and have neither a connection nor
   the random service.** The helper's own children (`fetch-pack`, then `index-pack`) are therefore without it: git's temporary file
   names fall back to the clock, the process and a counter where `getrandom` fails (`0009`), which is safe because the file is made
   with `O_CREAT | O_EXCL` and the name need only be new. This is narrower than ADR-0339 item 5 ("the network the line gave"): a hook cannot reach
   the line's hosts. It is the safer reading until a hook that needs the network is asked for, and INV-0339-4's hook test passes
   trivially for the second host. A helper that runs in another directory (`cmd->dir`) is forked, because `libc.a` has no
   `posix_spawn_file_actions_addchdir_np`; git sets none for the helper. git's account is three connections (`GIT_NET_BUDGET`, git, the
   helper and one spare) where other programs have two. **Also found and fixed:** `libposix`'s `clone_conn` asked a file service for
   ALL and then for read alone, and the shell gives a `+w` or `create-tree` entry everything but `publish`, so a child of any program
   (a fork or a spawn) held a read-only connection to the directories its parent could write: `index-pack` in a clone could not write
   `.git` and a hook could not write the work tree. It now asks for ALL, then ALL without `publish`, then read
   (`a_child_keeps_the_write_of_an_entry_that_lacks_publish`; a child's own directory stays read-only, ADR-0341 item 7).
4. **The ceiling.** The shell sets `GIT_CEILING_DIRECTORIES` for a description with `repository`: the parent of the repository it found,
   or the parent of the starting directory when it found none (ADR-0339 item 2), replacing any value on the line. git never goes up
   into a ceiling directory (measured: the ceiling itself is not searched), so the parent is the directory its search must not
   reach. That parent is not in the program's table (`/home` above `~/proj`): to git it does not exist, and three places had to
   cope. (a) A ceiling entry that cannot be resolved is dropped by git, harmlessly. (b) `strbuf_realpath` of an absolute path lstat's every
   component and dies on a missing one (`Invalid path '/home'`, measured): `ports/patches/git/0007-*` takes a missing ancestor as the
   plain directory it is. (c) `libposix` reported the lstat of an ancestor of an entry as a refused read, and the hint said
   `+r / git status`: `Posix::refused` now gives `ENOENT` with no report for a path that lies above an entry (`crates/zoraya-posix`, host
   test `the_ancestors_of_an_entry_are_not_reported`); a sibling of an entry is still reported. This narrows INV-0153-7 ("every path no
   entry covers is reported"): the ancestors of an entry are not paths the program was refused, and a person is never told to
   give `/`. ADR-0153 rule 10's note limits the shell's own variables to `PATH` and `HOME`; ADR-0339's "Tensions" already names this one.
5. **Writing the repository.** The entry is `Tree` where the shell holds write on the directory and `Read` where it holds read only.
   The search stops at the first directory the shell holds nothing of. A `.git` that is a file or link, a repository that is the home
   directory (`/home` as the shell's `shell.home`) or `/` refuse the line with `to allow it: +w ~ git ...`.
6. **Build choices.** The second libcurl is built from the curl port's pin and patches into its own tree, library only. mbedTLS 3.6.8
   is configured for TLS 1.2 and 1.3 as a client (no server, DTLS, sockets, timing, self-test or threading, and no NEON or crypto-extension code: `zcc` has
   no `arm_neon.h`; `MBEDTLS_HAVE_ASM` stays defined and the library's portable C is what runs), with `-D__unix__` so that its Unix code (`unistd.h`, `gmtime_r`) is used, and `getrandom` as the only entropy
   source (a patch under `__zoraya__`; `/dev/random` is not in the program). `git` links no TLS library. `/etc/gitconfig` and an
   `info/exclude` template are the image's; `git-remote-https` is a copy of `git-remote-http` (no hard links).
   **The test PKI.** mbedTLS 3.6 enforces no name constraint and cannot read a certificate with a critical one (`-0x2562`); the production
   roots (108) all parse. The git test therefore has its own root (`tests/net/tls-git`) whose name constraint `permitted;DNS:.zoraya` is not critical:
   OpenSSL, rustls and any RFC 5280 verifier still enforce it, mbedTLS ignores it. Proposed text for ADR-0461 (not edited here: the architect's
   decision is accepted): "git's HTTPS test uses a test root with a non-critical name constraint, because mbedTLS cannot read a critical one".
7. **The four names** (ADR-0347): `statvfs`, `alarm`, `getnameinfo`, `execlp` are patched out (`0005`), not exported; `ports/apps/gitshim.c`
   is deleted. `git help` therefore starts no viewer and a `git://` connection prints no address.
8. **Not built in this part** (known limits, see the part's report): `git commit` with the message typed in `vi` has a script in the
   window's tests and not in the serial script; a hook's refusal to write through git's own directory is tested as far as the
   child's read-only mark reaches it; the `Expect: 100-continue` and a push over 1 MiB (`http.postBuffer`) are not exercised;
   `git submodule`, `git daemon`, `git-upload-pack` as a server and the ssh transport do not exist.

## Consequences

- **Positive.** `git clone`, `commit`, `log`, `push` work over https with the system's roots and nothing a repository or the
  environment names; the helper is the only child with a connection; each rule is a description line and a host test.
- **Negative.** Four descriptions where one might do; `-C DIR clone` is not a clone with a network; a hook has no network.
- **Follow-up.** A hook that needs the network, `addchdir_np` in `libposix`, a measurement of mbedTLS against OpenSSL.

## Invariants

- **INV-0464-1.** The form of a command is chosen by the line's first word from the image's descriptions only, and the plain
  lookup never gives a form. *Check:* `cargo test -p zoraya-shell` (`the_form_of_a_command_is_chosen_by_its_first_word`).
- **INV-0464-2.** `git clone URL` with no directory designates exactly the directory `url_dir_name` gives, and a name it cannot be
  sure of refuses the line. *Check:* `cargo test -p zoraya-shell` and `-p zoraya-describe`.
- **INV-0464-3.** Only the transport helper is started with a network connection and the random service; no other child of git has
  either. *Check:* `ports/check-early.sh` reads `ports/patches/git/0006-*` (the only `POSIX_SPAWN_GRANT_` use) and the in-QEMU script's hook
  that tries a connection (`tests/shell/apps-git.script`).
- **INV-0464-4.** `GIT_CEILING_DIRECTORIES` is the shell's value for a description with `repository`, whatever the line wrote.
  *Check:* `tests/shell/apps-git.script` (a line that sets it) and `cargo test -p zoraya-shell`.

## Open questions

- **Decisions the architect may overrule (recorded by the fix pass of the review of 10h):** (a) `libposix` gives `ENOENT` with no report for
  a path above an entry, for every POSIX program and every call, which narrows INV-0153-7 (`ls ..` fails with no `+r ..` hint); the
  alternative is to limit it to stat, lstat and access. (b) A hook has no network and no random service, narrower than ADR-0339 item 5.
- The helper is chosen by a flag that only `transport-helper.c` sets, never by a name (review finding 1); `GIT_NET_BUDGET` is 3 (git, its
  helper, one spare).

- Whether a hook may be given the line's hosts (ADR-0339 item 5 says it may; built: it may not).
- `-C DIR clone` and the other commands that name something (`git remote add`, `git worktree add`, `git bundle`): forms for them when asked.
- The `Expect: 100-continue` of a large push and `http.postBuffer`: measured when a push over 1 MiB is tested.

## Revisit triggers

- `libposix` gives a forked child a connection (ADR-0287 item 3 changes), or a hook that needs the network is asked for.
- A second command with subcommands is described (a table inside one description, Option B).

## Architect's decision

(Left empty for the human.)
