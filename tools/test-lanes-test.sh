#!/bin/sh
# The behaviour behind make check-test-lanes: make test-changed (DRY=1) with a
# fake map and a fake lanes file. A step with no lane fails the run and says
# so; a step on the skip list prints `SKIPPED: STEP: REASON` and passes; an
# optin step is SKIPPED until its variable is 1; a step with a lane is listed
# as run. This fails on the old behaviour (steps dropped silently).
set -u
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat >"$tmp/map" <<'M'
a/   check test-laned test-skipped test-optin
b/   check test-laned test-unlaned
M
cat >"$tmp/lanes" <<'L'
lane test-laned
lane test-optin
optin PAR_TEST_OPTIN test-optin -- takes long
skip test-skipped -- needs the ports archives (make fetch-ports)
L
bad=0
expect() { # what, command's output file, pattern
	grep -q -- "$3" "$2" || { echo "test-lanes-test: $1: no '$3' in:" >&2; sed 's/^/    /' "$2" >&2; bad=1; }
}
refute() {
	! grep -q -- "$3" "$2" || { echo "test-lanes-test: $1: unexpected '$3' in:" >&2; sed 's/^/    /' "$2" >&2; bad=1; }
}
run() { # paths: the dry run of test-changed on a fake change
	TEST_MAP="$tmp/map" TEST_LANES="$tmp/lanes" TEST_CHANGED_PATHS="$1" DRY=1 tools/test-changed.sh >"$tmp/out" 2>&1
}

run "a/x"
st=$?
[ "$st" = 0 ] || { echo "test-lanes-test: laned, skipped and optin steps must pass the dry run (exit $st)" >&2; bad=1; }
expect "skip list" "$tmp/out" 'SKIPPED: test-skipped: needs the ports archives'
expect "optin off" "$tmp/out" 'SKIPPED: test-optin: takes long'
expect "laned runs" "$tmp/out" 'run: .*test-laned'
refute "laned not skipped" "$tmp/out" 'SKIPPED: test-laned'

PAR_TEST_OPTIN=1 run "a/x"
refute "optin on" "$tmp/out" 'SKIPPED: test-optin'
expect "optin on runs" "$tmp/out" 'run: .*test-optin'

run "b/y"
st=$?
[ "$st" = 1 ] || { echo "test-lanes-test: a step with no lane must fail the run (exit $st)" >&2; bad=1; }
expect "unlaned" "$tmp/out" 'FAIL  test-unlaned: no lane'

# A path the map does not know selects every step of plan-lib.sh: they have
# lanes in the real file only (this fake one has none), so the run fails.
run "zzz/unknown"
[ "$?" = 1 ] || { echo "test-lanes-test: an unmapped path (every step) with no lanes must fail" >&2; bad=1; }
[ "$bad" = 0 ] || exit 1
echo "test-lanes-test: ok"
