#!/bin/sh
# Builds ncurses' own test programs from the pinned archive for the host, to
# record their sessions (ADR-0332 item 10). Output: build/ports/host/curses-tests/bin.
# Host tools only; nothing here enters the image.
set -eu
cd "$(dirname "$0")/../.."
SRC=$(tools/term-terminfo/terminfo.sh dir)
OUT=build/ports/host/curses-tests
mkdir -p "$OUT/build" "$OUT/bin"
[ -x "$OUT/bin/ncurses" ] && [ -x "$OUT/bin/view" ] && exit 0
(
    cd "$OUT/build"
    "$OLDPWD/$SRC/configure" --without-shared --without-debug --without-ada --without-cxx --without-cxx-binding \
        --without-manpages --enable-widec --enable-mixed-case --without-pkg-config --with-pthread=no >configure.log 2>&1
    make -j"${CARGO_BUILD_JOBS:-3}" -C include >build.log 2>&1
    make -j"${CARGO_BUILD_JOBS:-3}" -C ncurses >>build.log 2>&1
    for d in panel menu form; do make -j"${CARGO_BUILD_JOBS:-3}" -C $d >>build.log 2>&1; done
    make -C test ncurses lrtest hanoi view >>build.log 2>&1
) || { tail -30 "$OUT/build/build.log" >&2; exit 1; }
for p in ncurses lrtest hanoi view; do
    [ -x "$OUT/build/test/$p" ] && cp "$OUT/build/test/$p" "$OUT/bin/"
done
ls "$OUT/bin"
