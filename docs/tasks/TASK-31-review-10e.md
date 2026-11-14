# TASK-31: review of 10e, the userland (branch `work/10e`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...work/10e` at `fe3c81c` (base `0c3ada9`): the 56 descriptions of `user/describe/posix/`, the
planner change in `crates/zoraya-shell/src/grants.rs` and its tests, `posix_page` in `user/src/bin/zosh.rs`, the five
`libposix` names in `crates/zoraya-posix/src/ffi.rs`, `ports/Makefile`, `ports/check-userland.sh`, toybox patch 0002,
`tools/posix-surface-check`, `tools/zoraya-elf-check`, the lint `crates/zoraya-describe/tests/posix_names.rs`, both
POSIX scripts, ADR-0343 and ADR-0344 (proposed). Read with the code around it: `zoraya-command::parse`, `grants::plan`,
`through_links`, `merge`, zosh's `plan_with` and the entry table (`zosh.rs:1700-1740`), the file service's narrowed
connections (`zoraya-fs-service/src/lib.rs`: `open`, `set_attributes`, `clone_connection`), toybox's `patch.c`,
`sed.c` and `find.c` in the unpacked tree. Against ADR-0336 (items 1-10, INV-0336-1..6), ADR-0342 (INV-0342-1, -4,
-5), ADR-0153 rules 2, 5 and 10, ADR-0311 item 5 and plan row 10e.

What was run here, in `zoraya-10e` at `fe3c81c`:

- `make check`: all checks passed (exit 0).
- `make test-posix POSIX_SCRIPTS=tests/posix/zosh-userland.script POSIX_ACCELS=auto`: `PASS: 124 steps` (HVF); the C
  test printed `link -1 1 mknod -1 1 pw 1 utf8 4 3 20ac`.
- `make test-posix POSIX_SCRIPTS=tests/posix/zosh.script POSIX_ACCELS=auto`: **FAIL** (finding 1).
- The host's `/bin/dash` and `/bin/sh` on GNU hello's `build-aux/config.sub sun4`: both print `sparc-sun-sunos4.1.1`
  (finding 15).

Not run: `make check-ports` from a clean tree, W4, TCG, toybox's own tests. The review of `tr`, `expr` and `diff`
that ADR-0336 item 2 asks for is not done here either (finding 3).

## Findings

1. **error (must-fix).** `tests/posix/zosh.script:39`. **What.** The edit changed `toybox cat ../hello.txt ...` to
   `/bin/cat ../hello.txt ...`. `/bin/cat` now has a trusted description, so `../hello.txt` is a designated `read`
   file outside every directory entry, and zosh refuses the line before it runs (`zosh.rs:1722-1736`, ADR-0153
   rule 10). Run here: `zosh: /bin/cat: /hello.txt: a POSIX program is given directories only`, then `FAIL: timed out
   waiting for "cat: missing.txt: No such file or directory"`. Nothing after that step ran, so the edited Ctrl-Z steps
   (`/bin/sleep 30`) and the rest of the script are unverified. The report says the script "was not re-run". INV-0153-7
   ("libposix's half from zosh") is the check that broke, and it needs an *undescribed* program by definition.
   **Scenario.** The candidate run's smoke set or the nightly fails on `posix-zosh`, or someone "fixes" it by adding
   `+r /`, and the test stops checking INV-0153-7. **Fix.** Use the undescribed binary for that step
   (`/usr/libexec/toybox cat ../hello.txt ../hello.txt missing.txt`, hint `+r / /usr/libexec/toybox`), and run the
   whole script before merging.

2. **error (must-fix).** `user/describe/posix/cp.cmd:15-17`. **What.** `when -r FROM tree` (and `-R`, `-a`) gives the
   *source* of a copy `Kind::Tree`, which plans as `Access::Tree` = `rights::ALL & !PUBLISH` (`grants.rs:58`,
   `entries_for` at `grants.rs:164`), the rights of `+w`: write, create, remove and rename everything under FROM.
   Copying needs read, and `Kind::Read` on a directory already gives the tree to read. The page's own words say
   "With -r, -R or -a FROM is a tree to read", and `grants` shows "anything in it changed". This is the most-used form
   of the command and a security statement wider than the tool needs (ADR-0336 item 5, CP-05). **Scenario.** An agent
   types `/bin/cp -r ~/proj /tmp/backup` (or a script line does); the line gives `/bin/cp` the right to change or
   remove all of `~/proj` without any `+w`. **Fix.** Delete the three `when` lines (FROM stays `read`). Note that
   `cp -r` still does not work after that (finding 9), which is a reason to say so on the page, not to widen FROM.

3. **error (must-fix).** `ports/toybox-applets.txt:61-63` (`tr`, `expr`, `diff`), the file's header comment and
   the part report ("Not done"). **What.** ADR-0336 item 2 (accepted): "Each is enabled only when a reviewer who did
   not port it has read it **and** it passes toybox's own tests for it and the stage 6 workloads". The three are
   enabled and busybox is removed, while the review has not happened (the report hands it on) and toybox's own
   tests (`tests/tr.test`, `tests/expr.test`, `tests/diff.test`) were not run; only W5 and W10 ran. `expr` has no
   workload at all. The header comment says they are admitted "after the review in docs/tasks/TASK-31-part-10e.md",
   which contains no review. `check-userland.sh` admits them by name, so INV-0336-1's check passes for a gate that was
   not met. **Scenario.** The stage merges with three unreviewed `pending` programs in `/bin`, each with a
   description that tells the person it is the POSIX tool; ADR-0336 rests "three commands on a review of unreviewed
   upstream code" and that review never happens. **Fix.** Before merge: a review of `toys/pending/{tr,expr,diff}.c`
   by a session that did not port them (a record next to this one), and toybox's test files for the three run (on
   the host build or in QEMU), with results in the report; or keep busybox's two until then and leave `expr` out.

4. **warning (should-fix).** `user/describe/posix/*.cmd` (every `Lets through:` line), `docs/design.md` ("lets through
   only the options the description lists"), ADR-0344 item 4, against `zosh.rs:1050-1056`. **What.** An option the
   description does not list is not refused: `zoraya_command::parse` fails, `plan_with` takes
   `Parsed::default()`, and the program runs with the current directory and no designations. The QEMU script shows it:
   `/bin/find . -name t.c -exec ...` runs although `-n` is no option of `find.cmd`. The security consequence is benign
   (an unparsed line designates nothing), but the words are false, and for four descriptions (`find`, `test`, `expr`,
   `env` with `-` words) the description is never used, because every real expression starts with `-`: `find src
   -name x` from another directory gets no `src` and fails, with no hint that the line went undescribed. `find.cmd`
   says "the shell is to hand words starting with - after PATH to find as operands", a feature that does not exist.
   **Scenario.** The architect reads the appendix as "what each tool may be told" and accepts ADR-0344 on it; a
   person reads `help find` and believes `find ~/src -name '*.c'` is designated. **Fix.** Reword `Lets through` to
   "Designated with: ..." and say on each page that another option runs the program with the current directory only;
   in `find.cmd` and `test.cmd` say that today every expression runs undescribed. Record the gap (a rule for words
   after the operands) as ADR-0344's open question, not as present behaviour.

5. **warning (should-fix).** `user/describe/posix/tar.cmd:12-13`. **What.** `-f ARCHIVE create` in all modes and
   `-C DIR tree` also with `-c`. ADR-0336 item 5 (accepted) says `tar -x -f ARCHIVE -C DIR` *reads* the archive, and
   gives DIR as a tree to extract into, not to create from. So `tar -t -f X` and `tar -x -f X` are granted to create or
   replace X, and `tar -c -f a.tar -C src .` gets `+w src`. That is wider than the decision allows; ADR-0344 records it
   as "a loss", but a proposed ADR cannot widen an accepted one's grants. **Scenario.** `tar -t -f ~/thesis.pdf` (a
   typo, or an agent probing) gives the right to replace the file; `tar -c -z -f out.tgz -C ~/proj .` gives write to
   all of `~/proj`. **Fix.** Either describe `-f` as `read` and creation through standard output (`tar -c -z . >
   out.tgz`, which the redirection designates) and `-C` as `read`, accepting that `-x -C DIR` then needs `+w DIR`; or
   add per-mode option kinds (`when -c -f create`) to `zoraya-describe` with a test. Until then, the narrower form.

6. **warning (should-fix).** `crates/zoraya-shell/src/grants.rs:225-228` against the new block at `:255-270`.
   **What.** Review S16's rule ("a kind that narrows the parent needs the path strictly below", for operands from a
   value) lists `Create | Remove | RemoveTree | Rename`. A `cwd none` `read`/`write` operand now narrows the parent
   too, but is not in `narrows`, so a non-literal operand equal to the current directory passes the inside check and
   is planned as the *parent* of the current directory narrowed to its name, with `WRITE` for `-i`. The file service
   limits a narrowed directory name to read-only opens (`fs-service/src/lib.rs:1912-1925`), so the effect today is
   read of the current directory, but the connection's own object is the parent, and with `WRITE` a raw
   `SetAttributes` (`lib.rs:2608`) acts on the parent (finding 13). **Scenario.** `d=.; sed -i s/a/b/ $d` from
   `~/proj` plans `~` narrowed to `proj` with `READ|WRITE|SYNC`, an entry on a directory the line never named.
   **Fix.** Add `(d.cwd == Cwd::None && matches!(kind, Kind::Read | Kind::Write))` to `narrows`, and a host test with
   a value operand naming the current directory (refused) and one naming a file in it (allowed).

7. **warning (should-fix).** `crates/zoraya-shell/src/grants.rs:255-270` with `through_links` (`:355`).
   **What.** ADR-0153 rule 5: a designated path that is a link object becomes its target when the target is inside
   the current directory or a grant, else the line is refused with a hint. `through_links` resolves `e.path`, which
   for the new narrowed entries is the *parent*; the file's own name is never checked. Before this change a `read`
   operand of `sed` was planned at the file's own path and followed. **Scenario.** `notes.txt -> drafts/notes.txt`
   inside the current directory: `sed -i s/a/b/ notes.txt` used to be planned at the target; now it gets the parent
   narrowed to the link's name, the client follows the link to `drafts/notes.txt`, which no entry covers, and `sed`
   fails with a plain `Permission denied` and no hint. A link to a file outside, which used to be refused with the
   hint "put +r ... before the command", now runs and fails without it. (It fails closed: the service allows only the
   one name.) **Fix.** For a `Name` access, resolve `resolve(parent, name)` in `through_links` and narrow to the
   target's own parent and name; host tests for a link inside, outside, and a dangling one. The same gap exists for
   `Create` names (pre-existing); one fix covers both.

8. **warning (should-fix).** INV-0336-6's checks, `grants_tests.rs:793-827`, `zosh-userland.script:28-46`.
   **What.** The invariant says "in the current directory **and outside it**" and its check is "a host test of the
   shell's plan for both cases, and an in-QEMU script **comparing the directory listings**". The host test has the
   file in the current directory only (`cwd "/w"`, operand `notes.txt`); nothing tests `/elsewhere/notes.txt`,
   `sed -i x .`, a directory operand, or an operand from a value. The script runs `ls` once after the edit and looks
   for `other.txt` and for no `toybox` (the temporary file's prefix); it does not compare a listing before and after.
   The fallback test (`:818-827`) uses `.all()` over `lines`, which is true for an empty list, and does not assert
   that the fallback rights lack `CREATE` and `REMOVE`. The checks would catch the main regression (both asserts on
   `name` fail if the new block is removed), so they are real, but they cover half the invariant. INV-0344-1 says
   "never as the current directory" and its test asserts that only for the read case. **Fix.** Host cases: a literal
   file outside the current directory (`/x/notes.txt` with cwd `/w`: entry `/x` narrowed, no `/w` entry), `.` and a
   value; in the fallback test assert `lines` is non-empty and `rights & (CREATE|REMOVE|RENAME) == 0`. In the script,
   `ls` before and after `sed -i` on a file in another directory, given literally, and compare.

9. **warning (should-fix).** Descriptions that cannot do what their usage line promises: `cp.cmd` (`-r`), `gzip.cmd`,
   `gunzip.cmd` (without `-c`), `mkdir.cmd` (`-p`), by reading. **What.** `TO create` is the parent narrowed to one
   name. A narrowed directory name opens read-only (`fs-service/src/lib.rs:1912-1925`), so `cp -r src backup` can
   make `backup` and cannot create anything in it, although `cp.cmd`'s own example is that line. `gzip FILE` (`write`
   = Change on FILE alone) has to create `FILE.gz` and remove `FILE`, neither of which it is given, so `gzip`
   and `gunzip` without `-c` always fail. `mkdir -p a/b/c` creates `c` in an `a/b` that does not exist. None of these
   was run. ADR-0336 driver 3 is "every tool can be used as the shell means it to be"; a page that always fails pushes
   the person to `+w .`, which is the opposite of the userland's point. **Fix.** Run each example of the 56 pages once
   in a QEMU script (this would have found finding 1's sibling too); for those that cannot work, remove the example,
   say on the page what the line needs (`+w DIR`), or (for `gzip`) describe only `-c` and `-d -c`.

10. **warning (should-fix).** `ports/Makefile:368` and `:1123`. **What.** INV-0109-2/INV-0143-1's check ("no port
    was compiled with `__linux__`") greps each port's log for its compile commands. awk's compile runs inside `sh -c`
    without echo, so `build/ports/logs/awk.log` is empty (0 bytes here), and `awk` is not in the loop's list, so the
    new port escapes the check. **Fix.** Echo the commands (`sh -xc`, or `set -x` in the loop) and add `awk` to the
    list. On the incremental-tree failure the report mentions: it is real and pre-existing. Here `dash.log`,
    `pdpmake.log`, `tcc.log`, `libtcc1.log` hold 0 compile lines (`dash.log` is `Making all in src`), because a recipe
    re-run by a changed `SYSROOT_STAMP` overwrites its log with a no-op make. It is not caused by 10e, but 10e changed
    `libc.a`, and this also shows those ports' recipes do not relink against the new `libc.a`. Worth a backlog line
    (keep the compile log separate from the relink, or make the check read the newest log that has compile lines).

11. **warning (should-fix).** `ports/Makefile:366-367`, `docs/dependencies.md`. **What.** awk's parser is generated
    by whatever `bison` is first on the host's PATH ("GNU Bison 2.3 (Apple's) or later"). Locally that is Apple's 2.3
    from 2006, in CI the distribution's 3.8. The generated `awkgram.tab.c` differs between the two, so the program in
    the image depends on the host. `sources.lock` already pins `bison 3.8.2` (9d builds it for NetSurf). **Fix.** Use
    the pinned host bison the ports already build, or record bison's version in `awk-grammar.log` and accept the
    difference in the ADR. A note, if the architect accepts host variance.

12. **warning (should-fix).** ADR-0343 and ADR-0344 contradict what was built. ADR-0343 Option A (the recommendation)
    says `fchdir` and `readv` "are built over what libposix has", and item 3 defines `fchdir`'s meaning, but `fchdir`
    is not exported and patch 0002 turns it into `ENOSYS` (that is Option B, which the ADR rejects). Item 4 says `stat
    -f` becomes an error "no file system status on Zoraya", but the patch makes `statfs` return `ENOSYS`. The context
    says "these six names" for a table of 16. ADR-0344 item 1 says the links are relative (`../usr/libexec/toybox`),
    and they are absolute (`ports/Makefile`, the report). ADR-0344's open question asks whether awk's names need a
    block, and ADR-0343's block already has them. **Scenario.** The architect accepts ADR-0343 "as recommended" and so
    decides an `fchdir` that does not exist. **Fix.** Make ADR-0343's recommendation say what was done (Option B for
    `fchdir`, `statfs`, `pathconf`; A for the rest), fix item 4 and ADR-0344 items 1 and 7 and the open question.
    Separately, say why a relative link was "not followed by zosh's lookup": that is a zosh bug in its own right
    (finding 14).

13. **note.** `crates/zoraya-fs-service/src/lib.rs:2608-2626` (pre-existing, widened by 10e). `set_attributes` acts on
    the connection's own object when it has `WRITE`, with no check for a narrowed connection. A narrowed `Create` or
    `ChangeName` connection's object is the *parent* directory, so a raw `SetAttributes` (or a `Clone`, then
    `SetAttributes` on the clone) changes the parent's mode and time. `libposix` does not send this for a path (its
    `chmod` opens by name, which `may_name` refuses), and every program given such an entry today is a trusted image
    binary that does not exec others, so this is not reachable now. But 10e adds `WRITE` narrowed entries for every
    `cwd none` file operand, and INV-0153-2 says a narrowed connection gives its one name. **Fix (later, with an
    owner):** refuse `SetAttributes` (and `set_limit`) on a connection with `only` set, and add a host test in
    `zoraya-fs-service`.

14. **note.** `tools/zoraya-elf-check/src/main.rs:30`. `path.ancestors().skip(1).any(|a| a.join(inside).is_file())`
    includes the host's own `/`, so a link in the image to `/usr/bin/env` or any other path that exists on the build
    host passes as "found under the image's root". **Fix.** Take the image root as an argument (or stop at the walk's
    starting directory) and resolve only below it. A relative link that zosh does not follow (the reason the links
    are absolute) is a lookup bug that the absolute links hide. It belongs in the backlog with its evidence.

15. **note.** W4 / GNU hello `config.sub sun4` (the report's "cause unknown"). On the host the same file prints
    `sparc-sun-sunos4.1.1` under `/bin/dash` and `/bin/sh`. By reading `config.sub` (timestamp 2025-07-10): for `$1 =
    sun4`, `case $1 in` takes `*)` and the alias table is `case $field1 in ... sun4) basic_machine=sparc-sun`. The only
    way to end in `cpu=sun4 vendor=unknown` is that alias table's default `basic_machine=$1`, so `$field1` was **not**
    `sun4`. `field1` comes from `IFS="-" read field1 field2 field3 field4 <<EOF` (`config.sub:123-126`). So on Zoraya,
    dash's builtin `read` from a here-document in the shell's own process (no fork, unlike configure's `cat
    <<_ACEOF`, which works) returned nothing, or did not split. This is a dash/libposix fault, not a toybox one, and it
    is cheap to confirm: `IFS=- read a b <<EOF` / `x-y` / `EOF` / `echo "$a/$b"` in a POSIX script. Not run here.

16. **note.** Untested `libposix` additions. `lchown`, `settimeofday` and `readv` (`ffi.rs:1754`, `:2386-2413`)
    have no host test and no QEMU step; `link` and `mknod(S_IFCHR)` are covered by the C test. `readv` reuses
    `vectors` and the `writev` pattern and is sound by reading (count limited to 0..=1024, the total checked, NULL
    refused), and its `// SAFETY:` comment ("C's contract for readv") is thin but the contract is `vectors`'s. Add a
    host test in `crates/zoraya-posix/tests/` for `readv` over two buffers with a short read, `lchown` to self and to
    another uid (`EPERM`), and `settimeofday` (`EPERM`).

17. **note.** Toybox patch 0002, `replace_tempfile`. The original is truncated and then refilled from the copy in
    `/tmp`. If `xsendfile` fails half-way (quota, `ENOSPC`, an I/O error) `error_exit` runs the exit handler, which
    still has `tempfile2zap` set to the temporary name and unlinks the only complete copy. ADR-0336 item 6 accepts
    "an interrupted one can leave the file short"; deleting the copy is more than that. The line's `/tmp` goes at the
    line's end anyway, so the copy could only be kept for the person to see if the error says where it is. **Fix:** set
    `tempfile2zap = (char *)1` before the `O_TRUNC` open, and on a failed copy-back say "the edited text is in
    /tmp/toybox-XXXX until this line ends". Also: `sed -i` on a link now writes through it, where upstream (and GNU)
    replace the link with a file. Say so on `sed`'s page.

18. **note.** Lesser check gaps. INV-0336-4 is checked on `ls` alone. That is adequate for the mechanism
    (`shell.path` order), but the invariant's "sh the toybox one" is false for `echo`, `test`, `printf` and `true`,
    which are dash builtins. Say so in ADR-0336's note or the script comment. INV-0336-5 is checked for `find -exec`
    only (not `xargs` or `env`). They share `posix_spawn`'s table, so a note is enough. The script runs `find`
    *undescribed* (finding 4), so the check holds for "find's entries", whatever they are. INV-0342-4's check is named
    "a C test in the libc conformance step" and lives in the POSIX script: update the invariant's check text or move
    the test. The INV-0336-2 lint's path rule matches only the argument names `FILE|DIR|ARCHIVE|PATH`, so
    `PROGFILE`, `REFFILE`, `PATCHFILE` and `dd`'s `if=FILE` would pass as `text`. Today they are `read` (and `dd` is
    explained), but the lint would not catch a slip. A list of names in the test is enough. `toybox`'s `.config` also
    enables `egrep`, `fgrep` and `[` (OLDTOY). They have no link in `/bin`, so INV-0336-1 holds, but
    `/usr/libexec/toybox egrep` exists; a note in the list's header.

19. **note.** Deviations recorded but not decided. The ADR-0212 image cache is skipped, though the architect ordered
    it first on 2026-11-04 (ADR-0336 item 10(a), plan row 10e "the image cache if it is not there"). W4 (89 % of wall
    time loading programs) shows the cost. Deferring an architect's order needs the architect's word, not only
    proposed ADR-0344 item 8: put it at the top of the report's "For the architect". `narrow` with a list of names is
    left to 10d, which the plan allows ("built here or in 10d, whichever comes first"). ADR-0344 item 2 says that until
    then `sed` with two files of one directory "sees that directory (not its subdirectories)". That is wrong: a
    directory connection's rights reach every name below it (as `Kind::Read` on a directory always has), so the
    fallback gives read and **write to every file in the subtree**. The `grants` words "read and change files in"
    should say "anything in" like the other fallbacks. Also unrecorded: every described POSIX program other than
    `sed` refuses a file operand outside the current directory (`grep x ../f.c`: "a POSIX program is given directories
    only"), because only `cwd none` gets narrowed names. That is ADR-0153 rule 10, but it is new for the 50 commands
    that were undescribed before, and `docs/design.md`'s "Known limits" should say it.

## What was checked and found sound

- The planner's narrowed delivery for `sed`: one file in the current directory gives the parent narrowed to the name
  with `READ|CLONE` or `READ|WRITE|SYNC|CLONE` and no `CREATE`, `REMOVE` or `RENAME`. The service enforces the name on
  `open`, `remove`, `make_link`, `rename` and `narrow`, and opens a directory of that name read-only, so a directory
  operand gives no more than the old `Read`/`Change` did. Two files in one directory fall back to the union of those
  rights. The fallback test would fail if the merge kept a name. `grants` shows `change notes.txt (operand FILE)`, as
  run.
- `sed`'s description: no option names a file. `-i` changes only the operands' kind. `w`/`r` commands in a script
  reach nothing (run: `r other.txt` cannot read the sibling). `xargs`, `env` and `find` give no path kinds to their
  command words, so a child holds only the parent's entries.
- `link` returns `EPERM` whatever the names. `mknod` goes through `mknodat`, which creates only a FIFO and returns
  `EPERM` for every other type, regular files included. `lchown` is `fchownat(..., AT_SYMLINK_NOFOLLOW)` with
  `chown_ok`. `settimeofday` returns `EPERM`. The `// SAFETY:` comments are present.
- `posix_page` matches by the description's `program` path only, so `help` cannot be pointed at an untrusted page.
  `/usr/libexec/toybox` run by path is undescribed, as ADR-0336 item 8 wants.
- INV-0336-1 (`check-userland.sh`): the list is compared with `.config`'s NEWTOY commands and the source directory of
  each. It would fail on an extra or missing command or a `pending` one other than the three. INV-0336-3: busybox is
  gone from `sources.lock`, the image, the Makefile and `docs/dependencies.md`. The copyleft check reads README's
  rows. INV-0342-1's second half (`posix-surface-check imports`) reads undefined symbols of the program's own
  objects, so a block line no program imports fails. The `awk=` tree is passed (`$(AWK)/objs`).
- The new libc exceptions (`sigsetjmp.s`, `sigsetjmp_tail.c`) are musl's own code, and the block names `sigsetjmp`.

## Recommendation

Do not merge yet. Findings 1-3 block: a test the branch broke and did not run, a description that gives `+w` on the
source of every recursive copy, and an accepted ADR's gate for the three `pending` commands that is not met. Findings
4-9 should be fixed in this part, because they are what the descriptions claim as security statements. Findings 10-12
are small, and the notes can go to the backlog.

## Check of the fix pass

A check of the changed lines (TASK-31: one check after a review that found an error), by `opus-review`, of
`git diff fe3c81c f46b2c6` on `work/10e` (six commits, `2b81987`..`f46b2c6`), read with `grants::plan`,
`through_links`, `merge`, zosh's `plan_with`, the new `userland_options` and the placeholder block
(`zosh.rs:1790-1830`), `zoraya_command::parse` and `is_option_word`, the file service's `open` (`IS_LINK`, the
narrowed read-only rule, `fs-service/src/lib.rs:1894-1925`), toybox's `sendfile_len` and the patched `copy_tempfile`
and `zoraya` copy-back in the unpacked tree, and dash's `config.h` and `input.c`. Nothing was built or run here (the
other lanes had the machine); the fix pass's own runs are listed in the part report and not repeated. Every claim
below is by reading.

### Each finding of the review

| Finding | Fixed by the diff? |
|---|---|
| 1 (must-fix) | Yes. The step runs `/usr/libexec/toybox cat` (undescribed), so INV-0153-7 is checked again; the hint text follows. |
| 2 (must-fix) | Yes. The three `when ... FROM tree` lines are gone; the new lint `no_option_widens_a_read_operand_to_write_unless_it_changes_it` would fail on the old `cp.cmd` (a `read` operand, `-r` makes it `Tree`, `cp` not in the exception list); the QEMU step checks `read` and no `anything in`. |
| 3 (must-fix) | No, and the report says so: toybox's tests ran on a host build, the independent read of `toys/pending/{tr,expr,diff}.c` is not done. The header comment no longer claims a review. The gate is the architect's. |
| 4 | Yes as ADR-0336 item 5 says, with a regression (N3). |
| 5 | Yes, the narrower form; the `grants` step checks `read (option -f)` and no `create or replace`. |
| 6 | Yes. `narrows` includes a `cwd none` `Read`/`Write` operand; the test fails without it (a value `.` and the cwd's own path are refused, a value file is allowed). |
| 7 | Partly: a link to outside is now refused with the hint, but the inside case still does not run for `sed` (N1), and following `create` names broke `ln -sf` and `mv` onto a link (N2). |
| 8 | Yes, and the placeholder bug it found is fixed; the QEMU step would fail without the placeholder (`../d/n.txt` resolved from `/bin`). The fallback test now asserts a non-empty list and no `CREATE`/`REMOVE`/`RENAME`. |
| 9 | Yes: run in QEMU and the pages say what each line needs. |
| 10, 11, 12 | Yes. `sh -xc` puts awk's `zcc` lines in `awk.log`, `awk` is in the loop; the pinned bison is a prerequisite and its version is logged; the ADRs say what was built. |
| 13, 14 | Recorded in `docs/backlog.md`, as the review allowed. |
| 15 | Yes, with the cause found: `USE_TEE 0` in the built `config.h`. No other port calls `tee` or `splice` (searched: awk, make, pdpmake, tcc, toybox `lib/` and `toys/`), so nothing else depended on dash's tee path. |
| 16, 17, 18, 19 | Yes, or recorded as the report says (18 partly; INV-0342-4's text is the architect's). |

### New findings

N1. **warning (should-fix).** `crates/zoraya-shell/src/grants.rs:386-410` with `zosh.rs:1799-1830`. **What.**
    For a `ReadName`/`ChangeName` entry (only `cwd none` programs get these: `sed`, `curl`, `wget`) whose name is a
    link inside the current directory, the entry is *replaced* by the target's directory narrowed to the target's
    name. The program still opens the name it was given (`notes.txt`), which resolves in its table to the current
    directory, and for a `cwd none` program that is now the dead placeholder (no entry covers the cwd, and the line
    designates something, so `designated` adds it). Libposix learns a link's target only from the service's
    `IS_LINK` answer (`fs-service/src/lib.rs:1894`) on a connection that reaches the link, and none does. So
    `sed -i s/a/b/ notes.txt` with `notes.txt -> drafts/notes.txt` still fails, now with an error from a closed
    channel and no hint; ADR-0344 item 2 and the report say the link "is followed". The host test checks the plan
    only; no QEMU step runs a link. **Fix.** Keep the link's own entry as well (its directory narrowed to the link's
    name, `READ | CLONE` only, so the open meets the link and the client follows it to the second entry), or say
    in ADR-0344 and `sed.cmd` that a link operand of a `cwd none` program does not work yet; add a QEMU step with a
    link inside the current directory either way.

N2. **warning (should-fix).** `grants.rs:392-395` (`Access::Create(_)` in `opens`). **What.** A `create` name is
    now followed at the name itself. That is right for a program that opens through the name (`cp`, `touch`,
    `wget -O`), and wrong for one that replaces the link object: `ln -s[f] T L` and `mv A L` act on `L`, not on what
    it points to. Two regressions, both closed, one with a hint that asks for a wider grant than the line needs:
    `ln -sf new L` where `L -> /old/place` (re-pointing a link, the main use of `-f`) is now refused "leads through a
    link to /old/place ... put +w /old/place before the command"; with `L -> drafts/x` inside, the `Create` entry
    moves to `drafts`, the current directory stays read-only, and `ln` cannot replace `L` (before, the narrowed `/w`
    entry merged with the cwd's and the line worked). `mv` onto a link outside is refused the same way. **Fix.**
    Follow only `ReadName`/`ChangeName` at the name (what finding 7 asked), keep `Create` at its directory as
    before, or make it a description key (a `create` that does not follow); host tests for `ln -sf` over a link
    inside and outside, and for `cp` through a dangling link (kept).

N3. **warning (should-fix).** `user/src/bin/zosh.rs:1641-1686` with `gzip.cmd:5`, `gunzip.cmd:5` (and `xargs.cmd:5`,
    `find.cmd`). **What.** The refusal covers every parse error, "missing operand" included, so a description whose
    operand is required although the program reads standard input without one now refuses a correct line that ran
    before: `tar -c src | gzip > src.tgz`, `gzip -c < big.log > big.log.gz` (the form `gzip.cmd` itself recommends
    for the least rights) and `gunzip < x.gz` are refused from `zosh` with "missing operand FILE, which its
    description does not let through". `xargs` without a command (it runs `echo`) and `find` without a path (it
    searches `.`) likewise. None of these is a hole; they are lines the fix pass did not try. **Fix.** Make the
    operand optional where the program reads standard input or has a default (`[FILE...]` for `gzip` and `gunzip`;
    `[PATH]` for `find`; `[COMMAND]` for `xargs`), check the 56 descriptions for the same, and add a QEMU step with
    `gzip` in a pipeline.

N4. **note.** `zosh.rs:1650` with `zoraya_command::parse`'s `--help` rule (`zoraya-command/src/lib.rs:141`). A
    `--help` anywhere before `--` makes the parse `Help`, so `/bin/find . --help -delete` passes the refusal and runs
    with `Parsed::default()`. No designations follow (the current directory only, or nothing for `cwd none`), so it
    is no widening, but the line runs words the description does not let through. Either treat `--help` as help only
    as the first word, or say so in `userland_options`'s comment.

N5. **note.** Toybox patch 0002's copy-back, `sendfile_len` (`lib/portability.c:749-779`). A failed `read` of the
    `/tmp` copy ends the loop with the bytes copied so far, which is not `< 0`, so the patch takes it as success,
    closes and unlinks the only whole copy. Only a write failure is caught (the case the test forces). A read error
    on the line's `/tmp` is unlikely; compare the result with the copy's size (`fstat(fdout)`) to close it. An
    `xclose(out)` failure exits without the "kept in" message (the copy is kept, since `tempfile2zap` is already 1).

N6. **note.** The root cause of finding 15 is that libposix's `tee` fails with `ENOSYS` where Linux gives `EINVAL`
    for a descriptor that is not a pipe, which is what dash (and any other program with a `tee` fallback) tests for.
    `--disable-tee` fixes dash; a backlog line for `tee`'s errno would keep the next port from meeting it.

N7. **note.** `through_links` treats a `+r` grant as a root for any access (pre-existing, ADR-0153 rule 5 says "an
    explicit grant" without its kind); the new name-following extends that to the last component, so `+r ~/s sed -i x
    notes.txt` with `notes.txt -> ~/s/plan.txt` plans `~/s` narrowed to `plan.txt` with `WRITE`, under a grant the
    person wrote as read. The new test asserts only the `ReadName` case. A note for the ADR's owner: whether rule 5's
    root should be a write grant for a writing access.

Checked and sound: the `narrows` rule for `..`, `.` and a grant's own path as values (refused, the literal `.`
still allowed as written); the strictly-below rule for followed names (a link to the cwd itself is refused); the
placeholder (a dead connection, added only when no directory entry covers the cwd, so it shadows no real entry);
`USERLAND` (one name per line of the applet list plus `awk`, matched against trusted descriptions found by program
path only, so a native command of the same name is not affected); the new lint's widening rule and `names_a_path`
(each has a test that fails on the old form); `tar.cmd`'s rights (`read` for `-f` and `-C`, `-x` needs `+w`); patch
0002's order (`tempfile2zap = 1` before the truncating open, so the exit handler keeps the copy); `dash` with
`USE_TEE 0`.

### Recommendation after the fix pass

Mergeable once the architect decides finding 3 (ADR-0336 item 2's gate for `tr`, `expr`, `diff`); nothing in the
fix pass opens a hole: N1-N3 all fail closed. N2 and N3 are small and break lines that worked before the fix pass
(`ln -sf`, `gzip` in a pipeline), so fix them on the branch before merge; N1 can be fixed there too or recorded in
ADR-0344 and `sed.cmd` as a limit. The notes can go to the backlog.

## Check of open_or_link

A read of `57bc20c` (`File::open_or_link` in `user/src/files.rs`, `Held::open_followed` in `user/src/bin/zosh.rs`),
as merged in `08bdde9` (the merge does not touch either function). No build and no run in this check.

Can a link make zosh open a file outside the room's table? No. Every hop of the table walk goes back through
`covering`: an absolute target is normalised with `zoraya_resolve::join("/", 0, ..)`, a relative one with
`join(&e.path, 0, ..)` from the entry's absolute path (`open_or_link` has already put the link's directory in
front), so `..` is removed before the lookup and cannot leave `/`. A path no entry covers goes to `Held::open`,
which refuses it (`ACCESS_DENIED`); a path under a file entry with a rest is refused there too. Each open is made
with `wanted & e.rights` of the entry that covers the hop, so a link from a `+w` directory into a `+r` one gets
read rights only. The walk is bounded (40 hops, then `IS_LINK`, as `File::open_followed` was). The table's own
callers ask for `READ | CLONE` or `READ` with flags 0 (command lookup, `type`, loading an image), so nothing here
writes or creates through a link.

Does it widen anything? Compared with before, an absolute or `..`-climbing link in a room may now reach a file under
*another* entry of the same table, where before it stayed in the link's entry and failed. That entry is one the room
already grants, at its own rights, and zosh could open the same file by its literal path; so the shell's authority is
not widened (INV-0172-4 holds: `covers` and `open` decide on the same table). ADR-0153 rule 5 governs paths
*designated to a program* (`link_target`, unchanged); this function is the shell's own read of an executable, and
the program gets the image, not a connection. `File::open_followed` (the console's root, the broker) behaves as
before: the absolute target is trimmed and taken from the connection's root in both versions.

Findings:

C1. note. `user/src/bin/zosh.rs:531-535`: the doc comment of `Held::open_followed` still says a table follows links
"inside the covering entry only ... and never leads out of the entry". After this commit that is false. Scenario:
a later reviewer trusts the comment when judging a rule-5 change. Fix: say that a link is followed through the
table, each hop within the covering entry's rights, and refused where no entry covers it.

C2. warning. No test fails on a hostile link. `room-gui` step 17 (`echo hi | tr h H`) does fail without the fix
(the absolute `/bin/tr -> /usr/libexec/toybox` was looked for under `/bin`; the commit reports the failure and
the pass), but it tests only the good case. Nothing checks that a link in a room's `+w` directory to a path the
room does not grant is refused, or that a link into a `+r` entry does not give write. Scenario: a later change
falls back to `Held::Root`-style resolution when `covering` returns `None`, and every test still passes. Fix: the
table walk needs no hardware; move it to `zoraya-resolve` (a function over the entries' paths and a one-step
opener, as `place` is) with host tests: absolute target outside the table refused, `../..` target outside refused,
link from `+w` into `+r` gets `+r`'s rights, a loop of two links ends after 40 hops. Or, smaller, add to
`room-gui` a step that makes `ln -s /zoraya/bin/<native> x` (or any path outside the room) in the room's `+w`
directory and runs `./x` / names it on `shell.path`, expecting "no such command".

C3. note. `user/src/files.rs`, `open_or_link`: `&path[..path.len() - remaining.len()]` still trusts the server's
`remaining`; a reply with a longer `remaining` panics the shell. Not new (the old loop had the same line) and the
room's connections are the volume server's, so it is only a robustness note; `checked_sub` with
`INVALID_ARGUMENT` would close it.

Recommendation: the fix is sound and fails closed; mergeable. Fix C1 on the branch (one comment); C2 can go to the
backlog with the host test as the preferred form, but should be done before the table walk changes again.

## Gate review of tr, expr and diff (ADR-0336 item 2)

The reviewer did not port these three. Read: ADR-0336 (item 2, the revisit trigger), `work/10e` at `08bdde9`:
`user/describe/posix/{tr,expr,diff}.cmd`, `ports/toybox-applets.txt`, `ports/patches/toybox/`, `tests/posix/w5.script`,
`tests/posix/w10.script`, `tests/posix/rootfs/tests/w10.sh`; and `toys/pending/{tr,expr,diff}.c` of toybox 0.8.15. The
archive is on disk (`build/ports/src/toybox-0.8.15.tar.gz`). Its SHA-256 is the one in `ports/sources.lock`
(`44e2bab4...`), and the three files in `build/ports/work/toybox-0.8.15` are byte for byte the archive's: no Zoraya
patch touches them. Nothing was fetched.

How it was run: a host build of the pristine archive in a scratch directory (macOS, clang, `allnoconfig` with only
`TR`, `EXPR`, `DIFF`). It used `-funsigned-char` to match the target (`zcc` defines `__CHAR_UNSIGNED__`) and
`-fsanitize=address,undefined`. A second build used `-O2` without sanitizers, for timings. No QEMU was run in this
review. W5 and W10 were not re-run; their logs in the 10e worktree (`target/test-logs/posix-w{5,10}-auto.log`,
16:12) end with exit code 0.

**toybox's own tests** (`scripts/test.sh tr|expr|diff`, sanitized build): tr 6/6, expr 44/44, diff 19/19 PASS, with no
sanitizer report. This confirms the implementer's counts, now under ASan and UBSan. These tests do not reach the
defects below.

### Findings

G1. error. `toys/pending/tr.c:116-183` (`expand_set`): heap buffer overflow, a write. The size check
`if (i >= size)` runs once per input item, but a single item adds up to 256 bytes: a range `X-Y` adds `Y-X+1` bytes,
and a class up to 52 or 62 (`[:xdigit:]` writes `set[i+6..i+11]`). With `i` anywhere in `1..255` and a range or
class that follows, `tr` writes past the 256-byte buffer, by up to 255 bytes on later rounds as well. Reproduced
with ASan: `tr "ab<0x01>-<0xff>" y` and `tr "<250 zeros>[:punct:]" y` both stop with `heap-buffer-overflow` (WRITE).
Scenario: a script runs `tr "$set" ...` on data from a file or the network (configure scripts and git's scripts
build tr sets). The overflow corrupts `tr`'s heap, including malloc's metadata. The damage stays inside `tr`'s own
process and table (the terminal, `/tmp` of the line, system read), so nothing goes beyond what a normal process
holds. It is still memory-unsafe code in a command scripts feed with data, which is exactly the review's question.
Fix: a third toybox patch. Before each item, grow when `i + 256 > size` (for example
`if (i > size - 256) set = xrealloc(set, size += 256);`, with the growth at the top of the loop). Add a regression
case on the image.

G2. warning. `toys/pending/tr.c:52-56` (`map_translation`): `if (set2[k + 1]) k++` uses a NUL byte as the end of
SET2, but SET2 is a counted buffer (`TT.len2`) and is not terminated. (a) When SET2 expands to exactly a multiple of
256 bytes, the loop reads one byte past the buffer. ASan reports `READ of size 1 ... 0 bytes after 256-byte region`
for `tr <300 a's> "<0x01>-<0xff>z"`. After an `xrealloc` the bytes past `len2` are uninitialised, so `k` can walk on
past the buffer. (b) A NUL in SET2 stops the walk: `echo abc | tr abc 'x\000y'` prints `xxx`, where POSIX and GNU
give `x`, NUL, `y`. Fix (same patch): `if (k + 1 < TT.len2) k++;`.

G3. warning. `toys/pending/tr.c:192`: `while ((n = read(0, ...)))` treats `-1` as data. On a read error `tr` spins
at full CPU forever, writing nothing. On the host, `tr a b < /tmp` (stdin a directory) ran until the 10 s alarm. On
Zoraya, any read error on fd 0 does the same (a directory, `EIO` from a closed terminal or pipe server). Only the
person's own line hangs, and Ctrl-C ends it. Fix (same patch): `while ((n = read(0, toybuf, sizeof(toybuf))) > 0)`,
then `if (n < 0) perror_exit("read")`.

G4. warning. `toys/pending/expr.c:163`: comparisons compute `cmp = a - b`, which overflows.
`expr 9223372036854775807 \> -1` prints `0` (UBSan: `signed integer overflow`). The arithmetic at `expr.c:184-188`
overflows the same way: `expr 9223372036854775807 + 1`, and `-9223372036854775808 / -1`. That is undefined
behaviour in C. On AArch64 `sdiv` does not trap, so nothing crashes, but the wrong value comes back with exit 0 or
1, never 2. Scenario: a configure-style script compares sizes or versions with `expr` and takes the wrong branch.
Fix: compare with `(a > b) - (a < b)`, and use `__builtin_{add,sub,mul}_overflow` (with `b == -1 && a == LLONG_MIN`
for `/` and `%`) that ends in `error_exit("overflow")`, exit 2, as GNU does. If not patched, `expr.cmd` must say
that results wrap.

G5. warning. `toys/pending/diff.c:142-169, 300-307`: the Hunt-Szymanski core is quadratic in time (and grows its
candidate lists) when a file has many equal lines. One file of N lines `x` against the same file plus one line
took 0.71 s at N=5000, 1.03 s at 10000 and 5.62 s at 20000 (`-O2`, host). The sanitized build ran past 60 s at
50000. 20000 distinct lines take 0.00 s. A larger input makes it run longer, not unsafely: only CPU, memory under
the quota, Ctrl-C ends it, and nothing goes beyond a normal process. Scenario: `diff` on generated output with
many blank or repeated lines (logs, CSV), on the slower guest. Fix: record it as a known limitation in `diff.cmd`
(or `docs/design.md`). ADR-0336's revisit trigger (OpenBSD's `diff`) applies if it bites.

G6. warning. `toys/pending/diff.c:528, 533, 891`: `diff -r` follows symbolic links by default
(`DIRTREE_SYMFOLLOW`), and toybox's `dirtree` has no loop detection. With a link `s/loop -> ..` on both sides, the
walk descends until open fds run out. The host (1M fds) ran past 20 s with no output; at `ulimit -n 64` it ends
with `diff: xdup: Too many open files`, exit 2. On Zoraya `OPEN_MAX` is 512 (`zoraya_abi::posix`), so the walk
ends in that error after at most about 512 levels. Links are followed only through the line's own table (10e's
`open_or_link`), so a link cannot reach anything `diff` was not given. The flaw is cost and a misleading error, not
rights. Fix: say in `diff.cmd` that `-r` follows links and that a loop ends in an error. Better, a patch that
compares each directory's dev/ino with its ancestors'. `--no-dereference` is (rightly) not let through.

G7. warning. `toys/pending/diff.c:403`: `print_line_matching_regex` loops `for (i = a; a > 0; --i)`, testing `a`
instead of `i`. With `-F REGEX` and no line above the hunk matching, it reads `off_set[-1]` and below. ASan stops
with `heap-buffer-overflow` (READ of size 4) on `diff -F zzz` of `seq 1 10` against the same with line 8 changed.
`diff.cmd` does not let `-F` through, so `zosh` refuses the option. But a script under `sh` calls `/bin/diff`
with whatever options it likes, and gets the out-of-bounds read inside its own process. Fix: keep `-F` out of
`diff.cmd` (it is), and include `i > 0` in the toybox patch of G1, because scripts are outside the description's
reach.

G8. warning. The gate's workloads barely touch the commands. W5 runs one `tr a-z A-Z`. W10 runs `diff A B` on two
identical files, so it exercises neither the LCS code, `-u` output, `-r`, nor a non-zero exit. `expr` has no
workload. toybox's tests ran against the host's libc, not the image's. Fix: add to the userland script on the
image: `diff -u` of two differing files against an expected hunk (exit 1); `diff -r` of two trees with an
`Only in` line; `tr -d '\r'`, `tr -s`, `tr -c`; `expr 7 / 0` and `expr 1 + a` with exit 2; and regressions for G1
to G3 (the two overflowing sets of G1, the `x\000y` case of G2, `tr a b < DIR` ending with an error).

G9. note. POSIX gaps that `tr.cmd` does not state (`expr.cmd` does state its own). `[c*n]` and `[c*]` in SET2 are
not supported: `echo abc | tr abc '[x*]'` prints `[x*`. With `-d` and SET2 but without `-s`, SET2 is silently
ignored where POSIX makes it a usage error: `tr -d a b` prints `bc`, exit 0. `[=c=]` takes one byte. Fix: one
sentence in `tr.cmd`'s body.

G10. note. `toys/pending/expr.c:79`: `strtoll` with only `*end` checked accepts `""` as 0 (`expr '' + 1` prints
`1`), and also leading blanks and a sign, and clamps out-of-range numbers silently (`ERANGE` is ignored). POSIX
requires an integer argument. No safety impact. `diff.c:56` holds file offsets as `int`: for a file over 2 GiB the
offsets wrap and `fseek` fails or seeks wrongly. That is wrong output or `fseek failed`, not memory-unsafe.

### Checked and found sound

- The descriptions let through only what ADR-0336 allows. `tr` and `expr` have only `text` operands. The code
  matches: `tr` touches only fd 0 and fd 1, and `expr` opens nothing. `diff` has two `read` operands, and a `read`
  directory is `read` and `clone` (ADR-0153's table), which covers `-r`'s tree as `grep -r`'s. No option takes a
  path. The options left out are harmless except `-F` (G7): `-L`, `-S`, `--*-line-format`, `--color`,
  `--no-dereference`, `-d`, `-t`, `-T` and `--strip-trailing-cr` for diff, and `-t` for tr. `diff -` and a FIFO
  operand copy the input to `$TMPDIR` or `/tmp`, which is the line's own `/tmp` and counts against its quota. No
  rights beyond the line's table.
- `expr.cmd` is right. There is no `length`, `substr`, `index` or `match` in the code: `expr length abc` gives
  `Unexpected extra input 'abc'`, exit 2. toybox's own `config EXPR` help does not say so, but `help expr` on
  Zoraya is the description's page, which does. Exits: 0 and 1 by `is_false`, and 2 for errors, through
  `TOYFLAG_ARGFAIL(2)` (`lib/lib.c:23`). `expr 1 + a` exits 2, as checked. diff uses `ARGFAIL(2)` as well.
- `TT.map[set[k]]` and `TT.map[toybuf[i]]` in `tr` are in bounds because `char` is unsigned on the target. On a
  signed-char host (macOS) they would index below the map, so a host build for tests needs `-funsigned-char`, as
  this one had.
- `expr` recursion (`eval_expr`) is bounded by the argument count. On Zoraya's 64 KiB stack with its guard page, a
  deeply parenthesised line ends in a stack fault of `expr` alone (ADR-0034), not in corruption. 40000 levels ran
  on the host's 8 MiB stack. The regex is libc's `regcomp` (BRE), the same as for `grep` and `sed`. Its
  backreference blow-up is theirs, no worse.
- diff's growth arithmetic for `v[]`, `offset[]` and `kcand[]` (`size * 11 / 10`, checked once per step against at
  most one increment), its binary detection (NUL in the first block unless `-a`), and `quote_filename`'s two
  passes under an unsigned `char`.
- busybox is no longer in `ports/sources.lock`.

### Verdict

**Enable with these changes.** `expr` and `diff`: enable. Fix G4 (or state the wrap in `expr.cmd`) and document
G5 and G6. `tr`: enable only with a toybox patch for G1, which is an error, together with the one-line fixes of G2
and G3 and G7's `i > 0`. All four are under 15 lines, in a third file of `ports/patches/toybox/`. The regressions
of G8 must run on the image. Until that patch lands, ADR-0336 item 2's condition ("a reviewer ... has read it")
is met for `expr` and `diff` and not for `tr`. `tr` is already listed in `ports/toybox-applets.txt` at `08bdde9`,
so the patch is due before 10e merges. None of the defects lets a hostile input do more than the process could do
anyway: each stays inside the program's own table. G1 is still memory corruption from script data, and the gate
exists to keep that out.
