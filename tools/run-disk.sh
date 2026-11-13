#!/bin/sh
# The disk of `make run`, `make run-net` and `make run-gui` (and of
# `make test-hand-run`, which boots the same image): made from rootfs/ and
# kept between runs, so that a person's files stay, but only while it is a
# volume of this system.
#
# Usage: tools/run-disk.sh IMAGE
#
# IMAGE.key records what IMAGE was made for: the on-disk format version
# (crates/zoraya-fs/src/format.rs; fsd mounts no other) and a checksum of
# rootfs/system, which is read-only at run time for everyone (ADR-0289 item 8)
# and so changes only with a new image. When the key is missing or differs, the
# kept image is moved to IMAGE.old, or to IMAGE.old.N (the next free N from 1)
# if that exists: an earlier old disk, with a person's files, is never
# overwritten. Then a new one is made. Without this, an image from before a format change (format 2, Q5) fails
# to mount ("fsd: mount: Corrupt"): the shell runs without a volume, its prompt
# is "/>", and every network command fails with "the file service is gone",
# since the shell grants it /system/tls/roots (ADR-0228 item 3). One from before
# a change to /system keeps the old system files (an old root store) forever.
#
# EXTRA_ROOT (TASK-27 9a, proposed ADR-0310) is a directory whose files are
# added to the volume over rootfs/'s, with their descriptions (user/describe/)
# in the ramdisk: programs built outside the repository, such as the ports'
# curl and wget in build/rootfs-apps (`make apps`). It defaults to
# build/rootfs-apps when that exists; EXTRA_ROOT= (empty) takes none. Its
# files go in the key, so a new curl makes a new disk. /system is read-only to
# everyone (ADR-0289 item 8), so an EXTRA_ROOT may not hold files under it.
# EXTRA_ROOT may name several directories, separated by colons (TASK-27 9c:
# build/rootfs-apps:build/rootfs-python); later ones are copied over earlier
# ones. DISK_SIZE (default 64M) sets the volume's size, and is in the key:
# curl, wget and the 52 MB Python part do not fit in 64M.
#
# Needs target/debug/zoraya-mkfs (make tools-storage).

set -eu

[ $# = 1 ] || { echo "usage: tools/run-disk.sh IMAGE" >&2; exit 2; }
image=$1
mkfs=target/debug/zoraya-mkfs
size=${DISK_SIZE:-64M}

[ -x "$mkfs" ] || { echo "run-disk.sh: $mkfs not built (make tools-storage)" >&2; exit 2; }
format=$(sed -n 's/^pub const FORMAT_VERSION: u16 = \([0-9][0-9]*\);$/\1/p' crates/zoraya-fs/src/format.rs)
[ -n "$format" ] || { echo "run-disk.sh: no FORMAT_VERSION in crates/zoraya-fs/src/format.rs" >&2; exit 2; }
# File names and contents, in a fixed order: the same tree gives the same sum
# on macOS and Linux (cksum is POSIX).
system=$(cd rootfs && find system -type f | LC_ALL=C sort | while read -r f; do
    printf '%s %s\n' "$f" "$(cksum < "$f")"
done | cksum)
extra=${EXTRA_ROOT-build/rootfs-apps}
if [ -n "$extra" ]; then
    # The default is optional; a root named on purpose must exist.
    optional=
    [ -z "${EXTRA_ROOT+x}" ] && optional=1
    keep=
    oldifs=$IFS; IFS=:
    for d in $extra; do
        [ -n "$d" ] || continue
        if [ ! -d "$d" ]; then
            [ -n "$optional" ] && continue
            echo "run-disk.sh: EXTRA_ROOT $d is not a directory" >&2
            exit 2
        fi
        if [ -e "$d/system" ]; then
            echo "run-disk.sh: EXTRA_ROOT $d holds /system, which only rootfs/ may" >&2
            exit 2
        fi
        keep="${keep:+$keep:}$d"
    done
    IFS=$oldifs
    extra=$keep
fi
extra_sum=
if [ -n "$extra" ]; then
    # Directory names are in the sum's input: the order of the roots matters.
    extra_sum=$(oldifs=$IFS; IFS=:; for d in $extra; do
        IFS=$oldifs
        echo "root $d"
        (cd "$d" && find . -type f | LC_ALL=C sort | while read -r f; do
            printf '%s %s\n' "$f" "$(cksum < "$f")"
        done)
        IFS=:
    done | cksum)
fi
key="format $format; size $size; system $system"
[ -z "$extra_sum" ] || key="$key; extra $extra_sum"

if [ -f "$image" ] && [ -f "$image.key" ] && [ "$(cat "$image.key")" = "$key" ]; then
    exit 0
fi
if [ -f "$image" ]; then
    old="$image.old"
    n=0
    while [ -e "$old" ]; do
        n=$((n + 1))
        old="$image.old.$n"
    done
    mv "$image" "$old"
    echo "run-disk.sh: $image was made for another format or another /system;" \
        "it is now $old, and a new one is made from rootfs/" >&2
fi
rm -f "$image.key"
mkdir -p "$(dirname "$image")"
if [ -n "$extra" ]; then
    stage=$(mktemp -d "${TMPDIR:-/tmp}/zoraya-disk.XXXXXX")
    trap 'rm -rf "$stage"' EXIT
    cp -R rootfs/. "$stage/"
    oldifs=$IFS; IFS=:
    for d in $extra; do cp -R "$d"/. "$stage/"; done
    IFS=$oldifs
    "$mkfs" --size "$size" --from "$stage" "$image"
else
    "$mkfs" --size "$size" --from rootfs "$image"
fi
printf '%s\n' "$key" > "$image.key"
