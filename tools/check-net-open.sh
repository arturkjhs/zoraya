#!/bin/sh
# INV-0230-1: the network test steps reach only the runner's own test servers
# (127.0.0.1, which the guest sees at 10.0.2.2), never the Internet. How:
#  - every test step has QEMU's `restrict=on` (and `ipv6=off`) and reaches the test
#    servers through `--net-guestfwd` rules that run `zoraya-test-httpd --relay
#    127.0.0.1:PORT`;
#  - the steps that need UDP to tools/test-dns (`--net-open`) or run person mode (test-hand-run's
#    network leg) run inside tools/confine-loopback.sh, which lets them open connections to the
#    loopback only; tools/run-qemu.sh verifies that its own process is confined (a datagram to
#    a documentation address must fail) before it starts QEMU with `--net-open`.
# What is checked:
#  1. the command lines tools/run-qemu.sh really prints (QEMU stands in): `restrict=on` and
#     `ipv6=off` in every test mode, with every network option the Makefile uses (guestfwd,
#     forward, pcap, subnet, gui), never empty; none on `--net-open` (but ipv6=off) and none on a
#     person's run; `--net-open` refused outside the confinement (also with the marker
#     variable alone) and `--net-internet` refused on CI;
#  2. every Makefile line with --net-open, --net-internet, or person mode with --net (a call of
#     RUN_NET_CMD or `$(RUN_QEMU) --net` without --test) belongs to a definition listed in
#     tools/net-open-allowed.txt, and every listed one still has such a line; a --net-internet
#     definition is a `hand-` target; every other such definition but `run-net` is started only
#     through CONFINED;
#  3. CONFINED is `tools/confine-loopback.sh $(MAKE) ...`, and nothing in the Makefile or the
#     workflow sets ZORAYA_LOOPBACK_ONLY;
#  4. every --net-guestfwd is a relay to 127.0.0.1: the macros NET_RELAY and NS_RELAY are exactly
#     what is expected, and every use of the flag is one of them;
#  5. the host-side test servers (tools/test-httpd, tools/test-dns) listen on 127.0.0.1 only;
#  6. the wrapper really confines: inside it a TCP connection and a UDP datagram to an outside
#     address fail at once, the loopback works, and `--verify` says confined (and says not
#     confined outside). It says so when the host cannot confine (exit 75: a step then prints
#     SKIPPED). On CI the confined steps are not run (CONFINED prints SKIPPED), so nothing runs
#     unconfined there.
set -eu
cd "$(dirname "$0")/.."
fail() { echo "check-net-open: $*" >&2; exit 1; }
# The -netdev value of the command line run-qemu.sh prints for the arguments; empty if it dies.
cmd() { QEMU=/bin/echo tools/run-qemu.sh "$@" --accel tcg Makefile 2>/dev/null | tr ' ' '\n' | grep -A1 '^-netdev$' | tail -1; }
need() { # what, arguments...: the netdev must have restrict=on and ipv6=off
	what=$1; shift
	nd=$(cmd "$@")
	[ -n "$nd" ] || fail "$what: run-qemu.sh printed no -netdev"
	case $nd in *,restrict=on*) ;; *) fail "$what has no restrict=on ($nd)" ;; esac
	case $nd in *,ipv6=off*) ;; *) fail "$what has no ipv6=off ($nd)" ;; esac
}
need "a test with --net" --test --net
need "a test with --net and guestfwd" --test --net --net-guestfwd 10.0.2.50:80=/bin/true
need "a test with --net and --net-forward" --test --net --net-forward 47015
need "a test with --net and --net-pcap" --test --net --net-pcap /dev/null
need "a test with --net, a subnet and guestfwd" --test --net --net-subnet 203.0.113.0/24 --net-guestfwd 203.0.113.50:80=/bin/true
need "a GUI test with --net" --test --gui --net --net-guestfwd 10.0.2.50:80=/bin/true
# A guestfwd on the network's own addresses (the host .2, DNS .3, lease .15) is refused by QEMU on CI.
for a in 10.0.2.2 10.0.2.3 10.0.2.15 10.0.2.0 10.0.2.255; do
	if QEMU=/bin/echo tools/run-qemu.sh --test --net --net-guestfwd "$a:80=/bin/true" --accel tcg Makefile >/dev/null 2>&1; then
		fail "run-qemu.sh takes a --net-guestfwd on $a, which QEMU refuses"
	fi
done
host=$(sed -n 's/^NET_HOST := //p' Makefile)
case $host in 10.0.2.*) ;; *) fail "NET_HOST is not an address of 10.0.2.0/24 ($host)" ;; esac
case ${host##*.} in 0 | 2 | 3 | 15 | 255) fail "NET_HOST $host is one of the network's own addresses" ;; esac
if grep -n -e 'NET_RELAYS\?,10\.0\.2\.2\b' -e 'NET_RELAY,10\.0\.2\.2\b' -e 'NS_NET)\.2,' Makefile | grep -v '^[0-9]*:#'; then
	fail "a guestfwd on the host address of its network"
fi
nd=$(cmd --net); [ -n "$nd" ] || fail "a person's run printed no -netdev"
case $nd in *restrict=on*) fail "a person's run has restrict=on" ;; esac
nd=$( (unset CI; cmd --test --net --net-internet) )
[ -n "$nd" ] || fail "--net-internet printed no -netdev"
case $nd in *restrict=on*) fail "--net-internet still has restrict=on" ;; esac
if QEMU=/bin/echo tools/run-qemu.sh --test --net --net-open --accel tcg Makefile >/dev/null 2>&1; then
	fail "--net-open runs outside tools/confine-loopback.sh"
fi
if ZORAYA_LOOPBACK_ONLY=1 QEMU=/bin/echo tools/run-qemu.sh --test --net --net-open --accel tcg Makefile >/dev/null 2>&1; then
	fail "--net-open takes the marker variable for confinement"
fi
if CI=1 QEMU=/bin/echo tools/run-qemu.sh --test --net --net-internet --accel tcg Makefile >/dev/null 2>&1; then
	fail "--net-internet runs on CI"
fi
person='([$][(]RUN_QEMU[)] --net )|call RUN_NET_CMD'
defs() { awk -v pat="$1" '/^[^\t #][^#]*[:=]/ { split($0, a, /[ :=]/); d = a[1] }
	(/^\t/ || /^[A-Za-z_]/) && $0 ~ pat { print d }' Makefile | sort -u; }
persondefs() { awk -v pat="$person" '/^[^\t #][^#]*[:=]/ { split($0, a, /[ :=]/); d = a[1] }
	(/^\t/ || /^[A-Za-z_]/) && $0 ~ pat && !/--test/ { print d }' Makefile | sort -u; }
used=$( { defs '--net-open|--net-internet'; persondefs; } | sort -u)
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
# The definitions that must run inside the confinement.
confined_defs=$( { defs '--net-open'; persondefs; } | sort -u | grep -v -x -e run-net -e RUN_NET_CMD || true)
[ -n "$confined_defs" ] || fail "no definition to run inside the confinement (is the awk out of date?)"
for d in $confined_defs; do
	case $d in *-run) ;; *) fail "$d runs unrestricted and is not a -run target started through CONFINED" ;; esac
	grep -q "CONFINED,[a-z-]*,$d)" Makefile || fail "no step starts $d through CONFINED"
	[ "$(grep -v "^$d:" Makefile | grep -c "[^-A-Za-z]$d[^-A-Za-z]" | tr -d ' ')" = 1 ] || fail "$d is started other than through CONFINED"
done
# CONFINED is the wrapper, and nothing hands out its marker.
grep -A4 '^CONFINED = ' Makefile | grep -q 'tools/confine-loopback\.sh \$(MAKE) ' || fail "CONFINED is not tools/confine-loopback.sh \$(MAKE) ..."
if grep -n 'ZORAYA_LOOPBACK_ONLY' Makefile .github/workflows/*.yml; then fail "something sets ZORAYA_LOOPBACK_ONLY"; fi
# Every guestfwd is a relay to 127.0.0.1.
grep -q '^NET_RELAY = --net-guestfwd "\$(1):\$(2)=\$(CURDIR)/\$(HTTPD) --relay 127\.0\.0\.1:\$(2)"$' Makefile || fail "NET_RELAY is not a relay to 127.0.0.1"
grep -q '^NS_RELAY = \$(CURDIR)/\$(HTTPD) --relay 127\.0\.0\.1:\$\$port$' Makefile || fail "NS_RELAY is not a relay to 127.0.0.1"
rest=$(grep -h -e '--net-guestfwd' Makefile | grep -v '^NET_RELAY = ' |
	sed -e 's/--net-guestfwd "\$(NS_NET)\.[0-9]*:[0-9]*=\$(NS_RELAY)"//g' | grep -e '--net-guestfwd' || true)
[ -z "$rest" ] || fail "a --net-guestfwd that is neither NET_RELAY nor NS_RELAY: $rest"
for t in test-httpd test-dns; do
	if grep -rnE '0\.0\.0\.0|\[::\]|INADDR_ANY|UNSPECIFIED' tools/$t/src; then fail "tools/$t may listen beyond 127.0.0.1"; fi
	grep -qE 'bind\(\("127\.0\.0\.1"' tools/$t/src/main.rs || fail "tools/$t does not bind 127.0.0.1"
done
grep -q 'starts_with("127.0.0.1:")' tools/test-httpd/src/main.rs || fail "zoraya-test-httpd --relay does not refuse a target other than 127.0.0.1"
# The wrapper: an outside TCP connection and UDP datagram fail at once (EPERM or an unreachable
# network), the loopback works, the command gets the marker, and --verify says confined (not outside).
probe='import os, socket, subprocess, sys
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
ok = blocked(socket.SOCK_STREAM, ("198.51.100.7", 80)) and blocked(socket.SOCK_DGRAM, ("198.51.100.7", 53))
ok = ok and subprocess.call(["tools/confine-loopback.sh", "--verify"]) == 0
sys.exit(0 if ok and os.environ.get("ZORAYA_LOOPBACK_ONLY") == "1" else 3)'
confined="not on this host (a step prints SKIPPED)"
if command -v python3 >/dev/null 2>&1; then
	rc=0
	tools/confine-loopback.sh python3 -I -c "$probe" >/dev/null 2>&1 || rc=$?
	case $rc in
		0) confined="proved to block an outside TCP and UDP" ;;
		75) ;;
		*) fail "tools/confine-loopback.sh does not confine: an outside connection was not refused (exit $rc)" ;;
	esac
	if [ "$rc" = 0 ]; then
		if tools/confine-loopback.sh --verify >/dev/null 2>&1; then fail "--verify says confined outside the wrapper"; fi
		# Inside the wrapper, run-qemu.sh accepts --net-open and prints a netdev without restrict=on.
		nd=$(tools/confine-loopback.sh sh -c 'QEMU=/bin/echo tools/run-qemu.sh --test --net --net-open --accel tcg Makefile 2>/dev/null' | tr ' ' '\n' | grep -A1 '^-netdev$' | tail -1)
		[ -n "$nd" ] || fail "--net-open does not start inside the wrapper"
		case $nd in *restrict=on*) fail "--net-open still has restrict=on" ;; esac
		case $nd in *,ipv6=off*) ;; *) fail "--net-open has no ipv6=off ($nd)" ;; esac
	fi
fi
echo "check-net-open: ok ($(echo "$used" | wc -l | tr -d ' ') definitions without restrict=on, all listed; every guestfwd is a relay to 127.0.0.1; loopback wrapper: $confined)"
