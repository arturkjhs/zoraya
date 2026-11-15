#!/bin/sh
# Host test of ports/nvi/build.sh's log (INV-0390-2): run as the Makefile runs
# it (`sh -x`, with dash where there is one, as CI's /bin/sh is), on an empty
# nvi tree and a stand-in zcc, its log has one zcc compile line per object, as
# check-early.sh counts them. The three compiles at a time once traced into one
# file and split each other's lines (113 or 114 lines for 115 objects).
# Usage: ports/tests/nvi-build-log.sh (from the repository's root).
set -eu
root=$(pwd)
tmp=${TMPDIR:-/tmp}/zoraya-test-nvi-log.$$
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src" "$tmp/bin"
sh=$(command -v dash || echo /bin/sh)

# Every source build.sh names, empty; the awk scripts and README it reads.
for f in $(sed -n 's/^[A-Z]*="\(.*\)"$/\1/p' ports/nvi/build.sh | grep '\.c'); do
  mkdir -p "$tmp/src/${f%/*}" && : > "$tmp/src/$f"
done
: > "$tmp/src/common/options.awk"
mkdir -p "$tmp/src/ex" && : > "$tmp/src/ex/ex.awk"
echo 'version 2.2.2)' > "$tmp/src/README"

# The stand-in compiler makes the file -o names.
cat > "$tmp/bin/zcc" <<'EOF'
#!/bin/sh
while [ $# -gt 0 ]; do [ "$1" = -o ] && : > "$2"; shift; done
EOF
chmod +x "$tmp/bin/zcc"
zcc=$tmp/bin/zcc

for round in 1 2 3 4 5 6 7 8 9 10; do
  "$sh" -x ports/nvi/build.sh "$tmp/src" "$tmp/obj" "$tmp/nvi" "$zcc" "$tmp/curses" "$tmp/db/include" \
    "$tmp/db/lib" "-O2 -I$root/a/long/enough/include/path/to/give/the/line/many/words" > "$tmp/nvi.log" 2>&1 ||
    { tail -20 "$tmp/nvi.log"; echo "nvi-build-log: FAILED: build.sh failed"; exit 1; }
  objects=$(find "$tmp/obj" -name '*.o' | wc -l | tr -d ' ')
  # The count of check-early.sh's compiles().
  lines=$(grep -c -e "$zcc.* -c " "$tmp/nvi.log" || true)
  [ "$objects" -gt 100 ] || { echo "nvi-build-log: FAILED: $objects objects"; exit 1; }
  [ "$lines" = "$objects" ] || { echo "nvi-build-log: FAILED: round $round: $lines compile lines for $objects objects"; exit 1; }
  [ ! -e "$tmp/obj/log" ] || { echo "nvi-build-log: FAILED: build.sh left $tmp/obj/log"; exit 1; }
done
echo "nvi-build-log: passed ($lines compile lines for $objects objects, $sh, 10 rounds)"
