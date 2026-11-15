#!/bin/sh
# INV-0334-1 and the build-log half of INV-0334-2, and the image half of ADR-0335's
# files (INV-0335-4's names are the description lint's):
#
#   check-curses.sh LOGS SYSROOT ROOTFS
#
# INV-0334-1: the sysroot and the image hold exactly one terminfo reader: ncurses' two archives
#   (libncursesw.a, libtinfow.a), no other curses, termcap, terminfo, readline or editline archive,
#   and ports/sources.lock names no other such library.
# INV-0334-2: the configure log of the port records the configured terminfo directory, the disabled
#   home lookup and an empty fallback list.
# INV-0335-2 (strings): vi names no system start-up file, openpty or flock; less no key, history or system file.
# Also: the image has the four entries in letter-named directories, less, vi, and ex and view as
#   links to vi.
set -eu
logs=$1 sysroot=$2 rootfs=$3
fail=0
say() { echo "check-curses: $*"; fail=1; }

# --- INV-0334-1
for d in "$sysroot/usr/lib" "$rootfs/usr/lib"; do
  [ -d "$d" ] || { say "$d is missing: nothing was checked"; continue; }
  for f in "$d"/*; do
    case $(basename "$f") in
      libncursesw.a | libtinfow.a) ;;
      *curses* | *termcap* | *terminfo* | *tinfo* | *ncurses* | *readline* | *edit* | *slang* | *termlib*)
        say "$f is a curses, termcap or terminfo library besides ncurses' two archives" ;;
    esac
  done
  for a in libncursesw.a libtinfow.a; do [ -f "$d/$a" ] || say "$d/$a is missing"; done
done
other=$(awk '!/^#/ && NF { print $1 }' ports/sources.lock | grep -i -E 'curses|termcap|terminfo|tinfo|readline|libedit|slang' | grep -v -x ncurses || true)
[ -z "$other" ] || say "ports/sources.lock names another curses-like library: $other"

# --- INV-0334-2, the build log
log=$logs/ncurses-configure.log
if [ -f "$log" ]; then
  grep -q 'checking for default terminfo directory... /usr/share/terminfo$' "$log" || say "the configured terminfo directory is not /usr/share/terminfo"
  grep -q 'checking for list of terminfo directories... /usr/share/terminfo$' "$log" || say "the list of terminfo directories is not /usr/share/terminfo alone"
  grep -q 'checking if ~/.terminfo is wanted... no$' "$log" || say "the home lookup is not disabled"
  grep -q 'checking for list of fallback terminal descriptions... no$' "$log" || say "the fallback list is not empty"
else
  say "no $log (make ncurses first)"
fi

# --- the image's files
[ -d "$rootfs/bin" ] || say "$rootfs/bin is missing: the image was not checked"
if [ -d "$rootfs/bin" ]; then
  for n in less vi; do [ -f "$rootfs/bin/$n" ] && [ ! -L "$rootfs/bin/$n" ] || say "/bin/$n is not a file of the image"; done
  for n in ex view; do [ "$(readlink "$rootfs/bin/$n" 2>/dev/null)" = /bin/vi ] || say "/bin/$n is not a link to /bin/vi"; done
  for p in x/xterm-256color x/xterm v/vt100 d/dumb; do [ -f "$rootfs/usr/share/terminfo/$p" ] || say "$rootfs/usr/share/terminfo/$p is missing"; done
  [ "$(find "$rootfs/usr/share/terminfo" -type f | wc -l | tr -d ' ')" = 4 ] || say "the image's terminfo directory does not hold exactly four files"
fi
# --- INV-0335-2, the strings half: what the patches compiled out is not in the binaries.
[ -f "$rootfs/bin/vi" ] && [ -f "$rootfs/bin/less" ] || say "$rootfs/bin/vi or less is missing: the strings were not checked"
if [ -f "$rootfs/bin/vi" ] && [ -f "$rootfs/bin/less" ]; then
  for pat in '/etc/vi.exrc' 'openpty' 'flock'; do
    if LC_ALL=C strings -a "$rootfs/bin/vi" | grep -q -F -e "$pat"; then say "/bin/vi names $pat, which a patch removed"; fi
  done
  for pat in '/etc/sysless' '.lesskey' 'lesshst' 'openpty' 'flock'; do
    if LC_ALL=C strings -a "$rootfs/bin/less" | grep -q -F -e "$pat"; then say "/bin/less names $pat, which is compiled out"; fi
  done
fi
[ $fail = 0 ] && echo "check-curses: passed"
exit $fail
