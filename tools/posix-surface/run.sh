#!/bin/sh
# TASK-02 single entry point. Run on the Mac from anywhere:
#   tools/posix-surface/run.sh            # all stages
#   tools/posix-surface/run.sh build static   # selected stages
# Stages run inside the Alpine aarch64 measurement VM over SSH; results come back to
# tools/posix-surface/results/ (machine-readable) and docs/research/task-02/*.csv (published tables).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
VM=${PSURF_VM:-alpine-posix}
SSHCFG=${PSURF_SSHCFG:-$HOME/.lima/$VM/ssh.config}
SSH="ssh -F $SSHCFG -o BatchMode=yes lima-$VM"
STAGES=${*:-"env fetch build static interposer dynamic minbuild strace callsites analyze"}

$SSH true || { echo "cannot reach VM lima-$VM via $SSHCFG" >&2; exit 1; }
# sync this directory (minus results) to ~/psurf/tools in the VM
tar -C "$HERE" --exclude=./results -cf - . | $SSH 'rm -rf ~/psurf/tools && mkdir -p ~/psurf/tools && tar -C ~/psurf/tools -xf -'

for s in $STAGES; do
  echo "=== stage $s"
  $SSH "PSURF_WORKLOADS='${PSURF_WORKLOADS:-}' sh ~/psurf/tools/remote/$s.sh"
done

mkdir -p "$HERE/results"
$SSH 'tar -C ~/psurf/results -cf - .' | tar -C "$HERE/results" -xf -
# sources.lock is an input once recorded: keep the first one
[ -f "$HERE/sources.lock" ] || cp "$HERE/results/sources.lock" "$HERE/sources.lock" 2>/dev/null || true
for f in libc-surface.csv syscall-surface.csv; do
  [ -f "$HERE/results/$f" ] && cp "$HERE/results/$f" "$REPO/docs/research/task-02/$f"
done
echo "results in $HERE/results"
