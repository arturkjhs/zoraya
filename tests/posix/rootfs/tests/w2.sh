#!/bin/sh
# W2 and W2-j (ADR-0110 item 2): Lua's plain Makefile with tcc as compiler
# and tcc -ar as archiver, as TASK-02's remote/workloads.sh W2 and TASK-02b's
# remote/parallel.sh ran it, each build followed by W2-check. `sh w2.sh
# serial` is W2, once with pdpmake (TASK-02's set t1p, where `make` is
# pdpmake: /sets/t1p first on PATH) and once with GNU make; `sh w2.sh
# parallel` is W2-j, GNU make at -j2 and -j4 in both jobserver styles. Each
# build is in its own fresh tree (trees.sh).
rc=0
build() { # id [make arguments]
	id=$1
	shift
	cd "/work/$id" || exit 1
	echo "W2-STEP $id"
	make "$@" CC=tcc AR="tcc -ar rcu" RANLIB=true posix || { echo "W2-FAIL $id make $?"; rc=1; }
	out=$(sh -c './src/lua -e "print(string.format(\"lua ok %d\", 6*7))"')
	if [ "$out" = "lua ok 42" ]; then
		echo "W2-OK $id"
	else
		echo "W2-FAIL $id check: $out"
		rc=1
	fi
}
case ${1:-serial} in
serial)
	PATH=/sets/t1p:$PATH build w2-p
	build w2-g
	;;
parallel)
	for j in 2 4; do for st in pipe fifo; do
		build w2j-$st-j$j -j$j --jobserver-style=$st
	done; done
	;;
esac
echo "W2-STATUS $rc"
