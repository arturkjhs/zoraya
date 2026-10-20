#!/bin/sh
# Stage: M3 candidate call sites (pattern match restricted to lines in the binaries' DWARF line tables).
. "$(dirname "$0")/common.sh"
python3 -I "$TOOLS/analysis/callsites.py" "$W"
