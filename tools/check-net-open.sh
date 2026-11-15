#!/bin/sh
# INV-0230-1: the network test steps reach only the runner's own test servers
# (127.0.0.1, which the guest sees at 10.0.2.2), never the Internet. How:
#  - every test step has QEMU's `restrict=on` and reaches the test servers through
#    `--net-guestfwd` rules that run `zoraya-test-httpd --relay 127.0.0.1:PORT`;
#  - the two steps that need UDP to tools/test-dns (`--net-open`) run inside
#    tools/confine-loopback.sh, which lets them open connections to the loopback
#    only; tools/run-qemu.sh refuses `--net-open` outside it.
# What is checked:
#  1. tools/run-qemu.sh puts `restrict=on` on a test's network unless --net-open
#     or --net-internet is given (the printed command line, with a stand-in for
#     QEMU), refuses --net-open outside the wrapper and --net-internet on CI, and
#     does not put restrict=on on a person's `make run-net`;
#  2. every Makefile line with --net-open, --net-internet, or person mode with
#     --net belongs to a definition listed in tools/net-open-allowed.txt, and every
#     listed one still has such a line; a --net-internet definition is a `hand-`
#     target; a --net-open definition is started only through CONFINED;
#  3. every --net-guestfwd in the Makefile runs a relay to 127.0.0.1;
#  4. the host-side test servers (tools/test-httpd, tools/test-dns) listen on
#     127.0.0.1 only;
#  5. the wrapper really confines: inside it a TCP connection and a UDP datagram
#     to an outside address fail at once, and the loopback works. It says so when
#     the host cannot confine (exit 75: the two steps skip there too).
set -eu
cd "$(dirname "$0")/.."
fail() { echo "check-net-open: $*" >&2; exit 1; }
cmd() { QEMU=/bin/echo tools/run-qemu.sh "$@" --accel tcg Makefile 2>&1 | tr ' ' '\n' | grep -A1 '^-netdev$' | tail -1; }
case $(cmd --test --net) in *restrict=on*) ;; *) fail "a test with --net has no restrict=on" ;; esac
case $(ZORAYA_LOOPBACK_ONLY=1 cmd --test --net --net-open) in *restrict=on*) fail "--net-open still has restrict=on" ;; esac
case $(env -u CI cmd --test --net --net-internet) in *restrict=on*) fail "--net-internet still has restrict=on" ;; esac
case $(cmd --net) in *restrict=on*) fail "a person's run has restrict=on" ;; esac
if env -u ZORAYA_LOOPBACK_ONLY QEMU=/bin/echo tools/run-qemu.sh --test --net --net-open --accel tcg Makefile >/dev/null 2>&1; then
	fail "--net-open runs outside tools/confine-loopback.sh"
fi
if CI=1 QEMU=/bin/echo tools/run-qemu.sh --test --net --net-internet --accel tcg Makefile >/dev/null 2>&1; then
	fail "--net-internet runs on CI"
fi
defs() { awk -v pat="$1" '/^[^\t #][^#]*[:=]/ { split($0, a, /[ :=]/); d = a[1] }
	(/^\t/ || /^[A-Za-z_]/) && $0 ~ pat { print d }' Makefile | sort -u; }
# Person mode is `--net` without `--test`: keep only such lines of the last kind.
used=$( { defs '--net-open|--net-internet'; awk '/^[^\t #][^#]*[:=]/ { split($0, a, /[ :=]/); d = a[1] }
	(/^\t/ || /^[A-Za-z_]/) && /\$\(RUN_QEMU\) --net / && !/--test/ { print d }' Makefile; } | sort -u)
allowed=$(grep -v '^#' tools/net-open-allowed.txt | awk 'NF { print $1 }' | sort -u)
[ -n "$used" ] || fail "no --net-open line found in Makefile (is the awk out of date?)"
for d in $used; do
	echo "$allowed" | grep -qx "$d" || fail "$d starts QEMU without restrict=on and is not in tools/net-open-allowed.txt"
done
for d in $allowed; do
	echo "$used" | grep -qx "$d" || fail "$d is in tools/net-open-allowed.txt but has no such line"
done
for d in $(defs '--net-internet'); do
	case $d in hand-*) ;; *) fail "$d uses --net-internet and is not a hand- target" ;; esac
done
for d in $(defs '--net-open'); do
	# test-net-dns-run is started by test-net-dns, through CONFINED, and by nothing else.
	step=${d%-run}
	grep -q "^$step:" Makefile || fail "$d has no step $step"
	grep -q "CONFINED,[a-z-]*,$d)" Makefile || fail "$step does not start $d through CONFINED"
	[ "$(grep -v "^$d:" Makefile | grep -c "[^-A-Za-z]$d[^-A-Za-z]" | tr -d ' ')" = 1 ] || fail "$d is started other than through CONFINED"
done
bad=$(grep -n -e '--net-guestfwd' Makefile | grep -v -e '--relay 127\.0\.0\.1:' -e 'NS_RELAY' || true)
[ -z "$bad" ] || fail "a --net-guestfwd that is not a relay to 127.0.0.1: $bad"
grep -q '^NS_RELAY = \$(CURDIR)/\$(HTTPD) --relay 127\.0\.0\.1:' Makefile || fail "NS_RELAY is not a relay to 127.0.0.1"
for t in test-httpd test-dns; do
	if grep -rnE '0\.0\.0\.0|\[::\]|INADDR_ANY|UNSPECIFIED' tools/$t/src; then fail "tools/$t may listen beyond 127.0.0.1"; fi
	grep -qE 'bind\(\("127\.0\.0\.1"' tools/$t/src/main.rs || fail "tools/$t does not bind 127.0.0.1"
done
# The wrapper: an outside TCP connection and UDP datagram fail at once (EPERM or an unreachable
# network), the loopback works, and the command gets the marker run-qemu.sh wants.
probe='import os, socket, sys
def blocked(kind, addr):
    s = socket.socket(socket.AF_INET, kind); s.settimeout(3)
    try:
        s.connect(addr) if kind == socket.SOCK_STREAM else s.sendto(b"x", addr)
    except OSError as e:
        return e.errno in (1, 13, 101)
    return False
lst = socket.socket(); lst.bind(("127.0.0.1", 0)); lst.listen(1)
c = socket.socket(); c.connect(lst.getsockname())
u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); u.sendto(b"x", ("127.0.0.1", lst.getsockname()[1]))
ok = blocked(socket.SOCK_STREAM, ("192.0.2.1", 80)) and blocked(socket.SOCK_DGRAM, ("192.0.2.1", 53))
sys.exit(0 if ok and os.environ.get("ZORAYA_LOOPBACK_ONLY") == "1" else 3)'
confined="not on this host (the steps skip)"
if command -v python3 >/dev/null 2>&1; then
	rc=0
	tools/confine-loopback.sh python3 -I -c "$probe" >/dev/null 2>&1 || rc=$?
	case $rc in
		0) confined="proved to block an outside TCP and UDP" ;;
		75) ;;
		*) fail "tools/confine-loopback.sh does not confine: an outside connection was not refused (exit $rc)" ;;
	esac
fi
echo "check-net-open: ok ($(echo "$used" | wc -l | tr -d ' ') definitions without restrict=on, all listed; every guestfwd is a relay to 127.0.0.1; loopback wrapper: $confined)"
