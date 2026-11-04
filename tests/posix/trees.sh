#!/bin/sh
# The build trees of the acceptance workloads (ADR-0110 item 1), made on the
# host into the image's root as TASK-02's harness made them on Linux: a fresh,
# writable copy of the pinned source for every build (remote/workloads.sh's
# fresh_run, remote/parallel.sh's tree), so that no build sees another's
# products. The guest has no cp (T1's applets), and the copy is the harness's
# work, not the workload's.
#
# Usage: trees.sh ROOT, where ROOT/src holds tcc, lua, sqlite, w3, w8 and w10.
set -eu
root=$1
src=$root/src
work=$root/work
rm -rf "$work"
mkdir -p "$work"
tree() { # id source
	cp -R "$src/$2" "$work/$1"
}
# W1: tcc compiles itself.
tree w1 tcc
# W2: Lua, once with each make; W2-j: GNU make at -j2 and -j4, both styles.
tree w2-p lua
tree w2-g lua
for j in 2 4; do for st in pipe fifo; do tree w2j-$st-j$j lua; done; done
# W3: SQLite from a dash script; W3-j: the same as a Makefile (TASK-02b).
tree w3 sqlite
for j in 2 4; do for st in pipe fifo; do
	tree w3j-$st-j$j sqlite
	cp "$src/w3/Makefile" "$work/w3j-$st-j$j/Makefile"
done; done
# W8: serial, then -j2 and -j4 in both styles; W9: two copies to interrupt.
tree w8-j1 w8
for j in 2 4; do for st in pipe fifo; do tree w8-$st-j$j w8; done; done
tree w9-int w8
tree w9-term w8
# W10: the parallel script and its serial baseline.
tree w10-parallel w10
tree w10-serial w10
# W4 (make measure-w4): GNU hello's configure and make, with each make.
tree w4-g hello
tree w4-p hello
