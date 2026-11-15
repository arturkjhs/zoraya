#!/bin/sh
# Runs a command (and everything it starts) with network access to the loopback
# only: INV-0230-1 for the network test steps that need UDP to the runner's own
# DNS server, which QEMU's `restrict=on` forbids (docs/adr/0230-network-testing.md).
# `tools/run-qemu.sh --net-open` refuses to start unless `--verify` succeeds in its
# own process, so an environment variable alone does not open the door.
#
#   tools/confine-loopback.sh COMMAND [ARGUMENT...]
#   tools/confine-loopback.sh --verify
#   tools/confine-loopback.sh --probe
#
# macOS: sandbox-exec denies outbound network but the loopback (TCP and UDP;
#   SLIRP's DNS forwarder sends UDP to the host's resolver itself, which is
#   denied; the system's getaddrinfo, which asks a daemon, is not used by QEMU).
#   Known limit: the whole loopback of the host stays reachable, not only the
#   runner's servers (docs/design.md).
# Linux: a network namespace with only `lo`, up: `unshare -n` when this process is
#   root (CI's container, which has CAP_SYS_ADMIN and CAP_NET_ADMIN for the
#   network steps), else `unshare -rn`. The command's own servers must run inside
#   it too, so the caller starts the whole step under this wrapper.
# Elsewhere, or when the host does not allow it: exit status 75 and nothing runs;
#   the caller reports a skip locally and a failure on CI, never runs unconfined.
#   `--probe` only checks that the confinement works here (exit 0 or 75).
#   `--verify` checks that THIS process is confined: one datagram to 198.51.100.7
#   (TEST-NET-2, documentation addresses, never routed) must fail with the error
#   the confinement gives (EPERM on macOS; ENETUNREACH in a namespace with only
#   `lo`), exit 0; any other outcome, exit 1.
set -eu
verify() {
    command -v python3 >/dev/null 2>&1 || return 1
    python3 -I -c '
import os, platform, socket, sys
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.settimeout(2)
try:
    s.sendto(b"x", ("198.51.100.7", 9))
except OSError as e:
    if platform.system() == "Darwin":
        sys.exit(0 if e.errno == 1 else 1)
    sys.exit(0 if e.errno == 101 and os.listdir("/sys/class/net") == ["lo"] else 1)
sys.exit(1)'
}
case "${1:-}" in
    --verify) verify; exit $? ;;
    --probe) shift; set -- sh -c 'exit 0' ;;
esac
[ $# -gt 0 ] || { echo "usage: $0 [--probe|--verify] COMMAND..." >&2; exit 2; }
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
        if [ "$(id -u)" = 0 ]; then flags=-n; else flags=-rn; fi
        # lo up without `ip`: SIOCSIFFLAGS with IFF_UP|IFF_RUNNING.
        export ZORAYA_LOUP='import socket, fcntl, struct; fcntl.ioctl(socket.socket(), 0x8914, struct.pack("16sH14x", b"lo", 0x41))'
        up='ip link set lo up >/dev/null 2>&1 || python3 -I -c "$ZORAYA_LOUP"'
        # shellcheck disable=SC2086
        unshare $flags sh -c "$up" >/dev/null 2>&1 || exit 75
        # shellcheck disable=SC2086
        exec unshare $flags sh -c "$up"'; exec "$@"' sh "$@"
        ;;
    *) exit 75 ;;
esac
