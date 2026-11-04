#!/bin/sh
# W8 (ADR-0110 item 2; TASK-02b remote/parallel.sh): the synthetic recursive
# build, serial (-j1) and at -j2 and -j4 in both jobserver styles, each in its
# own fresh tree, each followed by TASK-02b's W8 check: four archives, and
# every table ends with "0 };". The check's `tail -n 1 | grep -q` is written
# with dash's read and case, since T1's applets have neither (the same test:
# the table's last line holds "0 };"). The recipe count is the recipes make
# echoed that compile, generate or archive: 18 a leaf, 72 in all.
# With arguments, only those builds (CI runs each in a job of its own).
rc=0
for b in ${*:-j1 pipe-j2 fifo-j2 pipe-j4 fifo-j4}; do
	id=w8-$b
	case $b in
	j1) args=-j1 ;;
	*) args="-j${b#*-j} --jobserver-style=${b%-j*}" ;;
	esac
	cd "/work/$id" || exit 1
	echo "W8-STEP $id"
	# shellcheck disable=SC2086
	make $args > make.out 2>&1 || { echo "W8-FAIL $id make $?"; rc=1; }
	n=0
	while read -r line; do
		case $line in tcc*|./mktab*) n=$((n + 1)) ;; esac
	done < make.out
	echo "W8-RECIPES $id $n"
	[ "$n" = 72 ] || { echo "W8-FAIL $id recipes $n"; rc=1; }
	ok=$(sh -c 'test "$(ls */*/libleaf.a | wc -l)" -eq 4 && for f in */*/t?.h; do last=; while read -r l; do last=$l; done < "$f"; case $last in *"0 };"*) ;; *) exit 1 ;; esac; done && echo W8-OK')
	if [ "$ok" = W8-OK ]; then
		echo "W8-OK $id"
	else
		echo "W8-FAIL $id check"
		rc=1
	fi
done
echo "W8-STATUS $rc"
