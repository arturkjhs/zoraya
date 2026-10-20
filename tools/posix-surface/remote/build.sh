#!/bin/sh
# Stage 2: build every measured program from the pinned sources with gcc, dynamically linked against musl,
# into $PFX/bin. Builds happen in copies under $BLD; $SRC stays untouched. Host tools (Alpine make, gcc) are used
# only to *build*; workloads later run with PATH restricted to the measured binaries.
. "$(dirname "$0")/common.sh"
export CC=gcc CFLAGS="-O2 -g -U_FORTIFY_SOURCE" LDFLAGS=""
J=$(nproc)
fresh() { rm -rf "$BLD/$1"; cp -Rp "$SRC/$1" "$BLD/$1"; chmod -R u+w "$BLD/$1"; cd "$BLD/$1"; }
inst() { install -m755 "$1" "$PFX/bin/$2"; }

log dash;    fresh dash;    ./configure -q >/dev/null; make -s -j$J >/dev/null; inst src/dash dash
log oksh;    fresh oksh;    ./configure --enable-small --cflags="$CFLAGS" >/dev/null; make -s -j$J >/dev/null; inst oksh oksh
log mrsh;    fresh mrsh;    ./configure --without-readline >/dev/null 2>&1 || ./configure >/dev/null; make -s -j$J >/dev/null; inst mrsh mrsh
log toybox;  fresh toybox;  make -s defconfig >/dev/null; make -s -j$J >/dev/null; inst toybox toybox
log busybox; fresh busybox; make -s defconfig >/dev/null
  # defconfig enables a few applets that need kernel headers or features absent on musl; keep the build going
  sed -i 's/^CONFIG_TC=y/# CONFIG_TC is not set/' .config
  yes '' | make -s oldconfig >/dev/null; make -s -j$J >/dev/null; inst busybox busybox
log pdpmake; fresh pdpmake; make -s -j$J CC=gcc CFLAGS="$CFLAGS" >/dev/null; inst make pdpmake
log gmake;   fresh gmake;   CFLAGS="$CFLAGS -std=gnu17" ./configure -q --disable-nls --without-guile >/dev/null; make -s -j$J >/dev/null; inst make gmake
log tcc;     fresh tcc;     patch -s -p1 < "$TOOLS/patches/build/tcc-aarch64-wchar.patch"; ./configure --prefix="$PFX/tcc" --config-musl >/dev/null; make -s -j$J >/dev/null; make -s install >/dev/null
  ln -sf "$PFX/tcc/bin/tcc" "$PFX/bin/tcc"

# record what was built
{ for b in "$PFX"/bin/*; do printf '%s\t%s\n' "$(basename "$b")" "$(file -bL "$b" | cut -d, -f1-2)"; done; } > "$RES/built.txt"
"$PFX/bin/tcc" -v >> "$RES/built.txt" 2>&1 || true
cat "$RES/built.txt"
