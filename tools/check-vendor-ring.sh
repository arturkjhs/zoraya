#!/bin/sh
# INV-0289-2, offline: the tracked files of vendor/ring match the SHA-256
# manifest vendor/ring.sha256 (one "hash  path" line per file, sorted by path).
# An edit to ring's C or assembly, which runs in every TLS program, fails here
# until the manifest is regenerated on purpose: tools/vendor-ring.sh
# regenerates both, or `tools/check-vendor-ring.sh --write` rewrites the
# manifest alone after a reviewed change.
set -eu
cd "$(dirname "$0")/.."
manifest=vendor/ring.sha256
list() {
  LC_ALL=C git ls-files vendor/ring | LC_ALL=C sort | while IFS= read -r f; do
    printf '%s  %s\n' "$(shasum -a 256 "$f" | cut -d' ' -f1)" "$f"
  done
}
if [ "${1:-}" = "--write" ]; then
  list >"$manifest"
  exit 0
fi
[ -f "$manifest" ] || { echo "check-vendor-ring: $manifest is missing" >&2; exit 1; }
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
list >"$tmp"
if ! diff -u "$manifest" "$tmp" >"$tmp.diff"; then
  echo "check-vendor-ring: vendor/ring differs from $manifest:" >&2
  head -20 "$tmp.diff" >&2
  rm -f "$tmp.diff"
  exit 1
fi
rm -f "$tmp.diff"
