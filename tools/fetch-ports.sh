#!/bin/sh
# Fills the ports' cache from ports/sources.lock (ADR-0109 item 6). A line of
# the lock may name more than one URL: the program's own, then mirrors.
# FETCH_PORTS_RETRIES sets curl's retries per URL (default 4).
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
[ -r "$lock" ] || { echo "fetch-ports: cannot read $lock" >&2; exit 1; }
mkdir -p "$cache"

if [ -n "$only" ] && ! entries "$lock" | awk -v n="$only" '$1 == n { found = 1 } END { exit !found }'; then
    echo "fetch-ports: $only is not in $lock" >&2
    exit 1
fi

status=0
entries "$lock" | while read -r name version hash file urls; do
    [ -z "$only" ] || [ "$name" = "$only" ] || continue
    [ -n "$urls" ] || { echo "fetch-ports: malformed line for $name in $lock" >&2; exit 1; }
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
    # The first URL is the program's own; the others are mirrors. Each is
    # tried in turn, with retries on any error (a reset connection too) and
    # time limits; whichever gives the locked SHA-256 enters the cache.
    tmp=$path.part
    done=0
    for url in $urls; do
        rm -f "$tmp"
        echo "fetch-ports: $name $version from $url"
        if ! curl -fsSL --retry "${FETCH_PORTS_RETRIES:-4}" --retry-all-errors --retry-delay 5 \
            --connect-timeout 30 --max-time 600 -o "$tmp" "$url"; then
            echo "fetch-ports: download of $url failed" >&2
            continue
        fi
        got=$(sha256 "$tmp")
        if [ "$got" != "$hash" ]; then
            echo "fetch-ports: $url has SHA-256 $got, the lock says $hash: refused" >&2
            continue
        fi
        mv "$tmp" "$path"
        done=1
        break
    done
    rm -f "$tmp"
    if [ "$done" = 0 ]; then
        echo "fetch-ports: no URL of $name gave the locked archive" >&2
        exit 1
    fi
done || status=$?
exit $status
