#!/bin/sh
# Stage: M4 validation patches. Build patched dash and pdpmake, rerun the workloads that exercise the patched
# sites (W1, W2, W4) with them, in both modes, into raw-m4/. m4_compare.py checks behaviour is unchanged and
# reports what moved in the libc/syscall data.
. "$(dirname "$0")/common.sh"
RAW=$W/raw-m4; rm -rf "$RAW"; mkdir -p "$RAW"
MAINRES=$RES; RES=$RES/m4; rm -rf "$RES"; mkdir -p "$RES"   # statuses of the patched runs stay separate
M4=$PFX/m4; rm -rf "$M4"; mkdir -p "$M4"
export CC=gcc CFLAGS="-O2 -g -U_FORTIFY_SOURCE"
for p in dash pdpmake; do
  d=$BLD/$p-m4; rm -rf "$d"; cp -Rp "$SRC/$p" "$d"; chmod -R u+w "$d"; cd "$d"
  patch -s -p1 < "$TOOLS/patches/m4/$(ls "$TOOLS/patches/m4" | grep "^$p-")"
  if [ $p = dash ]; then ./configure -q >/dev/null; make -s -j"$(nproc)" >/dev/null; install -m755 src/dash "$M4/dash"
  else make -s CC=gcc CFLAGS="$CFLAGS" >/dev/null; install -m755 make "$M4/pdpmake"; fi
  log "built patched $p"
done
SET_G=m4g SET_P=m4p RUNTAG=M4
. "$TOOLS/remote/workloads.sh"
mkset m4g gmake "$M4/dash"; mkset m4p "$M4/pdpmake" "$M4/dash"
for MODE in dynamic strace; do
  mkdir -p "$RAW/$MODE"; : > "$RES/$MODE-status.tsv"
  for w in W1 W2 W4; do $w; done
done
python3 -I "$TOOLS/analysis/m4_compare.py" "$W"
