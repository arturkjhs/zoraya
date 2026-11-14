#!/bin/sh
# Runs make test's steps in parallel lanes, with a summary as make test's.
#
#   tools/test-par.sh [STEP...]      (no steps: every step of make test)
#
# The build comes first, alone: the kernels, the runner, the storage tools,
# the POSIX image and the host tests (built, not run). Then the steps run in
# lanes: a lane is a list of steps that share a disk image or a build
# directory, run one after another; lanes share nothing, and up to
# PAR_QEMU (3) of the QEMU lanes run at once, beside one host lane (the
# host tests). On this 16 GiB host each QEMU guest takes 128 MiB to 2 GiB;
# every lane runs under tools/mutants/watchdog.sh. Each step's output is in
# build/logs-par/STEP.log. Steps not named are not run. Exit status: 0 if
# every step passed.

set -u

make=${MAKE:-make}
# Internal: one lane, or one step, run by the script itself under the
# watchdog.
case "${1:-}" in
    __lane | __step) internal=$1; shift ;;
    *) internal= ;;
esac
logs=build/logs-par
max_qemu=${PAR_QEMU:-3}
[ "${DRY:-0}" = 1 ] || mkdir -p "$logs"
[ -n "$internal" ] || [ "${DRY:-0}" = 1 ] || rm -f "$logs"/*.log "$logs"/*.status

checks="fmt-check clippy adr-check check-user-access check-kernel-tty check-host-paths check-argv check-test-caps check-tokens check-fetch-ports check-ci-jobs check-run-disk check-charged-alloc check-untrusted-gate check-quota-tests check-ethd-maps check-tls-roots check-netsurf-refusal check-ci-plan check-test-lanes"
# The lanes, the steps run alone and the steps that are skipped come from
# tools/test-lanes.txt (make check-test-lanes checks it against make test's
# steps and the test map).
lanes_file=${TEST_LANES:-tools/test-lanes.txt}
solo=$(awk '$1 == "solo" { printf "%s ", $2 }' "$lanes_file")
solo=${solo% }
lanes=$(awk '$1 == "lane" { $1 = ""; sub(/^ /, ""); printf "%s;", $0 }' "$lanes_file")
all_lanes="$solo $(echo "$lanes" | tr ';' ' ')"
# Steps run only when VAR=1 (optin) and steps with no lane (skip): "STEP|REASON" a line.
optin=$(awk '$1 == "optin" { v = $2; s = ""; r = ""; seen = 0
    for (i = 3; i <= NF; i++) { if ($i == "--") { seen = 1; continue }
        if (seen) r = r " " $i; else s = s " " $i }
    n = split(s, w, " "); for (i = 1; i <= n; i++) print w[i] "|" v "|" substr(r, 2) }' "$lanes_file")
skipped_steps=$(awk '$1 == "skip" { s = ""; r = ""; seen = 0
    for (i = 2; i <= NF; i++) { if ($i == "--") { seen = 1; continue }
        if (seen) r = r " " $i; else s = s " " $i }
    n = split(s, w, " "); for (i = 1; i <= n; i++) print w[i] "|" substr(r, 2) }' "$lanes_file")

if [ -n "$internal" ]; then
    wanted=${WANTED:- $* }
elif [ $# -gt 0 ]; then
    wanted=" $* "
else
    wanted=" test-unit $checks $all_lanes "
fi
has() { case "$wanted" in *" $1 "*) return 0 ;; esac; return 1; }

# A step asked for and not run: SKIPPED with its reason when the lanes file
# allows it, FAIL when nothing knows it (a step with no lane must not pass
# silently). Both are printed in the summary.
skip_lines=
unlaned=
if [ -z "$internal" ]; then
    known=" test-unit $checks $all_lanes "
    for s in $wanted; do
        hit=$(printf '%s\n' "$skipped_steps" | awk -F'|' -v s="$s" '$1 == s { print $2; exit }')
        if [ -n "$hit" ]; then
            skip_lines="$skip_lines$s|$hit
"
            wanted=$(echo " $wanted " | sed "s/ $s / /")
            continue
        fi
        hit=$(printf '%s\n' "$optin" | awk -F'|' -v s="$s" '$1 == s { print $2 "|" $3; exit }')
        if [ -n "$hit" ]; then
            var=${hit%%|*}
            eval "val=\${$var:-0}"
            if [ "$val" != 1 ]; then
                skip_lines="$skip_lines$s|${hit#*|}
"
                wanted=$(echo " $wanted " | sed "s/ $s / /")
            fi
            continue
        fi
        case "$known" in
            *" $s "*) ;;
            *) unlaned="$unlaned $s" ;;
        esac
    done
    wanted=" $(echo $wanted) "
fi

# The SKIPPED and FAIL lines of the steps that do not run.
summary_skips() {
    printf '%s' "$skip_lines" | while IFS='|' read -r s why; do
        [ -n "$s" ] && echo "  SKIPPED: $s: $why"
    done
    for s in $unlaned; do
        echo "  FAIL  $s: no lane in $lanes_file and not in its skip list: the step would not run (add a lane, or a skip line with the reason)"
    done
}

start_all=$(date +%s)
summary=
failed=0

# One step: its log, its status file and its time.
step() {
    s=$1
    t0=$(date +%s)
    "$make" --no-print-directory "$s" POSIX_IMAGE_DEP= >"$logs/$s.log" 2>&1
    echo "$? $(($(date +%s) - t0))" >"$logs/$s.status"
}

# A lane: its steps in order; a failure ends the lane (the steps after it
# are marked not run).
lane() {
    stopped=0
    for s in "$@"; do
        has "$s" || continue
        if [ "$stopped" = 1 ]; then
            echo "skip 0" >"$logs/$s.status"
            continue
        fi
        step "$s"
        read -r st _ <"$logs/$s.status"
        [ "$st" = 0 ] || stopped=1
    done
}

if [ "$internal" = __step ]; then
    step "$1"
    exit 0
fi
if [ "$internal" = __lane ]; then
    lane "$@"
    exit 0
fi

# DRY=1: the plan and the summary lines for the steps that will not run, no run.
if [ "${DRY:-0}" = 1 ] && [ -z "$internal" ]; then
    echo "make test-par (dry): run:${wanted}"
    summary_skips
    exit "$([ -n "$unlaned" ] && echo 1 || echo 0)"
fi

# 1. The checks, one after another (seconds), and the build, alone.
for c in $checks; do
    has "$c" && step "$c"
done
needs_qemu=0
for l in $all_lanes; do has "$l" && needs_qemu=1; done
build_log="$logs/build.log"
t0=$(date +%s)
{
    if [ "$needs_qemu" = 1 ]; then
        "$make" --no-print-directory build/zoraya-kernel.elf build/zoraya-kernel-test.elf tools-storage &&
            "$make" --no-print-directory -s posix-image &&
            ${CARGO:-cargo} build -q -p zoraya-test-runner
    fi &&
        # The ports the lanes' steps need, built once here so that the lanes do not
        # build them at the same time (needs `make fetch-ports` once). Not -s: the ports'
        # checks read the compile commands in the logs (ci-build is not -s either).
        if has test-posix-suites || has test-posix-threadtest3; then
            "$make" --no-print-directory -f ports/Makefile libc suites workloads
        fi &&
        if has test-apps || has test-netsurf; then
            "$make" --no-print-directory apps netsurf
        fi &&
        if has test-python || has test-python-regr; then
            "$make" --no-print-directory python python-testlib
        fi &&
        if has test-unit; then
            ${CARGO:-cargo} test -q --workspace --exclude zoraya-kernel --exclude zoraya-user --exclude zoraya-rt --no-run
        fi
} >"$build_log" 2>&1
build_status=$?
build_time=$(($(date +%s) - t0))

# 2. The steps that run alone, then the lanes: the host lane, and the QEMU
# lanes, max_qemu at once.
if [ "$build_status" = 0 ]; then
    for s in $solo; do
        has "$s" && tools/mutants/watchdog.sh "$0" __step "$s"
    done
    if has test-unit; then
        WANTED="$wanted" tools/mutants/watchdog.sh "$0" __step test-unit &
        host=$!
    else
        host=
    fi
    qemu=
    old_ifs=$IFS
    IFS=';'
    set -f
    for l in $lanes; do
        IFS=$old_ifs
        any=0
        for s in $l; do has "$s" && any=1; done
        if [ "$any" = 1 ]; then
            # Wait for a QEMU slot.
            while :; do
                n=0
                live=
                for p in $qemu; do
                    if kill -0 "$p" 2>/dev/null; then
                        n=$((n + 1))
                        live="$live $p"
                    fi
                done
                qemu=$live
                [ "$n" -lt "$max_qemu" ] && break
                sleep 2
            done
            # shellcheck disable=SC2086 # a lane is words
            WANTED="$wanted" tools/mutants/watchdog.sh "$0" __lane $l &
            qemu="$qemu $!"
        fi
        IFS=';'
    done
    IFS=$old_ifs
    set +f
    wait
fi

# 3. The summary.
line() { printf '  %-5s %-18s %5ss  %s\n' "$1" "$2" "$3" "$4"; }
echo "make test-par: $(($(date +%s) - start_all)) s (logs in $logs/)"
if [ "$build_status" = 0 ]; then
    line ok build "$build_time" ""
else
    line FAIL build "$build_time" "$build_log"
    failed=1
fi
for s in test-unit $checks $all_lanes; do
    has "$s" || continue
    if [ ! -f "$logs/$s.status" ]; then
        line "-" "$s" 0 "not run"
        continue
    fi
    read -r st secs <"$logs/$s.status"
    case "$st" in
        0) line ok "$s" "$secs" "" ;;
        skip) line "-" "$s" 0 "not run after a failure in its lane" ;;
        *)
            line FAIL "$s" "$secs" "$logs/$s.log"
            grep -E -m 4 'FAIL|^error|error\[|panicked|timed out|\*\*\* ' "$logs/$s.log" | sed 's/^/          /'
            failed=1
            ;;
    esac
done
summary_skips
[ -z "$unlaned" ] || failed=1
[ "${FULL:-0}" = 1 ] || echo "  (local: TCG runs, the tour's second run and crash rounds past 2 skipped; FULL=1 runs them)"
exit "$failed"
