#!/bin/sh
# Fills the ports' cache from ports/sources.lock (ADR-0109 item 6).
#
#   fetch-ports.sh LOCK CACHE              download what is missing, verify all
#   fetch-ports.sh --verify LOCK CACHE NAME   verify one cached archive, offline
#
# A download goes to a temporary name and enters the cache only when its
# SHA-256 matches the lock; an archive already in the cache with the right
# hash is left alone. A mismatch is an error and leaves nothing in the cache
# (INV-0109-4). Only the first form uses the network.
set -eu

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

entries() {
    sed -e 's/#.*//' -e '/^[[:space:]]*$/d' "$1"
}

verify_only=0
if [ "${1:-}" = --verify ]; then
    verify_only=1
    shift
fi
[ $# -ge 2 ] || { echo "usage: $0 [--verify] LOCK CACHE [NAME]" >&2; exit 2; }
lock=$1
cache=$2
only=${3:-}
mkdir -p "$cache"

if [ -n "$only" ] && ! entries "$lock" | awk -v n="$only" '$1 == n { found = 1 } END { exit !found }'; then
    echo "fetch-ports: $only is not in $lock" >&2
    exit 1
fi

status=0
entries "$lock" | while read -r name version hash file url; do
    [ -z "$only" ] || [ "$name" = "$only" ] || continue
    [ -n "$url" ] || { echo "fetch-ports: malformed line for $name in $lock" >&2; exit 1; }
    path=$cache/$file
    if [ -f "$path" ]; then
        got=$(sha256 "$path")
        if [ "$got" = "$hash" ]; then
            [ "$verify_only" = 1 ] || echo "fetch-ports: $file ok"
            continue
        fi
        echo "fetch-ports: $path has SHA-256 $got, the lock says $hash" >&2
        if [ "$verify_only" = 1 ]; then exit 1; fi
        echo "fetch-ports: removing it and downloading again" >&2
        rm -f "$path"
    elif [ "$verify_only" = 1 ]; then
        echo "fetch-ports: $path is missing: run 'make fetch-ports'" >&2
        exit 1
    fi
    tmp=$path.part
    rm -f "$tmp"
    echo "fetch-ports: $name $version from $url"
    if ! curl -fsSL --retry 3 -o "$tmp" "$url"; then
        rm -f "$tmp"
        echo "fetch-ports: download of $url failed" >&2
        exit 1
    fi
    got=$(sha256 "$tmp")
    if [ "$got" != "$hash" ]; then
        rm -f "$tmp"
        echo "fetch-ports: $url has SHA-256 $got, the lock says $hash: refused" >&2
        exit 1
    fi
    mv "$tmp" "$path"
done || status=$?
exit $status
