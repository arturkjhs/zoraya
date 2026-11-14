#!/bin/sh
# The terminal's width table from the pinned musl (ADR-0332 item 9, INV-0332-4).
#
#   gen.sh write   regenerate crates/zoraya-width/src/table.rs
#   gen.sh check   regenerate into a temporary file and compare with the
#                  committed one (needs the pinned archive in build/ports/src,
#                  which `make fetch-ports` fills); and, offline, that the
#                  committed table names the lock's musl archive
#
# The archive is the one of ports/sources.lock, verified against its SHA-256
# here; only src/ctype and the headers wcwidth.c includes are read.
set -eu
cd "$(dirname "$0")/../.."
LOCK=ports/sources.lock
TABLE=crates/zoraya-width/src/table.rs
CACHE=${PORTS_CACHE:-build/ports/src}

read -r _ version sha file _ <<EOL
$(awk '$1 == "musl" { print }' "$LOCK")
EOL
pin="musl $version sha256:$sha"

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

generate() {
    out=$1
    archive=$CACHE/$file
    [ -f "$archive" ] || { echo "term-width: $archive is missing: make fetch-ports" >&2; return 2; }
    [ "$(sha256 "$archive")" = "$sha" ] || { echo "term-width: $archive is not the pinned archive" >&2; return 2; }
    work=$(mktemp -d)
    tar -xzf "$archive" -C "$work" "musl-$version/src/ctype"
    "${HOSTCC:-cc}" -O1 -w -Dwcwidth=musl_wcwidth -I"$work/musl-$version/src/ctype" \
        -o "$work/gen" tools/term-width/gen.c "$work/musl-$version/src/ctype/wcwidth.c"
    "$work/gen" "$pin" > "$out"
    rm -rf "$work"
}

case ${1:-check} in
write) generate "$TABLE.new" && mv "$TABLE.new" "$TABLE" && echo "term-width: wrote $TABLE" ;;
check)
    head -1 "$TABLE" | grep -q -F "from the wcwidth of $pin." ||
        { echo "term-width: $TABLE was not generated from the pinned $pin" >&2; exit 1; }
    if [ -f "$CACHE/$file" ]; then
        tmp=$(mktemp)
        trap 'rm -f "$tmp"' EXIT
        generate "$tmp"
        cmp -s "$tmp" "$TABLE" || { echo "term-width: $TABLE differs from musl's wcwidth: tools/term-width/gen.sh write" >&2; exit 1; }
        echo "term-width: the table is musl's wcwidth for every code point"
    else
        echo "term-width: pin matches the lock; $CACHE/$file absent, the table itself is not recompared (make fetch-ports)"
    fi ;;
*) echo "usage: gen.sh write|check" >&2; exit 2 ;;
esac
