#!/bin/sh
# Stage: M1 dynamic lower bound. Runs every workload with the LD_PRELOAD interposer.
. "$(dirname "$0")/common.sh"
MODE=dynamic
. "$TOOLS/remote/workloads.sh"
setup_sets
rm -rf "$RAW/dynamic"; mkdir -p "$RAW/dynamic"; : > "$RES/dynamic-status.tsv"
for w in $WORKLOADS; do $w; done
cat "$RES/dynamic-status.tsv"
