# TASK-31 part 10j: installed programs (ADR-0396, ADR-0397, ADR-0399)

Branch `work/10j`, from `origin/main` `fffd7e0f`. No spec file: the part is derived from ADR-0396, ADR-0397 and ADR-0399
(accepted 2026-11-15, Option C, B, B) and the architect's notes of 2026-11-15 in `docs/backlog.md` (measure the SHA-256 for a
64 MB program; one command per package). What was decided where those are silent: proposed ADR-0394.

## What was built

- `zoraya-settings`: `sha256` (streaming, FIPS 180-4 vectors), `apps` (the record `/config/apps`, the saved words and what may be
  saved, the install check, the log and its cap, the table, the instance list; the one writer of INV-0397-1 and INV-0399-3).
- `zoraya-describe`: `volume_key_problem` (the keys an installed description may have) and `for_volume` (every kind `text`, no authority).
- `zoraya-shell`: `installed` (lookup order, launch check, the refusal of `+w` over `/apps` and `/`, the saved words as plan entries,
  the gate and the typed-word rule), `netgrant::saved_scope`, `Io::words` (a builtin sees whether each word was typed).
- `zosh`: the builtins `install` and `programs` (`user/src/bin/zosh/apps.rs`, `user/describe/builtins/`), the lookup of installed
  names, `help`, completion, `grants` with `(saved)`, the launch check (`posix_child::load_checked`), chunked writes of the record.
- Tests: host tests in the three crates (`cargo test -p zoraya-settings -p zoraya-shell -p zoraya-describe`),
  `tests/posix/zosh-install.script` (198 steps, `make test-posix`, in `tools/ci-tests.txt` under TCG),
  `tests/posix/zosh-measure-install.script` (the measurement; not a step of `make test`).

## Invariants

INV-0396-1 to 5, INV-0397-1 to 5 and INV-0399-1, 2, 3 and 5 are `checked` (`docs/adr/invariants.md`). **Not built:** INV-0397-6
(a serial script cannot type the grant line; the delivery code never touches the record) and INV-0399-4 (the Programs page of
`settings`: the window holds no `/config`; it needs a panel message and the panel's right to write the record, a part of its own).
Both stay `not yet applicable (stage 10)` with a note in the ADR's open questions (ADR-0001 rule 6) and ADR-0394 item 11.

## Measurement: SHA-256 at a start (the architect's note of 2026-11-15)

`make test-posix POSIX_SCRIPTS=tests/posix/zosh-measure-install.script POSIX_IMAGE_SIZE=640M` (HVF, Apple M4, 512 MiB guest; the
program is a tcc program with 63 MB of initialised data: 64,340,807 bytes, the loader's limit is 64 MiB). `uptime` before and after
each start of `f.txt` printing, installed name against the same file by its path:

| run | by installed name (check, load, run) | by path (load, run) |
|---|---|---|
| 1 | 1.112 s | 0.984 s |
| 2 | 1.098 s | 0.976 s |
| 3 | 1.168 s | 0.979 s |

The hash costs about 0.146 s a start on average (about 440 MB/s); the load, which reads the 64 MB through the file service into the
image, costs about 1.0 s with or without it. `install` of that file (copy, hash, record) took 1.7 s. Not measured under TCG (CI),
where the step is skipped for time. For the 1.4 MiB program of ADR-0396's assumption the hash is about 3 ms.

## Review and checks

- `make check`: exit 0 at each commit. Host tests of the three crates pass.
- `make test-posix POSIX_SCRIPTS=tests/posix/zosh-install.script` (HVF): PASS, 198 steps, `zoraya-fsck` clean.
- The independent review and its dispositions: `docs/tasks/TASK-31-review-10j.md`.

## For the architect

- ADR-0394 (proposed): Option A, the page as its own part (Option C).
- `tools/ci-tests.txt` and `Makefile` (`POSIX_SCRIPTS`, `POSIX_TIMEOUTS`) changed: a candidate run is for you to tag (`candidate/work-10j-a`
  on the head in the merge report); I did not tag it.
- `+w /` is refused as ADR-0396 item 2 says; `tests/shell/system-ro.script` changed (`+w /home`).
- Hand check, in a desktop terminal: `mkdir p; ... install ./p +w ~/notes`, `programs`, `programs revoke NAME --all` (the commands of the script).
