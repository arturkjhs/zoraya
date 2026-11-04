#!/bin/sh
# Fails unless the CI workflow runs everything `make test` runs (ADR-0213's
# Decision): every step of TEST_STEPS as `make STEP`, except test-qemu, which
# must appear as `make test-qemu-CONFIG` for every configuration, and
# test-posix, which must appear as `make test-posix POSIX_SCRIPTS="..."
# POSIX_ACCELS="..."` lines that together run every script under every
# accelerator, W8 (POSIX_ONCE) as each of its builds' scripts under the first.
#
#   TEST_STEPS=... QEMU_CONFIGS=... POSIX_SCRIPTS=... POSIX_ACCELS=...
#   POSIX_ONCE=... W8_BUILDS=... tools/check-ci-jobs.sh WORKFLOW
set -eu
workflow=$1
[ -r "$workflow" ] || { echo "check-ci-jobs: cannot read $workflow" >&2; exit 2; }

# Every command the workflow gives, one a line, without YAML's list dash,
# key or quoting: `run:` and `cmd:` values and the matrix's list items.
commands=$(sed -n -E -e 's/^[[:space:]]*(- )?(run|cmd):[[:space:]]*//p' \
	-e 's/^[[:space:]]*- //p' "$workflow" |
	sed -e "s/^'\(.*\)'\$/\1/" -e 's/^"\(.*\)"$/\1/' | grep -E '^make ' || true)

missing=0
need() { # what, for the message
	echo "check-ci-jobs: $workflow does not run $1" >&2
	missing=$((missing + 1))
}
has_make() { # target
	echo "$commands" | grep -E -q "(^|[;&|[:space:]])make $1([[:space:]]|\$)"
}

# The (script, accelerator) pairs the test-posix lines run.
pairs=$(echo "$commands" | grep -E '(^|[[:space:]])make test-posix([[:space:]]|$)' | while read -r line; do
	scripts=$(echo "$line" | sed -n 's/.*POSIX_SCRIPTS="\([^"]*\)".*/\1/p')
	accels=$(echo "$line" | sed -n 's/.*POSIX_ACCELS="\([^"]*\)".*/\1/p')
	[ -n "$scripts" ] && [ -n "$accels" ] || { echo "UNSPLIT"; continue; }
	for s in $scripts; do for a in $accels; do echo "$(basename "$s" .script):$a"; done; done
done)
has_pair() {
	echo "$pairs" | grep -q -x "$1"
}

first_accel=${POSIX_ACCELS%% *}
for step in $TEST_STEPS; do
	case $step in
	test-qemu)
		for c in $QEMU_CONFIGS; do has_make "test-qemu-$c" || need "make test-qemu-$c"; done ;;
	test-posix)
		if echo "$pairs" | grep -q -x UNSPLIT; then
			need "test-posix with both POSIX_SCRIPTS=\"...\" and POSIX_ACCELS=\"...\" on every line"
		fi
		for s in $POSIX_SCRIPTS; do
			name=$(basename "$s" .script)
			case " $POSIX_ONCE " in
			*" $name "*)
				for b in $W8_BUILDS; do
					has_pair "$name-$b:$first_accel" || need "$name-$b.script under $first_accel"
				done ;;
			*)
				for a in $POSIX_ACCELS; do has_pair "$name:$a" || need "$name.script under $a"; done ;;
			esac
		done ;;
	*)
		has_make "$step" || need "make $step" ;;
	esac
done
if [ "$missing" -gt 0 ]; then
	echo "check-ci-jobs: $missing missing" >&2
	exit 1
fi
echo "check-ci-jobs: $workflow runs every step of make test"
