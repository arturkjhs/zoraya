#!/bin/sh
# W3 and W3-j (ADR-0110 item 2): tcc on SQLite's amalgamation, as TASK-02's
# remote/workloads.sh W3 ran it from a dash script, then W3-check (a query
# printing 42); `sh w3.sh parallel` is W3-j, TASK-02b's Makefile of the same
# commands at -j2 and -j4 in both jobserver styles. Both compiles take
# -DSQLITE_OMIT_LOAD_EXTENSION, ADR-0210 item 4: libc.a has no
# dlopen (INV-0005-2), which SQLite's run-time extensions need.
rc=0
check() { # id
	out=$(sh -c 'echo "create table t(x); insert into t values(40),(2); select sum(x) from t;" | ./sqlite3')
	if [ "$out" = 42 ]; then
		echo "W3-OK $1"
	else
		echo "W3-FAIL $1 check: $out"
		rc=1
	fi
}
case ${1:-serial} in
serial)
	cd /work/w3 || exit 1
	echo "W3-STEP w3"
	sh -c 'tcc -O2 -DSQLITE_THREADSAFE=0 -DSQLITE_OMIT_LOAD_EXTENSION -c sqlite3.c && tcc -DSQLITE_OMIT_LOAD_EXTENSION -c shell.c && tcc -o sqlite3 shell.o sqlite3.o -lm' ||
		{ echo "W3-FAIL w3 build $?"; rc=1; }
	check w3
	;;
parallel)
	for j in 2 4; do for st in pipe fifo; do
		id=w3j-$st-j$j
		cd "/work/$id" || exit 1
		echo "W3-STEP $id"
		make -j$j --jobserver-style=$st || { echo "W3-FAIL $id make $?"; rc=1; }
		check "$id"
	done; done
	;;
esac
echo "W3-STATUS $rc"
