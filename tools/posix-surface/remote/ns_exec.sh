#!/bin/sh
# Runs as the mapped root of a fresh user+mount namespace (unshare --map-root-user --mount).
# Makes /bin show the measured set: every name the set provides points at the measured binary, every other
# name falls through to the original /bin (still reachable at $ORIG). Then drops back to the real uid/gid in a
# nested user namespace and execs the workload. Absolute-path executions (/bin/sh from make, #!/bin/sh shebangs,
# /bin/uname from config.guess) therefore run measured binaries; anything under /usr/bin is still Alpine's and
# shows up as a leak in the analysis.
# usage: ns_exec.sh <newbin-dir> <orig-mountpoint> <uid> <gid> <cmd...>
set -eu
NEW=$1 ORIG=$2 U=$3 G=$4; shift 4
mount --bind /bin "$ORIG"
mount --bind "$NEW" /bin
exec unshare --user --map-user="$U" --map-group="$G" "$@"
