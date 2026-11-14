#!/bin/sh
# Fails unless, in every job of every workflow given (default: all under
# .github/workflows/), each `make fetch-ports` comes after a step that restores
# build/ports/src from Actions' cache (actions/cache or actions/cache/restore)
# and a step that saves it (actions/cache or actions/cache/save) exists in the
# job. The cache is only a copy: tools/fetch-ports.sh checks every archive's
# SHA-256 and downloads again one that fails.
set -eu
[ $# -gt 0 ] || set -- .github/workflows/*.yml
rc=0
for f in "$@"; do
	awk -v f="$f" '
	/^  [A-Za-z0-9_-]+:[[:space:]]*$/ { job = $1; restored = 0; kind = "" }
	/^[[:space:]]*- / && $0 !~ /uses:/ { kind = "" }
	/uses:[[:space:]]*actions\/cache\/save@/ { kind = "save"; next }
	/uses:[[:space:]]*actions\/cache\/restore@/ { kind = "restore"; next }
	/uses:[[:space:]]*actions\/cache@/ { kind = "both"; next }
	/path:[[:space:]]*build\/ports\/src[[:space:]]*$/ {
		if (kind == "restore" || kind == "both") restored = 1
		if (kind == "save" || kind == "both") saves[job] = 1
		next }
	/run:[[:space:]]*make fetch-ports[[:space:]]*$/ {
		fetches[job] = 1
		if (!restored) { printf "%s: job %s fetches the ports without restoring build/ports/src from the cache first\n", f, job; bad = 1 } }
	END { for (j in fetches) if (!(j in saves)) { printf "%s: job %s fetches the ports without a step that saves build/ports/src\n", f, j; bad = 1 }
		exit bad }
	' "$f" || rc=1
done
[ $rc = 0 ] && echo "check-fetch-cache: every fetch of the ports has its cache steps"
exit $rc
