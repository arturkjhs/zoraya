#!/bin/sh
# W1 (ADR-0110 item 2): tcc compiles itself, as TASK-02's remote/workloads.sh
# W1 ran it: configure run by dash, the build by GNU make, then W1-check. The
# configure line is ADR-0109 item 8's: --config-musl dropped, the Zoraya
# target named. The tree /work/w1 is a fresh copy of /src/tcc (trees.sh).
cd /work/w1 || exit 1
echo "W1-STEP configure"
sh ./configure --cc=tcc --cpu=aarch64 --targetos=Zoraya --prefix=/work/w1/inst || echo "W1-FAIL configure $?"
echo "W1-STEP make"
make || echo "W1-FAIL make $?"
echo "W1-STEP check"
sh -c './tcc -v && printf "int main(){return 0;}\n" > t.c && ./tcc -B. -o t t.c && ./t && echo W1-OK'
rc=$?
[ "$rc" = 0 ] || echo "W1-FAIL check $rc"
echo "W1-STATUS $rc"
