# TASK-31 part ci-expr: a syntax check of the workflow files

Branch `work/ci-expr`, from origin/main f969cf4.

## Problem

Twice a workflow edit made GitHub reject the whole workflow (a run with zero jobs): a missing comma between two
quoted arguments of `hashFiles(...)`. The file was still valid YAML.

## What exists

- `tools/check-workflow-syntax.rb` (ruby; the CI image installs ruby for YAML parsing, `.github/ci/Dockerfile`, and
  macOS has it; no new dependency). For every `.github/workflows/*.yml`:
  - parses as YAML (psych), with the error's line;
  - no line indented with a tab; no mapping with a key twice (at any level);
  - every `${{` has its `}}`; inside, quotes, parentheses and brackets balance; in every call's arguments no two
    values sit side by side (the missing comma), no leading, doubled or trailing comma, no dangling operator; the same
    for a `hashFiles(...)` written outside `${{ }}` (a `run:` line). Whole-line YAML comments are skipped.
  - Errors are `FILE:LINE: message`. It is a conservative structural check, not GitHub's grammar: it knows no
    function names and no types.
- The YAML parse calls `Psych.parse_stream(text)` with one argument (the filename argument is positional in psych 3 and a keyword in psych 5; Ruby 3.2 on CI rejected the two-argument call).
- `--selftest` (in the same file): the historical case `hashFiles('docs/adr/0398-*.md' 'docs/adr/0389-*.md')`, a
  trailing, leading and doubled comma, an unbalanced `${{`, an unbalanced paren, an unterminated string, a missing
  comma in `format(...)`, tabs, duplicate keys (top level and nested), invalid YAML, `hashFiles` in a `run:` line, good
  expressions (nested calls, `{{` inside strings, `''` escapes, index, `!`, empty calls, comments) and the repository's
  own workflow files.
- `make check-workflow-syntax` runs the selftest, then the check on the workflows. It is in `CHECK_STEPS` (so
  `make check`), in `TEST_STEPS` (so `make test`, and `check-ci-jobs` requires it in the workflow), and in the
  `check` job's matrix of `.github/workflows/test.yml`. `tools/test-map.txt` maps the script to `check`.

## What was run

- `make check`: all steps ok, including check-ci-jobs, check-ci-plan, check-test-lanes and the new step.
- By hand, on a copy of `test.yml` with the break applied (the comma removed between the last two arguments of the
  ports-build `hashFiles(...)`, line 252): the check failed with exit 1,
  `bad.yml:252: in ${{ }}: two values side by side ('string' follows a value): a comma or an operator is missing`.
  A copy with a trailing comma failed with `trailing comma before ')'`. The unbroken files pass.
- Not run: the CI candidate (not asked).

## Limits

- It does not evaluate expressions or know GitHub's function signatures (argument counts, types).
- Expressions in `if:` values written without `${{ }}` are not checked.
