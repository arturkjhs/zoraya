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
mkdir -p "$logs"
[ -n "$internal" ] || rm -f "$logs"/*.log "$logs"/*.status

checks="fmt-check clippy adr-check check-user-access check-kernel-tty check-host-paths check-argv check-test-caps check-tokens check-fetch-ports check-ci-jobs check-run-disk"
# The QEMU lanes, each a list of steps sharing an image: test-files and
# test-crash write build/crash.img; test-posix-gui and test-tour-gui
# build/posix-gui.img.
# test-boot runs alone, before the lanes: its console test needs a burst to
# overflow the guest's ring while a job runs, which other guests' load can
# keep from happening (2026-11-09).
solo="test-boot"
lanes="test-qemu;test-libposix;test-files test-crash test-keeper test-hand-run;test-posix;test-posix-gui test-tour-gui;test-gui;test-net;test-fuzz test-fuzz-net"
all_lanes="$solo $(echo "$lanes" | tr ';' ' ')"


if [ -n "$internal" ]; then
    wanted=${WANTED:- $* }
elif [ $# -gt 0 ]; then
    wanted=" $* "
else
    wanted=" test-unit $checks $all_lanes "
fi
has() { case "$wanted" in *" $1 "*) return 0 ;; esac; return 1; }

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
[ "${FULL:-0}" = 1 ] || echo "  (local: TCG runs, the tour's second run and crash rounds past 2 skipped; FULL=1 runs them)"
exit "$failed"
