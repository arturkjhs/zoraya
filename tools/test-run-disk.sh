#!/bin/sh
# INV-0310-2: tools/run-disk.sh and EXTRA_ROOT. A scratch copy of the layout
# run-disk.sh reads (rootfs/, the format constant, a mkfs that records what it
# was given) shows that:
#  1. a disk is made from rootfs/ and an existing EXTRA_ROOT, whose files end
#     up on it;
#  2. nothing is made twice for the same inputs, and a changed extra file
#     makes a new disk and keeps the old one as .old;
#  3. EXTRA_ROOT= takes no extra root, and a named root that is missing, or
#     that holds /system, is refused;
#  4. several roots separated by colons all end up on the disk, a missing
#     second one is refused, the order of the roots is in the key, and so is
#     DISK_SIZE.
# No QEMU and no real mkfs: the scratch mkfs copies the tree it is given.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
dir=$(mktemp -d "${TMPDIR:-/tmp}/zoraya-run-disk.XXXXXX")
trap 'rm -rf "$dir"' EXIT
cd "$dir"
mkdir -p rootfs/system/tls rootfs/bin crates/zoraya-fs/src target/debug build/rootfs-apps/bin
echo roots > rootfs/system/tls/roots
echo sh > rootfs/bin/sh
echo 'pub const FORMAT_VERSION: u16 = 7;' > crates/zoraya-fs/src/format.rs
cat > target/debug/zoraya-mkfs <<'MK'
#!/bin/sh
# zoraya-mkfs --size S --from DIR IMAGE: the image is a listing of DIR.
while [ $# -gt 3 ]; do shift; done
from=$2
shift 2 2>/dev/null || true
( cd "$from" && find . -type f | LC_ALL=C sort ) > "$1"
MK
chmod +x target/debug/zoraya-mkfs
echo curl-v1 > build/rootfs-apps/bin/curl
run() { sh "$here/tools/run-disk.sh" "$@" 2>err; }
# With EXTRA_ROOT set (a subshell: an assignment before a function call may stay).
runx() { ( EXTRA_ROOT=$1; export EXTRA_ROOT; shift; run "$@" ); }
fail() { echo "test-run-disk: FAIL: $*" >&2; exit 1; }

run disk.img
grep -q './bin/curl' disk.img || fail "the default build/rootfs-apps is not on the disk"
grep -q './bin/sh' disk.img || fail "rootfs/ is not on the disk"
cp disk.img first.copy
run disk.img
[ ! -s err ] || fail "a current disk was replaced: $(cat err)"
cmp -s disk.img first.copy || fail "a current disk changed"
[ ! -e disk.img.old ] || fail "a current disk was kept as .old"

echo curl-v2 > build/rootfs-apps/bin/curl
run disk.img
[ -f disk.img.old ] || fail "a new curl did not make a new disk and keep the old"
grep -q 'another format or another /system' err || fail "the replacement was not said"

# No extra root.
runx '' none.img
grep -q './bin/curl' none.img && fail "EXTRA_ROOT= still added the apps"
grep -q './bin/sh' none.img || fail "EXTRA_ROOT= lost rootfs/"
# A named root that is not there is an error; the default may be absent.
if runx nowhere bad.img; then fail "a missing EXTRA_ROOT was accepted"; fi
rm -rf build/rootfs-apps
run absent.img
grep -q './bin/curl' absent.img && fail "an absent default root added files"
# /system may not come from an extra root.
mkdir -p evil/system/tls
echo mine > evil/system/tls/roots
if runx evil evil.img; then fail "an EXTRA_ROOT with /system was accepted"; fi
grep -q 'holds /system' err || fail "the refusal does not say why: $(cat err)"
[ ! -e evil.img ] || fail "a disk was made from an EXTRA_ROOT with /system"
# Several roots (TASK-27 9c).
mkdir -p build/rootfs-apps/bin build/rootfs-python/bin
echo curl-v3 > build/rootfs-apps/bin/curl
echo py-v1 > build/rootfs-python/bin/python3
runx build/rootfs-apps:build/rootfs-python two.img
grep -q './bin/curl' two.img || fail "the first of two roots is not on the disk"
grep -q './bin/python3' two.img || fail "the second of two roots is not on the disk"
grep -q './bin/sh' two.img || fail "rootfs/ is not on a disk of two roots"
# A second root that is not there is refused, and no disk is made.
if runx build/rootfs-apps:build/nowhere miss.img; then fail "a missing second root was accepted"; fi
[ ! -e miss.img ] || fail "a disk was made with a missing second root"
# The same roots again: nothing is made.
runx build/rootfs-apps:build/rootfs-python two.img
[ ! -s err ] || fail "a current two-root disk was replaced: $(cat err)"
[ ! -e two.img.old ] || fail "a current two-root disk was kept as .old"
# Another order is another key (the roots are listed in that order).
runx build/rootfs-python:build/rootfs-apps two.img
[ -f two.img.old ] || fail "a change of the order of the roots did not make a new disk"
grep -q 'another format or another /system' err || fail "the new order was not said"
# A new size is another key.
cp two.img.key size.key
( DISK_SIZE=128M; export DISK_SIZE; EXTRA_ROOT=build/rootfs-python:build/rootfs-apps; export EXTRA_ROOT; run two.img )
grep -q 'size 128M' two.img.key || fail "DISK_SIZE is not in the key: $(cat two.img.key)"
cmp -s two.img.key size.key && fail "a new DISK_SIZE did not change the key"
[ -f two.img.old.1 ] || fail "a new DISK_SIZE did not make a new disk"
echo "test-run-disk: ok"
