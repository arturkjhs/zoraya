#!/bin/sh
# Runs a command and kills any process under it that takes too much memory.
#
#   tools/mutants/watchdog.sh COMMAND [ARGS...]
#
# macOS ignores `ulimit -v`, so a test that grows without bound (a mutant
# that never consumes a queue, say) is stopped by watching instead: once a
# second this reads the memory of the command and of every process
# descended from it (with tools/footprint, macOS's physical footprint, which
# counts compressed pages: the TASK-20 freeze grew under a small resident
# size; without it, or off macOS, the resident size from ps), kills (-9) one that holds more than WATCHDOG_MB, and
# kills the largest when all of them together hold more than
# WATCHDOG_TOTAL_MB. A killed
# test fails, so a mutation tool counts its mutant as caught. Each kill is
# reported on stderr. The exit status is the command's.
#
# The host tests also cap themselves at 512 MiB (crates/zoraya-test-alloc);
# this covers what that does not: other binaries, the build, QEMU.
# docs/backlog.md records why both exist.

per_process_mb=${WATCHDOG_MB:-3072}
total_mb=${WATCHDOG_TOTAL_MB:-12288}
# The footprint helper; WATCHDOG_FOOTPRINT=none falls back to ps's RSS.
footprint=${WATCHDOG_FOOTPRINT:-$(dirname "$0")/../../target/debug/zoraya-footprint}

[ $# -ge 1 ] || { echo "usage: $0 COMMAND [ARGS...]" >&2; exit 2; }

"$@" &
root=$!

# Prints "pid rss_kib command" for $root and every process descended from it.
descendants() {
    ps -A -o pid=,ppid=,rss=,comm= | awk -v root="$root" '
        { pid[NR] = $1; parent[$1] = $2; rss[$1] = $3; $1 = $2 = $3 = ""; cmd[pid[NR]] = $0 }
        END {
            for (i = 1; i <= NR; i++) {
                p = pid[i]
                if (p == root) { print p, rss[p], cmd[p]; continue }
                for (q = parent[p]; q != "" && q > 1; q = parent[q])
                    if (q == root) { print p, rss[p], cmd[p]; break }
            }
        }'
}

# The list with each process's memory from the helper where it answers. The
# helper's lines and the list go to awk on stdin, a "--" between them (macOS's
# awk refuses a newline in a -v value); if awk fails, the list as ps gave it.
measured() {
    list=$(descendants)
    if [ -x "$footprint" ]; then
        pids=$(echo "$list" | awk '{ print $1 }')
        # shellcheck disable=SC2086 # one argument per pid
        merged=$({ "$footprint" $pids 2>/dev/null; echo "--"; echo "$list"; } | awk '
            $0 == "--" { body = 1; next }
            !body { kib[$1] = $2; next }
            NF { if ($1 in kib) $2 = kib[$1]; print }') && [ -n "$merged" ] || merged=$list
        echo "$merged"
    else
        echo "$list"
    fi
}

watch() {
    while kill -0 "$root" 2>/dev/null; do
        list=$(measured)
        echo "$list" | while read -r pid rss cmd; do
            [ -n "$pid" ] || continue
            if [ "$rss" -gt $((per_process_mb * 1024)) ]; then
                echo "watchdog: killed $pid ($cmd) at $((rss / 1024)) MiB, above $per_process_mb MiB" >&2
                kill -9 "$pid" 2>/dev/null
            fi
        done
        total=$(echo "$list" | awk '{ s += $2 } END { print s + 0 }')
        if [ "$total" -gt $((total_mb * 1024)) ]; then
            biggest=$(echo "$list" | sort -k2 -n -r | head -1)
            set -- $biggest
            echo "watchdog: all together $((total / 1024)) MiB, above $total_mb MiB: killed $1 ($3) at $(($2 / 1024)) MiB" >&2
            kill -9 "$1" 2>/dev/null
        fi
        sleep 1
    done
}

watch &
watcher=$!
wait "$root"
status=$?
kill "$watcher" 2>/dev/null
wait "$watcher" 2>/dev/null
exit $status
