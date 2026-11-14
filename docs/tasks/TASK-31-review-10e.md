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
