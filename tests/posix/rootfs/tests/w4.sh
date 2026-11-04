#!/bin/sh
# W4 (ADR-0110 item 10, make measure-w4): GNU hello's real configure and
# make, as TASK-02's remote/workloads.sh W4 ran them, once in the GNU make
# set and once in pdpmake's (W4p), each in a fresh tree, then the check
# ./hello. Not an acceptance test: T2's applets and rows are not built, so
# the run records how far each step gets. A step that fails prints
# W4-STOP, not -FAIL: the runner does not stop the measurement there.
run() { # id set
	id=$1
	cd "/work/$id" || exit 1
	case $2 in
	p) PATH=/sets/t1p:/bin:/usr/bin ;;
	*) PATH=/bin:/usr/bin ;;
	esac
	export PATH
	echo "W4-STEP $id configure"
	sh ./configure CC=tcc LD=tcc AR="tcc -ar" CONFIG_SHELL=/bin/sh --disable-nls
	rc=$?
	echo "W4-CONFIGURE $id $rc"
	if [ "$rc" != 0 ]; then
		echo "W4-STOP $id configure $rc"
		[ -f config.log ] && while read -r l; do
			case $l in *"not found"*|*"error"*|*"rror:"*) echo "W4-LOG $l" ;; esac
		done < config.log
		return
	fi
	echo "W4-STEP $id make"
	make
	rc=$?
	echo "W4-MAKE $id $rc"
	[ "$rc" = 0 ] || { echo "W4-STOP $id make $rc"; return; }
	echo "W4-STEP $id check"
	./hello
	echo "W4-CHECK $id $?"
}
run w4-g g
run w4-p p
echo "W4-DONE"
