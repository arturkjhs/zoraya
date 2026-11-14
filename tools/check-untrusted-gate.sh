#!/bin/sh
# INV-0268-1 and INV-0268-2 (ADR-0268 point 5).
#
# While the refusal of `room new --untrusted` is in the shell and the panel
# (their lines marked `untrusted-gate: refusal`), this passes after checking that
# tools/untrusted-gate.txt is well formed and every step it names exists. If the
# refusal is gone from any of them, it fails unless every condition of the ADR
# holds: no test of stage 1 is `pending`, every named step is a step of
# `make test` and of the CI workflow, and every invariant of ADR-0260 to
# ADR-0267, INV-0268-3, ADR-0269 and ADR-0270 (INV-0262-4 aside) and INV-0172-2
# and INV-0172-3 is `checked`, with no word in its status that says part is open.
# Every marker is counted (two in the shell, one in the panel): losing one opens it. The same for stage 2 and the `+net` refusal (lines
# marked `untrusted-gate: net-refusal`; they stay until stage 2 is ready and
# INV-0262-4 is `checked`). The architect's go-ahead is not a thing
# a script can read: it is the commit that removes the markers.
set -eu
cd "$(dirname "$0")/.."
gate=tools/untrusted-gate.txt
fail=0
bad() { echo "check-untrusted-gate: $*" >&2; fail=1; }

[ -r "$gate" ] || { echo "check-untrusted-gate: no $gate" >&2; exit 1; }

required="charged-allocation memory-hog fork-bomb masked-time cpu-spinners disk-filler disk-filler-tmp service-floods keeper many-rooms hostile-programs fuzzing net-account-flood net-fuzz"
entries=$(grep -v '^#' "$gate" | grep -v '^[[:space:]]*$' || true)
for name in $required; do
	echo "$entries" | awk -v n="$name" '$2 == n { found = 1 } END { exit !found }' ||
		bad "$gate does not list $name (ADR-0268 point 3 names it)"
done
echo "$entries" | awk '$1 != "1" && $1 != "2" { exit 1 }' || bad "a line of $gate has no stage 1 or 2"

# The steps of `make test` and of the workflow.
test_steps=$(sed -n 's/^TEST_STEPS := //p;s/^TEST_STEPS += //p' Makefile | tr '\n' ' ')
# The workflow's matrix is the plan's: the full list is tools/ci-tests.txt.
workflow=.github/workflows/test.yml
jobs_list=tools/ci-tests.txt
step_exists() { # a step name
	case " $test_steps " in *" $1 "*) ;; *) return 1 ;; esac
	[ -r "$workflow" ] && [ -r "$jobs_list" ] || return 1
	if [ "$1" = test-qemu ]; then
		grep -q 'make test-qemu-' "$workflow" "$jobs_list"
	else
		grep -q -E "make $1([^[:alnum:]_-]|\$)" "$workflow" "$jobs_list"
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

# The status cell of an invariant's row (the text from its last `| checked:`
# on; a row whose status does not begin so has none) must say `checked:` and
# not say that part of the check is open (ADR-0268 point 2: "checked", not
# "partly checked").
invariants_ok() { # a pattern of ids, then ids left out
	rows=$(grep -E "^\| ($1) " docs/adr/invariants.md || true)
	[ -z "${2:-}" ] || rows=$(echo "$rows" | grep -v -E "^\| ($2) " || true)
	not_checked=$(echo "$rows" | awk '
		NF == 0 { next }
		{
			at = 0; line = $0
			while ((i = index(substr(line, at + 1), "| checked:")) > 0) at += i
			id = $2
			if (at == 0) { print id " (status is not `checked:`)"; next }
			status = substr(line, at)
			if (status ~ /[Nn]ot yet|[Pp]ending/ || tolower(status) ~ /(^|[^[:alnum:]])open([^[:alnum:]]|$)/)
				print id " (its status says that part is open or not yet checked)"
		}')
	if [ -n "$not_checked" ]; then
		echo "$not_checked" | sed 's/^/  not checked: /' >&2
		return 1
	fi
}

# The markers of the refusal, counted one by one (a file with two of three
# gone would still be a file with a marker): the shell's two (entry to an
# untrusted room, `room new --untrusted`) and the panel's one (a launch).
markers() { grep -c "$1" "$2" || true; }
zosh_refusals=$(markers 'untrusted-gate: refusal' user/src/bin/zosh.rs)
panel_refusals=$(markers 'untrusted-gate: refusal' crates/zoraya-panel/src/lib.rs)
if [ "$zosh_refusals" -ge 2 ] && [ "$panel_refusals" -ge 1 ]; then
	pending=$(echo "$entries" | awk '$1 == "1" && $3 == "pending"' | wc -l | tr -d ' ')
	echo "check-untrusted-gate: the refusal is in place ($zosh_refusals markers in the shell, $panel_refusals in the panel); $pending test(s) of stage 1 are still pending"
else
	echo "check-untrusted-gate: a marker of the refusal is gone ($zosh_refusals of 2 in the shell, $panel_refusals of 1 in the panel): the gate is open" >&2
	ready 1 || bad "the refusal of room new --untrusted is gone while a test of stage 1 is pending or missing"
	# INV-0262-4 waits for stage 2 (ADR-0268 point 2).
	invariants_ok "INV-026[0-7]-[0-9]+|INV-0268-3|INV-0269-[0-9]+|INV-0270-[0-9]+|INV-0172-2|INV-0172-3" "INV-0262-4" ||
		bad "the refusal is gone while invariants of ADR-0260 to ADR-0270 are not checked"
fi

# The +net refusal (ADR-0268 point 7) stays until stage 2 is ready, whether or
# not the gate is open: without it the first stage would open +net as well.
net_refusal=$(( $(markers 'untrusted-gate: net-refusal' user/src/bin/zosh.rs) + $(markers 'untrusted-gate: net-refusal' crates/zoraya-panel/src/lib.rs) ))
if [ "$net_refusal" -eq 0 ]; then
	ready 2 || bad "no +net refusal is marked (untrusted-gate: net-refusal) while a test of stage 2 is pending or missing"
	invariants_ok "INV-0262-4" ||
		bad "no +net refusal is marked while INV-0262-4 (the connection's stream rings charged to the client) is not checked"
fi

# The refusal is one function called from two places in the shell (a native
# program's grant and a POSIX program's): each call carries its own marker,
# so that deleting one call (review of the fixes, warning 6) fails here even
# though the function and its marker are still there.
net_calls=$(markers 'untrusted-gate: net-refusal-call' user/src/bin/zosh.rs)
if [ "$net_refusal" -gt 0 ] && [ "$net_calls" -lt 2 ]; then
	bad "a call of the +net refusal is gone from the shell ($net_calls of 2 markers 'untrusted-gate: net-refusal-call'): one path to a connection is open"
fi

# The refusal of a window of a POSIX program in an untrusted room (ADR-0317,
# INV-0317-1): one in the shell (the request) and one in the panel (the
# record). Unlike the gate's, it stays after the gate opens: a window of a
# POSIX program is for a trusted room until an ADR says otherwise.
window_shell=$(markers 'untrusted-gate: window-refusal' user/src/bin/zosh.rs)
window_panel=$(markers 'untrusted-gate: window-refusal' crates/zoraya-panel/src/lib.rs)
if [ "$window_shell" -lt 1 ] || [ "$window_panel" -lt 1 ]; then
	bad "a marker of the window refusal is gone ($window_shell of 1 in the shell, $window_panel of 1 in the panel; untrusted-gate: window-refusal, ADR-0317)"
fi

# The refusal of a description's web rule in an untrusted room (ADR-0324
# item 7, INV-0324-3): the function in the shell's crate and its two calls in
# zosh (a native command's scope and a POSIX program's). Like the window's,
# it stays after the gate opens: the gate's second stage opens a room's own
# network words, never a description's web.
web_fn=$(markers 'untrusted-gate: web-refusal (' crates/zoraya-shell/src/netgrant.rs)
web_calls=$(markers 'untrusted-gate: web-refusal-call' user/src/bin/zosh.rs)
if [ "$web_fn" -lt 1 ] || [ "$web_calls" -lt 2 ]; then
	bad "a marker of the web refusal is gone ($web_fn of 1 in crates/zoraya-shell/src/netgrant.rs, $web_calls of 2 calls in the shell; untrusted-gate: web-refusal, ADR-0324)"
fi

[ "$fail" -eq 0 ] || exit 1
echo "check-untrusted-gate: ok"
