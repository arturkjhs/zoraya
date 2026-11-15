#!/bin/sh
# An `ar` for git's Makefile (TASK-31 ports-early). zoraya-ar names a member by
# the file's base name and replaces a member of the same name (ADR-0109 item 3),
# and git has objects of one name in different directories (`fsck.o` and
# `tree.o` are each in libgit.a twice: the top directory's and reftable's), so
# an archive made from them lost one of each and the link failed with git's
# own functions undefined. This wrapper gives each object a name that holds its
# path (`reftable_tree.o`) in a scratch directory and archives those.
#
#   ar.sh ZAR OPERATION ARCHIVE OBJECT...
set -eu
zar=$1 op=$2 archive=$3
shift 3
dir=$(mktemp -d "${TMPDIR:-/tmp}/git-ar.XXXXXX")
trap 'rm -rf "$dir"' EXIT
list=
for f in "$@"; do
  n=$(printf '%s' "$f" | sed -e 's|^\./||' -e 's|/|_|g')
  cp "$f" "$dir/$n"
  list="$list $dir/$n"
done
rm -f "$archive"
"$zar" "$op" "$archive" $list
