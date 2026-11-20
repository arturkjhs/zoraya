#!/bin/sh
# INV-0451-2 (ADR-0451 item 5): libzoraya-rust.a defines only names listed in
# an accepted ADR, each failing, and is linked into no program but those
# tools/zcargo links.
#
#   - ports/rust/names.txt is the list; every name in it is in ADR-0451's item 5
#     (a name enters only by an ADR) and is not an export of libc.a's tier list;
#   - the archive, if built (build/sysroot/usr/lib), defines exactly those names;
#   - no file but the allowed ones names the archive: zcc and zc++ (every C and
#     C++ link line) never do, nor do the cmake support file and the ports'
#     Makefile outside its `rust` rule.
set -eu
cd "$(dirname "$0")/.."
bad=0
list=$(sed -e 's/#.*//' -e '/^[[:space:]]*$/d' ports/rust/names.txt | sort)
adr=docs/adr/0451-what-libposix-answers-for-rust-std.md
# Item 5's paragraph: from "5. **`libzoraya-rust.a`**" to the next numbered item.
para=$(awk '/^5\. \*\*`libzoraya-rust\.a`\*\*/ { on = 1 } /^6\. \*\*/ { on = 0 } on' "$adr")
[ -n "$para" ] || { echo "check-rust-archive: no item 5 in $adr"; exit 1; }
for n in $list; do
    echo "$para" | grep -q "\`$n\`" || { echo "check-rust-archive: $n is in ports/rust/names.txt and not in $adr item 5"; bad=1; }
done
# And every name item 5 lists is in the file (the other direction).
for n in setuid setgid setgroups setsid chroot sigaltstack sched_getaffinity sendfile splice waitid; do
    echo "$para" | grep -q "\`$n\`" || { echo "check-rust-archive: $adr item 5 no longer lists $n: update this check with the ADR"; bad=1; }
    echo "$list" | grep -qx "$n" || { echo "check-rust-archive: item 5 lists $n and ports/rust/names.txt does not"; bad=1; }
done
archive=build/sysroot/usr/lib/libzoraya-rust.a
if [ -f "$archive" ]; then
    defined=$(nm -g --defined-only "$archive" 2>/dev/null | awk 'NF == 3 { print $3 }' | sort -u)
    [ "$defined" = "$list" ] || {
        echo "check-rust-archive: $archive defines:"; echo "$defined" | sed 's/^/  /'
        echo "check-rust-archive: ports/rust/names.txt lists:"; echo "$list" | sed 's/^/  /'
        bad=1
    }
    # libc.a exports none of them (libposix hides setgroups and setsid as __zoraya_NAME).
    if [ -f build/sysroot/usr/lib/libc.a ]; then
        exports=$(nm -g --defined-only build/sysroot/usr/lib/libc.a 2>/dev/null | awk 'NF == 3 { print $3 }' | sort -u)
        for n in $list; do
            echo "$exports" | grep -qx "$n" && { echo "check-rust-archive: libc.a exports $n"; bad=1; }
        done
    fi
else
    echo "check-rust-archive: $archive is not built; its names are not compared"
fi
# Every other place that names the archive.
allowed='Makefile tools/zcargo tools/zcargo-link tools/check-rust-archive.sh tools/test-zcargo.sh tools/rust-musl-target.sh ports/Makefile ports/rust/zoraya-rust.c ports/rust/names.txt'
for f in $(git grep -l -e 'zoraya-rust' -- . ':!docs' ':!*.md' 2>/dev/null || true); do
    case " $allowed " in *" $f "*) continue ;; esac
    echo "check-rust-archive: $f names the archive and is not one of: $allowed"
    bad=1
done
# The two C drivers never do.
for f in tools/zcc tools/zc++; do
    ! grep -q 'zoraya-rust' "$f" || { echo "check-rust-archive: $f names the archive"; bad=1; }
done
[ "$bad" = 0 ] || exit 1
echo "check-rust-archive: ok ($(echo "$list" | wc -l | tr -d ' ') names)"
