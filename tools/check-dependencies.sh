#!/bin/sh
# INV-0461-3: every pinned port of ports/sources.lock has a row in docs/dependencies.md, and the rows that carry a
# licence decision say it: mbedTLS names the licence option used, git says GPL-2.0-only.
# Run from the repository's root by `make check-fetch-ports` (part of `make check`).
set -eu
lock=${1:-ports/sources.lock}
doc=${2:-docs/dependencies.md}
fail=0
say() { echo "check-dependencies: $*"; fail=1; }
[ -f "$lock" ] && [ -f "$doc" ] || { echo "check-dependencies: $lock or $doc is missing"; exit 1; }

# The words a row may use for a lock name when it is not the name itself (case is ignored).
alias_of() {
  case $1 in
    sqlitesrc) echo 'sqlite' ;;
    pkgconfig) echo 'pkg-config' ;;
    libctest) echo 'libc-test' ;;
    nvi2) echo 'nvi' ;;
    db1) echo 'DB 1' ;;
    tcc) echo 'tinycc' ;;
    awk) echo 'one-true-awk' ;;
    openssh) echo 'openssh' ;;
    *) echo "$1" ;;
  esac
}

names=$(awk 'NF >= 5 && $1 !~ /^#/ { print $1 }' "$lock")
[ -n "$names" ] || { say "no pin found in $lock (control)"; exit 1; }
for n in $names; do
  word=$(alias_of "$n")
  if ! grep -i -E '^\|' "$doc" | grep -i -q -F -e "$word"; then
    say "$n (pinned in $lock) has no row in $doc (looked for \"$word\")"
  fi
done

row() { grep -i -E '^\|' "$doc" | grep -E "^\| $1 "; }
m=$(row 'mbedTLS' || true)
[ -n "$m" ] || say "no mbedTLS row"
printf '%s\n' "$m" | grep -q 'Apache-2.0 OR GPL-2.0-or-later' || say "the mbedTLS row does not give its licence"
printf '%s\n' "$m" | grep -q 'GPL-2.0-or-later option is used' || say "the mbedTLS row does not say which licence option is used"
g=$(row 'git' || true)
[ -n "$g" ] || say "no git row"
printf '%s\n' "$g" | grep -q 'GPL-2.0-only' || say "the git row does not give its licence"

[ $fail = 0 ] && echo "check-dependencies: passed ($(echo "$names" | wc -w | tr -d ' ') pins)"
exit $fail
