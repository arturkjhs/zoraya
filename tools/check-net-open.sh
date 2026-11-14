#!/bin/sh
# INV-0230-1: the tests open no connection outside 127.0.0.1 and the guest's
# SLIRP network. Three things are checked:
#  1. tools/run-qemu.sh puts `restrict=on` on a test's network unless
#     --net-open is given (the printed command line, with a stand-in for QEMU),
#     and not on a person's `make run-net`;
#  2. every Makefile line with --net-open belongs to a definition listed in
#     tools/net-open-allowed.txt, and every listed one still has such a line;
#  3. the host-side test servers listen on 127.0.0.1 only.
set -eu
cd "$(dirname "$0")/.."
fail() { echo "check-net-open: $*" >&2; exit 1; }
cmd() { QEMU=/bin/echo tools/run-qemu.sh "$@" --accel tcg Makefile 2>&1 | tr ' ' '\n' | grep -A1 '^-netdev$' | tail -1; }
case $(cmd --test --net) in *restrict=on*) ;; *) fail "a test with --net has no restrict=on" ;; esac
case $(cmd --test --net --net-open) in *restrict=on*) fail "--net-open still has restrict=on" ;; esac
case $(cmd --net) in *restrict=on*) fail "a person's run has restrict=on" ;; esac
used=$(awk '/^[^\t #][^#]*[:=]/ { split($0, a, /[ :=]/); d = a[1] }
	(/^\t/ || /^[A-Za-z_]/) && /--net-open/ { print d }' Makefile | sort -u)
allowed=$(grep -v '^#' tools/net-open-allowed.txt | awk 'NF { print $1 }' | sort -u)
[ -n "$used" ] || fail "no --net-open line found in Makefile (is the awk out of date?)"
for d in $used; do
	echo "$allowed" | grep -qx "$d" || fail "$d starts QEMU with --net-open and is not in tools/net-open-allowed.txt"
done
for d in $allowed; do
	echo "$used" | grep -qx "$d" || fail "$d is in tools/net-open-allowed.txt but has no --net-open line"
done
if grep -rnE 'bind\(\(?"0\.0\.0\.0"|\[::\]|INADDR_ANY' tools/test-httpd/src; then fail "a test server may listen beyond 127.0.0.1"; fi
grep -q 'TcpListener::bind(("127.0.0.1"' tools/test-httpd/src/main.rs || fail "tools/test-httpd does not bind 127.0.0.1"
echo "check-net-open: ok ($(echo "$used" | wc -l | tr -d ' ') definitions with --net-open, all listed)"
