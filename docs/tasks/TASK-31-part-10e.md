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
