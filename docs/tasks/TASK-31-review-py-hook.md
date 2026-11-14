# TASK-31: review of the python3 build hook (branch `work/py-hook`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...work/py-hook` at `181406a` (`98a872b` "python3 build hook for built-in modules
(PYTHON_EXTRA_MODULES)" and the report commit): `ports/Makefile`, `Makefile`, `tests/python-ext/`,
`tests/shell/python-ext.script`, the wiring in `tools/` and the docs. Read with the code around it (the python port's
markers and `PYTHON_SRC_TREE`/`HOSTPY_TREE`, `check-python-cache`, the CI workflow's build and tests jobs,
`tools/test-par.sh`), and with what the implementer's worktree holds after its run (`build/ports/work/python-build-ext`,
`build/rootfs-python-ext`, `build/ports/logs/`, `build/logs-par/build.log`).

What was run (this review): `KEEP_GOING=1 make check` in the branch's worktree (23 of 23 passed); `make -p -n -f
ports/Makefile` of `origin/main`'s and the branch's `ports/Makefile`, compared; the hash pipeline of `ports/Makefile:864`
on a scratch directory with a symbolic link and with a name holding a space; the variable's propagation by the
environment and by a top-level command line (`make -p -n`). No CPython build, no QEMU (the implementer ran them).

## Findings

1. **error (procedural, before merge).** `Makefile:750` (`test-python-ext`), `.github/workflows/test.yml` (tests job),
   `Makefile:697`. **What.** The step builds a second CPython inside the CI tests job. The build artifact leaves out
   the python source tree, the host python and the build trees (`test.yml`, "Pack build/ and target/"), and
   `Makefile:697` says why: "so that the test jobs need neither the source tree nor the host python". In a tests job
   `make test-python-ext` must therefore unpack CPython again (`PYTHON_SRC_TREE`), build the host python
   (`HOSTPY_TREE`), cross-build CPython with `-j3`, compile the library, and then boot under TCG, all inside the job's
   45 minutes (the job timeout matches on `test-python`). None of that has run anywhere: the report lists the CI
   candidate as not run, and the bare layout's tests job installs only `qemu-system-arm clang zstd`, not the build
   job's packages. **Scenario.** The first nightly or candidate-full run on `main` after the merge times out or fails
   in the host python's configure in the tests job, and the red nightly becomes the next session's first item.
   **Fix.** Run the candidate (`gh workflow run test.yml --ref work/py-hook`; the plan includes `test-python-ext`
   since `Makefile` changed) and merge only when the `make test-python-ext` job is green, with its time noted in the
   report. If it is close to 45 minutes, build the `-ext` python in `ci-build` instead and add
   `build/rootfs-python-ext` to the artifact (and the ports cache paths, with `tests/python-ext/**` in the key).

2. **warning.** `ports/Makefile:864`. **What.** The hash does not cover what the docs say it covers ("every file of
   the directory, by name and content", `docs/design.md`). (a) `find . -type f` leaves out symbolic links and does not
   descend into a linked directory, so a `lib/libfoo.a` that is a link to a library built elsewhere is not hashed.
   (b) `xargs` splits names on blanks; for a name with a space `shasum` reports "No such file or directory" for each
   half, and since the pipeline's status is the last `shasum`'s, the recipe goes on with a hash that leaves that file
   out. Both shown on a scratch directory: the hash stayed `d5f7e5f5...` after the linked library's content changed,
   and after the content of `my mod.c` changed. **Scenario.** Someone keeps the module's library in its own build tree
   and links it into DIR; after rebuilding the library, `make -f ports/Makefile python PYTHON_EXTRA_MODULES=DIR` says
   nothing to do and the image keeps the old library. **Fix.** `find -L . -type f -print0 | LC_ALL=C sort -z | xargs -0
   shasum -a 256`, and fail if any `shasum` fails (write the list to a file and check its status, or `set -o pipefail`
   under `bash`); or refuse names outside `[A-Za-z0-9._/+-]` (finding 3) and keep `-L`.

3. **warning.** `ports/Makefile:862` to `:864`, `:923` to `:925`. **What.** DIR is put into shell commands and into a
   `sed` replacement unquoted and unchecked. A space splits it (`$(abspath)` makes two words of it; `test -f a b/Setup`);
   a `'` breaks the `sed` quoting; a `|` ends the `s|...|...|` expression; and a `&` in the path is `sed`'s "the
   matched text", so `-I@EXTRA@` becomes `-I/work/R@EXTRA@D/mods` for DIR `/work/R&D/mods`. DIR is the builder's own
   input, so this is robustness, not a boundary; but each case fails late (in makesetup or the compile) with a message
   that does not name the cause. **Fix.** At the top of the `ifneq ($(PYEXT),)` block, refuse a DIR whose absolute
   path has a character outside `[A-Za-z0-9._/+-]` with a clear message (`$(if $(shell ...),$(error ...))` or a check
   in the hash recipe, which runs first).

4. **warning.** `tools/test-par.sh:169`. **What.** The ports are built once before the lanes "so that the lanes do not
   build them at the same time", but `test-python-ext` is not in that list, so with `PAR_LONG=1` its step builds the
   host python (if missing) and a whole CPython with `-j3` inside the `test-python` lane while up to three QEMU guests
   and the host-test job run in the other lanes. **Scenario.** `make test-par PAR_LONG=1`: the CPython build competes
   with three guests for CPU and memory (the host's memory limits, `CLAUDE.md`), and the guests' timing tests run on a
   loaded host. **Fix.** Add `if has test-python-ext; then "$make" -f ports/Makefile python-ext-sample && "$make" -f
   ports/Makefile python PYTHON_EXTRA_MODULES=build/python-ext-sample; fi` to the build phase (the step then finds the
   stamp up to date; the hash is stable, since `zoraya-ar` writes no dates).

5. **note.** `tests/shell/python-ext.script`. **What.** The test is real: if the hook did nothing,
   `build/rootfs-python-ext` would not exist and the recipe's `cp -R` fails; if the hook built python without the
   module (the implementer's first try, and the case checked here in the built tree: `Modules/config.c` of
   `python-build-ext` has `{"zsample", PyInit_zsample}`, `MODOBJS` starts with `Modules/zoraya-ext/zsample.o`), the
   script never sees `42` (the echoed command line holds `6, 7, 0`, not `42`). But it then fails only at the runner's
   900 s timeout. **Fix.** Add `! Traceback` and `! ModuleNotFoundError` to the script, so that a missing module or a
   failed `assert` ends the run at once.

6. **note.** `ports/Makefile:923`. **What.** The touch of `Makefile` and `Modules/config.c` to 2000 is there because
   `Setup.local` is written after configure has already run makesetup. CPython's configure only creates
   `Modules/Setup.local` "if test ! -f Modules/Setup.local" (`configure:37335` of the unpacked 3.15.2), so writing it
   into `$(PYTHON_BUILD)/Modules/` before configure would let configure's own makesetup read it, without the time
   trick. **Fix.** Optional: `mkdir -p $(PYTHON_BUILD)/Modules` and write `Setup.local` (and the link) before the
   configure line; drop the `touch`.

7. **note.** `ports/Makefile:924`, `docs/design.md` (the hook's paragraph). **What.** Only
   `$(PYTHON_BUILD)/Modules/zoraya-ext` is made for the objects; a source in a subdirectory of DIR
   (`@EXTRASRC@/sub/x.c`) fails to compile for a missing output directory. A `*shared*` line in DIR's `Setup` builds a
   shared module that can never load (no `dlopen`). `python-ext-sample`'s `cp tests/python-ext/sample/*`
   (`ports/Makefile:852`) fails on a subdirectory. **Fix.** Create the object directories for every directory of DIR
   (`cd DIR && find . -type d`), or say in the docs that sources sit at DIR's top level; refuse `*shared*` in the hook.

8. **note.** `ports/Makefile:756` to `:761`. **What.** `PYTHON_EXTRA_MODULES` is read from the environment and from a
   top-level command line too (make exports command-line variables to sub-makes through `MAKEFLAGS`; checked with
   `make -p -n`). With it in the environment, `make python`, `make ci-build` or `make test-python` build the `-ext`
   python, while the top-level `Makefile:658` still names `build/rootfs-python`: `test-python` then fails with no rule
   for `build/rootfs-python/.stamp`, or runs on a stale default. **Fix.** Use the variable only when
   `$(origin PYTHON_EXTRA_MODULES)` is `command line`, or name this in the docs.

9. **note.** `tools/test-map.txt:211`. **What.** `tools/zoraya-ar/` maps to `test-python` but not to
   `test-python-ext`, though the sample's `libzhelper.a` and every library python links are made by it. The added
   `tests/python/` and `tools/run-disk.sh` lines name `test-python-ext`, which reads neither (harmless, runs more).
   **Fix.** Add `test-python-ext` to the `tools/zoraya-ar/` line.

10. **note.** The two builds share log names (`build/ports/logs/python*.log`: the `-ext` build overwrites the
   default's) and the hash file (`build/ports/obj/python-ext.hash`, one for any DIR); two `-ext` builds of different
   DIRs in one tree at the same time would also share the build directory. Nothing reads the python logs (the
   `__linux` check of `check-ports` reads other ports' logs), so this only costs a confusing log. Worth one line in
   the limits.

## The curl failure in `make test-changed`

Not related to this change. The diff touches nothing of curl (`ports/patches/curl/` and curl's rules unchanged), and
the logs show two curl configures running at the same time in the same tree: `build/ports/work/curl-8.23.0/config.log`
interleaves two runs (lines of `configure:45xxx`, late in one run, alternate with `configure:5xxx`, the start of
another), `build/logs-par/build.log` has `sed: conftest.c: No such file or directory` and `cat: conftest.er1: No such
file or directory` (one run deleting the other's test files), the first run failed at "cannot compute suffix of
executables" because its link test got the other run's `socketpair` program (`rust-lld: error: undefined symbol:
socketpair`), and `curl-configure.log`, written with `>` by both, holds a hole of blanks. The configure that
`test-par.sh`'s build phase started (one `make apps netsurf`, sequential) ended at 20:49:27; the tree was still being
written at 20:49:34, so another `make` building curl in that worktree ran beside it. `Missing definition of macro swrite`
(`curl_setup.h:944`) is the compile after the broken configure. Redo: `rm -rf build/ports/work/curl-8.23.0` and
`make test-changed` again with nothing else building in that worktree.

## Checked and sound

- The default build is unchanged without the variable: the database of `make -p -n -f ports/Makefile` gives the same
  prerequisites for `build/rootfs-python/.stamp` on `origin/main` and on the branch, the same `PYTHON_ROOT`,
  `PYTHON_BUILD`, `PYTHON_STAGE`, and the recipe differs only by the comment lines and the `if [ -n "" ]` line, which is
  false. `PYEXT_DEP` is empty, so the hash rule is not even defined. The CI ports cache key changes because
  `ports/Makefile` changed (one rebuild of the cached ports, as for any change there), not because of the hook.
- The shared source tree: the link `Modules/zoraya-ext` is read only through an `-ext` `Setup.local`; the default
  build's makesetup does not see it, and `python-testlib` reads only `Lib/`. The touch of `Makefile` and `config.c`
  is in `python-build-ext` only. The `-ext` build re-unpacking the tree would touch `python-src.stamp` and so rebuild
  the default next time, as the default's own re-unpack would.
- The hash covers names (the `./path` column of `shasum`'s output) and content, in a fixed order (`LC_ALL=C sort`);
  it is written only on change, and the stored value in the worktree equals a fresh one. `zoraya-ar` writes no dates
  (`tools/zoraya-ar/src/lib.rs:8`), so the sample's `libzhelper.a` does not change the hash at each run; the report's
  limit about `ar` holds for other tools only.
- The built `-ext` tree: `Setup.local` has `*static*` first and the substituted line; `zsample` is first in
  `MODBUILT_NAMES`; `python3` holds `PyInit_zsample`; `target/test-logs/python-ext-auto.log` ends with `True` and the
  power-off.
- Wiring: `TEST_STEPS`, `.PHONY`, `tools/ci-tests.txt`, `tools/plan-lib.sh`'s `all`, the lane and its `PAR_LONG` opt-in,
  and the map agree: `check-ci-jobs`, `check-ci-plan` and `check-test-lanes` pass in `make check`.
- No ADR is needed: the hook is a build option, no interface of a program changes, no dependency is added
  (`docs/dependencies.md` unchanged). Docs (`docs/design.md`, `ports/README.md`) describe what the code does, apart
  from the hash's coverage (finding 2).

## Recommendation

Merge after finding 1 (a green candidate run with `make test-python-ext` in it). Findings 2 to 4 are small and
worth doing on the branch before that run; 5 to 10 may go to the backlog.
