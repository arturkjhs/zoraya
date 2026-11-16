#!/bin/sh
# Fails unless the two shards of the kernel test suite (`--shard N/2` of
# tools/run-qemu.sh, SHARD_2_FIRST in kernel/src/testing.rs) together run each
# registered test exactly once, and shard 1 holds every test the runner
# expects to panic (`--expect-panic` in the Makefile's EXPECT), because shard 2
# is run with no expected panic. The shards are the registered list cut at
# SHARD_2_FIRST: shard 1 is the tests before it, shard 2 the tests from it on,
# so the union is the list and the intersection is empty when the name is
# registered once and the names are unique. The guest prints the cut it used
# (`shard: N/M runs tests LO..HI of TOTAL`) and the Makefile expects that line.
set -eu
src=${1:-kernel/src/testing.rs}
makefile=${2:-Makefile}

names=$(sed -n '/^static TESTS: /,/^];/p' "$src" | sed -n -E 's/^[[:space:]]*name: "([^"]+)",$/\1/p')
total=$(echo "$names" | wc -l | tr -d ' ')
first=$(sed -n -E 's/^const SHARD_2_FIRST: &str = "([^"]+)";$/\1/p' "$src")
[ -n "$first" ] || { echo "check-test-shards: no SHARD_2_FIRST in $src" >&2; exit 1; }

dups=$(echo "$names" | sort | uniq -d)
[ -z "$dups" ] || { echo "check-test-shards: registered twice: $dups" >&2; exit 1; }
split=$(echo "$names" | grep -n -x -F "$first" | cut -d: -f1)
[ -n "$split" ] || { echo "check-test-shards: SHARD_2_FIRST $first is not a registered test" >&2; exit 1; }
split=$((split - 1))

bad=0
panics=$(sed -n -E 's/^[[:space:]]*(EXPECT_FAULTS :=)?[[:space:]]*--expect-panic ([A-Za-z0-9_]+).*/\2/p' "$makefile")
for p in $panics harness_resumes_after_panic; do
	idx=$(echo "$names" | grep -n -x -F "$p" | cut -d: -f1 || true)
	if [ -z "$idx" ]; then
		echo "check-test-shards: $p is not a registered test" >&2; bad=1
	elif [ $((idx - 1)) -ge "$split" ]; then
		echo "check-test-shards: $p is in shard 2 (index $((idx - 1)), the cut is $split)" >&2; bad=1
	fi
done
n=$(echo "$panics" | wc -l | tr -d ' ')
want=$(sed -n -E 's/^const EXPECTED_FAILURES: usize = ([0-9]+);$/\1/p' "$src")
[ "$n" = "$want" ] || { echo "check-test-shards: EXPECTED_FAILURES is $want, EXPECT lists $n" >&2; bad=1; }
[ "$bad" = 0 ] || exit 1
echo "check-test-shards: $total tests, shard 1 = 0..$split, shard 2 = $split..$total"
