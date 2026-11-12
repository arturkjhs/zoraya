#!/bin/sh
# INV-0260-1: every resource ADR-0260's table marks **hard** names, in
# tools/quota-tests.txt, a row of tools/untrusted-gate.txt that has a step (not
# `pending`). A resource with no test is reported; it fails the check only when
# the refusal of `room new --untrusted` is gone from the shell and the panel
# (the same condition as check-untrusted-gate.sh), so the gap stays visible while
# the gate is closed.
set -eu
cd "$(dirname "$0")/.."
adr=docs/adr/0260-what-is-limited.md
map=tools/quota-tests.txt
gate=tools/untrusted-gate.txt
gaps=0
rows=$(grep -E '^\| .*\*\*hard\*\*' "$adr" | awk -F'|' '{ gsub(/^ +| +$/, "", $2); print $2 }')
[ -n "$rows" ] || { echo "check-quota-tests: no hard row in $adr" >&2; exit 1; }
echo "$rows" | while IFS= read -r resource; do
	line=$(grep -F "$resource |" "$map" | head -1 || true)
	if [ -z "$line" ]; then echo "check-quota-tests: $resource: not in $map" >&2; exit 1; fi
done || exit 1
for resource in $(echo "$rows" | tr ' ' '_'); do
	name=$(echo "$resource" | tr '_' ' ')
	tests=$(grep -F "$name |" "$map" | head -1 | awk -F'|' '{ gsub(/ /, "", $2); print $2 }')
	ok=0
	for t in $(echo "$tests" | tr ',' ' '); do
		[ "$t" = - ] && continue
		step=$(grep -v '^#' "$gate" | awk -v n="$t" '$2 == n { print $3 }')
		if [ -n "$step" ] && [ "$step" != pending ]; then ok=1; else echo "check-quota-tests: $name: $t is not a gate row with a step" >&2; exit 1; fi
	done
	if [ "$ok" = 0 ]; then
		echo "check-quota-tests: gap: no test reaches the refusal of: $name"
		gaps=$((gaps + 1))
	fi
done
without=$(grep -c 'untrusted-gate: refusal' user/src/bin/zosh.rs crates/zoraya-panel/src/lib.rs | awk -F: '$2 < 1 { n++ } END { print n + 0 }')
if [ "$gaps" -gt 0 ] && [ "$without" -gt 0 ]; then
	echo "check-quota-tests: the refusal is gone while $gaps resource(s) have no test" >&2
	exit 1
fi
echo "check-quota-tests: ok ($gaps gap(s) while the gate is closed)"
