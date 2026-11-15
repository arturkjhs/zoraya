#!/bin/sh
# Builds vttest from the pinned archive for the host, to record its sessions
# (ADR-0332 item 10, ADR-0391 item 10). Output: build/ports/host/vttest/bin/vttest.
# Host tools only; nothing here enters the image. The archive comes from the
# ports' cache (`make fetch-ports`); its SHA-256 is checked against the lock.
set -eu
cd "$(dirname "$0")/../.."
LOCK=ports/sources.lock
CACHE=${PORTS_CACHE:-build/ports/src}
read -r _ version sha file _ <<EOL
$(awk '$1 == "vttest" { print }' "$LOCK")
EOL
OUT=build/ports/host/vttest
[ -x "$OUT/bin/vttest" ] && [ -f "$OUT/.pin" ] && [ "$(cat "$OUT/.pin")" = "$version $sha" ] && exit 0
archive=$CACHE/$file
[ -f "$archive" ] || { echo "vttest: $archive is missing: make fetch-ports" >&2; exit 2; }
if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$archive" | cut -d' ' -f1); else got=$(shasum -a 256 "$archive" | cut -d' ' -f1); fi
[ "$got" = "$sha" ] || { echo "vttest: $archive is not the pinned archive" >&2; exit 2; }
rm -rf "$OUT"
mkdir -p "$OUT/src" "$OUT/bin"
tar -xzf "$archive" -C "$OUT/src"
(
    cd "$OUT/src"/vttest-*
    ./configure >../../configure.log 2>&1
    make -j"${CARGO_BUILD_JOBS:-3}" vttest >../../build.log 2>&1
    cp vttest "$OLDPWD/$OUT/bin/vttest"
) || { tail -30 "$OUT/build.log" 2>/dev/null >&2 || tail -30 "$OUT/configure.log" >&2; echo "vttest: the build failed" >&2; exit 1; }
echo "$version $sha" > "$OUT/.pin"
ls "$OUT/bin"
