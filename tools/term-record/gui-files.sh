#!/bin/sh
# The files tests/gui/terminal.script plays in the window: the recording of
# `less` (crates/zoraya-term/recordings/less-page.rec) cut at its mark "the end"
# (less-end.rec: the full-screen program on the screen).
#
#   gui-files.sh write     make them
#   gui-files.sh check     fail unless they are what write would make (make check)
set -eu
cd "$(dirname "$0")/../.."
REC=crates/zoraya-term/recordings/less-page
OUT=tests/gui/terminal.root/recordings
offset=$(awk '$2 == "the" && $3 == "end" { print $1 }' "$REC.marks")
[ -n "$offset" ] || { echo "gui-files: no mark 'the end' in $REC.marks" >&2; exit 1; }
make_files() {
    head -c "$offset" "$REC.rec" > "$1/less-end.rec"
}
case ${1:-check} in
write) mkdir -p "$OUT"; make_files "$OUT"; echo "gui-files: wrote $OUT" ;;
check)
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    make_files "$tmp"
    for f in less-end.rec; do
        cmp -s "$tmp/$f" "$OUT/$f" || { echo "gui-files: $OUT/$f is not cut from $REC.rec: tools/term-record/gui-files.sh write" >&2; exit 1; }
    done ;;
*) echo "usage: gui-files.sh write|check" >&2; exit 2 ;;
esac
