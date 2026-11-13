#!/bin/sh
# Tests the docs-only rule of tools/ci-plan.sh (plan-lib.sh's docs_only) on a
# scratch repository: a push to main whose every changed path is under docs/
# and that changes no posix-surface block gets `docs_only=1`; everything else,
# and an unusable before commit, gets the full set.
#
#   tools/check-ci-plan.sh
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ci-plan-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
git init -q .
git config user.name ci-plan-test
git config user.email ci-plan-test@localhost
git config commit.gpgsign false
mkdir -p tools docs/adr src
for f in ci-plan.sh plan-lib.sh ci-tests.txt test-map.txt; do cp "$here/tools/$f" tools/; done
printf 'x\n' > docs/design.md
printf 'x\n' > src/code.rs
printf 'x\n' > README.md
printf 'abs\n' > docs/adr/posix-surface-T9.txt
printf 'prose\n\n```posix-surface-added\nabs\nacos\n```\n\nmore prose\n' > docs/adr/0999-surface.md
printf 'prose\n' > docs/adr/0998-plain.md
git add -A
git commit -q -m base
base=$(git rev-parse HEAD)

fail=0
# expect NAME EXPECTED_docs_only BEFORE: plan for BASE..HEAD
expect() {
	out=$(DOCS_BEFORE=$3 DOCS_AFTER=$(git rev-parse HEAD) sh tools/ci-plan.sh origin/main 2>/dev/null)
	got=$(echo "$out" | sed -n 's/^docs_only=//p')
	if [ "$got" != "$2" ]; then
		echo "check-ci-plan: $1: docs_only=$got, expected $2" >&2
		fail=$((fail + 1))
	fi
	if [ "$2" = 1 ]; then
		echo "$out" | grep -q '^tests=\[\]$' && echo "$out" | grep -q '^build=false$' ||
			{ echo "check-ci-plan: $1: a docs-only plan has no tests and no build" >&2; fail=$((fail + 1)); }
	else
		echo "$out" | grep -q '^build=true$' ||
			{ echo "check-ci-plan: $1: the full set needs the build" >&2; fail=$((fail + 1)); }
	fi
}
step() { # NAME: commit the working tree
	git add -A
	git commit -q -m "$1"
}
reset_to() { git reset -q --hard "$1"; }

printf 'y\n' >> docs/design.md; printf 'q\n' >> docs/adr/0998-plain.md
step docs
expect "docs only" 1 "$base"

reset_to "$base"
printf 'prose changed\n\n```posix-surface-added\nabs\nacos\n```\n\nmore prose\n' > docs/adr/0999-surface.md
step prose-in-block-file
expect "prose edit of an ADR with a block" 1 "$base"

reset_to "$base"
printf 'prose\n\n```posix-surface-added\nabs\nacos\nasin\n```\n\nmore prose\n' > docs/adr/0999-surface.md
step block
expect "docs and a changed block" 0 "$base"

reset_to "$base"
printf 'prose\n\n```posix-surface-added\nabs\n```\n\nmore prose\n' > docs/adr/0999-surface.md
step block-removed
expect "a block shrinks" 0 "$base"

reset_to "$base"
printf 'new\n\n```posix-surface-added\nabs\n```\n' > docs/adr/0997-new.md
printf 'y\n' >> docs/design.md
step new-block
expect "a new ADR with a block" 0 "$base"

reset_to "$base"
printf 'abs\nacos\n' > docs/adr/posix-surface-T9.txt
step tier-list
expect "a tier list" 0 "$base"

reset_to "$base"
printf 'y\n' >> docs/design.md; printf 'y\n' >> src/code.rs
step code
expect "docs and one code file" 0 "$base"

reset_to "$base"
printf 'y\n' >> README.md
step readme
expect "README.md outside docs/" 0 "$base"

reset_to "$base"
printf 'y\n' >> docs/design.md
step docs
expect "no before (first push)" 0 0000000000000000000000000000000000000000
expect "empty before" 0 ""
expect "unknown before" 0 1234567890abcdef1234567890abcdef12345678
expect "an empty change" 0 "$(git rev-parse HEAD)"

# A merge commit: the diff against the first parent (the before of a push to
# main), and a mixed push of two commits.
reset_to "$base"
main_head=$base
git checkout -q -b side
printf 'y\n' >> docs/design.md
step side-docs
git checkout -q -
git merge -q --no-ff -m merge side
expect "a merge of docs" 1 "$main_head"
git checkout -q -b side2 "$main_head"
printf 'y\n' >> src/code.rs
step side-code
git checkout -q -
git merge -q --no-ff -m merge2 side2
expect "a merge with code" 0 "$main_head"

# A force push: before is not an ancestor.
git checkout -q -b other "$base"
printf 'z\n' >> docs/design.md
step other
other=$(git rev-parse HEAD)
git checkout -q -
expect "before not an ancestor" 0 "$other"

# FULL=1 and the candidate plan are unchanged by the docs-only inputs' absence.
out=$(FULL=1 sh tools/ci-plan.sh 2>/dev/null)
echo "$out" | grep -q '^docs_only=0$' || { echo "check-ci-plan: FULL=1 gives docs_only=0" >&2; fail=$((fail + 1)); }

if [ "$fail" -gt 0 ]; then
	echo "check-ci-plan: $fail failed" >&2
	exit 1
fi
echo "check-ci-plan: the docs-only rule holds"
