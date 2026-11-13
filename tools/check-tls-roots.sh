#!/bin/sh
# ADR-0228 item 3: the image ships Mozilla's root store, not a placeholder or
# the test PKI. This checks the file the production image carries
# (rootfs/system/tls/roots) on the source, offline:
#
#  1. it has at least MIN_ROOTS certificate blocks (a store of a few is a test
#     store), and the count its header states is the number of blocks;
#  2. every block is base64 of a DER certificate (a SEQUENCE, 0x30 0x82, and
#     at least 300 bytes), and no block is repeated;
#  3. its header records the source (CCADB) and the date it was fetched;
#  4. none of the test PKI's certificates (tests/net/tls/*.pem) is among them,
#     and docs/dependencies.md states the same count.
#
# The handshake against these roots is not run here: `cargo test -p zoraya-tls`
# (tests/production_roots.rs) loads every block into the verifier.
set -eu
cd "$(dirname "$0")/.."

MIN_ROOTS=${MIN_ROOTS:-100}
roots=rootfs/system/tls/roots
fail=0
bad() { echo "check-tls-roots: $*" >&2; fail=1; }

[ -r "$roots" ] || { echo "check-tls-roots: no $roots" >&2; exit 1; }
dir=$(mktemp -d "${TMPDIR:-/tmp}/zoraya-roots.XXXXXX")
trap 'rm -rf "$dir"' EXIT

# One block per file, the base64 on one line.
awk -v dir="$dir" '
	/^-----BEGIN CERTIFICATE-----/ { n++; body = ""; inside = 1; next }
	/^-----END CERTIFICATE-----/ { if (inside) { print body > (dir "/" sprintf("%04d", n)); close(dir "/" sprintf("%04d", n)) } inside = 0; next }
	inside { gsub(/[ \r]/, ""); body = body $0 }
' "$roots"

if base64 -d </dev/null >/dev/null 2>&1; then decode="base64 -d"; else decode="base64 -D"; fi
count=0
for f in "$dir"/*; do
	[ -e "$f" ] || break
	count=$((count + 1))
	size=$($decode < "$f" 2>/dev/null | wc -c | tr -d ' ')
	head=$($decode < "$f" 2>/dev/null | head -c 2 | od -An -tx1 | tr -d ' \n')
	if [ "$head" != 3082 ] || [ "$size" -lt 300 ]; then
		bad "block $(basename "$f") is not a DER certificate (starts $head, $size bytes)"
	fi
done
[ "$count" -ge "$MIN_ROOTS" ] || bad "only $count certificates (at least $MIN_ROOTS expected): not Mozilla's store"
dups=$(cat "$dir"/* | sort | uniq -d | wc -l | tr -d ' ')
[ "$dups" = 0 ] || bad "$dups certificate(s) appear twice"

stated=$(sed -n 's/.*[^0-9]\([0-9][0-9]*\) certificates\..*/\1/p' "$roots" | head -n 1)
[ "$stated" = "$count" ] || bad "the header states ${stated:-no count} certificates, the file holds $count"
grep -q 'Common CA Database' "$roots" || bad "the header does not name its source (CCADB)"
grep -Eq 'fetched [0-9]{4}-[0-9]{2}-[0-9]{2}' "$roots" || bad "the header does not give the date it was fetched"
grep -q 'CDLA-Permissive-2.0' "$roots" || bad "the header does not give the data's license"

for pem in tests/net/tls/*.pem; do
	one=$(awk '/BEGIN CERTIFICATE/{i=1;next}/END CERTIFICATE/{i=0}i' "$pem" | tr -d ' \r\n')
	[ -n "$one" ] || continue
	if cat "$dir"/* | grep -qxF "$one"; then bad "$pem (test PKI) is in the production roots"; fi
done
grep -q "$count certificates with the Websites trust bit" docs/dependencies.md \
	|| bad "docs/dependencies.md does not state $count certificates for $roots"

[ "$fail" = 0 ] || exit 1
echo "check-tls-roots: ok ($count roots)"
