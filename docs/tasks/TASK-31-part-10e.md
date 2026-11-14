# TASK-31 part 10e: the userland (branch work/10e)

Written by the 10e implementation session (Sonnet). Review by `opus-review` is the dispatcher's.

## What exists

- `ports/toybox-applets.txt` (52 commands, ADR-0336 item 1 and the three of `pending`: `tr`, `expr`, `diff`) and the
  build from it (`ports/Makefile`: `TOYBOX_APPLETS`, plus the build option `toybox-float` that `seq` needs). The probe:
  linking all of them against the old `libc.a` left 16 undefined names (ADR-0343's table).
- Layout (ADR-0336 item 8): `/usr/libexec/toybox`, absolute links `/bin/NAME` (a relative link `../usr/libexec/toybox`
  was not followed by `zosh`'s lookup, so the links are absolute; `zoraya-elf-check` follows absolute targets under
  the image root). `toybox` is not in `/bin`. busybox is out of `ports/Makefile`, `sources.lock`, `README.md`,
  `docs/dependencies.md` and the image.
- libc names: ADR-0343 (proposed), 26 names with the program behind each: musl's own code for most; libposix gains
  `link` (EPERM), `lchown`, `mknod`, `settimeofday` (EPERM), `readv`; musl's `sigsetjmp` assembly and tail are built.
  `fchdir`, `statfs`, `pathconf` stay out: toybox patch 0002 makes them fail with `ENOSYS`, drops `find -execdir`
  and changes `sed -i` (below). `posix-surface-check` reads `NAME PROGRAM` lines and has an `imports` command.
- `sed -i`: patch 0002 of toybox (`lib/lib.c`): the result goes to a file in `/tmp` and is copied back into the original
  (opened `O_WRONLY|O_TRUNC`). The shell side: `Access::ReadName` / `ChangeName` in `zoraya-shell`'s `grants`: a
  file operand of a `cwd none` description is its directory narrowed to the name (`READ|CLONE`, or `READ|WRITE|SYNC|CLONE`).
  Several names in one directory fall back to the directory whole (words `read anything in`, `read and change files in`).
- Descriptions: 56 files in `user/describe/posix/` (the 52, plus `sh`, `cc`, `pdpmake`, `awk`), written by a Sonnet
  subagent under rules I set (see ADR-0344 item 4), each with a `Lets through:` last line; the 11 native names that
  share a command say so on their first body line. `help NAME` / `help /bin/NAME` show the POSIX page
  (`user/src/bin/zosh.rs`, `posix_page`). The lint (INV-0336-2): `crates/zoraya-describe/tests/posix_names.rs`, run by
  `make check` inside `check-argv`.
- one-true-awk: commit `72e9b56caddb4a6f8d1405315a5183c26b8650a4` (`version 20261106`, second edition), sha256 in
  `ports/sources.lock`; bison (host) makes the grammar, a host-built `maketab` the table; no patch; `/bin/awk`.
- Checks: `ports/check-userland.sh` (in `make check-ports`): INV-0336-1, -3, INV-0342-1 second half.
  `tests/posix/zosh-userland.script` (in `make test-posix`, a line in `tools/ci-tests.txt`): INV-0336-4, -5, -6,
  INV-0342-4, -5 and the done-when lines. Host test `a_file_of_a_cwd_none_program_is_given_as_one_narrowed_name`.

## What was run (commands and results, this session)

- `make check`: 22 of 22 passed (after the last code change).
- `cargo test -p zoraya-shell`: 151 passed before my two new tests, all passed after (`cargo test -q -p zoraya-shell`).
- `cargo test -p posix-surface-check`: 43 passed + the new one.
- `cargo test -q -p zoraya-describe --test posix_names`: 3 passed.
- `make -f ports/Makefile toybox`, `awk`, `rootfs`: built; `sh ports/check-userland.sh ...`: `check-userland: passed`.
- `make check-ports`: reaches and passes the userland step and the elf check; it fails on an incremental tree at
  "no port was compiled with __linux__" because `dash.log` and others hold no compile commands when nothing was
  rebuilt (the check is for clean builds; not a change of mine).
- `make test-posix POSIX_SCRIPTS="tests/posix/zosh-userland.script" POSIX_ACCELS="auto"`: `PASS: 124 steps` (HVF).
  It runs: `grep -n TODO *.c` (`t.c:2:/* TODO: check */`), `grants grep ...`, `grants sed -i ...` (`change notes.txt`),
  `sed -i s/colour/color/ notes.txt` (the file changes), `sed -i 'r other.txt' notes.txt` (cannot read the sibling),
  `tar -c -z -f out.tgz notes.txt t.c` and `tar -t -z -f`, `awk`, `help sed`, `help /bin/ls`, `ls --help` against
  `dash h.sh` (`ls: Unknown option 'help'`, toybox's), `/bin/find ... -exec /bin/cat /tests/w9-check.sh {} +` refused
  and then allowed by `+r /tests`, and a C program (tcc) showing `getpwuid` works, `link` and `mknod` give `EPERM`, and
  `MB_CUR_MAX` 4 with `mbtowc` 3 bytes to `0x20ac`.
- `make test-posix POSIX_SCRIPTS="tests/posix/w5.script tests/posix/w10.script" POSIX_ACCELS="auto"`: `PASS: 45 steps`,
  `PASS: 17 steps`: the T1 workloads W5 (`tr`) and W10 (`diff`) on toybox's commands.
- `make measure-w4 W4_ACCELS=hvf`: see W4 below. `tests/posix/zosh.script` was changed (`toybox cat` to `/bin/cat`
  etc.) and not re-run here beyond the hint line it shares; the candidate CI runs it.

## W4 (ADR-0005's condition for T2), HVF, local

`build/w4-report.md`: wall 39.25 s for the whole run; `configure` takes 19.7 s (GNU make set) and 19.5 s (pdpmake
set) and stops at `configure: error: cannot run /bin/sh ./build-aux/config.sub` (the test `sh config.sub sun4`;
under `dash -x` the script prints `Invalid configuration 'sun4': machine 'sun4-unknown' not recognized`: not a
toybox tool; unexamined whether it is dash or the 2025-07-10 `config.sub`). So `configure` of GNU hello gets past
`sed`, `awk` and `grep` (126 `checking` lines) and stops at the next step. Counters: 1207 images, 141 `exec` (mean
48.985 ms), 639 `posix_spawn` (mean 43.995 ms), 426 forks, loading programs 89 % of wall time, 3205 broker round
trips (3.00 per child), 709735 system calls, peak 11 fds, peak 8 pids, peak /tmp 44 KiB. TCG was not run (CLAUDE.md:
CI runs it). The `t2-probe` row in the report is the stale T2 applet set (8 names: `__h_errno_location getgrgid
getgrouplist getgroups gethostbyname hstrerror mkdtemp sethostname`), commands that ADR-0336 does not list.

## Decisions (ADR-0343, ADR-0344, both `proposed`)

Recommendations in one line: ADR-0343 accept (names with a program each; `fchdir`/`statfs`/`pathconf` out).
ADR-0344 accept (layout, narrowed one-name read/change, `sed -i` copy-back, minimal descriptions, image cache and names
list deferred).

## Not done / limits

- The image cache of ADR-0212 is not built: it is a broker, `posix_child` and `libposix` protocol change with its own
  security argument (INV-0212-2 to -4) and did not fit; the userland does not depend on it (W4 shows the cost: 89 % of
  wall time loads programs).
- `narrow` with a list of names is not built (10d may build it; note for 10d and 10f).
- The review of `tr`, `expr` and `diff` by a reviewer who did not port them is the dispatcher's `opus-review`; I ran
  W5 and W10 and nothing more. `expr` has no workload; it is built and described and not exercised.
- Descriptions: option names are invented long forms (the format requires one); `find`, `test`, `expr`, `env` need
  words that start with `-` as operands, which no description can declare (the agent recorded it in find and test).
  `sed` lacks `-e`/`-f`; `cp`/`mv` take one `FROM` and one `TO`; `dd` designates nothing; `tar -f` is `create` for
  `-x` and `-t` (a loss).
- `tar` prints a hint that it tried to read `/`.
- Only the HVF run; no TCG; no GUI.
- `zosh.script` and the GUI/tour scripts were not re-run (`tests/posix/zosh.script` was edited for `/bin/cat`).
- Reports per option: ADR-0343 block lines for `awk` are checked by `check-userland.sh` once `make awk` ran.
- one-true-awk's licence (Lucent notice): the architect is asked to confirm (ADR-0336 open question).

## For the architect

- W4's numbers above (HVF only) and the first blocker after `sed`.
- The list of options each description lets through (appendix; regenerate with `cargo test -p zoraya-describe --test
  posix_names -- --nocapture`).
- Wall time of the session: about 3.5 hours.

## Fix pass after the review (`docs/tasks/TASK-31-review-10e.md`, branch `docs/review-10e`)

Done by `opus-review` in this worktree, on the review's findings and the architect's instruction on `config.sub`.
Each fix has a test; where a commit says "shown by hand", the test was run with the fix undone and failed. Sections
above this one describe the part as first delivered; where they differ, this section is the current state.

| Finding | Result |
|---|---|
| 15, `config.sub sun4` (architect) | **Fixed.** Cause found in QEMU: dash 0.5.13 reads a non-terminal, non-seekable standard input through `tee(2)` when its configure finds it; libposix's `tee` fails with `ENOSYS` (ADR-0100 item 6), and dash falls back to byte reads only on `EINVAL`, so every builtin `read` from a here-document or a pipe saw end of file (`IFS=- read a b <<EOF` gave `HEREDOC[/]`, `echo x-y \| (read ...)` gave `PIPE[/]`); `config.sub`'s `IFS=- read field1 ... <<EOF` read nothing, so `sun4` missed its alias. dash is now configured with its own `--disable-tee` (`ports/Makefile`); no libposix change. Test: `tests/posix/invariants.script` (a here-document, a pipe, a script piped into `sh`, `config.sub sun4` = `sparc-sun-sunos4.1.1`); it failed with the old dash and passes with the new. W4's `configure` now gets past `config.sub` and stops at `configure: error: cannot guess build type; you must specify one` (`config.guess` does not know Zoraya's `uname`; 128 `checking` lines; `docs/backlog.md`). |
| 1 (must-fix) | **Fixed.** The INV-0153-7 step runs `/usr/libexec/toybox cat` (undescribed). `tests/posix/zosh.script` passes to the end, the Ctrl-Z steps included. |
| 2 (must-fix) | **Fixed.** `cp.cmd`'s three `when ... FROM tree` lines are gone; a lint test in `posix_names.rs` refuses a `when` that widens a `read` operand to a writing kind (except `chmod -R` and `sed -i`, which change the operand). QEMU: `grants /bin/cp -r src backup` shows `~/src read (operand FROM)`. |
| 5 | **Fixed, the narrower form.** `tar -f ARCHIVE` and `-C DIR` are `read` in every mode; an archive is made through standard output; `-x` needs `+w` where it extracts. Per-mode option kinds are ADR-0344's open question (a change of the description format). |
| 6 | **Fixed.** A `cwd none` read or write operand is in `narrows`; host test with a value naming the current directory (refused) and one naming a file in it (allowed). |
| 7 | **Fixed.** `through_links` follows a narrowed name that opens (read, change, create) at the name itself, to a target strictly below the current directory or a grant, else refuses with the hint; `remove` and `rename` keep the link. Host test: a link inside, outside (refused, `+r` or `+w` hint), under a grant, dangling, to the current directory itself, and `rm` of a link. |
| 4 | **Fixed as ADR-0336 item 5 says.** `zosh` refuses a line of a userland command (the names of `ports/toybox-applets.txt` and `awk`, a list `user/build.rs` generates) that its description does not parse, naming the word and `/usr/libexec/toybox NAME`. Other POSIX descriptions (`curl`, `tcc`, `python3`, ...) are unchanged. Consequence: `/bin/find`, `/bin/test`, `/bin/env` and `/bin/xargs` with `-` words are refused from `zosh` (their pages say so); a rule for them is ADR-0344's open question. No new ADR: item 5 of the accepted ADR-0336 is the decision. The appendix is now true: an option not listed is refused. |
| 8 | **Fixed, and a bug found.** Host tests for a file outside the current directory and a non-trivial fallback. In QEMU `sed -i ... ../d/n.txt` from `~/e` failed (`sed tried to read /d/n.txt`): with nothing designated in or below the current directory the program started in `/bin`. `zosh` now gives the current directory's placeholder also when the line designates a file outside it. The script compares `ls -a d` before and after. |
| 9 | **Run and described.** `cp -r src backup` works in the current directory (its entry merges with the current directory's), `cp -r src e/bk` fails (`Permission denied`) and works with `+w e`; `gzip FILE` and `gunzip FILE` fail without `+w .` and work with it; `-c` works; `mkdir -p a/b/c` fails (`'a': Permission denied`) and works with `+w .`. The pages say so; the script runs each. |
| 3 | **Partly; the gate stays open.** toybox's own tests ran on a host build of the pristine 0.8.15 source (macOS, `make macos_defconfig`, `make test_tr`, `test_expr`, `test_diff`; the Zoraya patches do not touch the three files): `tr` 6 of 6, `expr` 44 of 44, `diff` 19 of 19 passed (the host's libc, not the image's). `expr` gets a test in `invariants.script`; it showed that toybox's `expr` has no `length`, `substr`, `index` or `match`, which `expr.cmd` claimed (corrected). **The review of `toys/pending/{tr,expr,diff}.c` by a reader who did not port them is not done**: this pass reviewed and ported nothing of them. ADR-0336 item 2's gate is the architect's to waive or satisfy; the header of `ports/toybox-applets.txt` says so. |
| 10 | **Fixed.** awk's recipes run under `sh -xc`, so `awk.log` holds its compile commands (11 lines with `zcc`), and `awk` is in the `__linux__` check's list. The incremental-tree failure (other ports' logs overwritten by a no-op make) is pre-existing: `docs/backlog.md`. |
| 11 | **Fixed.** awk's grammar is made by the pinned bison 3.8.2 of `ports/sources.lock`, built into `build/ports/host` (always, not only when the host's is older than 3); its version is the first line of `awk-grammar.log`. |
| 12 | **Fixed.** ADR-0343's recommendation says what was built (Option A for `readv`, `settimeofday`, `sigsetjmp`; Option B for `fchdir`, `statfs`, `pathconf`) and item 4 says `ENOSYS`; ADR-0344 items 1, 2, 4 and 7, its consequences and open questions are corrected. |
| 17 | **Fixed.** Patch 0002's copy-back keeps the `/tmp` copy once the original is truncated and says where it is on a failed copy (`sed: /limited/big: the edited text is kept in /tmp/toybox-XXXXXX: Quota exceeded`). Test in `invariants.script` (the 64K limit of `/limited`). `sed.cmd` says `-i` writes through a link. |
| 16 | **Done.** The C test of `zosh-userland.script` covers `lchown` (own identity `0`, another `-1` with `EPERM`), `settimeofday` (`-1`, `EPERM`) and `readv` over two buffers with a short read (32 bytes). New coverage of existing code: nothing to break. |
| 13, 14 | **Recorded** in `docs/backlog.md`. |
| 18 | **Partly.** The path rule of the INV-0336-2 lint also matches names ending in `FILE`, `DIR`, `ARCHIVE`, `PATH` (`PROGFILE`, `REFFILE`, `PATCHFILE`), with a test; the userland script says why `ls` is INV-0336-4's check (dash builtins); the applet list's header names `egrep`, `fgrep`, `[`. Not done: INV-0342-4's check text (ADR-0342 is accepted: the architect's), INV-0336-5 for `xargs` and `env`. |
| 19 | **Recorded.** `docs/design.md` names the limits (the fallback gives the subtree to read and change; described POSIX programs other than `sed` refuse a file operand outside the current directory). The image cache stays for the architect (first item below). |

### What the fix pass ran (this worktree)

- A scratch probe through `make test-posix POSIX_SCRIPTS=<probe> POSIX_ACCELS=auto` (not committed): `R1[/]` (here-document),
  `R2[]`, `R3[x/y]` (a file on standard input), `R4[/]` (a pipe), `cat <<EOF` worked, and `sh -x config.sub sun4` showed
  `basic_machine=sun4` after the `read`.
- `make test-posix POSIX_SCRIPTS=tests/posix/invariants.script POSIX_ACCELS=auto`: with the old dash `FAIL: timed out
  waiting for "HEREDOC[x/y]"`; after `--disable-tee` `PASS: 76 steps`; with the `expr` and `sed -i` steps
  `PASS: 86 steps`. With the old patch 0002 the `sed -i` step failed (`sed: short write: Quota exceeded`, then `cat: /tmp/toybox-*: No such file or directory`: the only whole copy was deleted).
- `make measure-w4 W4_ACCELS=hvf`: `configure` stops at `cannot guess build type`; wall 41.59 s, 1321 images, 87 % of
  the wall time loading programs (`build/w4-report.md`).
- `make test-posix POSIX_SCRIPTS="tests/posix/invariants.script tests/posix/zosh-userland.script
  tests/posix/zosh.script" POSIX_ACCELS=auto`: `PASS: 86`, `PASS: 209`, `PASS: 94` steps (HVF), the last run after every change; the runs are
  listed at the end of this section.
- `make test-posix POSIX_SCRIPTS="tests/posix/hello.script tests/posix/w5.script tests/posix/w10.script" POSIX_ACCELS=auto`
  (after the dash change): `PASS: 14`, `PASS: 45`, `PASS: 17` steps.
- `cargo test -q -p zoraya-shell`: 154 passed (with the fixes for 6 and 7 disabled: 152 passed, 2 failed).
- `cargo test -q -p zoraya-describe --test posix_names`: 5 passed (with the old `cp.cmd`, the new lint fails naming
  `-r`, `-R`, `-a`).
- toybox's tests on the host, as above.
- `make check`: all checks passed, before each commit.

Not run: TCG, `make check-ports` from a clean tree, the GUI and tour scripts, `make test-changed` as a whole.

### Second round: the check of the fix pass (N1 to N7) and the candidate run's wget link

| Item | Result |
|---|---|
| N2 | **Fixed.** Only `ReadName`/`ChangeName` are followed at the name; a `create` name is followed at its directory as before, so `ln -sf new L` and `mv A L` over a link (inside or outside) plan `L` in its own directory again. Host test (`ln -sf` over a link inside, outside and dangling, `mv` onto a link outside, `cp` through a dangling link); with `Create` back in the rule the test fails. |
| N3 | **Fixed.** `zosh` refuses only an option mistake (`UsageError::is_option_error` in `zoraya-command`: no such option, a value given or missing); a missing or extra operand runs as before. Host test `only_an_option_mistake_is_an_option_error`; QEMU steps `gzip -c < in.txt > in.gz`, `tar -c notes.txt \| gzip > n.tgz`, `gunzip < in.gz`, `echo zz \| xargs`. Before the fix the first of them failed (`missing operand FILE, which its description does not let through`). |
| N1 | **Fixed.** A followed read/change name keeps the link's own name with the same rights (no `CREATE`, `REMOVE`, `RENAME`), so the program's open meets the link and libposix follows it. QEMU step: `sed -i` through `nl.txt -> d/n.txt` changes `d/n.txt`. Without the link's entry (the first fix pass) it fails; with the link's name read-only it fails too (`Permission denied` on the copy back: the service checks the rights before it answers that the name is a link), hence the same rights. Host test asserts the entry. |
| N5 | **Fixed.** The copy-back compares the bytes copied with the `/tmp` copy's size (`fstat`) and checks `close`, so a failed read keeps the copy and says where. No test forces a read failure of the line's `/tmp`; the quota test (write failure) still passes. |
| N4, N6, N7 | `docs/backlog.md`. |
| wget (candidate run 43609007070) | **Cause:** 10e put musl's `sigsetjmp` in `libc.a` (ADR-0343, toybox's `gzip`). wget's configure then finds it, `HAVE_SIGSETJMP` turns on `USE_SIGNAL_TIMEOUT`, and `run_with_timeout` calls `setitimer`, which `libc.a` does not export: `undefined symbol: setitimer`. **Fix:** `ac_cv_func_sigsetjmp=no` for wget's configure (`ports/Makefile`), its behaviour on `main`. curl needs `HAVE_ALARM` as well for its alarm path (`alarm` is excluded), Python has no `sigsetjmp`. The `Broken pipe` panic in that log is `zoraya-ar t ... \| grep -q` in `ports/check-broken-archives.sh` (pre-existing noise, not a failure; backlog). |

Run in the second round: `cargo test -q -p zoraya-shell` 154 passed, `cargo test -q -p zoraya-command` 17 passed;
`make -f ports/Makefile wget curl` from a ports tree with no OpenSSL, curl or wget built: both link (`HAVE_SIGSETJMP`
undefined in wget's `config.h`); `make test-posix POSIX_SCRIPTS="tests/posix/invariants.script
tests/posix/zosh-userland.script tests/posix/zosh.script" POSIX_ACCELS=auto`: `PASS: 86`, `PASS: 228`, `PASS: 94`
steps (HVF); `make check` passed. Not run: `make check-ports` (its `__linux__` step fails on an incremental tree,
backlog), `make test-apps`, NetSurf and Python builds (neither detects `sigsetjmp`: Python's source has none,
NetSurf's is Duktape's compile-time option).

### For the architect (fix pass)

- ADR-0336 item 2's independent review of `tr`, `expr` and `diff` is still open (finding 3): waive it or have it done.
- The image cache of ADR-0212 (item 10(a), ordered first on 2026-11-04) is not built; deferring it needs your word.
- Refusing undescribed options (finding 4) makes `/bin/find`, `/bin/test`, `/bin/env` and `/bin/xargs` with `-` words
  unusable from `zosh` until a rule exists (ADR-0344 open question); the native `find` and scripts run by `sh` are
  unaffected.
- `tar` follows the narrower form (finding 5); ADR-0336 item 5's examples (`tar -c -f` creating, `-x -C DIR` as a
  tree) need per-mode option kinds (ADR-0344 open question).

## Appendix: options each description lets through

```
awk: -F, --field-separator FS -v, --assign -f, --file PROGFILE
basename: -a, --multiple -s, --suffix SUFFIX
bzcat: 
cat: -u, --unbuffered -v, --show-nonprinting
cc: -c, --compile-only -o, --output FILE -I, --include DIR
chmod: -R, --recursive -f, --silent -v, --verbose
cmp: -l, --verbose -s, --silent -n, --bytes N
cp: -r, --recursive -R, --recurse -a, --archive -f, --force -n, --no-clobber -p, --preserve -i, --interactive -v, --verbose
cpio: -i, --extract -o, --create -t, --list -d, --make-directories -v, --verbose
cut: -b, --bytes LIST -c, --characters LIST -f, --fields LIST -d, --delimiter DELIM -s, --only-delimited
dash: 
date: -u, --utc -d, --date STRING -r, --reference FILE
dd: 
diff: -u, --unified -U, --unified-context N -r, --recursive -N, --new-file -q, --brief -i, --ignore-case -b, --ignore-space-change -w, --ignore-all-space -B, --ignore-blank-lines -s, --report-identical-files -a, --text
dirname: 
du: -a, --all -c, --total -h, --human-readable -k, --kilobytes -s, --summarize -x, --one-file-system -d, --max-depth N
echo: -n, --no-newline -e, --escapes -E, --no-escapes
env: -i, --ignore-environment
expr: 
file: -b, --brief -h, --no-dereference -L, --dereference -s, --special-files
find: -L, --follow
grep: -E, --extended-regexp -F, --fixed-strings -i, --ignore-case -v, --invert-match -c, --count -l, --files-with-matches -L, --files-without-match -n, --line-number -o, --only-matching -r, --recursive -R, --dereference-recursive -s, --no-messages -w, --word-regexp -x, --line-regexp -q, --quiet -H, --with-filename -h, --no-filename -A, --after-context N -B, --before-context N -C, --context N
gunzip: -c, --stdout -f, --force -k, --keep -t, --test
gzip: -c, --stdout -d, --decompress -f, --force -k, --keep -t, --test
head: -n, --lines N -c, --bytes N -q, --quiet -v, --verbose
ln: -s, --symbolic -f, --force -v, --verbose
ls: -a, --all -A, --almost-all -l, --long -1, --one-per-line -d, --directory -h, --human-readable -R, --recursive -F, --classify -p, --slash -r, --reverse -t, --time -S, --size -s, --size-blocks -i, --inode
make: -C, --directory DIR -f, --file FILE -j, --jobs N
mkdir: -p, --parents -m, --mode MODE -v, --verbose
mv: -f, --force -n, --no-clobber -i, --interactive -v, --verbose
od: -A, --address-radix RADIX -t, --format TYPE -j, --skip-bytes N -N, --read-bytes N -c, --chars -v, --output-duplicates
patch: -R, --reverse -p, --strip N -i, --input PATCHFILE
pdpmake: -C, --directory DIR -f, --file FILE -j, --jobs N
printf: 
realpath: -s, --no-symlinks -q, --quiet
rm: -f, --force -r, --recursive -R, --recurse -v, --verbose
rmdir: 
sed: -n, --quiet -i, --in-place -E, --regexp-extended -r, --regex-extended
seq: -w, --equal-width -f, --format FORMAT -s, --separator SEP
sh: -c, --command COMMAND
sleep: 
sort: -b, --ignore-leading-blanks -c, --check -d, --dictionary-order -f, --ignore-case -n, --numeric-sort -r, --reverse -s, --stable -u, --unique -k, --key KEYDEF -t, --field-separator SEP -o, --output FILE
stat: -L, --dereference -c, --format FORMAT
tail: -n, --lines N -c, --bytes N -f, --follow
tar: -c, --create -x, --extract -t, --list -z, --gzip -j, --bzip2 -v, --verbose -f, --file ARCHIVE -C, --directory DIR
tcc: -c, --compile-only -o, --output FILE -I, --include DIR
tee: -a, --append -i, --ignore-interrupts
test: 
touch: -a, --access -c, --no-create -m, --modification -r, --reference REFFILE
tr: -c, --complement -C, --complement-chars -d, --delete -s, --squeeze-repeats
true: 
uname: -a, --all -m, --machine -n, --nodename -o, --operating-system -p, --processor -r, --kernel-release -s, --kernel-name -v, --kernel-version
uniq: -c, --count -d, --repeated -u, --unique -i, --ignore-case -f, --skip-fields N -s, --skip-chars N -w, --check-chars N
wc: -c, --bytes -l, --lines -m, --chars -w, --words -L, --max-line-length
which: -a, --all
xargs: -0, --null -r, --no-run-if-empty -t, --verbose -n, --max-args N
xxd: -p, --plain -r, --reverse -i, --include -c, --cols N -g, --groupsize N -l, --len N -s, --seek N
yes: 
zcat: 
```
