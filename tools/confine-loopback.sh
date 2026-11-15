#!/bin/sh
# Runs a command (and everything it starts) with network access to the loopback
# only: INV-0230-1 for the network test steps that need UDP to the runner's own
# DNS server, which QEMU's `restrict=on` forbids (docs/adr/0230-network-testing.md).
# `tools/run-qemu.sh --net-open` refuses to start outside this wrapper.
#
#   tools/confine-loopback.sh COMMAND [ARGUMENT...]
#
# macOS: sandbox-exec denies outbound network but the loopback (TCP and UDP;
#   SLIRP's DNS forwarder sends UDP to the host's resolver itself, which is
#   denied; the system's getaddrinfo, which asks a daemon, is not used by QEMU).
# Linux: a user and network namespace (`unshare -rn`) with only `lo`, up. The
#   command's own servers must run inside it too, so the caller starts the whole
#   step under this wrapper.
# Elsewhere, or when the host does not allow it: exit status 75 and nothing runs;
#   the caller reports a skip, never runs the command unconfined.
#   `--probe` only checks that the confinement works here (exit 0 or 75).
set -eu
case "${1:-}" in
    --probe) shift; set -- sh -c 'exit 0' ;;
esac
[ $# -gt 0 ] || { echo "usage: $0 [--probe] COMMAND..." >&2; exit 2; }
export ZORAYA_LOOPBACK_ONLY=1
case $(uname -s) in
    Darwin)
        command -v sandbox-exec >/dev/null 2>&1 || exit 75
        profile='(version 1) (allow default)
(deny network-outbound (remote ip))
(allow network-outbound (remote ip "localhost:*"))'
        # A sandbox that cannot be applied here (nested, or denied) is a skip, not an unconfined run.
        sandbox-exec -p "$profile" /usr/bin/true >/dev/null 2>&1 || exit 75
        exec sandbox-exec -p "$profile" "$@"
        ;;
    Linux)
        command -v unshare >/dev/null 2>&1 || exit 75
        unshare -rn sh -c 'ip link set lo up 2>/dev/null || ifconfig lo up 2>/dev/null; exit 0' >/dev/null 2>&1 || exit 75
        exec unshare -rn sh -c 'ip link set lo up 2>/dev/null || ifconfig lo up; exec "$@"' sh "$@"
        ;;
    *) exit 75 ;;
esac
