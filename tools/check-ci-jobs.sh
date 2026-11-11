#!/bin/sh
# Fails unless the CI workflow runs everything `make test` runs (ADR-0213's
# Decision): every step of TEST_STEPS as `make STEP`, except test-qemu, which
# must appear as `make test-qemu-CONFIG` for every configuration, and
# test-posix, which must appear as `make test-posix POSIX_SCRIPTS="..."
# POSIX_ACCELS="..."` lines that together run every script under every
# accelerator, W8 (POSIX_ONCE) as each of its builds' scripts under the first.
#
#   TEST_STEPS=... QEMU_CONFIGS=... POSIX_SCRIPTS=... POSIX_ACCELS=...
#   POSIX_ONCE=... W8_BUILDS=... GUI_RUNS_ALL=... tools/check-ci-jobs.sh WORKFLOW [TESTS_LIST]
#
# The tests job's matrix is not in the workflow: on a merge candidate it is
# the part of the FULL list (TESTS_LIST, tools/ci-tests.txt) that
# tools/ci-plan.sh picks, so this checks the full list, and that the workflow
# reads the plan, that the test-gui lines (GUI_RUNS="script:accel:scale ...")
# together run every run of GUI_RUNS_ALL, each in exactly one job, and that FULL=1 gives the plan the whole list.
set -eu
workflow=$1
list=${2:-tools/ci-tests.txt}
[ -r "$workflow" ] || { echo "check-ci-jobs: cannot read $workflow" >&2; exit 2; }
[ -r "$list" ] || { echo "check-ci-jobs: cannot read $list" >&2; exit 2; }

# Every command the workflow gives, one a line, without YAML's list dash,
# key or quoting: `run:` and `cmd:` values and the matrix's list items.
commands=$(sed -n -E -e 's/^[[:space:]]*(- )?(run|cmd):[[:space:]]*//p' \
	-e 's/^[[:space:]]*- //p' "$workflow" |
	sed -e "s/^'\(.*\)'\$/\1/" -e 's/^"\(.*\)"$/\1/' | grep -E '^make ' || true)
commands=$(printf '%s\n%s\n' "$commands" "$(grep -E '^make ' "$list")")

missing=0
need() { # what, for the message
	echo "check-ci-jobs: $workflow does not run $1" >&2
	missing=$((missing + 1))
}
has_make() { # target
	echo "$commands" | grep -E -q "(^|[;&|[:space:]])make $1([[:space:]]|\$)"
}

# The (script, accelerator) pairs the test-posix lines run.
pairs=$(echo "$commands" | grep -E '(^|[[:space:]])make test-posix([[:space:]]|$)' | while read -r line; do
	scripts=$(echo "$line" | sed -n 's/.*POSIX_SCRIPTS="\([^"]*\)".*/\1/p')
	accels=$(echo "$line" | sed -n 's/.*POSIX_ACCELS="\([^"]*\)".*/\1/p')
	[ -n "$scripts" ] && [ -n "$accels" ] || { echo "UNSPLIT"; continue; }
	for s in $scripts; do for a in $accels; do echo "$(basename "$s" .script):$a"; done; done
done)
has_pair() {
	echo "$pairs" | grep -q -x "$1"
}

first_accel=${POSIX_ACCELS%% *}
for step in $TEST_STEPS; do
	case $step in
	test-qemu)
		for c in $QEMU_CONFIGS; do has_make "test-qemu-$c" || need "make test-qemu-$c"; done ;;
	test-posix)
		if echo "$pairs" | grep -q -x UNSPLIT; then
			need "test-posix with both POSIX_SCRIPTS=\"...\" and POSIX_ACCELS=\"...\" on every line"
		fi
		for s in $POSIX_SCRIPTS; do
			name=$(basename "$s" .script)
			case " $POSIX_ONCE " in
			*" $name "*)
				for b in $W8_BUILDS; do
					has_pair "$name-$b:$first_accel" || need "$name-$b.script under $first_accel"
				done ;;
			*)
				for a in $POSIX_ACCELS; do has_pair "$name:$a" || need "$name.script under $a"; done ;;
			esac
		done ;;
	*)
		has_make "$step" || need "make $step" ;;
	esac
done

# The runs of the test-gui lines against GUI_RUNS_ALL, as multisets.
if echo "$TEST_STEPS" | grep -q -w test-gui; then
	gui_lines=$(grep -E '^make test-gui([[:space:]]|$)' "$list" || true)
	if echo "$gui_lines" | grep -q -v 'GUI_RUNS="[^"]*"'; then
		need "test-gui with GUI_RUNS=\"...\" on every line (a line without it runs every run)"
	fi
	have=$(echo "$gui_lines" | sed -n 's/.*GUI_RUNS="\([^"]*\)".*/\1/p' | tr ' ' '\n' | grep . | sort)
	want=$(echo "${GUI_RUNS_ALL:-}" | tr ' ' '\n' | grep . | sort)
	[ -n "$want" ] || need "GUI_RUNS_ALL to compare the test-gui lines with (the Makefile gives none)"
	for r in $(echo "$want" | uniq); do
		n=$(echo "$have" | grep -c -x -F "$r" || true)
		[ "$n" = 1 ] || need "GUI run $r in exactly one test-gui job (it is in $n)"
	done
	for r in $(echo "$have" | uniq); do
		echo "$want" | grep -q -x -F "$r" || need "GUI run $r is not in GUI_RUNS_ALL"
	done
fi
if ! grep -q 'fromJSON(needs.plan.outputs.tests)' "$workflow"; then
	need "its tests from the plan (fromJSON(needs.plan.outputs.tests))"
fi
if grep -q -E '^[[:space:]]*- .?make test' "$workflow"; then
	need "only the full list in $list: a test command is in the workflow's matrix"
fi
planned=$(FULL=1 sh tools/ci-plan.sh 2>/dev/null | sed -n 's/^summary=full set: \([0-9]*\) jobs$/\1/p')
listed=$(grep -c -E '^make ' "$list")
if [ "${planned:-0}" != "$listed" ]; then
	need "all $listed jobs of $list in the full plan (tools/ci-plan.sh with FULL=1 gives ${planned:-none})"
fi
# Every test step of TEST_STEPS must be in the full plan and in the plan of a
# change the map does not know (which runs every step: plan-lib.sh's $all).
for mode in full unmapped; do
	if [ "$mode" = full ]; then
		plan=$(FULL=1 sh tools/ci-plan.sh 2>/dev/null || true)
	else
		plan=$(CI_PLAN_PATHS="unmapped-path-for-check-ci-jobs/x" sh tools/ci-plan.sh origin/main 2>/dev/null || true)
	fi
	plan=$(echo "$plan" | sed -n 's/^tests=//p')
	for step in $TEST_STEPS; do
		case $step in
		test-*) ;;
		*) continue ;;
		esac
		case $step in
		test-qemu) pat='"make test-qemu-' ;;
		*) pat="\"make $step[\" ]" ;;
		esac
		if ! echo "$plan" | grep -E -q "$pat"; then
			need "make $step in the $mode plan (tools/ci-plan.sh, plan-lib.sh's all)"
		fi
	done
done
if [ "$missing" -gt 0 ]; then
	echo "check-ci-jobs: $missing missing" >&2
	exit 1
fi
echo "check-ci-jobs: $workflow runs every step of make test"
