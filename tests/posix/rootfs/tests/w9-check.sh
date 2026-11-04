#!/bin/sh
# After W9's interrupted build (ADR-0110 item 4): the partial targets left,
# a table that exists and does not end with "0 };" (TASK-02b's
# remote/w9_session.py), counted with dash alone, in the tree $1.
cd "$1" || exit 1
n=0
for f in */*/t?.h; do
	[ -e "$f" ] || continue
	last=
	while read -r l; do last=$l; done < "$f"
	case $last in *"0 };"*) ;; *) n=$((n + 1)) ;; esac
done
echo "W9-PARTIAL $n"
