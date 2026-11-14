#!/bin/sh
# The jobs CI runs on a merge candidate (tag candidate/NAME, pull request):
# what the change since origin/main touches (tools/test-map.txt, the longest
# prefix; a path not in the map runs every step, as in make test-changed)
# plus a smoke set: boot (test-boot), the shell's files script (test-files),
# one GUI run (terminal, host accelerator, scale 1), the POSIX hello
# script and the machines a person runs by hand (test-hand-run). Host tests: the changed crates and their dependents. A change that
# needs only `check` (docs, the map) runs no machine at all, and no build.
# The fuzz jobs of a candidate run 60 s a target unless the map sends the
# change to them; the full set runs 600 s.
#
#   tools/ci-plan.sh [BASE]     (BASE: origin/main; FULL=1: the full set)
#   DOCS_BEFORE=SHA DOCS_AFTER=SHA tools/ci-plan.sh
#       a push to main: if only docs/ changed between the two commits
#       (plan-lib.sh's docs_only) the plan is `docs_only=1`, no tests, no
#       build, and `doc_checks` names the checks to run; any other change, or
#       an unusable BEFORE, gives the full set.
#
# Prints key=value lines for $GITHUB_OUTPUT: `tests` (a JSON array of the
# make commands of tools/ci-tests.txt, the full list or a part of it),
# `build` (true if any job needs the build) and `summary`. The reasoning goes
# to stderr. A base that cannot be found runs the full set.

set -u

base=${1:-origin/main}
map=tools/test-map.txt
list=tools/ci-tests.txt
cargo=${CARGO:-cargo}

json() { # lines on stdin -> JSON array
    awk 'BEGIN { printf "[" } { gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); printf "%s\"%s\"", (NR > 1 ? "," : ""), $0 } END { print "]" }'
}
jobs() { grep -v -E '^#|^$' "$list"; }
emit() { # the jobs on stdin, why
    sel=$(cat)
    n=$(echo "$sel" | grep -c . || true)
    echo "tests=$(echo "$sel" | grep . | json)"
    if [ "$n" -gt 0 ]; then echo "build=true"; else echo "build=false"; fi
    echo "summary=$1: $n jobs"
    echo "ci-plan: $1: $n jobs" >&2
    echo "$sel" | grep . | sed 's/^/  /' >&2
}

if [ "${FULL:-0}" = 1 ]; then
    jobs | emit "full set"
    echo "docs_only=0"
    exit 0
fi
if [ -n "${DOCS_AFTER:-}" ]; then
    # shellcheck source=tools/plan-lib.sh
    . tools/plan-lib.sh
    if docs_only "${DOCS_BEFORE:-}" "$DOCS_AFTER"; then
        echo "tests=[]"
        echo "build=false"
        echo "docs_only=1"
        echo "doc_checks=$doc_checks"
        echo "summary=docs only: the document checks ($doc_checks), 0 jobs"
        echo "ci-plan: docs only: $doc_checks" >&2
    else
        jobs | emit "full set (push to main)"
        echo "docs_only=0"
    fi
    exit 0
fi
if [ -n "${CI_PLAN_PATHS:-}" ]; then
    # For trying the plan on a made-up change (paths one a line).
    changed=$CI_PLAN_PATHS
elif ! mb=$(git merge-base "$base" HEAD 2>/dev/null); then
    echo "ci-plan: no merge base with $base: full set" >&2
    jobs | emit "full set (no merge base)"
    exit 0
fi
[ -n "${CI_PLAN_PATHS:-}" ] || changed=$(git diff --name-only "$mb" HEAD | sort -u)
echo "ci-plan: $(echo "$changed" | grep -c .) changed paths since $base" >&2

# shellcheck source=tools/plan-lib.sh
. tools/plan-lib.sh
plan_changed

# The jobs of the full list that run a step; test-qemu is one job a
# configuration. test-unit takes the packages found.
sel=$(
    jobs | while read -r line; do
        target=$(echo "$line" | awk '{ print $2 }')
        for s in $steps; do
            if [ "$target" = "$s" ]; then echo "$line"; break; fi
            if [ "$s" = test-qemu ] && [ "${target#test-qemu-}" != "$target" ]; then echo "$line"; break; fi
        done
    done
)
if [ "$everything" = 1 ]; then
    sel=$(printf '%s\n%s\n' "make test-unit" "$sel")
elif [ -n "$packages" ]; then
    args=
    for p in $packages; do args="$args -p $p"; done
    sel=$(printf '%s\n%s\n' "make test-unit UNIT_ARGS=\"${args# }\"" "$sel")
fi

# Anything to run at all runs the smoke set too.
if [ -n "$(echo "$sel" | grep .)" ]; then
    has() { echo "$sel" | grep -q -F "$1"; }
    has "make test-boot" || sel=$(printf '%s\n%s\n' "$sel" "make test-boot")
    has "make test-files" || sel=$(printf '%s\n%s\n' "$sel" "make test-files")
    has "make test-gui" || sel=$(printf '%s\n%s\n' "$sel" 'make test-gui GUI_RUNS="terminal:auto:1"')
    has "make test-hand-run" || sel=$(printf '%s\n%s\n' "$sel" "make test-hand-run")
    has "hello.script" || sel=$(printf '%s\n%s\n' "$sel" 'make test-posix POSIX_SCRIPTS="tests/posix/hello.script" POSIX_ACCELS="auto"')
fi
# A candidate's fuzz jobs run 60 s a target, not the full set's 600 s
# (TASK-29, T3), when they are in the plan only because a path the map does
# not know widened it to every step: the long run is then not about the
# change, and the full set (main, nightly, candidate-full/) keeps it
# (ADR-0298 item 3). A change the map sends to a fuzz step (fuzz/, the
# script, a fuzzed decoder) keeps ADR-0268's 600 s (review R7).
case " $mapped " in
    *" test-fuzz "* | *" test-fuzz-net "*) fuzz_seconds=600 ;;
    *) fuzz_seconds=60 ;;
esac
echo "$sel" | grep . | sort -u | sed "s/FUZZ_SECONDS=600/FUZZ_SECONDS=$fuzz_seconds/" | emit "candidate"
