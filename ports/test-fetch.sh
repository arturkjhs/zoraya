#!/bin/sh
# Host test of the fetch step, offline (INV-0109-3, INV-0109-4): a local
# file:// source stands in for the network.
#   1. a matching archive enters the cache;
#   2. an archive whose SHA-256 differs from the lock is refused and leaves
#      nothing in the cache;
#   3. a cached archive that was corrupted later fails --verify, which
#      ports/Makefile runs before every unpack;
#   4. with the cache empty, ports/Makefile stops with a message naming
#      `make fetch-ports` and downloads nothing.
# Usage: ports/test-fetch.sh (from the repository's root).
set -eu
root=$(pwd)
tmp=${TMPDIR:-/tmp}/zoraya-test-fetch.$$
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/remote" "$tmp/cache"

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
fail() { echo "test-fetch: FAILED: $*"; exit 1; }

printf 'pretend this is a tarball\n' > "$tmp/remote/good.tar.gz"
good=$(sha256 "$tmp/remote/good.tar.gz")
printf 'pretend this is a tarball, but changed\n' > "$tmp/remote/bad.tar.gz"
cat > "$tmp/lock" <<LOCK
# name version sha256 file url
good 1 $good good.tar.gz file://$tmp/remote/good.tar.gz
LOCK
cat > "$tmp/badlock" <<LOCK
bad 1 $good bad.tar.gz file://$tmp/remote/bad.tar.gz
LOCK

tools/fetch-ports.sh "$tmp/lock" "$tmp/cache" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "a matching archive was refused"; }
[ -f "$tmp/cache/good.tar.gz" ] || fail "the matching archive is not in the cache"
echo "test-fetch: a matching archive enters the cache"

if tools/fetch-ports.sh "$tmp/badlock" "$tmp/cache" > "$tmp/out" 2>&1; then fail "a mismatching archive was accepted"; fi
grep -q "refused" "$tmp/out" || { cat "$tmp/out"; fail "no refusal message"; }
[ ! -e "$tmp/cache/bad.tar.gz" ] && [ ! -e "$tmp/cache/bad.tar.gz.part" ] || fail "the refused archive is in the cache"
echo "test-fetch: an archive with the wrong SHA-256 is refused and not cached (INV-0109-4)"

printf 'x' >> "$tmp/cache/good.tar.gz"
if tools/fetch-ports.sh --verify "$tmp/lock" "$tmp/cache" good > "$tmp/out" 2>&1; then fail "--verify accepted a corrupted cached archive"; fi
echo "test-fetch: a cached archive corrupted later fails --verify (INV-0109-4)"
if tools/fetch-ports.sh --verify "$tmp/lock" "$tmp/cache" nosuch > "$tmp/out" 2>&1; then fail "--verify passed a name not in the lock"; fi

# The same through ports/Makefile: a corrupted musl archive in the cache stops
# the build before anything is unpacked.
mkdir -p "$tmp/badcache"
musl=$(awk '$1 == "musl" { print $4 }' ports/sources.lock)
printf 'not musl\n' > "$tmp/badcache/$musl"
if make -f ports/Makefile CACHE="$tmp/badcache" WORK="$tmp/work2" OBJ="$tmp/obj2" LOGS="$tmp/logs2" \
    SYSROOT="$tmp/sysroot2" headers > "$tmp/out" 2>&1; then
    fail "ports/Makefile unpacked a corrupted archive"
fi
grep -q "the lock says" "$tmp/out" || { cat "$tmp/out"; fail "no hash message from ports/Makefile"; }
[ ! -e "$tmp/work2/musl-1.2.5" ] || fail "the corrupted archive was unpacked"
echo "test-fetch: ports/Makefile verifies a cached archive before unpacking it (INV-0109-4)"

mkdir -p "$tmp/empty"
if make -f ports/Makefile CACHE="$tmp/empty" WORK="$tmp/work" OBJ="$tmp/obj" LOGS="$tmp/logs" \
    SYSROOT="$tmp/sysroot" headers > "$tmp/out" 2>&1; then
    fail "ports/Makefile built without its sources"
fi
grep -q "run 'make fetch-ports' first" "$tmp/out" || { cat "$tmp/out"; fail "the message does not name make fetch-ports"; }
[ -z "$(ls -A "$tmp/empty")" ] || fail "ports/Makefile put something into the cache"
echo "test-fetch: an empty cache stops the build with a message naming make fetch-ports (INV-0109-3)"
cd "$root"
