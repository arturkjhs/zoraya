#!/bin/sh
# The checks of the ports built ahead of their parts (TASK-31 ports-early):
# ncurses, less, nvi2 and Berkeley DB 1 (ADR-0390), OpenSSH's client (ADR-0346)
# and git (ADR-0347). Each is checked only when it was built: the ports are
# not part of the image yet, so a clean `make ports` has none of them, and the
# script says so and passes.
#
#   check-early.sh LOGS ZCC SURFACE_CHECK ELF_CHECK BUILD_DIR
#
# INV-0390-1: the block of ADR-0390 names, for each program, names its objects import.
# INV-0390-2: every compile of a built port ran zcc and none named -D__linux.
# INV-0390-3: the curses archives name the terminfo directory; the programs load.
# The same three for ssh (INV-0346-*) and git (INV-0347-*) when they are built.
set -eu
logs=$1 zcc=$2 surface=$3 elfcheck=$4 build=$5
fail=0
say() { echo "check-early: $*"; fail=1; }
checked=""

# --- the compile logs (INV-0390-2 and its kin)
for l in ncurses less db1 nvi openssh git; do
  [ -f "$logs/$l.log" ] || continue
  checked="$checked $l"
  grep -q -e "$zcc" "$logs/$l.log" || say "$l.log shows no compile command"
  if grep -l -e '-D__linux' -e '-D__gnu_linux' "$logs/$l.log"; then say "$l was compiled with __linux__"; fi
done

# --- ncurses, less, nvi (ADR-0390)
nc=$build/ports/work/ncurses-build
if [ -d "$nc/objects" ] && [ -x "$build/ports/work/less-710/less" ] && [ -d "$build/ports/obj/nvi" ]; then
  "$surface" imports docs/adr/0390-surface-for-curses-less-and-vi.md \
    "ncurses=$nc/objects" "nvi=$build/ports/obj/nvi" "less=$build/ports/work/less-710" || fail=1
  curses=$build/ports/curses/usr
  LC_ALL=C grep -a -q '/usr/share/terminfo' "$curses/lib/libtinfow.a" || say "libtinfow.a does not name /usr/share/terminfo"
  # No lookup of $HOME/.terminfo: --disable-home-terminfo leaves the helper without a caller.
  if LC_ALL=C strings -a "$curses/lib/libtinfow.a" | grep -q '\.terminfo'; then say "libtinfow.a names a .terminfo in the home directory"; fi
  d=$build/ports/early-check
  rm -rf "$d" && mkdir -p "$d"
  cp "$build/ports/work/less-710/less" "$d/less"
  cp "$build/ports/work/nvi2-"*/nvi "$d/vi"
  "$elfcheck" "$d" || fail=1
  rm -rf "$d"
fi

# --- OpenSSH and git, when built
if [ -x "$build/ports/work/openssh-10.6p2/ssh" ] && [ -f docs/adr/0346-surface-for-ssh.md ]; then
  "$surface" imports docs/adr/0346-surface-for-ssh.md \
    "ssh=$build/ports/work/openssh-10.6p2" || fail=1
fi
if [ -x "$build/ports/work/git-2.56.1/git" ] && [ -f docs/adr/0347-surface-for-git.md ]; then
  "$surface" imports docs/adr/0347-surface-for-git.md \
    "git=$build/ports/work/git-2.56.1" || fail=1
fi

if [ -z "$checked" ]; then echo "check-early: none of the early ports was built; nothing to check"; fi
[ $fail = 0 ] && echo "check-early: passed"
exit $fail
