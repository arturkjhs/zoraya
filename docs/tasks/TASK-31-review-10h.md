# TASK-31 part 10h (git): independent review

Reviewed: `git diff origin/main...HEAD` on `work/10h`, HEAD `f1871d1a`. The worktree also held uncommitted edits while this
review ran (ADR-0464, `invariants.md`, patch `0006`, `apps-git.script`, `test-apps-git.sh`). They are named where they bear on
a finding, but the findings are against HEAD. Run here: `cargo test -p zoraya-shell -p zoraya-describe` (all passed),
`cargo test -p zoraya-posix --test files` (67 passed), `tools/check-dependencies.sh` (passed, 32 pins), and `shasum -a 256` of
the cached `mbedtls-3.6.8.tar.bz2` (matches the pin `bc54b818...b595b`). No QEMU was run, so no in-QEMU claim is confirmed here.

## Findings

1. **error**: `ports/patches/git/0006-transport-helper-gets-the-network.patch:35-41` (`zoraya_wants_services`). The helper is
   recognised by the basename of `cmd->args.v[0]`. For a `use_shell` child, that is the whole command string, taken from
   configuration. Scenario: a repository's `.git/config` sets `core.pager` (or `core.editor`, or a `filter.X.smudge`/`clean`
   that `.gitattributes` names, or a `!` alias) to `CMD...; :/git-remote-x`, or to any string with no `/` that starts with
   `git-remote-`. On `+net host:443 git pull`, or any paged command on a line with a `+net`, `prepare_cmd` makes `argv.v[1]`
   `/bin/sh`. That shell is then spawned with `POSIX_SPAWN_GRANT_NET | _RANDOM`: it holds a duplicate of git's connection
   and a share of the random service. `execve` in libposix starts the new image with `Shape::default()`
   (`crates/zoraya-posix/src/ffi/procs.rs:2582-2620`), so I found no way for the shell to use the connection today. Even so,
   ADR-0464 item 3 and INV-0464-3 ("no other child of git has either") are false as built: the hooks, editor and pager are not
   all forked, and the shell takes one of `GIT_NET_BUDGET`'s connections. `check-early.sh`'s grep cannot see this. Fix: mark the
   helper structurally. Add a field to `struct child_process`, set it only in `transport-helper.c:get_helper`, and require
   `!cmd->use_shell && !cmd->git_cmd`. Also require the resolved `argv.v[1]` to lie under git's exec path
   (`/usr/libexec/git-core/`). Add a test: a repository pager string ending in `/git-remote-x` must be forked, or `ps`/stats
   must show no connection.

2. **error**: `docs/adr/invariants.md:892` (INV-0464-3 "checked: ... `tools/test-apps-git.sh`"). The cited "in-QEMU script's
   hook that tries a connection (`tests/shell/apps-git.script`)" does not exist. `apps-git.script` has no hook, and
   `apps-git-hooks.script` makes no connection attempt. The hooks script also runs only when `build/rootfs-posix/bin/sh` exists,
   so it is always skipped in CI. Fix: add a hook that tries the line's host (and a second host) and expects a refusal, in a
   script that CI runs. Until then, reword the status to say what is checked.

3. **error**: `tests/shell/apps-git.script:68` (the INV-0464-4 check). `GIT_CEILING_DIRECTORIES=/home/proj/sub git status`, run
   in `/home/proj/sub`, cannot tell whether the shell replaced the value. git ignores a ceiling equal to the starting path
   (`path.c:1263-1265` requires `path[len] == '/'`), so the line's value has no effect either way and the line always prints
   "nothing to commit". Fix: write `GIT_CEILING_DIRECTORIES=/home/proj`. Honoured, it would stop git going up into `/home/proj`
   ("not a git repository"). Status `checked` at `invariants.md:893` must wait for that.

4. **error**: `user/src/bin/zosh.rs:1972` (`add_repository` takes the last `-C` only). git applies several `-C` options one after
   the other; the shell joins only the last one to the current directory. Scenario: home is a repository, the current directory
   is `/`, and a `sub` directory exists in home. `git -C ~ -C sub status`: the shell searches `/sub` and `/`, finds nothing,
   and sets the ceiling to `/`. git starts in `/home/sub`, goes up to `/home`, finds the home repository, and runs with what that
   repository's configuration names (fsmonitor, pager), using the `read` entry on `~` from the operand. INV-0339-1 says this
   line is refused and git is not started. Fix: refuse a second `-C`, or compose the values as git does
   (`join(join(cwd, a), b)`). Add a host test with two `-C`.

5. **should-fix**: `tests/shell/apps-git.script:129-149` and `invariants.md` (INV-0461-2, which the worktree flips to
   checked). No server presents a certificate from a root that is not in `/system/tls/roots`: the expired and wrong-name leaves
   are made by the trusted test root. INV-0461-2's "not made by a root of /system/tls/roots" is therefore not tested.
   `GIT_SSL_CAINFO=/etc/passwd` against the expired server shows nothing, because that handshake fails anyway. Fix: add a
   listener with `tests/net/tls/valid.pem`, whose root is not on the git disk, and expect a refusal. Point `GIT_SSL_CAINFO` and
   `http.sslCAInfo` at that root against that listener, and expect a refusal there too.

6. **should-fix**: `crates/zoraya-posix/src/process.rs:680-686`. The ancestor rule (ENOENT with no report) applies to every
   POSIX program and to every kind of access. Scenario: `ls ..` or `opendir("..")` from a current-directory entry now fails
   silently, where before it gave the hint `+r ..`. The accepted INV-0153-7 row (`invariants.md:304`) still states the full
   rule. This is not an oracle: the errno is ENOENT whether the path exists or not. Fix: limit the rule to metadata queries
   (`stat`/`lstat`/`access`/`realpath`'s walk) and keep the report for `open`/`opendir`. Alternatively, note the narrowing in
   INV-0153-7's status, pending ADR-0464.

7. **should-fix**: `crates/zoraya-shell/src/grants.rs` (`Kind::CreateTree` → `Access::CreateTree`, ALL & !PUBLISH). A
   create-tree operand that names an existing directory gives full write on it. `git init ~` gives git ALL & !PUBLISH on the
   whole home directory without `+w`, and makes home a repository. That is the case the repository rule refuses
   (ADR-0339 item 2). Fix: refuse a create-tree target that is the home directory, `/` or an ancestor of home, as `find` does.
   Record in ADR-0464 that an existing directory is given whole.

8. **should-fix**: ADR-0464 and `docs/design.md`, accuracy.
   - (a) Item 6 says no assembly, but `MBEDTLS_HAVE_ASM` is defined (`mbedtls_config.h:52`) and is not in `MBEDTLS_UNSET`.
   - (b) `design.md:2486` says the ceiling is "the repository's root (or the starting directory)"; the code sets the parent.
   - (c) The sizes "6,191,000" and "6,708,472" do not match the build tree (6,193,088 and 6,710,560 bytes). The first is a
     rounded figure presented as measured.
   - (d) `design.md` points to `docs/tasks/TASK-31-part-10h.md`, which does not exist at HEAD.
   - (e) At HEAD, `GIT_NET_BUDGET = 3` is in no ADR, and its comment contradicts itself (`zosh.rs:77-79`: "two were enough: they
     are", then 3). The worktree's ADR edit adds the budget.
   - (f) The test-PKI exception is documented only in `tests/net/tls-git/README.md`. It should also be in ADR-0464 or in
     ADR-0461's open questions: mbedTLS enforces no name constraint at all, and refuses to load a root with a critical one. The
     README's "108 certificates all parse under mbedTLS" names no command or check.

9. **note**: patch `0006` (as of HEAD), EACCES fallback. A refusal of the random share alone (ADR-0435 depth) also lands in the
   fallback, which starts the helper without a network although git has one. The person then sees a network refusal that is
   wrong. The worktree's `{NET|RANDOM, RANDOM}` changes the shape but not the conflation.

10. **note**: `git-clone.cmd`, `git-init.cmd` and `git-ls-remote.cmd` carry no `repository` key, so they get no ceiling.
    `ls-remote` is RUN_SETUP_GENTLY: it searches up from the current directory and reads the configuration of the repository it
    finds (`url.*.insteadOf`). This is bounded by git's table; record it.

11. **note**: under `Held::Root`, any directory except home and `/` can become a writable repository, including `/data` and
    `/data/home/ssh` (other programs' own directories). ADR-0339 names only home and `/`, but the architect should see this.

12. **note**: `git.cmd:11` lists `--directory` as `-C`'s long form. git has no such option, yet the shell takes it as `-C`
    (start of the search, a read entry) and git then fails. Drop it.

13. **note**: coverage.
    - INV-0339-4 lacks the second host, the path outside the repository, and the `.git/config` link followed by `git config`.
    - INV-0341-6 lacks the raw request.
    - INV-0339-5 lacks `GIT_SSH_COMMAND`, `credential.helper`, `core.askPass` and `SSH_ASKPASS`.
    - The `git clone git://...` line (`apps-git.script:149`) asserts nothing.
    - The hooks script runs only locally, after `make ports`.
    - `check-early.sh`'s "no other program links mbedTLS" list leaves out netsurf, python and the rest of the image.

14. **note**: `tools/test-map.txt` lacks `tests/shell/apps-git-hooks.script` and `tests/net/tls-git/`. Because the part changes
    `tools/test-map.txt`, CLAUDE.md requires a candidate run, which the architect tags.

15. **note**: patch `0007` is wider than its comment. It drops `REALPATH_MANY_MISSING` for every caller and every missing
    component, not only ancestors above an entry. The risk is low, because libposix still resolves links.

## Checked and sound

- `repository::find`: nearest-first search, stopping at the table's edge, Tree/Read by the shell's rights, refusal for a `.git`
  file, home or `/`.
- zosh's probe: the file service never follows a link and answers IS_LINK, so a `.git` symlink, or a link anywhere on the path,
  refuses the line; it fails closed.
- `derive`/`url_dir_name`: `..`, hidden and empty names are refused, the charset is plain, a name from a URL in a value cannot
  leave the current directory.
- Form selection uses the image's list only. A `-C` before `clone` selects the program's own description and gets no network.
- Unlisted global options (`--git-dir`, `--work-tree`, `-c`, `--exec-path`) are refused by `userland_options`. The shell sets
  `PATH`. `GIT_EXEC_PATH`, `GIT_TEMPLATE_DIR` and `GIT_CONFIG*` are not read. The line's `GIT_CEILING_DIRECTORIES` is replaced.
- `clone_conn`: no widening. The file service caps a clone at the source's rights, and the own directory still goes through
  `clone_read_only` on both the fork and the spawn path (`jobs.rs:1212-1215`, `1324-1325`).
- Patch `0006`'s descriptors and signals (CLOEXEC on the parent's pipe ends and the notify pipe, the old mask restored) match
  the fork path.
- Patch `0009`: `git_mkstemps_mode` opens with `O_CREAT|O_EXCL`, so predictable names allow only a bounded DoS.
- mbedTLS: entropy from `getrandom` only. The second libcurl is library-only, with `--with-ca-bundle` and no proxy, netrc or
  cookies.
- `test-httpd`'s CGI: the environment is set through `Command` with no shell, `..` is refused, bodies are capped, and it listens
  on 127.0.0.1 only.
- `sources.lock` pin and hash, `dependencies.md` rows, `check-dependencies.sh`.

## Verdict

Not ready to merge. Findings 1-4 must be fixed first: the helper selection by name, the INV-0464-3 and INV-0464-4 checks that
do not exist or cannot fail, and the second `-C`. Findings 5-8 should be fixed before INV-0461-2, INV-0339-x and INV-0464-x are
set to checked.

## Dispositions (changed-lines check of the fix pass)

Checked: `git diff f1871d1a..HEAD` (HEAD `c5e856f9`) plus the uncommitted trap change in `tools/test-apps-git.sh`. Run here:
the nine git patches applied in order to a fresh `git-2.56.1` from the cached tarball (`patch -p1`, all clean);
`cargo test -p zoraya-shell` (251 passed, among them `a_second_c_and_a_create_tree_over_home_are_refused`). No QEMU and no port
build were run, so no in-QEMU claim below is confirmed. The part report's `PASS: 139 steps` predates these fixes and the changed script.

- **1 (error): fixed in the code, its check is broken.** `zoraya_helper` is set only in `transport-helper.c:get_helper`, zeroed by
  `child_process_init`, and `zoraya_wants_services` requires it with `!use_shell && !git_cmd`. `zoraya_spawn` refuses an
  `argv.v[1]` outside `/usr/libexec/git-core/git-remote-`; the `-2` return falls through to `fork()` (`failed_errno` is then
  fork's), so the fallback fails closed. A helper name from `remote.X.vcs` with a `/` is not looked up in `PATH` and so cannot
  match the prefix; one without a `/` resolves only to a file in the exec path, where the image installs only `git-remote-http`
  and `-https`. Sound. The test the finding asked for (a pager string that imitates the name) was not added. New problems: R1, R2, R5.
- **2 (error): reworded, not fixed.** INV-0464-3 now says "partly". Its status still says "the helper is chosen by name, to be
  tightened", which is false after the fix (R5), and it cites `make check-ports`, which now fails (R1).
- **3 (error): fixed in the script.** `GIT_CEILING_DIRECTORIES=/home/proj` from `/home/proj/sub` would stop git before
  `/home/proj` if git honoured it. The status `checked` waits for a run, which has not happened since the change.
- **4 (error): fixed.** `Parsed::options` keeps every occurrence in order, so the count sees `-C a -C b`, and `-Ca` too.
  The host test covers only `one_directory(n)`, not zosh's counting, which no host test can reach.
- **5 (should-fix): partly fixed.** The second server (`tests/net/tls`, whose root the git disk does not hold: line 68 of
  `test-apps-git.sh` overwrites `/system/tls/roots`) is started, forwarded, substituted, and its log is checked. But
  `GIT_SSL_CAINFO` and `http.sslCAInfo` point at `/etc/passwd`, not at that server's root, so the line cannot show that a named
  root is ignored: the handshake fails whether libcurl reads `/etc/passwd` or not. No later https line succeeds while
  `http.sslCAInfo` is set either. See R3.
- **6: recorded, not changed.** It is in ADR-0464's open questions for the architect. Acceptable as a disposition.
- **7 (should-fix): fixed, with a false hint.** `create_tree_target` rebuilds the absolute path (`join(parent, 0, name)`, where
  `name` is one component of a normalised path), and `covers(target, home)` refuses home and every ancestor. `/` gives no entry
  at all (`parent_and_name`). Only `git-clone.cmd` and `git-init.cmd` use `create-tree`, so no other program changes. See R4.
- **8 (should-fix): mostly fixed.** (a), (b), (c), (e) and (f) are corrected. (d): the report exists now, but its lines 47-48 say "host
  test still to add" and "findings 5 to 15 ... are **open**", which step 7 made false.

New findings in the fix pass:

- **R1 (error)** `ports/check-early.sh:79-80`. `grep -l zoraya_helper ./*.c ./*.h` lists the `.c` glob first and gives
  `./run-command.c ./transport-helper.c ./run-command.h `. The check expects `./run-command.c ./run-command.h ./transport-helper.c `.
  Measured on the patched tree, both on the fresh one and on `build/ports/work/git-2.56.1`. Scenario: every
  `make check-ports`/`make apps` run reports INV-0464-3 broken and exits non-zero. Fix: compare a sorted list (`| sort | tr`),
  or reorder the expected string to match. Then run `sh ports/check-early.sh`.
- **R2 (warning)** `ports/check-early.sh:81`. `grep -q 'cmd->use_shell' run-command.c` holds for upstream git (2 hits in
  `prepare_cmd` and elsewhere), so this half of the check cannot fail. Fix: grep for the patched condition itself
  (`!cmd->zoraya_helper || cmd->use_shell || cmd->git_cmd`).
- **R3 (warning)** `tests/shell/apps-git.script:141-147`. As in 5: put `tests/net/tls/root.pem` on the git disk at a readable
  path, outside `/system/tls/roots`. Name it with `GIT_SSL_CAINFO` and `http.sslCAInfo` against `@TLSOTHER@`, and expect the
  refusal. Or clone from `@TLSVALID@` successfully while `http.sslCAInfo=/etc/passwd` is set.
- **R4 (warning)** `crates/zoraya-shell/src/repository.rs:153`. The hint `to allow it: +w {target} COMMAND` cannot be
  followed: `+w` adds a `Tree` entry, and the `CreateTree` entry from the operand stays in the plan. The same line with `+w ~`
  is refused again. Home as a repository is refused by ADR-0339 anyway. Fix: give no hint, or one that names a directory below
  home.
- **R5 (warning)** `docs/adr/invariants.md:892`. INV-0464-3's status still cites finding 1 as open ("chosen by name, to be
  tightened"). Also `docs/tasks/TASK-31-part-10h.md:47-48`, as in 8(d). Fix: restate what is now checked.
- **R6 (warning)** `tools/test-apps-git.sh:56`. In the committed step 7, the EXIT trap runs `wait $httpd` under `set -e` on a killed
  server: its status 143 ends the trap, and the script exits 143 even after a pass. The `|| true` added afterwards (committed as
  `05b90626` while this check ran) fixes that. `kill $httpd $httpd2` without `|| true` still ends the trap with status 1 if either server already exited (its
  `--lifetime`, or a crash), which turns a pass into a failure. Fix: `kill ... 2>/dev/null || true`.
- **R7 (note)** `ports/patches/git/0006`, the `run-command.h` hunk. The upstream comment `/* if this is to be git sub-command */`
  moved from `git_cmd` to `zoraya_helper`. The comment on `zoraya_spawn` ("the pid, or -1 with errno") leaves out `-2`.
- Still open from the review: INV-0339-4 is `checked` through `apps-git-hooks.script`, which CI skips (no `build/rootfs-posix`),
  and notes 9-15.

Verdict on the fix pass: findings 3, 4 and 7 are fixed in the code. Finding 1 is fixed in git, but its check (R1) fails on every
ports build. Fix R1 and the `kill` of R6, then rerun `tools/test-apps-git.sh`, before merging.
