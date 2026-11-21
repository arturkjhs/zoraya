#!/bin/sh
# Negative cases of tools/check-test-shards.sh: each edit of a copy of the
# Makefile must make it fail, and the real one must pass.
set -eu
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
sh tools/check-test-shards.sh >/dev/null || { echo "check-test-shards-test: the real Makefile fails" >&2; exit 1; }
fail() { # description, sed expression applied to the first match only
	sed "$2" Makefile >"$dir/Makefile"
	if cmp -s Makefile "$dir/Makefile"; then echo "check-test-shards-test: edit '$1' changed nothing" >&2; exit 1; fi
	if sh tools/check-test-shards.sh kernel/src/testing.rs "$dir/Makefile" >/dev/null 2>&1; then
		echo "check-test-shards-test: '$1' was not caught" >&2; exit 1
	fi
}
a=test-qemu-auto-smp4-2g-a
# Line numbers of the target lines, to edit one recipe only.
n=$(grep -n "^$a:" Makefile | cut -d: -f1)
fail "cpus 4 to 3 in one -a" "$((n + 1)),$((n + 3))s/cpus: 4/cpus: 3/"
fail "--mem differs in one -a" "$((n + 1)),$((n + 3))s/--mem 2G/--mem 1G/"
fail "-b without -a" "s/^test-qemu-auto-smp4-2g-a:/test-qemu-auto-smp4-2g-x:/"
fail "--shard 2/2 in a target without -b" "$((n + 3))s#--shard 1/2#--shard 2/2#"
echo "check-test-shards-test: ok"
