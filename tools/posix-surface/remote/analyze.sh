#!/bin/sh
# Stage: derive CSVs and summary tables from raw data.
. "$(dirname "$0")/common.sh"
python3 -I "$TOOLS/analysis/analyze.py" "$W"
