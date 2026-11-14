#!/bin/sh
# The terminal's conformance to the entry it claims (ADR-0332 item 10,
# INV-0332-1).
#
#   check.sh          offline: every capability of the entry's dump is either
#                     covered by a host test of zoraya-term or listed with a
#                     reason in the exceptions file, and nothing else is
#   check.sh pin      also compares the dump with the entry the pinned `tic`
#                     compiles from the pinned terminfo.src (needs
#                     `make check-terminfo`'s host tools; run by check-ports)
#   check.sh dump     writes the dump from the pinned tools
#
# A test covers a capability by calling `cap("name", &[args])`, `flag("name")`
# or `num("name")` of crates/zoraya-term/src/entry.rs, which fail for a name
# the dump does not hold, so the call is the coverage and not a comment about
# it. Exceptions: crates/zoraya-term/entry/exceptions.txt, one capability and
# its reason a line; an exception whose capability a test covers is an error
# (the list shrinks when the terminal learns the sequence).
set -eu
cd "$(dirname "$0")/../.."
DUMP=crates/zoraya-term/entry/xterm-256color.caps
EXC=crates/zoraya-term/entry/exceptions.txt
SRC=crates/zoraya-term/src
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

dump() {
    t=$(mktemp -d)
    tools/term-terminfo/terminfo.sh compile "$t/t"
    build/ports/host/ncurses/bin/infocmp -x -1 -A "$t/t" xterm-256color |
        perl -ne 'next if /^#/; next if $. == 2 || !/\S/; s/^\s+//; s/,\s*$//; print "$_\n"' | LC_ALL=C sort
    rm -rf "$t"
}

case ${1:-} in
dump) dump > "$DUMP"; echo "term-conformance: wrote $DUMP"; exit 0 ;;
pin)
    dump > "$TMP/pinned"
    cmp -s "$TMP/pinned" "$DUMP" || { diff "$DUMP" "$TMP/pinned" >&2 || true; echo "term-conformance: $DUMP is not the pinned entry: tools/term-conformance/check.sh dump" >&2; exit 1; }
    echo "term-conformance: the dump is the pinned xterm-256color" ;;
esac

# Names in the dump: the part before = or #, or the whole line (a flag).
sed -e 's/[=#].*//' "$DUMP" | LC_ALL=C sort -u > "$TMP/entry"
# Names the tests cover.
grep -rhoE '\b(cap|flag|num)\("[A-Za-z0-9]+"' "$SRC" --include='tests*.rs' | sed -e 's/.*("//; s/"$//' | LC_ALL=C sort -u > "$TMP/covered"
grep -v -e '^#' -e '^[[:space:]]*$' "$EXC" | awk '{print $1}' | LC_ALL=C sort > "$TMP/exc_all"
LC_ALL=C sort -u "$TMP/exc_all" > "$TMP/excepted"
fail=0
note() { echo "term-conformance: $*" >&2; fail=1; }

[ "$(wc -l < "$TMP/exc_all")" = "$(wc -l < "$TMP/excepted")" ] || note "a capability is listed twice in $EXC"
for n in $(comm -23 "$TMP/entry" "$TMP/covered" | comm -23 - "$TMP/excepted"); do note "capability $n has no test and no exception"; done
for n in $(comm -12 "$TMP/covered" "$TMP/excepted"); do note "capability $n has a test and is still an exception"; done
for n in $(comm -13 "$TMP/entry" "$TMP/covered"); do note "a test names $n, which the entry does not have"; done
for n in $(comm -13 "$TMP/entry" "$TMP/excepted"); do note "the exceptions name $n, which the entry does not have"; done
# Every exception has a reason.
awk '!/^#/ && NF && NF < 2 { print $1 }' "$EXC" | while read -r n; do echo "term-conformance: exception $n has no reason" >&2; done
[ -z "$(awk '!/^#/ && NF && NF < 2' "$EXC")" ] || fail=1
[ "$fail" = 0 ] || exit 1
echo "term-conformance: $(wc -l < "$TMP/entry" | tr -d ' ') capabilities: $(wc -l < "$TMP/covered" | tr -d ' ') covered by tests, $(wc -l < "$TMP/excepted" | tr -d ' ') excepted with a reason"
