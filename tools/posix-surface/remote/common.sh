# Sourced by every remote stage. Runs inside the Alpine VM.
set -eu
TOOLS=$(cd "$(dirname "$0")/.." && pwd)        # ~/psurf/tools (synced copy of tools/posix-surface)
W=${PSURF_WORK:-$HOME/psurf}
SRC=$W/src          # unpacked upstream trees, never modified after unpacking
BLD=$W/build        # out-of-tree copies used for building
PFX=$W/prefix       # installed measured binaries (prefix/bin is the only PATH entry in workloads)
RAW=$W/raw          # raw traces and logs
RES=$W/results      # derived machine-readable results, copied back to the Mac
DL=$W/dl
mkdir -p "$SRC" "$BLD" "$PFX/bin" "$RAW" "$RES" "$DL"
. "$TOOLS/versions.env"
log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
