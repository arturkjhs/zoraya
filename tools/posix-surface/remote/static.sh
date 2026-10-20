#!/bin/sh
# Stage: M1 static upper bound (import tables of the measured binaries, restricted to musl exports).
. "$(dirname "$0")/common.sh"
python3 -I "$TOOLS/analysis/static.py" "$RES"
