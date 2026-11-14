#!/bin/sh
# INV-0230-1: the tests open no connection outside 127.0.0.1 and the guest's
# SLIRP network. This is a partial guard: the invariant does NOT hold today (the
# network steps run with --net-open; see ADR-0230's open questions and
# docs/backlog.md), so INV-0230-1 is not `checked`. What is checked:
#  1. tools/run-qemu.sh puts `restrict=on` on a test's network unless
#     --net-open is given (the printed command line, with a stand-in for QEMU),
#     and not on a person's `make run-net`;
#  2. every Makefile line with --net-open belongs to a definition listed in
#     tools/net-open-allowed.txt, and every listed one still has such a line;
#     A run of person mode with --net and no --test (RUN_NET_CMD, which
#     test-hand-run uses) has no restrict=on either and is listed the same way;
#  3. the host-side test servers (tools/test-httpd, tools/test-dns) listen on
#     127.0.0.1 only.
set -eu
cd "$(dirname "$0")/.."
fail() { echo "check-net-open: $*" >&2; exit 1; }
cmd() { QEMU=/bin/echo tools/run-qemu.sh "$@" --accel tcg Makefile 2>&1 | tr ' ' '\n' | grep -A1 '^-netdev$' | tail -1; }
case $(cmd --test --net) in *restrict=on*) ;; *) fail "a test with --net has no restrict=on" ;; esac
case $(cmd --test --net --net-open) in *restrict=on*) fail "--net-open still has restrict=on" ;; esac
case $(cmd --net) in *restrict=on*) fail "a person's run has restrict=on" ;; esac
used=$(awk '/^[^\t #][^#]*[:=]/ { split($0, a, /[ :=]/); d = a[1] }
	(/^\t/ || /^[A-Za-z_]/) && (/--net-open/ || (/\$\(RUN_QEMU\) --net / && !/--test/)) { print d }' Makefile | sort -u)
allowed=$(grep -v '^#' tools/net-open-allowed.txt | awk 'NF { print $1 }' | sort -u)
[ -n "$used" ] || fail "no --net-open line found in Makefile (is the awk out of date?)"
for d in $used; do
	echo "$allowed" | grep -qx "$d" || fail "$d starts QEMU with --net-open and is not in tools/net-open-allowed.txt"
done
for d in $allowed; do
	echo "$used" | grep -qx "$d" || fail "$d is in tools/net-open-allowed.txt but has no --net-open line"
done
for t in test-httpd test-dns; do
	if grep -rnE '0\.0\.0\.0|\[::\]|INADDR_ANY|UNSPECIFIED' tools/$t/src; then fail "tools/$t may listen beyond 127.0.0.1"; fi
	grep -qE 'bind\(\("127\.0\.0\.1"' tools/$t/src/main.rs || fail "tools/$t does not bind 127.0.0.1"
done
echo "check-net-open: ok ($(echo "$used" | wc -l | tr -d ' ') definitions with --net-open, all listed)"
