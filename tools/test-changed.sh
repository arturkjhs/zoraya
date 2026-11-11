#!/bin/sh
# What a change needs (make test-changed): the paths changed since
# origin/main (committed, staged, unstaged and untracked), each mapped by
# tools/test-map.txt to the steps of make test that cover it; make check;
# the host tests of the changed crates and of the crates that depend on
# them; and those steps, in parallel lanes under HVF (tools/test-par.sh with
# FULL=0). A path the map does not know runs every step.
#
#   tools/test-changed.sh [BASE]     (BASE: origin/main)
#
# DRY=1 prints what would run and runs nothing.

set -u

base=${1:-origin/main}
map=tools/test-map.txt
cargo=${CARGO:-cargo}

changed=$(
    {
        git diff --name-only "$(git merge-base "$base" HEAD)" HEAD
        git diff --name-only HEAD
        git ls-files --others --exclude-standard
    } | sort -u
)
if [ -z "$changed" ]; then
    echo "make test-changed: nothing changed since $base; make check only"
    exec "${MAKE:-make}" --no-print-directory check
fi

# shellcheck source=tools/plan-lib.sh
. tools/plan-lib.sh
plan_changed

echo "make test-changed: since $base"
echo "  host tests: ${packages:-none}"
echo "  steps: ${steps:-none} (under HVF)"
[ "${DRY:-0}" = 1 ] && exit 0

status=0
"${MAKE:-make}" --no-print-directory check || status=1
if [ -n "$packages" ]; then
    args=
    for p in $packages; do args="$args -p $p"; done
    log=build/logs-par/host-tests.log
    mkdir -p build/logs-par
    t0=$(date +%s)
    # shellcheck disable=SC2086 # the -p words
    if tools/mutants/watchdog.sh "$cargo" test -q $args >"$log" 2>&1; then
        echo "  ok    host tests $(($(date +%s) - t0)) s: $(grep -c 'test result: ok' "$log") test binaries"
    else
        echo "  FAIL  host tests: $log"
        grep -E -m 6 'panicked|FAILED|^error' "$log" | sed 's/^/          /'
        status=1
    fi
fi
if [ -n "$steps" ]; then
    # shellcheck disable=SC2086 # the steps
    FULL=0 tools/test-par.sh $steps || status=1
fi
exit "$status"
