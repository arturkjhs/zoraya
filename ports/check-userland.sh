#!/bin/sh
# INV-0336-1, INV-0336-3 and INV-0342-1 (second half): ports/toybox-applets.txt
# is exactly the set of commands the built toybox enables, each from a source
# directory ADR-0336 admits; busybox is not in sources.lock or the image;
# every copyleft archive of sources.lock is in README.md's licence table with
# an ADR; every block line of the stage 10 as-built ADRs names a program whose
# objects import the name.
#
# Usage: check-userland.sh TOYBOX_TREE ROOTFS SURFACE_CHECK
set -eu
tree=$1
rootfs=$2
surface_check=$3
awk_tree=${4:-}
fail=0
say() { echo "check-userland: $*"; fail=1; }

list=$(sed -e 's/#.*//' ports/toybox-applets.txt | tr -s ' \t\n' '\n' | sed '/^$/d' | sort -u)

# --- INV-0336-1
[ -f "$tree/.config" ] || { echo "check-userland: no $tree/.config (make toybox first)"; exit 1; }
# Enabled applets: a CONFIG_X=y whose X is the name of a NEWTOY in toys/.
enabled=$(sed -n 's/^CONFIG_\([A-Z0-9_]*\)=y$/\1/p' "$tree/.config" | tr 'A-Z_' 'a-z-' | sort -u)
built=""
for n in $enabled; do
  if grep -q "NEWTOY($n," "$tree"/toys/*/*.c 2>/dev/null; then built="$built $n"; fi
done
built=$(printf '%s\n' $built | sort -u)
if [ "$built" != "$list" ]; then
  say "ports/toybox-applets.txt and the built configuration differ:"
  { printf '%s\n' "$list" > /tmp/.cu-list.$$; printf '%s\n' "$built" > /tmp/.cu-built.$$; diff /tmp/.cu-list.$$ /tmp/.cu-built.$$ || true; rm -f /tmp/.cu-list.$$ /tmp/.cu-built.$$; }
fi
for n in $list; do
  f=$(grep -l "NEWTOY($n," "$tree"/toys/*/*.c 2>/dev/null | head -1)
  [ -n "$f" ] || { say "$n: no source in the toybox tree"; continue; }
  case $(basename "$(dirname "$f")") in
    posix|lsb|other) ;;
    pending) case $n in tr|expr|diff) ;; *) say "$n comes from toys/pending, which ADR-0336 item 2 admits only for tr, expr and diff";; esac ;;
    *) say "$n comes from $(dirname "$f"), which ADR-0336 does not admit";;
  esac
done
# The image: every listed name is a link in /bin, and no other toybox link.
if [ -d "$rootfs/bin" ]; then
  for n in $list; do
    [ -L "$rootfs/bin/$n" ] || say "$n is not a link in the image's /bin"
  done
  [ -f "$rootfs/usr/libexec/toybox" ] || say "toybox is not in /usr/libexec"
  [ ! -e "$rootfs/bin/toybox" ] || say "toybox is in /bin (ADR-0336 item 8)"
  for l in "$rootfs"/bin/*; do
    [ -L "$l" ] || continue
    case $(readlink "$l") in
      *libexec/toybox) n=$(basename "$l"); printf '%s\n' "$list" | grep -qx "$n" || say "$n links to toybox and is not in the list";;
    esac
  done
fi

# --- INV-0336-3
if grep -q '^busybox[[:space:]]' ports/sources.lock; then say "busybox is in ports/sources.lock"; fi
[ ! -e "$rootfs/bin/busybox" ] || say "busybox is in the image"
# A copyleft archive needs a row in README.md's table naming an ADR.
for name in $(awk '!/^#/ && NF { print $1 }' ports/sources.lock); do
  row=$(grep "^| $name " ports/README.md | head -1 || true)
  [ -n "$row" ] || continue
  if printf '%s' "$row" | grep -Eq 'GPL|LGPL|AGPL'; then
    printf '%s' "$row" | grep -q 'ADR-0' || say "$name is copyleft and its row in README.md names no ADR"
  fi
done

# --- INV-0342-1, second half
if [ -d "$tree/generated/unstripped/obj" ]; then
  "$surface_check" imports docs/adr/0343-surface-for-the-userland.md \
    "toybox=$tree/generated/unstripped/obj" ${awk_tree:+"awk=$awk_tree"} || fail=1
fi
[ $fail = 0 ] && echo "check-userland: passed"
exit $fail
