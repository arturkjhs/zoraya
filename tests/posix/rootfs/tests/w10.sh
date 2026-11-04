#!/bin/sh
# W10 (ADR-0110 item 2; TASK-02b remote/parallel.sh): TASK-02b's w10.sh,
# unchanged, `parallel` and then `serial`, each in its own fresh tree; their
# outputs must be the same (busybox diff), and the parallel run's pipeline
# status line is printed.
rc=0
for v in parallel serial; do
	cd "/work/w10-$v" || exit 1
	echo "W10-STEP $v"
	sh w10.sh $v > out.txt 2>&1 || { echo "W10-FAIL $v $?"; rc=1; }
done
bg=0
while read -r line; do
	case $line in
	"pipeline status:"*) echo "W10-PIPELINE $line" ;;
	"background: 0 0") bg=1 ;;
	esac
done < /work/w10-parallel/out.txt
# Beyond TASK-02b's diff (review R13): both background jobs exited 0, and
# each run gave its lines: subshell, background, the count, three w10s,
# the status, the tables' count.
[ "$bg" = 1 ] || { echo "W10-FAIL background statuses"; rc=1; }
for v in parallel serial; do
	n=0
	while read -r line; do n=$((n + 1)); done < "/work/w10-$v/out.txt"
	[ "$n" = 8 ] || { echo "W10-FAIL $v: $n lines"; rc=1; }
done
if diff /work/w10-parallel/out.txt /work/w10-serial/out.txt; then
	echo "W10-OK"
else
	echo "W10-FAIL diff"
	rc=1
fi
echo "W10-STATUS $rc"
