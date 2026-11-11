#!/bin/sh
# Opens a TCP connection to 127.0.0.1:PORT every quarter of a second for at
# most SECONDS. With QEMU's `--net-forward PORT` (a hostfwd to the guest's
# 10.0.2.15) each connection makes SLIRP ask who has 10.0.2.15, which is what
# `make test-net` needs to see the guest answer an ARP request (plan row N1).
# It is bounded by its own alarm; the connection is closed at once and nothing
# is sent.
set -eu
[ $# -eq 2 ] || { echo "usage: net-poke.sh PORT SECONDS" >&2; exit 2; }
exec perl -e '
	use IO::Socket::INET;
	my ($port, $secs) = @ARGV;
	alarm $secs;
	while (1) {
		my $s = IO::Socket::INET->new(PeerAddr => "127.0.0.1:$port", Timeout => 1);
		close $s if $s;
		select(undef, undef, undef, 0.25);
	}
' "$1" "$2"
