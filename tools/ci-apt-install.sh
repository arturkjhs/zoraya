#!/bin/sh
# Install apt packages on a CI machine whose Ubuntu mirror may be slow or
# unreachable (2026-11-11: a runner fetched 1.4 MB in 8 minutes). Used by the
# fallback steps of .github/workflows/test.yml and by .github/ci/Dockerfile.
#
#   tools/ci-apt-install.sh PACKAGE...
#
# Order: the machine's own mirror (3 attempts, apt retries inside each), then
# each mirror of MIRRORS in turn (x86_64 only: other architectures use
# ports.ubuntu.com), written as a replacement of the noble sources. With
# APT_DEB_CACHE=DIR the .deb files are copied from DIR before and to DIR
# after, and a last attempt installs from them without the network.
set -u
[ "$#" -gt 0 ] || { echo "usage: $0 PACKAGE..." >&2; exit 2; }
SUDO=
[ "$(id -u)" = 0 ] || SUDO=sudo
opts="-o Acquire::Retries=5 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30"
# http: the base image has no ca-certificates yet; apt checks the signatures.
MIRRORS=${MIRRORS:-"http://mirrors.edge.kernel.org/ubuntu/ http://mirror.math.princeton.edu/pub/ubuntu/"}
cache=${APT_DEB_CACHE:-}
if [ -n "$cache" ]; then
	mkdir -p "$cache"
	$SUDO cp -n "$cache"/*.deb /var/cache/apt/archives/ 2>/dev/null || true
fi

try() {
	# shellcheck disable=SC2086
	$SUDO apt-get $opts update && $SUDO apt-get $opts install -y --no-install-recommends "$@"
}
succeed() {
	if [ -n "$cache" ]; then
		$SUDO cp -n /var/cache/apt/archives/*.deb "$cache"/ 2>/dev/null || true
		$SUDO chown -R "$(id -u):$(id -g)" "$cache" 2>/dev/null || true
	fi
	exit 0
}

for attempt in 1 2 3; do
	try "$@" && succeed
	echo "apt attempt $attempt on the default mirror failed"
	[ "$attempt" -lt 3 ] && sleep 20
done

if [ "$(uname -m)" = x86_64 ]; then
	$SUDO mkdir -p /etc/apt/ci-orig
	for f in /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list; do
		[ -e "$f" ] && $SUDO mv "$f" /etc/apt/ci-orig/
	done
	codename=$(. /etc/os-release && echo "${VERSION_CODENAME:-noble}")
	for m in $MIRRORS; do
		echo "apt: trying the mirror $m"
		# The security pocket is on the same archive mirrors.
		printf 'Types: deb\nURIs: %s\nSuites: %s %s-updates %s-security\nComponents: main universe\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n' \
			"$m" "$codename" "$codename" "$codename" | $SUDO tee /etc/apt/sources.list.d/ci-mirror.sources >/dev/null
		try "$@" && succeed
	done
	# Put the original sources back for whatever runs next.
	$SUDO rm -f /etc/apt/sources.list.d/ci-mirror.sources
	for f in /etc/apt/ci-orig/*; do
		[ -e "$f" ] || continue
		case $f in
		*/ubuntu.sources) $SUDO mv "$f" /etc/apt/sources.list.d/ ;;
		*) $SUDO mv "$f" /etc/apt/ ;;
		esac
	done
fi

if [ -n "$cache" ]; then
	echo "apt: from the cached packages, without the network"
	# shellcheck disable=SC2086
	$SUDO apt-get $opts install -y --no-install-recommends --no-download "$@" && succeed
fi
echo "apt: no attempt succeeded" >&2
exit 1
