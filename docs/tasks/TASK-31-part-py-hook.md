# TASK-31, part: a python3 build hook for built-in modules

Backlog, "Three requests from outside work", item 1. Branch `work/py-hook` from `origin/main` 3981656.

## What was built

- `make -f ports/Makefile python PYTHON_EXTRA_MODULES=DIR`: DIR holds `Setup` (CPython `Modules/Setup.local` lines), sources and
  prebuilt static libraries. Output `build/rootfs-python-ext` (build, stage and root paths get the suffix `-ext`).
  Without the variable the recipe is the old one (the added lines are a shell `if` that is false; `make -n` shows them,
  nothing else differs) and `build/rootfs-python` is not touched.
- The stamp depends on `build/ports/obj/python-ext.hash` (hash of DIR's file names and content, rewritten only when it
  changes), so a changed module or library rebuilds.
- `@EXTRA@` in `Setup` is DIR's absolute path (`-I`, `-L`), `@EXTRASRC@` is `zoraya-ext`, a link made in the unpacked source
  tree's `Modules/` to DIR: `makesetup` looks relative sources up in `Modules/` and builds an absolute source's object name
  from its whole path (found: `Modules//abs/path/zsample.o` failed). The hook also puts `*static*` first and makes
  `Makefile` and `Modules/config.c` older than `Setup.local` (first try: same-second timestamps kept `config.c`, the module was
  silently missing).
- Sample `tests/python-ext/sample` (C module `zsample.mul_add` over `lib/libzhelper.a`), built by
  `make -f ports/Makefile python-ext-sample`; `make test-python-ext` with `tests/shell/python-ext.script`, `tests/python-ext/ext.py`.
- Wiring: `Makefile` (target, `TEST_STEPS`, `.PHONY`), `tools/test-lanes.txt` (lane `test-python`, optin `PAR_LONG`),
  `tools/ci-tests.txt`, `tools/test-map.txt`, `tools/plan-lib.sh` (`all`). Docs: `docs/design.md` (CPython), `ports/README.md`.

## Decisions

- No ADR: a build-time hook for the person building the image; no interface for programs changes (0396 not used, no status set).
- Separate `-ext` paths, not a switch of the default build: the default image cannot change by accident.
- The sample test runs `test-python-ext` as its own step in the `test-python` lane (no network, a short script), not inside
  `test-python`, whose 15-minute script and servers are not needed.
- No dependency added (`docs/dependencies.md` unchanged).

## Run (this session, macOS, HVF)

- `make fetch-ports` (rc 0), `make -f ports/Makefile python-ext-sample` (rc 0), `make -f ports/Makefile python PYTHON_EXTRA_MODULES=...`
  (rc 0; first attempt without `*static*`-time fix built python without the module, found by the test; second failed on the
  object name; third built).
- `make test-python-ext`: `PASS: 17 steps`, `zoraya-fsck` clean, rc 0 (zsample imported, `mul_add(6,7,0)` = 42, TypeError on
  wrong arguments, `"zsample" in sys.builtin_module_names`). `build/rootfs-python` (default) does not exist after it.
- `KEEP_GOING=1 make check`: all passed (23 of 23).
- Not run: `make test-python`, `test-python-regr` (not needed; the default python3 is unchanged), the CI candidate, `make test`.

## Limits

- The hook builds the whole of CPython again (about as long as the default build); objects are not shared.
- The CI ports cache (key `ports/**`, ...) holds the default python3 only, and does not contain `tests/python-ext`; a CI job
  needing a cached `-ext` build would add DIR's hash to the key. `test-python-ext` in CI builds the `-ext` python in the test job.
- The sample's `libzhelper.a` is rebuilt by `ar` at each run; if `ar` stamps times, the hash changes at each run.
- The link `Modules/zoraya-ext` stays in the shared source tree (re-pointed at each hook build, ignored by the default build).
- Modules are built in only (no `dlopen`); C++ modules must name their runtime library in the `Setup` line.

## make test-changed

`make test-changed` (after the commit) passed its check part and then failed in its `apps netsurf` build step, before any QEMU
step: libcurl's compile stopped with `Missing definition of macro swrite` (`build/ports/logs/curl.log`: `curl_setup.h:944`;
curl's configure in this fresh worktree). That is the curl port, not the python hook (no python path is involved); not
investigated here, so the QEMU steps of `test-changed` did not run. `make test-python-ext` was run alone (above).

## Review fixes (TASK-31-review-py-hook, findings 2 to 4 and notes)

- Hash (2): `find -L . -type f -print0 | sort -z | xargs -0 shasum`, written to a list file whose status is checked. Shown by hand
  on a scratch directory with `my mod.c` and `lib/libx.a` as a link to a file outside DIR: the hash changed (61c0f5df... to
  1b2faf59... to f7a4a805...) after the content of the spaced file and of the linked library changed, and stayed when rewritten
  with the same bytes.
- DIR (3): refused unless it is `A-Za-z0-9._/+-` (checked in the hash recipe, which runs first, through the environment, so
  the value is not parsed by the shell); shown for `bad dir`, `q'x`, `a&b`: "has a character outside A-Za-z0-9._/+-". Recipe uses quote DIR.
- Only on the command line: `PYTHON_EXTRA_MODULES=x make -f ports/Makefile -n python` gives an error naming the command line; the
  top-level Makefile errors for any origin, pointing at `ports/Makefile` and `make test-python-ext`.
- `tools/test-par.sh` builds the sample and the `-ext` python before the lanes when `test-python-ext` is selected (`sh -n` only; not
  run: `test-par` is not for agent sessions).
- Script: `! Traceback`, `! ModuleNotFoundError`. `tools/zoraya-ar/` line of the map has `test-python-ext`. Logs of the `-ext` build:
  `python-ext.log`, `python-configure-ext.log`, `python-install-ext.log`, `python-compileall-ext.log`.
- To `docs/backlog.md`: Setup.local before configure, subdirectory sources, `*shared*`, artifact/cache.
- `make test-python-ext` again after the changes (the stamp rebuilt, since the hash file changed): `PASS: 19 steps`, fsck clean.
- Finding 1 (CI shape) is left to the candidate run.
