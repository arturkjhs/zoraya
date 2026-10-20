#!/bin/sh
# Stage 0: install pinned Alpine packages and record the environment.
. "$(dirname "$0")/common.sh"
sudo apk add -q $ALPINE_PKGS
{
  echo "alpine_release=$(cat /etc/alpine-release)"
  echo "kernel=$(uname -srm)"
  echo "musl=$(ldd 2>&1 | sed -n 's/^Version //p')"
  echo "nproc=$(nproc)"
  apk info -v 2>/dev/null | grep -E '^(clang[0-9]*|lld[0-9]*|llvm[0-9]*-libs|gcc|strace|musl|binutils|make|python3|ctags|git|gdb)-[0-9]' | sort | sed 's/^/pkg=/'
} > "$RES/env.txt"
cat "$RES/env.txt"
