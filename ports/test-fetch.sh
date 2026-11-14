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
# Local files fail or succeed at once: no retries.
export FETCH_PORTS_RETRIES=0
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

# A first URL that fails, then one that serves the wrong bytes, then the good
# one: the good one is cached, and the wrong bytes never are.
cat > "$tmp/mirrorlock" <<LOCK
good 1 $good mirrored.tar.gz file://$tmp/remote/missing.tar.gz file://$tmp/remote/bad.tar.gz file://$tmp/remote/good.tar.gz
LOCK
tools/fetch-ports.sh "$tmp/mirrorlock" "$tmp/cache" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "the third URL was not tried"; }
cmp -s "$tmp/cache/mirrored.tar.gz" "$tmp/remote/good.tar.gz" || fail "the mirror's archive is not the good one"
grep -q "refused" "$tmp/out" || { cat "$tmp/out"; fail "the wrong bytes were not refused"; }
echo "test-fetch: a failing URL and a wrong archive give way to a mirror with the locked archive"

if tools/fetch-ports.sh "$tmp/badlock" "$tmp/cache" > "$tmp/out" 2>&1; then fail "a mismatching archive was accepted"; fi
grep -q "refused" "$tmp/out" || { cat "$tmp/out"; fail "no refusal message"; }
[ ! -e "$tmp/cache/bad.tar.gz" ] && [ ! -e "$tmp/cache/bad.tar.gz.part" ] || fail "the refused archive is in the cache"
echo "test-fetch: an archive with the wrong SHA-256 is refused and not cached (INV-0109-4)"

# A hash `tar:HEX` is of the gunzipped file: the same tar, gzipped twice with
# different headers, is accepted both times; a different tar is refused.
mkdir -p "$tmp/tarsrc" "$tmp/tarcache"
printf 'one\n' > "$tmp/tarsrc/a"
tar cf "$tmp/remote/t.tar" -C "$tmp/tarsrc" a
gzip -n -c "$tmp/remote/t.tar" > "$tmp/remote/t1.tar.gz"
sleep 1
cp "$tmp/remote/t.tar" "$tmp/remote/named.tar" && gzip -c "$tmp/remote/named.tar" > "$tmp/remote/t2.tar.gz"
tarhash=$(gzip -dc "$tmp/remote/t1.tar.gz" | { if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi; } | cut -d' ' -f1)
printf 'two\n' > "$tmp/tarsrc/a"
tar cf "$tmp/remote/u.tar" -C "$tmp/tarsrc" a
gzip -n -c "$tmp/remote/u.tar" > "$tmp/remote/u.tar.gz"
cmp -s "$tmp/remote/t1.tar.gz" "$tmp/remote/t2.tar.gz" && fail "test setup: the gzip bytes should differ"
printf 'tarred 1 tar:%s t.tar.gz file://%s file://%s\n' "$tarhash" "$tmp/remote/t1.tar.gz" "$tmp/remote/t2.tar.gz" > "$tmp/tarlock"
tools/fetch-ports.sh "$tmp/tarlock" "$tmp/tarcache" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "a tar: archive was refused"; }
rm -f "$tmp/tarcache/t.tar.gz"
printf 'tarred 1 tar:%s t.tar.gz file://%s file://%s\n' "$tarhash" "$tmp/remote/u.tar.gz" "$tmp/remote/t2.tar.gz" > "$tmp/tarlock"
tools/fetch-ports.sh "$tmp/tarlock" "$tmp/tarcache" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "the same tar in other gzip bytes was refused"; }
grep -q "refused" "$tmp/out" || { cat "$tmp/out"; fail "a different tar was not refused"; }
echo "test-fetch: a tar: hash ignores the gzip bytes and refuses another tar"

# A git+REPO#COMMIT URL: the commit is fetched with git, archived with the
# prefix NAME-<7 digits>/, and accepted by the tar's hash; another commit's
# tar is refused.
if command -v git >/dev/null 2>&1; then
    mkdir -p "$tmp/repo/proj.git" && git -C "$tmp/repo/proj.git" init -q
    printf 'v1\n' > "$tmp/repo/proj.git/f"
    git -C "$tmp/repo/proj.git" add f
    git -C "$tmp/repo/proj.git" -c user.name=t -c user.email=t@t commit -q -m one
    c1=$(git -C "$tmp/repo/proj.git" rev-parse HEAD)
    printf 'v2\n' > "$tmp/repo/proj.git/f"
    git -C "$tmp/repo/proj.git" -c user.name=t -c user.email=t@t commit -q -am two
    c2=$(git -C "$tmp/repo/proj.git" rev-parse HEAD)
    git -C "$tmp/repo/proj.git" archive --format=tar --prefix="proj-$(echo "$c1" | cut -c1-7)/" "$c1" > "$tmp/remote/g.tar"
    gh=$(if command -v sha256sum >/dev/null 2>&1; then sha256sum < "$tmp/remote/g.tar"; else shasum -a 256 < "$tmp/remote/g.tar"; fi | cut -d' ' -f1)
    printf 'g 1 tar:%s g.tar.gz git+file://%s@%s\n' "$gh" "$tmp/repo/proj.git" "$c1" > "$tmp/glock"
    mkdir -p "$tmp/gcache"
    tools/fetch-ports.sh "$tmp/glock" "$tmp/gcache" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "a git+ source was refused"; }
    printf 'g 1 tar:%s g2.tar.gz git+file://%s@%s\n' "$gh" "$tmp/repo/proj.git" "$c2" > "$tmp/glock"
    if tools/fetch-ports.sh "$tmp/glock" "$tmp/gcache" > "$tmp/out" 2>&1; then fail "another commit's tree was accepted"; fi
    [ ! -e "$tmp/gcache/g2.tar.gz" ] || fail "the refused git archive is in the cache"
    echo "test-fetch: a git+ source gives the locked tree, another commit is refused"
fi

printf 'x' >> "$tmp/cache/good.tar.gz"
if tools/fetch-ports.sh --verify "$tmp/lock" "$tmp/cache" good > "$tmp/out" 2>&1; then fail "--verify accepted a corrupted cached archive"; fi
echo "test-fetch: a cached archive corrupted later fails --verify (INV-0109-4)"
# A restored cache entry is not trusted (CI's Actions cache): the corrupted
# archive is removed and downloaded again; a cache that holds the good archive
# makes no download (the source is gone, and the fetch still passes).
tools/fetch-ports.sh "$tmp/lock" "$tmp/cache" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "a corrupted cached archive was not fetched again"; }
cmp -s "$tmp/cache/good.tar.gz" "$tmp/remote/good.tar.gz" || fail "the corrupted cached archive was kept"
mv "$tmp/remote/good.tar.gz" "$tmp/remote/good.gone"
tools/fetch-ports.sh "$tmp/lock" "$tmp/cache" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "a good cached archive was fetched"; }
grep -q "from file://" "$tmp/out" && fail "a good cached archive caused a download"
mv "$tmp/remote/good.gone" "$tmp/remote/good.tar.gz"
printf 'x' >> "$tmp/cache/good.tar.gz"
echo "test-fetch: a corrupted cached archive is downloaded again, a good one is not (the Actions cache)"
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

# tools/check-fetch-cache.sh: the real workflows pass; a workflow that fetches
# without the restore step, or without the save step, fails.
tools/check-fetch-cache.sh > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "the workflows' fetch has no cache steps"; }
sed 's|actions/cache/restore@|actions/checkout@|' .github/workflows/test.yml > "$tmp/nores.yml"
if tools/check-fetch-cache.sh "$tmp/nores.yml" > "$tmp/out" 2>&1; then fail "a fetch without a restore step passed"; fi
sed 's|actions/cache/save@|actions/checkout@|' .github/workflows/test.yml > "$tmp/nosave.yml"
if tools/check-fetch-cache.sh "$tmp/nosave.yml" > "$tmp/out" 2>&1; then fail "a fetch without a save step passed"; fi
echo "test-fetch: every fetch in the workflows restores and saves the archives' cache"
