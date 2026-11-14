#!/bin/sh
# The terminal's four terminfo entries (ADR-0333 item 2, INV-0333-1).
#
#   terminfo.sh tools          build `tic` and `infocmp` for the host from the
#                              pinned ncurses archive (ports/sources.lock),
#                              into build/ports/host/ncurses/bin
#   terminfo.sh compile DIR    compile xterm-256color, xterm, vt100 and dumb
#                              from that release's terminfo.src into DIR, in
#                              ncurses' directory-tree form with letter-named
#                              directories, whatever the host's file system
#   terminfo.sh check          recompile into a temporary directory and
#                              compare paths and SHA-256 with the committed
#                              manifest, ports/terminfo.manifest
#   terminfo.sh write          rewrite the manifest from a compilation
#   terminfo.sh dir            the directory of the unpacked source (for the
#                              conformance tool)
#
# No installed ncurses is trusted: the host tools are the pinned release's.
# The archive comes from the ports' cache (`make fetch-ports`).
set -eu
cd "$(dirname "$0")/../.."
LOCK=ports/sources.lock
CACHE=${PORTS_CACHE:-build/ports/src}
HOST=build/ports/host/ncurses
MANIFEST=ports/terminfo.manifest
NAMES=xterm-256color,xterm,vt100,dumb

read -r _ version sha file _ <<EOL
$(awk '$1 == "ncurses" { print }' "$LOCK")
EOL
SRC=$HOST/src/ncurses-$version

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

unpack() {
    [ -f "$SRC/.unpacked" ] && return 0
    archive=$CACHE/$file
    [ -f "$archive" ] || { echo "terminfo: $archive is missing: make fetch-ports" >&2; exit 2; }
    [ "$(sha256 "$archive")" = "$sha" ] || { echo "terminfo: $archive is not the pinned archive" >&2; exit 2; }
    rm -rf "$HOST/src"
    mkdir -p "$HOST/src"
    tar -xzf "$archive" -C "$HOST/src"
    touch "$SRC/.unpacked"
}

tools() {
    [ -x "$HOST/bin/tic" ] && [ -x "$HOST/bin/infocmp" ] && [ -f "$HOST/bin/.pin" ] &&
        [ "$(cat "$HOST/bin/.pin")" = "$version $sha" ] && return 0
    unpack
    mkdir -p "$HOST/build" "$HOST/bin"
    log=$HOST/build.log
    (
        cd "$HOST/build"
        # --enable-mixed-case: ncurses chooses its directory layout by testing
        # the build host's file system for case sensitivity; this fixes it.
        "$OLDPWD/$SRC/configure" --prefix="$OLDPWD/$HOST/install" \
            --without-shared --without-debug --without-ada --without-cxx --without-cxx-binding \
            --without-manpages --without-tests --without-progs-install-dummy \
            --enable-widec --enable-mixed-case --disable-home-terminfo \
            --with-default-terminfo-dir=/usr/share/terminfo --without-pkg-config >configure.log 2>&1 || true
    ) >"$log" 2>&1
    if [ ! -f "$HOST/build/Makefile" ]; then
        # An option this release does not know: say what failed.
        tail -20 "$HOST/build/configure.log" >&2
        echo "terminfo: configure failed (see $HOST/build/configure.log)" >&2
        exit 1
    fi
    (cd "$HOST/build" && make -j"${CARGO_BUILD_JOBS:-3}" -C include && make -j"${CARGO_BUILD_JOBS:-3}" -C ncurses && make -C progs tic infocmp) >>"$log" 2>&1 ||
        { tail -30 "$log" >&2; echo "terminfo: build failed (see $log)" >&2; exit 1; }
    cp "$HOST/build/progs/tic" "$HOST/build/progs/infocmp" "$HOST/bin/"
    echo "$version $sha" > "$HOST/bin/.pin"
}

compile() {
    out=$1
    tools
    rm -rf "$out"
    mkdir -p "$out"
    # -x keeps the extensions (Ms, XM, Smulx, ...): the entry the terminal is
    # tested against has them. -e writes the four entries and not the ones
    # they `use=`.
    "$HOST/bin/tic" -x -e "$NAMES" -o "$out" "$SRC/misc/terminfo.src" >"$out.log" 2>&1 ||
        { cat "$out.log" >&2; echo "terminfo: tic failed" >&2; exit 1; }
    rm -f "$out.log"
    # tic also writes the aliases of an entry's name field (vt100-am): the
    # image holds exactly the four files of ADR-0333 item 2.
    find "$out" -type f | while read -r f; do
        case ${f#"$out"/} in
        x/xterm-256color | x/xterm | v/vt100 | d/dumb) ;;
        *) rm -f "$f" ;;
        esac
    done
    find "$out" -mindepth 1 -type d -empty -delete
}

# "sha256  path" for every file under DIR, sorted by path.
listing() {
    (cd "$1" && find . -type f | sed 's|^\./||' | LC_ALL=C sort | while read -r f; do
        printf '%s  %s\n' "$(sha256 "$f")" "$f"
    done)
}

case ${1:-check} in
tools) tools ;;
dir) unpack; echo "$SRC" ;;
compile) compile "${2:?usage: terminfo.sh compile DIR}" ;;
write)
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    compile "$tmp/terminfo"
    {
        echo "# The files of /usr/share/terminfo (ADR-0333 item 2, INV-0333-1): SHA-256 and path, for the output of"
        echo "# the host tic of ncurses $version on its terminfo.src. Written by tools/term-terminfo/terminfo.sh write;"
        echo "# checked by terminfo.sh check (make check-terminfo, part of make check-ports)."
        listing "$tmp/terminfo"
    } > "$MANIFEST"
    echo "terminfo: wrote $MANIFEST" ;;
check)
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    compile "$tmp/terminfo"
    listing "$tmp/terminfo" > "$tmp/got"
    grep -v '^#' "$MANIFEST" > "$tmp/want"
    if ! cmp -s "$tmp/got" "$tmp/want"; then
        diff "$tmp/want" "$tmp/got" >&2 || true
        echo "terminfo: the compiled entries differ from $MANIFEST" >&2
        exit 1
    fi
    # Exactly the four files, in letter-named directories.
    [ "$(wc -l < "$tmp/got" | tr -d ' ')" = 4 ] || { echo "terminfo: not exactly four files" >&2; exit 1; }
    for p in x/xterm-256color x/xterm v/vt100 d/dumb; do
        [ -f "$tmp/terminfo/$p" ] || { echo "terminfo: $p is missing" >&2; exit 1; }
    done
    echo "terminfo: the four entries are the manifest's" ;;
*) echo "usage: terminfo.sh tools|compile DIR|check|write|dir" >&2; exit 2 ;;
esac
