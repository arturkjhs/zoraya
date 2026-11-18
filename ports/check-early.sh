#!/bin/sh
# The checks of the ports built ahead of their parts (TASK-31 ports-early):
# ncurses, less, nvi2 and Berkeley DB 1 (ADR-0390), OpenSSH's client (ADR-0346)
# and git (ADR-0347). The ports are not part of the image yet, so CI's `make
# ports` builds none of them; each is checked when its tree is there. A tree
# that is there without its binaries is a failure (a half-built port), and the
# script fails too when ports exist and nothing could be checked.
#
#   check-early.sh LOGS ZCC SURFACE_CHECK ELF_CHECK BUILD_DIR LIBC_A
#
# INV-0390-1, INV-0391-1, INV-0346-1, INV-0347-1: each block names, for each program, a name its objects import.
# INV-0390-2: every compile of a built port ran zcc (one line per object) and none named -D__linux.
# INV-0390-3: the curses archives name the terminfo directory; the programs load.
# INV-0346-2: ssh and ssh-keygen name no askpass variable or configuration file.
# INV-0347-2: no object of git imports getenv but wrapper.o (and compat/regex's locale lookups).
# INV-0461-1: git-remote-http links mbedTLS and not OpenSSL, and no other program of the ports does (by symbol).
# INV-0461-2 (the part that no option turns verification off): git-remote-http names neither the options nor the
# files that would (by string); the behaviour is tested by `make test-apps` (tests/shell/apps-git.script).
# Also: libc.a defines every name of the three blocks, and no binary of the images holds an early shim's marker.
set -eu
logs=$1 zcc=$2 surface=$3 elfcheck=$4 build=$5 libc=${6:-}
work=$build/ports/work
fail=0
checked=0
exists=0
say() { echo "check-early: $*"; fail=1; }
nm_tool=$(command -v llvm-nm || command -v nm)

# The one directory of a port's tree, whatever its version.
tree() { ls -d "$work/$1"-[0-9]* 2>/dev/null | head -1; }

# Every compile of a port ran zcc, once per object, and none named __linux.
compiles() { # LOG NAME OBJECT_COUNT
  [ -f "$1" ] || { say "$2: no log $1"; return; }
  n=$(grep -c -e "$zcc.* -c " "$1" || true)
  [ "$n" -ge "$3" ] || say "$2: $n compile lines for $3 objects in $1"
  if grep -l -e '-D__linux' -e '-D__gnu_linux' "$1"; then say "$2 was compiled with __linux__"; fi
}
count_o() { find "$@" -name '*.o' 2>/dev/null | wc -l | tr -d ' '; }

# A block's names are defined by libc.a.
block_in_libc() { # ADR
  [ -n "$libc" ] && [ -f "$libc" ] || return 0
  defined=$("$nm_tool" --defined-only "$libc" 2>/dev/null | awk '$2 ~ /^[TW]$/ { print $3 }')
  for n in $(sed -n '/^```posix-surface-added/,/^```$/p' "$1" | awk 'NF == 2 && $1 !~ /^`/ { print $1 }'); do
    printf '%s\n' "$defined" | grep -qx "$n" || say "$n of $1 is not defined by libc.a"
  done
}

# --- ncurses, less, nvi (ADR-0390)
nc=$work/ncurses-build
less=$(tree less)
nvi=$(tree nvi2)
if [ -d "$nc" ] || [ -n "$less" ] || [ -n "$nvi" ]; then
  exists=1
  if [ -d "$nc/objects" ] && [ -x "$less/less" ] && [ -x "$nvi/nvi" ] && [ -d "$build/ports/obj/nvi" ]; then
    checked=1
    "$surface" imports docs/adr/0390-surface-for-curses-less-and-vi.md \
      "ncurses=$nc/objects" "nvi=$build/ports/obj/nvi" "less=$less" || fail=1
    # ADR-0391's block (creat, sleep): the same programs' objects import them (INV-0391-1).
    "$surface" imports docs/adr/0391-what-part-10d-decided-where-the-adrs-are-silent.md \
      "ncurses=$nc/objects" "less=$less" || fail=1
    compiles "$logs/ncurses.log" ncurses "$(count_o "$nc/objects")"
    compiles "$logs/less.log" less "$(count_o "$less" -maxdepth 1)"
    compiles "$logs/nvi.log" nvi "$(count_o "$build/ports/obj/nvi")"
    [ -f "$logs/db1.log" ] && compiles "$logs/db1.log" db1 "$(count_o "$build/ports/obj/db1")"
    curses=$build/ports/curses/usr
    LC_ALL=C grep -a -q '/usr/share/terminfo' "$curses/lib/libtinfow.a" || say "libtinfow.a does not name /usr/share/terminfo"
    if LC_ALL=C strings -a "$curses/lib/libtinfow.a" | grep -q '\.terminfo'; then say "libtinfow.a names a .terminfo in the home directory"; fi
    d=$build/ports/early-check
    rm -rf "$d" && mkdir -p "$d"
    cp "$less/less" "$d/less"
    cp "$nvi/nvi" "$d/vi"
    "$elfcheck" "$d" || fail=1
    rm -rf "$d"
    block_in_libc docs/adr/0390-surface-for-curses-less-and-vi.md
    block_in_libc docs/adr/0391-what-part-10d-decided-where-the-adrs-are-silent.md
  else
    say "ncurses, less or nvi has a tree and not all of its products"
  fi
fi

# --- OpenSSH
ssh=$(tree openssh)
if [ -n "$ssh" ]; then
  exists=1
  if [ -x "$ssh/ssh" ] && [ -x "$ssh/ssh-keygen" ]; then
    checked=1
    "$surface" imports docs/adr/0346-surface-for-ssh.md "ssh=$ssh" || fail=1
    compiles "$logs/openssh.log" openssh "$(count_o "$ssh")"
    for b in ssh ssh-keygen; do
      if LC_ALL=C strings -a "$ssh/$b" | grep -q -e 'SSH_ASKPASS' -e 'ssh_config' -e '\.ssh/config'; then
        say "$b names SSH_ASKPASS or a configuration file"
      fi
    done
    d=$build/ports/early-check
    rm -rf "$d" && mkdir -p "$d"
    cp "$ssh/ssh" "$ssh/ssh-keygen" "$d/"
    "$elfcheck" "$d" || fail=1
    rm -rf "$d"
    block_in_libc docs/adr/0346-surface-for-ssh.md
  else
    say "openssh has a tree and no ssh or ssh-keygen"
  fi
fi

# --- git
git=$(tree git)
if [ -n "$git" ]; then
  exists=1
  if [ -x "$git/git" ] && [ -x "$git/git-remote-http" ]; then
    checked=1
    "$surface" imports docs/adr/0347-surface-for-git.md "git=$git" "git=$git/compat" || fail=1
    compiles "$logs/git.log" git "$(count_o "$git")"
    # INV-0347-2, by symbol: nothing but wrapper.o (zoraya_getenv's own call) imports getenv.
    for o in $(find "$git" -name '*.o'); do
      if "$nm_tool" -u "$o" 2>/dev/null | grep -qw getenv; then
        case $o in "$git"/wrapper.o | "$git"/compat/regex/*) ;; *) say "$o imports getenv";; esac
      fi
    done
    d=$build/ports/early-check
    rm -rf "$d" && mkdir -p "$d"
    cp "$git/git" "$git/git-remote-http" "$d/"
    "$elfcheck" "$d" || fail=1
    rm -rf "$d"
    block_in_libc docs/adr/0347-surface-for-git.md
    # INV-0461-1, by symbol. The control first: the helper defines mbedTLS's client (a check that finds nothing
    # because nothing was linked would pass for the wrong reason). Then: no OpenSSL in it, no mbedTLS in `git`
    # and in no other program of the ports.
    "$nm_tool" --defined-only "$git/git-remote-http" 2>/dev/null | grep -qw mbedtls_ssl_handshake \
      || say "git-remote-http defines no mbedtls_ssl_handshake (control)"
    if "$nm_tool" --defined-only "$git/git-remote-http" 2>/dev/null | grep -q -w -e SSL_new -e SSL_CTX_new -e EVP_CIPHER_CTX_new -e OPENSSL_init_ssl; then
      say "git-remote-http links OpenSSL (INV-0461-1)"
    fi
    for other in "$git/git" "$build/rootfs-apps/bin/curl" "$build/rootfs-apps/bin/wget" "$build/rootfs-apps/bin/ssh" \
      "$build/rootfs-apps/bin/ssh-keygen" "$build/ports/work/curl-"[0-9]*/src/curl "$build/ports/work/wget-"[0-9]*/src/wget \
      "$build/ports/work/openssh-"[0-9]*/ssh; do
      [ -f "$other" ] || continue
      if "$nm_tool" --defined-only "$other" 2>/dev/null | grep -q -w mbedtls_ssl_handshake; then
        say "$other links mbedTLS (INV-0461-1)"
      fi
    done
    # The helper trusts the system's roots and reads no device file for entropy. The variables git itself names
    # (GIT_SSL_NO_VERIFY and the rest) are read through zoraya_getenv, which answers "not set" (INV-0347-2); that
    # they decide nothing is the test of INV-0461-2 (tests/shell/apps-git.script), not a string check.
    if ! LC_ALL=C grep -a -q '/system/tls/roots' "$git/git-remote-http"; then
      say "git-remote-http does not name /system/tls/roots"
    fi
    for word in /dev/urandom /dev/random; do
      if LC_ALL=C grep -a -q -e "$word" "$git/git-remote-http"; then
        say "git-remote-http names $word (INV-0310-5)"
      fi
    done
  else
    say "git has a tree and no git or git-remote-http"
  fi
fi

# --- no early shim in an image
for dir in "$build/rootfs-posix/bin" "$build/rootfs-apps/bin"; do
  [ -d "$dir" ] || continue
  if grep -l -a 'ZORAYA-EARLY-SHIM' "$dir"/* 2>/dev/null; then say "an image binary links an early shim ($dir)"; fi
done

if [ "$exists" = 0 ]; then
  echo "check-early: none of the early ports was built; nothing to check"
elif [ "$checked" = 0 ]; then
  say "early ports exist and none could be checked"
fi
[ $fail = 0 ] && echo "check-early: passed"
exit $fail
