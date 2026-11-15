#!/bin/sh
# The far end of the ssh tests (TASK-31 10g, ADR-0340 item 10, ADR-0230 item 3):
# two OpenSSH servers of the host on the loopback, with throwaway keys made
# here, started by the test that needs them and ended by it. The guest reaches
# them through `--net-guestfwd` relays (the Makefile), never directly, and
# nothing here connects to another machine.
#
#   tools/test-sshd.sh start DIR LIFETIME   makes DIR, its keys and two servers;
#                                           prints "PORTA PORTB USER FPA FPB" (one line)
#   tools/test-sshd.sh stop DIR             ends the servers
#
# Server A shows the key `host_a`; B shows `host_b`. DIR/wrong.pub is a key
# neither shows, for the test that stores a key for A's address that is not A's
# (a changed host key, INV-0340-3). DIR/client is the key the guest's tests use:
# both servers take it for the user running this script, and nothing else (no
# password, no other key). Each server logs at VERBOSE level to DIR/sshd-a.log
# and DIR/sshd-b.log, which the test reads for what was attempted. LIFETIME
# (seconds) ends a server nobody stopped. Exit status 75 when the host has no
# sshd, so that a caller can skip locally (and fail on CI).
set -eu

sshd_bin() {
	for p in /usr/sbin/sshd /usr/local/sbin/sshd /opt/homebrew/sbin/sshd; do
		[ -x "$p" ] && { echo "$p"; return 0; }
	done
	return 1
}

free_port() {
	python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'
}

wait_for() {
	python3 -I -c '
import socket, sys, time
end = time.time() + 15
while time.time() < end:
    try:
        socket.create_connection(("127.0.0.1", int(sys.argv[1])), 1).close()
        sys.exit(0)
    except OSError:
        time.sleep(0.2)
sys.exit(1)' "$1"
}

fingerprint() {
	ssh-keygen -l -E sha256 -f "$1" | awk '{ sub(/^SHA256:/, "", $2); print $2 }'
}

case "${1:-}" in
start)
	[ "$#" = 3 ] || { echo "usage: $0 start DIR LIFETIME" >&2; exit 2; }
	dir=$2
	life=$3
	sshd=$(sshd_bin) || { echo "test-sshd: no sshd on this host" >&2; exit 75; }
	command -v ssh-keygen >/dev/null 2>&1 || { echo "test-sshd: no ssh-keygen on this host" >&2; exit 75; }
	rm -rf "$dir"
	mkdir -p "$dir"
	dir=$(cd "$dir" && pwd)
	chmod 700 "$dir"
	for k in host_a host_b wrong client; do
		ssh-keygen -q -t ed25519 -N '' -C "throwaway-$k" -f "$dir/$k" >/dev/null
	done
	cp "$dir/client.pub" "$dir/authorized_keys"
	chmod 600 "$dir/authorized_keys" "$dir/client"
	user=$(id -un)
	pa=$(free_port)
	pb=$(free_port)
	[ "$pa" != "$pb" ] || pb=$(free_port)
	for s in a b; do
		port=$pa
		[ "$s" = b ] && port=$pb
		cat > "$dir/sshd_$s.conf" <<CONF
Port $port
ListenAddress 127.0.0.1
HostKey $dir/host_$s
PidFile $dir/sshd_$s.pid
AuthorizedKeysFile $dir/authorized_keys
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
AllowUsers $user
LogLevel VERBOSE
PrintMotd no
PermitUserEnvironment no
CONF
		perl -e 'alarm shift; exec @ARGV' "$life" "$sshd" -D -e -f "$dir/sshd_$s.conf" > "$dir/sshd-$s.log" 2>&1 &
		echo $! > "$dir/sshd_$s.wrapper"
	done
	wait_for "$pa" && wait_for "$pb" || {
		echo "test-sshd: a server did not start:" >&2
		cat "$dir"/sshd-?.log >&2
		"$0" stop "$dir"
		exit 1
	}
	echo "$pa $pb $user $(fingerprint "$dir/host_a.pub") $(fingerprint "$dir/host_b.pub")"
	;;
stop)
	[ "$#" = 2 ] || { echo "usage: $0 stop DIR" >&2; exit 2; }
	for s in a b; do
		[ -f "$2/sshd_$s.pid" ] && kill "$(cat "$2/sshd_$s.pid")" 2>/dev/null || true
		[ -f "$2/sshd_$s.wrapper" ] && kill "$(cat "$2/sshd_$s.wrapper")" 2>/dev/null || true
	done
	;;
*)
	echo "usage: $0 start DIR LIFETIME | stop DIR" >&2
	exit 2
	;;
esac
