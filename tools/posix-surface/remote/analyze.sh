#!/bin/sh
# Stage: derive CSVs and summary tables from raw data.
. "$(dirname "$0")/common.sh"
python3 -I "$TOOLS/analysis/analyze.py" "$W"
python3 -I "$TOOLS/analysis/configure_checks.py" "$W"
python3 -I "$TOOLS/analysis/fork_children.py" "$W"
python3 -I "$TOOLS/analysis/signals_delivered.py" "$W"
