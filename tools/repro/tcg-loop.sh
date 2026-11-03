#!/bin/sh
# Runs the test kernel of one commit under TCG again and again, to catch a
# rare hang, and on a hang takes the CPU's registers and a backtrace.
#
#   tools/repro/tcg-loop.sh COMMIT [RUNS] [PARALLEL]
#
# The commit is built once in its own worktree, build/repro-<commit>. Each run
# is the whole test kernel with `--gui`, as `make test`'s tcg-default runs it,
# with its own disk, QMP socket and gdb port, at most PARALLEL (default 2, the
# most: docs/backlog.md) at once, under tools/mutants/watchdog.sh. A run that
# has not printed `ZORAYA-TEST end` after HANG_S seconds (default 240; a good
# run takes about 60) is taken as hung: QMP gives 30 samples of the registers,
# then QEMU's gdb stub and lldb give `bt`, symbolised from the ELF. Results go
# to build/repro-<commit>/runs/<n>/; the summary is printed at the end.
#
# TASK-16 used it on 96c5a79 for the hang its review records.

set -u
commit=${1:?usage: $0 COMMIT [RUNS] [PARALLEL]}
runs=${2:-20}
parallel=${3:-2}
[ "$parallel" -le 2 ] || parallel=2
hang_s=${HANG_S:-240}
timeout_s=${TIMEOUT_S:-300}
top=$(cd "$(dirname "$0")/../.." && pwd)
cargo=${CARGO:-$HOME/.cargo/bin/cargo}
dir=$top/build/repro-$commit

if [ ! -d "$dir" ]; then
    git -C "$top" worktree add --detach "$dir" "$commit" >/dev/null || exit 1
fi
cd "$dir" || exit 1
PATH=$HOME/.cargo/bin:$PATH make -s build/zoraya-kernel-test.elf target/debug/zoraya-test-runner \
    >build.log 2>&1 || { echo "build failed: $dir/build.log"; exit 1; }
"$cargo" build -q -p zoraya-mkfs || exit 1
nm -n build/zoraya-kernel-test.elf >symbols.txt
mkdir -p runs

# Prints the symbol holding each hexadecimal address read on stdin.
symbolise() {
    awk 'NR == FNR { if ($2 ~ /^[tTwW]$/) { a[n] = $1; s[n++] = $3 } next }
         { x = tolower($1); best = "?"
           for (i = 0; i < n; i++) { if (a[i] <= x) best = s[i]; else break }
           print $1, best }' symbols.txt -
}

# Sends human monitor commands over QMP, one per argument, printing replies.
qmp() {
    python3 -I - "$@" <<'EOF'
import json, socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
f = s.makefile("rw")
f.readline()
f.write('{"execute":"qmp_capabilities"}\n'); f.flush(); f.readline()
for c in sys.argv[2:]:
    f.write(json.dumps({"execute": "human-monitor-command",
                        "arguments": {"command-line": c}}) + "\n"); f.flush()
    while True:
        r = json.loads(f.readline())
        if "return" in r or "error" in r:
            print(r.get("return", r.get("error")))
            break
EOF
}

one() {
    n=$1
    out=runs/$n
    mkdir -p "$out"
    disk=$out/disk.img
    sock=/tmp/zoraya-repro-$$-$n.sock
    port=$((12340 + n))
    target/debug/zoraya-mkfs --size 16M --from rootfs "$disk" >/dev/null
    "$top/tools/mutants/watchdog.sh" target/debug/zoraya-test-runner --timeout "$timeout_s" --label "run$n" -- \
        tools/run-qemu.sh --test --gui --qmp "$sock" --disk "$disk" --accel tcg build/zoraya-kernel-test.elf \
        >"$out/serial.log" 2>&1 &
    runner=$!
    waited=0
    while kill -0 $runner 2>/dev/null && [ $waited -lt "$hang_s" ]; do
        sleep 5
        waited=$((waited + 5))
        grep -q "ZORAYA-TEST end" "$out/serial.log" && break
    done
    if grep -q "ZORAYA-TEST end" "$out/serial.log"; then
        wait $runner
        echo "run $n: finished ($(grep -o 'pass=[0-9]* fail=[0-9]*' "$out/serial.log"))"
        rm -f "$disk"
        return
    fi
    if ! kill -0 $runner 2>/dev/null; then
        echo "run $n: ended without an end line, see $out/serial.log"
        return
    fi
    echo "run $n: no end after ${hang_s} s; last test: $(grep 'ZORAYA-TEST run' "$out/serial.log" | tail -1)"
    i=0
    while [ $i -lt 30 ]; do
        qmp "$sock" "info registers" | grep -o 'PC=[0-9a-f]*\|X30=[0-9a-f]*\|PSTATE=[0-9a-f]*' | tr '\n' ' '
        echo
        i=$((i + 1))
    done >"$out/registers.txt"
    grep -o 'PC=[0-9a-f]*' "$out/registers.txt" | cut -d= -f2 | symbolise | sort | uniq -c | sort -rn \
        >"$out/pc-functions.txt"
    grep -o 'X30=[0-9a-f]*' "$out/registers.txt" | cut -d= -f2 | symbolise | sort | uniq -c | sort -rn \
        >"$out/lr-functions.txt"
    qmp "$sock" "gdbserver tcp::$port" >/dev/null
    lldb -b build/zoraya-kernel-test.elf -o "gdb-remote 127.0.0.1:$port" -o "bt 40" \
        -o "register read pc lr sp" -o "detach" >"$out/lldb.txt" 2>&1
    qmp "$sock" "quit" >/dev/null 2>&1
    wait $runner
    echo "run $n: samples in $out (pc-functions.txt, lr-functions.txt, lldb.txt)"
}

n=1
while [ $n -le "$runs" ]; do
    j=0
    while [ $j -lt "$parallel" ] && [ $n -le "$runs" ]; do
        one $n &
        n=$((n + 1))
        j=$((j + 1))
    done
    wait
done
echo "summary: $(grep -l 'ZORAYA-TEST end' runs/*/serial.log 2>/dev/null | wc -l | tr -d ' ') of $runs runs finished"
