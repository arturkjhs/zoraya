#!/bin/sh
# INV-0268-1 and INV-0268-2 (ADR-0268 point 5).
#
# While the refusal of `room new --untrusted` is in the shell and the panel
# (their lines marked `untrusted-gate: refusal`), this passes after checking that
# tools/untrusted-gate.txt is well formed and every step it names exists. If the
# refusal is gone from any of them, it fails unless every condition of the ADR
# holds: no test of stage 1 is `pending`, every named step is a step of
# `make test` and of the CI workflow, and every invariant of ADR-0260 to
# ADR-0267, ADR-0269 and ADR-0270 (INV-0262-4 aside) and INV-0172-2 and
# INV-0172-3 is `checked`. The same for stage 2 and the `+net` refusal (lines
# marked `untrusted-gate: net-refusal`). The architect's go-ahead is not a thing
# a script can read: it is the commit that removes the markers.
set -eu
cd "$(dirname "$0")/.."
gate=tools/untrusted-gate.txt
fail=0
bad() { echo "check-untrusted-gate: $*" >&2; fail=1; }

[ -r "$gate" ] || { echo "check-untrusted-gate: no $gate" >&2; exit 1; }

required="charged-allocation memory-hog fork-bomb masked-time cpu-spinners disk-filler service-floods keeper many-rooms hostile-programs fuzzing net-account-flood net-fuzz"
entries=$(grep -v '^#' "$gate" | grep -v '^[[:space:]]*$' || true)
for name in $required; do
	echo "$entries" | awk -v n="$name" '$2 == n { found = 1 } END { exit !found }' ||
		bad "$gate does not list $name (ADR-0268 point 3 names it)"
done
echo "$entries" | awk '$1 != "1" && $1 != "2" { exit 1 }' || bad "a line of $gate has no stage 1 or 2"

# The steps of `make test` and of the workflow.
test_steps=$(sed -n 's/^TEST_STEPS := //p;s/^TEST_STEPS += //p' Makefile)
workflow=.github/workflows/test.yml
step_exists() { # a step name
	case " $test_steps " in *" $1 "*) ;; *) return 1 ;; esac
	[ -r "$workflow" ] || return 1
	if [ "$1" = test-qemu ]; then
		grep -q 'make test-qemu-' "$workflow"
	else
		grep -q -E "make $1([^[:alnum:]_-]|\$)" "$workflow"
	fi
}

ready() { # stage: succeeds if every test of the stage names a real step
	stage_ok=0
	for line in $(echo "$entries" | awk -v s="$1" '$1 == s { print $2 ":" $3 }'); do
		name=${line%%:*}; step=${line#*:}
		if [ "$step" = pending ]; then
			stage_ok=1
		elif ! step_exists "$step"; then
			bad "$name names step $step, which is not a step of make test run by $workflow"
			stage_ok=1
		fi
	done
	return $stage_ok
}

# Steps named must exist even while the gate is closed.
for line in $(echo "$entries" | awk '$3 != "pending" { print $2 ":" $3 }'); do
	step=${line#*:}
	step_exists "$step" || bad "${line%%:*} names step $step, which is not a step of make test run by $workflow"
done

invariants_ok() { # a pattern of ids
	not_checked=$(grep -E "^\| ($1) " docs/adr/invariants.md | grep -v -F 'checked:' || true)
	if [ -n "$not_checked" ]; then
		echo "$not_checked" | cut -d'|' -f2 | tr -d ' ' | sed 's/^/  not checked: /' >&2
		return 1
	fi
}

files_without_refusal=$(grep -c 'untrusted-gate: refusal' user/src/bin/zosh.rs crates/zoraya-panel/src/lib.rs | awk -F: '$2 < 1 { n++ } END { print n + 0 }')
if [ "$files_without_refusal" -eq 0 ]; then
	pending=$(echo "$entries" | awk '$1 == "1" && $3 == "pending"' | wc -l | tr -d ' ')
	echo "check-untrusted-gate: the refusal is in place; $pending test(s) of stage 1 are still pending"
else
	ready 1 || bad "the refusal of room new --untrusted is gone while a test of stage 1 is pending or missing"
	invariants_ok "INV-026[0-7]-[0-9]+|INV-0269-[0-9]+|INV-0270-[0-9]+|INV-0172-2|INV-0172-3" ||
		bad "the refusal is gone while invariants of ADR-0260 to ADR-0270 are not checked"
fi

net_refusal=$(grep -c 'untrusted-gate: net-refusal' user/src/bin/zosh.rs crates/zoraya-panel/src/lib.rs | awk -F: '$2 > 0 { n++ } END { print n + 0 }')
if [ "$net_refusal" -eq 0 ] && [ "$files_without_refusal" -ne 0 ]; then
	ready 2 || bad "+net is allowed in untrusted rooms while a test of stage 2 is pending or missing"
fi

[ "$fail" -eq 0 ] || exit 1
echo "check-untrusted-gate: ok"
