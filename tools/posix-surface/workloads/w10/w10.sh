#!/bin/sh
# W10 (TASK-02b): shell-level parallelism in dash. `sh w10.sh parallel` runs background jobs, waits for them,
# and runs pipelines whose stages are concurrent; `sh w10.sh serial` does the same work one command at a time
# (baseline for the syscall delta). Uses ../w8/mktab.c as the unit of work (about 0.4 s per table).
set -u
mode=${1:-parallel}
tcc -o mktab mktab.c || exit 1

if [ "$mode" = parallel ]; then
	./mktab 1 > t1.h & p1=$!
	./mktab 2 > t2.h & p2=$!
	(./mktab 3 > t3.h; echo "subshell job done") &
	wait "$p1"; s1=$?
	wait "$p2"; s2=$?
	wait
	echo "background: $s1 $s2"
	./mktab 4 | sort | wc -l                    # stages run concurrently
	yes w10 | head -n 3                         # head exits early; yes gets SIGPIPE
	echo "pipeline status: $?"
else
	./mktab 1 > t1.h; s1=$?
	./mktab 2 > t2.h; s2=$?
	./mktab 3 > t3.h; echo "subshell job done"
	echo "background: $s1 $s2"
	./mktab 4 > p.txt; sort p.txt > q.txt; wc -l < q.txt
	printf 'w10\nw10\nw10\n'
	echo "pipeline status: 0"
fi
cat t1.h t2.h t3.h | wc -l
