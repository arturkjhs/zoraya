#!/bin/sh
# Every step make test-changed can select has a lane in tools/test-lanes.txt
# or a line in its skip list: a step that is listed and not run must print
# SKIPPED or fail, never pass silently (work/test-fixes, the audit's gap).
#
#   TEST_STEPS=... tools/check-test-lanes.sh
#
# Reads the steps of tools/test-map.txt, of plan-lib.sh's `all` (what an
# unmapped path selects) and TEST_STEPS' test-* steps; checks the lanes file's
# own rules (an optin step is in a lane, a skip step is in none, a step is in
# one lane, every named step exists); then runs tools/test-lanes-test.sh.
set -eu
cd "$(dirname "$0")/.."
lanes=tools/test-lanes.txt
map=tools/test-map.txt
bad=0
fail() { echo "check-test-lanes: $*" >&2; bad=1; }

in_lanes=$(awk '$1 == "solo" { print $2 } $1 == "lane" { for (i = 2; i <= NF; i++) print $i }' "$lanes")
dups=$(echo "$in_lanes" | sort | uniq -d)
[ -z "$dups" ] || fail "in two lanes: $(echo $dups)"
skips=$(awk '$1 == "skip" { for (i = 2; i <= NF && $i != "--"; i++) print $i }' "$lanes")
optins=$(awk '$1 == "optin" { for (i = 3; i <= NF && $i != "--"; i++) print $i }' "$lanes")
for s in $skips; do
	echo "$in_lanes" | grep -qx "$s" && fail "$s is in a lane and in the skip list"
done
for s in $optins; do
	echo "$in_lanes" | grep -qx "$s" || fail "$s is an optin step with no lane"
done
awk '($1 == "skip" || $1 == "optin") && !/ -- ./ { print "check-test-lanes: no reason (-- WHY): " $0; bad = 1 } END { exit bad }' "$lanes" >&2 || bad=1

# The steps the map can select, and the steps an unmapped path selects.
selected=$(awk '/^#/ || NF < 2 { next } { for (i = 2; i <= NF; i++) print $i }' "$map" | sort -u)
all=$(sed -n 's/^all="\(.*\)"/\1/p' tools/plan-lib.sh | tr ' ' '\n')
[ -n "$all" ] || fail "no all=\"...\" in tools/plan-lib.sh"
# The explicit all-steps lines (Makefile, run-qemu.sh, test-runner) hold every step of `all`.
for prefix in Makefile tools/run-qemu.sh tools/test-runner/; do
	line=$(awk -v p="$prefix" '$1 == p { print }' "$map")
	[ -n "$line" ] || fail "$map has no line for $prefix (it touches every test)"
	for s in $all; do
		case " $line " in *" $s "*) ;; *) fail "$map: the line for $prefix lacks $s (every step of plan-lib.sh's all)" ;; esac
	done
done
covered=$(printf '%s\n%s\n' "$in_lanes" "$skips")
for s in $selected $all; do
	[ "$s" = check ] && continue
	echo "$covered" | grep -qx "$s" || fail "$s can be selected by make test-changed and has no lane and no skip line in $lanes"
done
# Every test step of make test, and no step in the file that does not exist.
if [ -n "${TEST_STEPS:-}" ]; then
	for s in $TEST_STEPS; do
		case "$s" in
		test-unit | test-qemu) ;;
		test-*) echo "$covered" | grep -qx "$s" || fail "$s is a step of make test and has no lane and no skip line in $lanes" ;;
		esac
	done
	for s in $covered; do
		[ -n "$s" ] || continue
		case " $TEST_STEPS " in *" $s "*) ;; *) fail "$s is in $lanes and is not a step of make test" ;; esac
	done
fi
[ "$bad" = 0 ] || exit 1
tools/test-lanes-test.sh
echo "check-test-lanes: ok"
