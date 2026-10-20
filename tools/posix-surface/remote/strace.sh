#!/bin/sh
# Stage: M2 syscall surface. Runs every workload under strace -f (no interposer, so its syscalls don't appear).
. "$(dirname "$0")/common.sh"
MODE=strace
. "$TOOLS/remote/workloads.sh"
setup_sets
rm -rf "$RAW/strace"; mkdir -p "$RAW/strace"; : > "$RES/strace-status.tsv"
for w in $WORKLOADS; do $w; done
cat "$RES/strace-status.tsv"
