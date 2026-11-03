#!/bin/sh
# Runs make targets one after another with their output in build/logs/, and
# prints only a summary: what passed, with counts, and what failed, with the
# path of its log and its first error lines. `make test` uses it; `make test
# V=1` prints everything instead.
#
#   tools/test-summary.sh TARGET...
#
# A configuration that cannot run on this host prints "[LABEL] skipped: REASON"
# (the Makefile's EL2_OR_SKIP). A skip is not a failure, but it is listed on
# its step's line and again at the end, so that it is never silent.
#
# It stops at the first failure, unless KEEP_GOING=1: a step after a failed
# build fails too, and a QEMU step can take minutes to time out. Exit status:
# 0 if every target passed, 1 otherwise.

set -u

make=${MAKE:-make}
logs=build/logs
mkdir -p "$logs"
rm -f "$logs"/*.log

passed=0
failed=0
summary=
failures=
skipped=
skips=

# What a log says about its counts, on one line.
counts() {
    log=$1
    awk '
        /test result: / {
            for (i = 1; i <= NF; i++) {
                if ($(i) ~ /^passed;?$/) unit_pass += $(i - 1)
                if ($(i) ~ /^failed;?$/) unit_fail += $(i - 1)
            }
        }
        /\] PASS: [0-9]+ passed/ { configs++ }
        /\] PASS: [0-9]+ steps/ { scripts++ }
        /^crash test: / { sub(/^crash test: /, ""); crash = $0 }
        /^\[[^]]+\] skipped: / { nskip++ }
        END {
            out = ""
            if (unit_pass + unit_fail > 0) out = out sprintf("%d host tests passed, %d failed", unit_pass, unit_fail)
            if (configs > 0) out = out (out ? "; " : "") sprintf("%d QEMU configurations passed", configs)
            if (scripts > 0) out = out (out ? "; " : "") sprintf("%d shell scripts passed", scripts)
            if (crash != "") out = out (out ? "; " : "") "crash: " crash
            if (nskip > 0) out = out (out ? "; " : "") sprintf("%d skipped", nskip)
            print out
        }
    ' "$log"
}

for target in "$@"; do
    if [ "$failed" -gt 0 ] && [ "${KEEP_GOING:-0}" != 1 ]; then
        skipped="$skipped $target"
        continue
    fi
    log="$logs/$target.log"
    start=$(date +%s)
    "$make" --no-print-directory "$target" >"$log" 2>&1
    status=$?
    seconds=$(($(date +%s) - start))
    skip_lines=$(grep -E '^\[[^]]+\] skipped: ' "$log" | sed 's/^/  /')
    if [ -n "$skip_lines" ]; then
        skips="$skips
$skip_lines"
    fi
    if [ "$status" -eq 0 ]; then
        passed=$((passed + 1))
        line=$(printf '  ok    %-18s %4ss  %s' "$target" "$seconds" "$(counts "$log")")
    else
        failed=$((failed + 1))
        line=$(printf '  FAIL  %-18s %4ss  %s' "$target" "$seconds" "$log")
        # The first lines that say why, so that the log is opened only if
        # these are not enough.
        why=$(grep -E -m 6 'FAIL|^error|error\[|panicked|failed|forbidden|timed out|Diff in|\*\*\* ' "$log" |
            cut -c1-160 | sed 's/^/          /')
        failures="$failures
$line
$why"
    fi
    summary="$summary
$line"
done

total=$#
echo "make test: $passed of $total passed, $failed failed (full logs in $logs/)$summary"
if [ -n "$skipped" ]; then
    echo "  not run after the failure:$skipped (KEEP_GOING=1 runs them)"
fi
if [ -n "$skips" ] && [ "$failed" -gt 0 ]; then
    echo "  skipped on this host:$skips"
fi
if [ "$failed" -gt 0 ]; then
    echo "failures:$failures"
    exit 1
fi
if [ -n "$skips" ]; then
    echo "make test: all checks passed; skipped on this host:$skips"
else
    echo "make test: all checks passed"
fi
