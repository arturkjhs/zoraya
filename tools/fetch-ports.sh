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
#
# Two forms in the lock beyond a plain SHA-256 and URL: a hash written
# `tar:HEX` is the SHA-256 of the gunzipped file, and a URL written
# `git+REPO@COMMIT` is fetched with git (the commit id is checked) and archived.
set -eu

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# `same FILE HASH`: HASH is the file's SHA-256, or, written `tar:HEX`, the
# SHA-256 of the file gunzipped. A snapshot that a server generates on demand
# (a git hosting site's) has gzip bytes that change from one machine to the
# next; the tar inside it does not (git's archive is deterministic).
same() {
    case $2 in
    tar:*) gzip -t "$1" 2>/dev/null && [ "$(gzip -dc "$1" 2>/dev/null | sha256 -)" = "${2#tar:}" ] ;;
    *) [ "$(sha256 "$1")" = "$2" ] ;;
    esac
}

# `git_archive URL@COMMIT OUT`: fetches the commit with git (the id is the
# check of what came), and writes the tar of its tree, gzipped, with the
# prefix NAME-<7 digits>/ that a hosting site's snapshot has.
git_archive() {
    repo=${1%@*}
    commit=${1##*@}
    work=$(mktemp -d)
    ok=1
    if git -C "$work" init -q && git -C "$work" fetch -q --depth 1 "$repo" "$commit" &&
        [ "$(git -C "$work" rev-parse FETCH_HEAD)" = "$commit" ]; then
        base=$(basename "$repo" .git)
        git -C "$work" archive --format=tar --prefix="$base-$(echo "$commit" | cut -c1-7)/" FETCH_HEAD |
            gzip -n > "$2" && ok=0
    fi
    rm -rf "$work"
    return $ok
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
        if same "$path" "$hash"; then
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
        case $url in
        git+*)
            if ! git_archive "${url#git+}" "$tmp"; then
                echo "fetch-ports: git fetch of $url failed" >&2
                continue
            fi
            ;;
        *)
            if ! curl -fsSL --retry "${FETCH_PORTS_RETRIES:-4}" --retry-all-errors --retry-delay 5 \
                --connect-timeout 30 --max-time 600 -o "$tmp" "$url"; then
                echo "fetch-ports: download of $url failed" >&2
                continue
            fi
            ;;
        esac
        got=$(sha256 "$tmp")
        if ! same "$tmp" "$hash"; then
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
