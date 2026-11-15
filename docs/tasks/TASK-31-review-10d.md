# TASK-31: review of part 10d, ncurses, `less` and `vi` (branch `work/10d`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff 26265ae5..c945d133` (5 commits on `work/ports-early`, 55 files). Read against ADR-0334, ADR-0335,
ADR-0383 (Option C), ADR-0389, ADR-0390, the proposed ADR-0391, the plan's 10d row
(`docs/research/stage-10/plan.md`) and the part report (`docs/tasks/TASK-31-part-10d.md`). The test scripts were
read against the runner's semantics (`tools/test-runner/src/script.rs`, `gui.rs`), not by their names.

## What was run

In the worktree of `work/10d` at `c945d133`, every break reverted after its run (`git status` clean):

| Command | Result |
|---|---|
| `make check` | `make test: all checks passed` |
| `make check-terminfo` | passes; with `terminfo.sh`'s replacement changed back to `kbs=^H` it fails (`the compiled entries differ from ports/terminfo.manifest`) |
| `cargo test -p zoraya-shell --lib grants_tests` | 35 pass; with `view.cmd`'s operand changed to `create` one fails (`vi_with_no_file_..._read_only`) |
| a temporary probe test of the planner with the real descriptions (removed) | see findings 1 and 13 |
| `sh ports/check-curses.sh build/ports/logs build/sysroot build/rootfs-posix` | passes; fails with a `libedit.a` added to a copy of the sysroot; fails with the configure log's home line changed to `yes`; **passes** with both directories missing (finding 7) |
| `nm -g build/sysroot/usr/lib/libc.a` | defines all seven names `ttyshim.c` defined (`tcflush`, `cfget/cfset{i,o}speed`, `creat`, `sleep`) |
| `strings -a` of the built `/bin/vi` and `/bin/less` | finding 14 |
| `git merge-tree --write-tree HEAD 1f1463e6` | `CONFLICT (content): Merge conflict in ports/check-broken-archives.sh` (finding 10) |
| `grep -c '^\| INV-' docs/adr/invariants.md` | 826, the Total |

Not run: any QEMU step (the script findings follow from the runner's code, cited), `make test-changed`, a ports build.

## Findings

### 1. must-fix: `vi +10 notes.txt` and `less +G log` are given the whole current directory

*Where:* `user/describe/posix/vi.cmd:6`, `ex.cmd`, `view.cmd`, `less.cmd:6` (the `[FILE...]` operand); the planner's
merge, `crates/zoraya-shell/src/grants.rs:464` (`by_path`: two names of one path give the directory).

*What:* nvi turns a historic `+CMD` argument into `-c CMD` (`common/main.c:513-525` of the pinned nvi2: `+` is
`-c$`), and less takes every argument that starts with `+` as an option (`main.c:286`, `isoptstring`). The shell's
parser takes `+10` as a second file operand. With `notes.txt` that is two names of one directory, and ADR-0391
item 8's fallback gives the directory whole. Probe with the real description: `vi +10 notes.txt` plans `/w`, name
`None`, rights `0xc7` (read, write, create): every file of the current directory may be replaced and any name created.
`less +G notes.txt` plans the whole directory to read. `vi + notes.txt` (go to the end) is the same.

*Scenario:* a person types the common idiom `vi +42 main.c` in `~/proj`. `vi`, and every `:!` child of it, can
rewrite any file of `~/proj`, though the line named one file. Nothing on the screen says so unless the person ran
`grants` first.

*Fix:* the descriptions must not take a `+` word as a file: either a `+CMD` form of `-c` the parser knows (a text
argument), or a refusal of an operand that begins with `+` with a hint (`write -c CMD`). Add a host test with
`+10` for all four descriptions.

### 2. must-fix: two files of one directory widen the grant to the directory, against accepted ADR-0335 item 3, before the architect has decided

*Where:* ADR-0391 item 8; `INV-0335-1`'s Status, `docs/adr/invariants.md:593`; the plan's 10d row ("`narrow` with
a list of names in the file protocol"; done when INV-0335-1 to -4 are checked).

*What:* ADR-0335 item 3 (accepted) says `vi a.txt b.txt` needs a connection narrowed to both names and that this "is
built with this port". The branch gives the directory with the union of the rights (`0xc7`, probe above), records it
in a proposed ADR and marks INV-0335-1 "checked ... partly". A proposed ADR does not make a deviation binding:
merged as it stands, the image ships a grant wider than the accepted decision until the architect reads ADR-0391.
`grants` does show it ("create and change anything in", `zosh-curses.script:50-52`), so it is truthful, but only to
a person who asks. Finding 1 shows how easily a line reaches this case without the person meaning two files.

*Fix:* until `narrow` takes a list, fail closed: refuse a line whose file operands give two names of one directory
to a `cwd none` editor or pager, with the hint `+w DIR vi a b` (or `+r` for `less`/`view`), which keeps the person's
choice explicit and is the least committing option. Or get the architect's decision on item 8 before the merge.
Either way, add a host test of the two-file plan (there is none in `grants_tests.rs`).

### 3. must-fix: the "no hint follows" half of INV-0334-2 is not checked

*Where:* `tests/posix/zosh-curses.script:12-15`; the runner, `tools/test-runner/src/script.rs:349-351` and `570-573`.

*What:* `x TEXT` fails only if TEXT lies between the last two matches, and it is evaluated when it is reached: it
looks back, never forward. Placed after `< t_curses: setup passed` it searches the output up to "setup passed",
while the shell's refusal hint would come after the program exits, before the prompt that the next line waits for.
Nothing searches that span. Also, `t_curses` runs from the shell's home, which its line is given to read, so a
lookup of `~/.terminfo` would succeed quietly (ENOENT) and produce no hint even with the step in the right place; the
ADR asks for `/tmp` for this reason.

*Scenario:* ncurses is rebuilt with the home lookup on; the configure-log half catches it, but if the log and the
build ever disagree (an incremental rebuild overwrote a log, as the part report describes), the in-QEMU half passes.

*Fix:* put `x hint` after the `< \r\n~>` that follows the program; run `t_curses setup` from a directory that
does not cover the home (`cd /usr` or similar), so a home lookup would be refused and hinted. Correct the Status text.

### 4. must-fix: INV-0335-3's script steps do not check what the Status column says

*Where:* `tests/posix/zosh-curses.script:66-74`; `docs/adr/invariants.md:595`.

*What:* each `x SIBLING-SECRET` sits right after its `>` and before its `< \r\n~>`, so by finding 3's rule it checks
the output of the step *before* it. The `x` after `!cat sibling.txt` searches `\r\n~>` of the earlier `cat
sibling.txt` (nothing); the `x` after `e sibling.txt` happens to search the `!cat` run's output; the `x` after
`r sibling.txt` searches the `:e` run's output; the output of `r sibling.txt` is searched by nothing. No step
asserts that the child ran and was refused (the report's `cat: sibling.txt: Permission denied` is not expected by
the script), so an `ex` that failed for another reason, or a missing `/bin/sh`, passes as well.

*Fix:* move each `x` after its prompt match, and expect the refusal text (`< cat: sibling.txt: Permission denied`;
for `:e` and `:r` whatever nvi prints for ENOENT). The Status column must not claim `:r` until then.

### 5. should-fix: the `ex -R` save test cannot fail

*Where:* `tests/posix/zosh-curses.script:75-80`.

*What:* `notes.txt` already holds `color` from the `wq` above; `w!` of the unchanged buffer would write the same
bytes, so `cat notes.txt` reads `color` whether the table refused the write or not. The only assertion is the prompt.

*Fix:* change the buffer first (`s/color/CHANGED/`, then `w!`), expect `notes.txt: Permission denied`, and expect
`color` afterwards.

### 6. should-fix: the window script's "refused a sibling" would pass with a grant of the whole directory

*Where:* `tests/gui/posix-curses.script:57-75`.

*What:* `:e other.txt` first, then `:w other.txt`. If `vi` held the directory, `:e` would read `secret` into the
buffer and `:w` would write `secret` back; `cat other.txt` then reads `secret` and the script passes. The screens
that would show the difference (`vi-refused`, `vi-refused-w`) are only kept, not read. INV-0335-1's Status names
this script as a check of the same property "through `vi` in a window".

*Fix:* drop the `:e` and write the edited buffer (`hello vi`) to `other.txt`, so a successful write changes the file;
or `screen` the status line after `:e` (`other.txt: new file`).

### 7. should-fix: `ports/check-curses.sh` passes when the sysroot or the image is missing

*Where:* `ports/check-curses.sh:22` (`[ -d "$d" ] || continue`), `:47`, `:54`.

*What:* run with two directories that do not exist it prints `check-curses: passed` (run above). INV-0334-1 and the
file half of INV-0391-3 then check nothing. The Makefile passes the right paths today, but a renamed `SYSROOT` or
`ROOTFS` would turn the check off silently.

*Fix:* fail when either directory, `$rootfs/bin` or `$rootfs/bin/vi` is missing, as the log half already does.

### 8. should-fix: ADR-0391's text contradicts itself and its invariants

*Where:* `docs/adr/0391-what-part-10d-decided-where-the-adrs-are-silent.md:16`, `:135`, `:149`.

*What:* line 16 says items 6 and 8 are "each narrower than the accepted text"; item 8 is wider (the ADR says so at
item 8 and in "Tensions"). The Follow-up still lists "`vttest`'s sessions recorded", which item 10 says is done.
INV-0391-4's Check is `make test-gui`, its Status `make test-tour-terminal` (the script is run by the latter).

*Fix:* "item 6 narrower, item 8 wider"; drop the vttest follow-up; make the Check say `make test-tour-terminal`.

### 9. should-fix: `sleep` is said to round up "as musl's does"; musl's truncates

*Where:* `crates/zoraya-posix/src/process.rs:58-60`; ADR-0391 item 1 (line 70); INV-0391-1.

*What:* the pinned musl's `src/unistd/sleep.c` returns `tv.tv_sec` of the remainder, which drops the fraction.
Rounding up is allowed by POSIX and is the safer choice (never too short a re-sleep), but the stated reason is false.
Also the FFI function `sleep` itself has no test; only `unslept_seconds` does (a `sleep` that always returned 0
would pass INV-0391-1's check).

*Fix:* say "rounded up (musl truncates; a re-sleep for the result is then never short)"; a C step in
`test-libposix` that interrupts `sleep(3)` with an alarm and checks the result would cover the FFI path.

### 10. should-fix: the change to `ports/check-broken-archives.sh` is not the same as ports-early's, and the two conflict

*Where:* `ports/check-broken-archives.sh:34-37`; `work/ports-early` commit `1f1463e6`.

*What:* both replace the `strlcpy` case, but ports-early names `zoraya_not_a_tier_name` and adds a three-line
comment; 10d names `zoraya_not_a_libc_name` and has none. `1f1463e6` is not an ancestor of this branch, and
`git merge-tree --write-tree HEAD 1f1463e6` reports a content conflict in that file. Whichever of the two merges
second will stop on it. (Not resolved here, as asked.)

*Fix:* rebase or merge `work/ports-early`'s head into `work/10d` and take ports-early's version of the file.

### 11. should-fix: INV-0391-5's host tests do not cover the cases where the plan differs from ADR-0335 item 3

*Where:* `crates/zoraya-shell/src/grants_tests.rs:1023-1093`.

*What:* the tests cover one file in and outside the current directory, no file, and `-R`/`view`/`less` read. They do
not cover two files of one directory (finding 2), `+CMD` (finding 1), or `-R` in a cluster (`ex -Rs`; the probe shows
it plans read, correctly, but nothing keeps it so). The Status claims "plan to the grants of ADR-0335 item 3".

*Fix:* add those cases with the plan they must give.

### 12. note: `view` as a `read` grant is safe and recorded

`view.cmd` gives the operand `read`; nvi's `:set noro` and `:w!` then meet the table and fail. It is narrower than
ADR-0335 item 9's "the same operand", recorded in ADR-0391 item 6, its Tensions, its open questions and
`docs/design.md`. `view.cmd:12` cites "ADR-0391" in the help text as if it were decided; it is proposed.

### 13. note: the shell permutes options, nvi does not

`vi notes.txt -R` plans `notes.txt` read only (probe: rights `0x81`), while nvi (musl's `getopt` stops at the first
operand) opens `notes.txt` writable and a second file named `-R`. The grant is narrower than what nvi does, so the
failure is a refused `:w`, not a leak. `help vi` could say to put options first.

### 14. note: the strings half of INV-0335-2 covers only the system start-up file

`check-curses.sh:55` looks for `/etc/vi.exrc`, `openpty`, `flock`. The built `/bin/vi` still holds `.exrc` and
`.nexrc` (`_PATH_EXRC`, `_PATH_NEXRC`, used by `:mkexrc` too), so the strings check cannot tell whether the home
and local start-up reads were removed; that rests on `patches/nvi2/0001` alone. The Status says "partly" for other
reasons; it could name this one too.

### 15. note: the `ls` step does not check that the directory holds the same names

`zosh-curses.script:81-84` expects `notes.txt` and `sibling.txt` but would pass with a third name (a temporary or
backup file), which INV-0335-1 ("the directory holds the same names afterwards") rules out. Expect the prompt with
`^` right after the two names.

### 16. note: `terminfo.sh`'s `awk` does not end the fragment

`tools/term-terminfo/terminfo.sh:82-85`: `f` is reset only when the line is changed. If a later release's
`xterm+kbs` lost its `kbs=^H`, the next `\tkbs=^H,` of any later entry would be changed and the count would still be
one. The manifest catches it for the four files (a revert of the replacement fails `make check-terminfo`, run above),
so the image is safe; resetting `f` on the next line that starts a new entry would make the script's own message true.

## Checked and found sound

- **ncurses' heap patch** (`patches/ncurses/0001`): the three arrays become `malloc`ed buffers of the same sizes; no
  `sizeof` of a converted array remains in `read_entry.c` (grep of the patched tree); a failed `malloc` gives
  `TGETENT_NO`; every path frees; the strings of a read entry are copied before the scratch is freed (as with the
  stack array before). The host `tic` is unpatched.
- **nvi's program name** (`patches/nvi2/0004`, `ports/nvi/compat.c`): the base name of `argv[0]`, bounded copy,
  `vi` when `argv[0]` is null or ends in `/`.
- **`kbs=^?`**: one line of a copy, the manifest and the conformance dump follow, the exception is gone, the key test
  compares Backspace with the entry's `kbs`; the break bites.
- **`creat` and `sleep`**: `O_WRONLY|O_CREAT|O_TRUNC` through `openat` (a cancellation point); `sleep` through
  `nanosleep_until` (cancellation, signals, remainder); widths rows `int 4` and `unsigned_int 4` match the C types.
- **The shim's removal**: `libc.a` defines all its names; OpenSSH and git were not relinked (the report says so).
- **The four descriptions**: `cwd none`; only the short options listed; no option names a file; `-R` maps to read
  (also in a cluster); `-c` is text, and what it runs meets the table.
- **Status row and grant line**: nothing in this diff writes to either (both are 10f's); `vi`'s and `less`'s messages
  stay in the program's grid; `grants`' words come from the trusted descriptions and the planner.
- **INV-0390-x**: removing "partly: CI builds none" is true now: `ports` depends on `rootfs`, which depends on
  `LESS_BIN` and `NVI_BIN`, and `ci-build` runs `ports` and then `check-ports`.
- **The Total**: 826 rows, 826 in the table; `make check` passes.
- **INV-0334-1's archive half**, the configure-log half of INV-0334-2, INV-0391-2 and the `view` read plan each fail
  when broken by hand (runs above). INV-0335-4's lint was read, not broken: it judges an option by its argument's name
  (`FILE`, `DIR`, `PATH`, `ARCHIVE`), and none of the four descriptions has an argument that names a file.
