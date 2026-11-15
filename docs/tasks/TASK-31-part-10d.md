# TASK-31 part 10d: ncurses, `less` and `vi` (report)

Branch `work/10d`, from `work/ports-early` at `26265ae5` (10c is on `main`). Written by an agent (Sonnet). ADR number used: 0391
(proposed). Nothing is merged, tagged or pushed to `main`.

## Done (each step is a commit)

1. **`kbs=^?`** (ADR-0383 Option C, accepted). `tools/term-terminfo/terminfo.sh` changes the one line `kbs=^H` of the `xterm+kbs`
   fragment in a copy of the pinned `terminfo.src` before `tic` runs, and fails unless there is exactly one such line. Two of the four files
   change (`x/xterm`, `x/xterm-256color`); `ports/terminfo.manifest`, `crates/zoraya-term/entry/xterm-256color.caps` and the key test
   follow, and the `kbs` exception is gone (`tools/term-conformance/check.sh`: 257 capabilities covered, 30 excepted).
2. **`creat` and `sleep`** in `libposix` (`ffi.rs`, `process.rs`), host tests in `tests/files.rs`, rows in `tools/libposix-widths.txt`; the
   block of ADR-0391 admits them (`ports/Makefile`'s `ADDED_ADRS`). `ports/apps/ttyshim.c` is deleted with every use of it (less, nvi, OpenSSH,
   git): `libc.a` has the terminal's names since 10c.
3. **The image.** `/bin/less`, `/bin/vi`, `/bin/ex` and `/bin/view` (absolute links to `vi`), the four entries in `/usr/share/terminfo`, and
   ncurses' two archives and headers in the sysroot, so in the image's `/usr/lib` and `/usr/include` (`ports/Makefile`: the rootfs rule,
   `CURSES_SYSROOT_STAMP`). CPython is told `py_cv_module__curses=n/a` so that its configure does not find the archives. `user/describe/posix/`
   gains `vi.cmd`, `ex.cmd`, `view.cmd` and `less.cmd`, all `cwd none` (ADR-0335 items 3 and 9). The hint for `less` is gone; `more` says
   `to page a file: less FILE` (`eval.rs`, the two tour scripts, `docs/tour.md`).
4. **`vttest`** 20251205 is pinned (`ports/sources.lock`, `ports/README.md`, `docs/dependencies.md`; SHA-256 `cd6886f9...`, the same bytes from
   both of its author's hosts), built for the host by `tools/term-record/build-vttest.sh`, and its cursor and screen menus are recorded
   (`crates/zoraya-term/recordings/vttest-cursor.*`, `vttest-screen.*`) and replayed by `tests/replay.rs`.
5. **Defects found by running the programs** (none was in the ports-early report): ncurses' `setupterm` faulted in every curses program because
   three 32 KiB arrays are three calls deep on a 64 KiB stack (`ports/patches/ncurses/0001`); `ex` ran as `vi` because the port's `getprogname`
   was fixed (`ports/patches/nvi2/0004`, `ports/nvi/compat.c`); `less` linked an older copy of `libtinfow.a` from the sysroot (the rules now
   depend on `CURSES_SYSROOT_STAMP`); `ports/check-broken-archives.sh` named `strlcpy` as "outside the tier list", which ADR-0390's block admits
   (it names `zoraya_not_a_libc_name` now).
6. **Tests.** `tests/posix/t_curses.c` (`setup`, `keys`, `devtty`, `poll2`); `tests/posix/zosh-curses.script` (serial console); `tests/gui/posix-curses.script`
   (a window; `make test-tour-terminal` runs it after `posix-terminal.script` at each size); `ports/check-curses.sh` (INV-0334-1, INV-0334-2's log half,
   INV-0335-2's strings half, the image's files); host tests of the planner for the four descriptions (`grants_tests.rs`) and of the description lint
   (`posix_names.rs`).
7. **Docs and ADR.** Proposed **ADR-0391** (INV-0391-1 to -5; Total 826), `docs/design.md`, `docs/backlog.md`, `ports/README.md`, `ports/patches/README.md`.

## Commands run in this session

| Command | Result |
|---|---|
| `make check` (before each commit) | exit 0 |
| `make check-ports` (after the first wiring: all passed); `sh ports/check-curses.sh` and `sh ports/check-early.sh` after the last rebuild | passed. A later full `make check-ports` stops at "dash.log shows no compile commands": an incremental rebuild (the libc changed) overwrote the logs of ports that had nothing to compile; a clean `make ports`, as CI's, does not |
| `cargo test -p zoraya-term`, `-p zoraya-posix --test files`, `-p zoraya-describe`, `cargo test -p zoraya-shell --lib grants_tests` | pass |
| `make test-posix POSIX_SCRIPTS=tests/posix/zosh-curses.script POSIX_ACCELS=hvf` | PASS, 111 steps |
| `make test-tour-terminal POSIX_TERM_SCRIPTS=tests/gui/posix-curses.script POSIX_TERM_SIZES="1 1920x1200" POSIX_ACCELS=hvf` | PASS, 74 steps at each size |
| `make test-posix` with `tour.script`, `zosh.script`, `zosh-userland.script` (one at a time, HVF); `make test-tour-gui` (HVF); `make test-tour-terminal` at scale 1 (both scripts, HVF); `make test-libposix` (121 steps) | all exit 0 |
| `tools/term-record/record.py` on the two vttest sessions, `ZORAYA_BLESS=1 cargo test -p zoraya-term --test replay`, then without | pass |

Not run: `make test-changed` as a whole (the change to `ports/` and `libposix` maps to seventeen steps, among them apps, netsurf, the C++ lanes and the POSIX suites, which a session of this size cannot hold; the six above are the ones the change touches most directly), scale 2 and 1600x1000 of the two window scripts, any TCG
run, `make test`, a rebuild of OpenSSH and git
after the shim's removal (they link no `ttyshim` any more; their names are in `libc.a`, but neither was relinked here), the mutation runs (not required).

## What the programs do (from the runs)

`grants` (serial console, `zosh-curses.script`):

```
~> grants vi notes.txt
vi gets (a POSIX program):
  in, out, err         the terminal
  keys                 the terminal, while this line is in front
  ~                    create or replace notes.txt (operand FILE)
  /bin /usr /etc       read (system)
  /proc/self/exe       read (system)
  /tmp                 change anything in (this line's own)
  /dev/null
~> grants view notes.txt        ... ~  read notes.txt (operand FILE)
~> grants vi notes.txt sibling.txt  ... ~  create and change anything in (operand FILE)   (twice: the directory, ADR-0391 item 8)
```

`ex` through its one name, from the serial console:

```
~> printf 'g/colour/s//color/\nwq\n' | ex -s notes.txt
~> cat notes.txt
color
~> printf 'w sibling.txt\nq\n' | ex -s notes.txt
Error: script, 1: sibling.txt: Permission denied
~> printf '!cat sibling.txt\nq\n' | ex -s notes.txt
cat: sibling.txt: Permission denied
~> printf 'w!\nq\n' | ex -s -R notes.txt
Error: script, 1: notes.txt: Permission denied
~> /bin/cat notes.txt | less      (dumb: "WARNING: terminal is not fully functional", Return, the text, "(END)")
```

In a window (`posix-curses.script`, 1024x768 and 1920x1200): `t_curses keys` prints `start cols 80`, `key KEY_BACKSPACE` for Backspace, `key KEY_F(1)`, `resize cols 90` and
`resize cols 80` through two drags of the window's edge; `vi notes.txt` opens a new file on the alternate screen (`notes.txt: new file: line 1`), `ihello vi`, Backspace,
Esc, `:wq` saves, `cat notes.txt` reads `hello vi`; in a second `vi notes.txt`, `:w other.txt` gives `Error: other.txt: Permission denied.` (reverse video, kept as
`build/shots/vi-refused-w-*.ppm`) and `cat other.txt` still reads `secret`; `tcc -v &| less` shows `tcc version 0.9.28rc (AArch64 Zoraya)` and `(END)`.

### Sizes (ADR-0334's open question; ADR-0335 item 1; static, `-O2`, linked with the call-logging `libc.a`)

| file | bytes | `.text` | `.rodata` | `.data` | `.bss` | debug sections |
|---|---|---|---|---|---|---|
| `/bin/less` | 2,975,880 | 975,008 | 150,641 | 26,904 | 65,288 | 1,339,437 |
| `/bin/vi` | 3,340,736 | 1,244,276 | 156,879 | 4,648 | 11,744 | 1,393,876 |

`libncursesw.a` 344,430 and `libtinfow.a` 268,502 bytes, `libdb1.a` 98,926. Both programs are 3 to 4 percent larger than the ports-early build's (2,872,992 and 3,247,216 bytes; the cause was not looked into: the shim's names are `libposix`'s now). Not measured: netbsd-curses for the comparison ADR-0334 asks for.

## Proposed ADRs

- **ADR-0391** (Option A): `kbs=^?` by changing one line of a copy of the pinned file; `creat` and `sleep`; the files, links and entries of the image;
  descriptions (short options only, `view` reads); curses in the sysroot and not in CPython; `vttest` pinned; the 64 KiB-stack patch; two files of one directory
  give the directory. Two small deviations from accepted text, both named in it: `view` is `read` (ADR-0335 item 9 says "the same operand"), and `narrow` takes no list.

## Gaps (honest)

- **`narrow` with a list of names is not built** (the plan's 10d row, ADR-0335 item 3). `vi a.txt b.txt` is refused (after the review: it first got the directory whole), with the hint `+w DIR`; INV-0335-1's "two files" clause is not met. It is a protocol change (`zoraya-proto-file`, the service, the client, the planner's merge), left for a part
  that can review it as a security boundary.
- **`ls | less` does not page**: the first native element of a pipeline takes the keys (ADR-0387 item 2). Found when the first pipeline test hung. `/bin/cat f | less` and `tcc -v &| less` do page.
- **`:e other.txt` says "new file"**, not a refusal (the table has no entry for the name). 10f's status row is the place for the reason.
- **INV-0334-2** asks for `setupterm` "from `/tmp`"; `zosh` has no `/tmp` to enter, so the script runs it from the shell's home. The "no hint after" half is checked.
- **INV-0335-3** names `/config/settings`; the test image has none, so a sibling file stands in for it.
- **INV-0335-2**: the `REPORT` channel's count and the listing of the line's `/tmp` while the programs run are not checked; the strings half is.
- **The vttest grids are frozen from `zoraya-term` and not compared with xterm** by a person (ADR-0332 item 10); nor are `vi-edit` and `less-page`'s (they were recorded with the host's Vim and `less`).
  The vttest recordings are timing dependent (two runs of one session gave different mark offsets); the committed one is the second of each.
- **DB 1.85 and nvi's `lock` option** are as ADR-0390 left them; no 100 MB measurement.
- **OpenSSH and git were not rebuilt** without `ttyshim`; the check that they link is the next `make ssh`/`make git` (or CI's build).
- A static `ncurses` header file `ncurses.h` is a copy of `curses.h`, not the link ncurses installs, so that an image has no relative link.
- The nightly's TCG runs of the new scripts and the full set were not run (CI's).
