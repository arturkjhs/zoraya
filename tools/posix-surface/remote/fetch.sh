#!/bin/sh
# Stage 1: download pinned sources, check or record SHA-256, unpack read-only into $SRC.
. "$(dirname "$0")/common.sh"
LOCK=$TOOLS/sources.lock          # committed to the repo; created on first run
NEWLOCK=$RES/sources.lock
: > "$NEWLOCK"

fetch() { # name url
  f=$DL/$(basename "$2")
  [ -s "$f" ] || { log "download $1"; curl -fsSL -o "$f.tmp" "$2"; mv "$f.tmp" "$f"; }
  h=$(sha256sum "$f" | cut -d' ' -f1)
  echo "$1 $h $(basename "$f")" >> "$NEWLOCK"
  if [ -f "$LOCK" ] && grep -q "^$1 " "$LOCK"; then
    grep -q "^$1 $h " "$LOCK" || { log "SHA-256 MISMATCH for $1"; exit 1; }
  fi
}
gitpin() { # name url commit
  d=$DL/$1.git
  [ -d "$d" ] || git clone -q "$2" "$d"
  git -C "$d" fetch -q origin "$3" 2>/dev/null || git -C "$d" fetch -q origin
  git -C "$d" cat-file -e "$3^{commit}"
  echo "$1 git:$3 -" >> "$NEWLOCK"
}

fetch dash    "$DASH_URL"
fetch oksh    "$OKSH_URL"
fetch toybox  "$TOYBOX_URL"
fetch busybox "$BUSYBOX_URL"
fetch gmake   "$GMAKE_URL"
fetch lua     "$LUA_URL"
fetch sqlite  "$SQLITE_URL"
fetch hello   "$HELLO_URL"
gitpin mrsh    "$MRSH_GIT"    "$MRSH_COMMIT"
gitpin tcc     "$TCC_GIT"     "$TCC_COMMIT"
gitpin pdpmake "$PDPMAKE_GIT" "$PDPMAKE_COMMIT"

unpack() { # name archive
  [ -d "$SRC/$1" ] && return
  mkdir -p "$SRC/$1.tmp"
  case "$2" in
    *.zip) (cd "$SRC/$1.tmp" && unzip -q "$2") ;;
    *) tar -xf "$2" -C "$SRC/$1.tmp" ;;
  esac
  # strip the single top-level directory
  set -- "$1" "$SRC/$1.tmp"/*
  mv "$2" "$SRC/$1"; rmdir "$SRC/$1.tmp"
}
unpack dash    "$DL/$(basename "$DASH_URL")"
unpack oksh    "$DL/$(basename "$OKSH_URL")"
unpack toybox  "$DL/$(basename "$TOYBOX_URL")"
unpack busybox "$DL/$(basename "$BUSYBOX_URL")"
unpack gmake   "$DL/$(basename "$GMAKE_URL")"
unpack lua     "$DL/$(basename "$LUA_URL")"
unpack sqlite  "$DL/$(basename "$SQLITE_URL")"
unpack hello   "$DL/$(basename "$HELLO_URL")"
for g in mrsh:$MRSH_COMMIT tcc:$TCC_COMMIT pdpmake:$PDPMAKE_COMMIT; do
  n=${g%%:*}; c=${g#*:}
  git -C "$DL/$n.git" worktree prune
  [ -d "$SRC/$n" ] || git -C "$DL/$n.git" worktree add -q --detach "$SRC/$n" "$c"
done
chmod -R a-w "$SRC"
log "sources ready"; cat "$NEWLOCK"
