#!/bin/sh
# Tests the docs-only rule of tools/ci-plan.sh (plan-lib.sh's docs_only) and the
# tree-proof rule (tools/ci-tree-proof.rb, ADR-0345) on a scratch repository: a push to main whose every changed path is under docs/
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
mkdir -p tools docs/adr docs/research/task-02 docs/research/desktop/mockups src
for f in ci-plan.sh plan-lib.sh ci-tests.txt test-map.txt ci-tree-proof.rb; do cp "$here/tools/$f" tools/; done
mkdir -p .github/workflows
cp "$here/.github/workflows/test.yml" .github/workflows/
printf 'x\n' > docs/design.md
printf 'x\n' > src/code.rs
printf 'x\n' > docs/tour.md
printf 'x\n' > docs/dependencies.md
printf 'x\n' > docs/adr/0211-acceptance.md
printf 'x\n' > docs/research/task-02/libc-surface.csv
printf 'x\n' > docs/research/desktop/mockups/t.txt
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

for f in docs/tour.md docs/dependencies.md docs/adr/0211-acceptance.md docs/research/task-02/libc-surface.csv docs/research/desktop/mockups/t.txt; do
	reset_to "$base"
	printf 'y\n' >> "$f"
	step read-by-tests
	expect "$f is read by a test" 0 "$base"
done

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


# The tree-proof rule (ADR-0345): a push to main that is not docs-only gets
# the document checks only when the merge commit's tree is the tree of a run of
# the full set that went green, every job present and successful, and the commit
# is a merge of two parents whose first is the previous head. The API is a
# directory of fixtures (a missing file is an API error).
git checkout -q -f -B tp "$base"
git checkout -q -b cand
printf 'y\n' >> src/code.rs
step candidate-code
cand=$(git rev-parse HEAD)
git checkout -q tp
git merge -q --no-ff -m merge-cand cand
merge=$(git rev-parse HEAD)
git checkout -q -b cand2 "$base"
printf 'z\n' >> src/code.rs
step other-code
cand2=$(git rev-parse HEAD)
git checkout -q tp
fix="$tmp/fixtures"
fakeapi="$tmp/fakeapi"
cat > "$fakeapi" <<'FAKE'
#!/bin/sh
f="$FIXTURES/$(echo "${1%%\?*}" | tr -c 'A-Za-z0-9\n' _)"
[ -r "$f" ] || exit 1
cat "$f"
FAKE
chmod +x "$fakeapi"
repo=o/r
key() { echo "repos/$repo/$1" | tr -c 'A-Za-z0-9\n' _; }
all_jobs() { # the job names of a full run, one a line
	echo build
	grep -v -E '^#|^$' tools/ci-tests.txt
	sed -n -E 's/^[[:space:]]*- (make .*)$/\1/p' .github/workflows/test.yml
}
jobs_json() { # names on stdin -> API answer; BAD_JOB gets BAD_CONCLUSION, DROP_JOB is left out
	awk -v bad="${BAD_JOB:-}" -v conc="${BAD_CONCLUSION:-success}" -v drop="${DROP_JOB:-}" '
		BEGIN { printf "{\"total_count\":1000,\"jobs\":[" }
		$0 == drop { next }
		{ gsub(/\\/, "\\\\"); gsub(/"/, "\\\"")
		  printf "%s{\"name\":\"%s\",\"conclusion\":\"%s\"}", (n++ ? "," : ""), $0, ($0 == bad ? conc : "success") }
		END { print "]}" }'
}
fixture() { # NAME HEAD_SHA CONCLUSION: one run, id 100 (BAD_JOB, DROP_JOB: see jobs_json)
	rm -rf "$fix"; mkdir -p "$fix"
	printf '{"workflow_runs":[{"id":100,"event":"push","conclusion":"%s","head_sha":"%s"}]}\n' "$3" "$2" > "$fix/$(key 'actions/workflows/test.yml/runs')"
	all_jobs | jobs_json > "$fix/$(key 'actions/runs/100/jobs')"
	unset BAD_JOB BAD_CONCLUSION DROP_JOB # an assignment before a function call may stay set
	: "$1"
}
proof_expect() { # NAME EXPECTED_docs_only [BEFORE]: plan of the merge commit
	out=$(CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 BEFORE=${3:-$base} \
		DOCS_BEFORE=${3:-$base} DOCS_AFTER=$merge SHA=$merge sh tools/ci-plan.sh origin/main 2>"$tmp/plan.err")
	got=$(echo "$out" | sed -n 's/^docs_only=//p')
	if [ "$got" != "$2" ]; then
		echo "check-ci-plan: tree proof: $1: docs_only=$got, expected $2" >&2
		grep "ci-" "$tmp/plan.err" | head -8 >&2
		fail=$((fail + 1))
	elif [ "$2" = 1 ]; then
		echo "$out" | grep -q '^summary=.*run 100' && echo "$out" | grep -q '^tests=\[\]$' ||
			{ echo "check-ci-plan: tree proof: $1: the summary names the run, no tests" >&2; fail=$((fail + 1)); }
	else
		echo "$out" | grep -q '^build=true$' ||
			{ echo "check-ci-plan: tree proof: $1: the full set needs the build" >&2; fail=$((fail + 1)); }
	fi
}
[ "$(git rev-parse "$merge^{tree}")" = "$(git rev-parse "$cand^{tree}")" ] ||
	{ echo "check-ci-plan: the fixture's merge and candidate trees differ" >&2; fail=$((fail + 1)); }

fixture ok "$cand" success
proof_expect "same tree, every job, success" 1
proof_expect "first parent is not the previous head" 0 "$cand"
fixture failed "$cand" failure
proof_expect "the run failed" 0
fixture other "$cand2" success
proof_expect "a different tree" 0
DROP_JOB="make test-boot" fixture ok "$cand" success
proof_expect "same tree, a job missing" 0
BAD_JOB="make test-boot" BAD_CONCLUSION=skipped fixture ok "$cand" success
proof_expect "same tree, a job skipped" 0
BAD_JOB=build BAD_CONCLUSION=failure fixture ok "$cand" success
proof_expect "same tree, the build failed" 0
fixture ok "$cand" success
rm "$fix/$(key 'actions/runs/100/jobs')"
proof_expect "API error reading the jobs" 0
fixture ok "$cand" success
rm "$fix/$(key 'actions/workflows/test.yml/runs')"
proof_expect "API error listing the runs" 0
# A head commit this checkout does not have: its tree comes from the API.
unknown=1111111111111111111111111111111111111111
fixture ok "$unknown" success
proof_expect "an unknown head, no tree from the API" 0
printf '{"tree":{"sha":"%s"}}\n' "$(git rev-parse "$merge^{tree}")" > "$fix/$(key "git/commits/$unknown")"
proof_expect "an unknown head, the API's tree is the same" 1
printf '{"tree":{"sha":"%s"}}\n' "$(git rev-parse "$cand2^{tree}")" > "$fix/$(key "git/commits/$unknown")"
proof_expect "an unknown head, the API's tree differs" 0
# The run is never its own proof.
fixture ok "$cand" success
out=$(CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=100 BEFORE=$base DOCS_BEFORE=$base DOCS_AFTER=$merge sh tools/ci-plan.sh origin/main 2>/dev/null)
echo "$out" | grep -q '^docs_only=0$' || { echo "check-ci-plan: tree proof: a run proves itself" >&2; fail=$((fail + 1)); }
# Not a merge: the candidate's own commit, one parent.
fixture ok "$cand" success
out=$(CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 BEFORE=$base DOCS_BEFORE=$base DOCS_AFTER=$cand SHA=$cand sh tools/ci-plan.sh origin/main 2>/dev/null)
echo "$out" | grep -q '^docs_only=0$' || { echo "check-ci-plan: tree proof: a one-parent commit is proven" >&2; fail=$((fail + 1)); }
# A merge whose tree differs from the candidate's (main moved: more than the merge).
git checkout -q -b moved "$base"
printf 'm\n' >> docs/design.md
step main-moved
git merge -q --no-ff -m merge-moved cand
merge=$(git rev-parse HEAD)
fixture ok "$cand" success
proof_expect "main moved: the merge's tree is not the candidate's" 0 "$(git rev-parse HEAD^1)"

# The plan job reads the runs through its token (INV-0345-2).
awk '/^  plan:/ { inplan = 1; next } /^  [a-z]+:/ { inplan = 0 } inplan && /^      actions: read/ { ok = 1 } END { exit !ok }' .github/workflows/test.yml ||
	{ echo "check-ci-plan: the plan job of test.yml lacks 'actions: read'" >&2; fail=$((fail + 1)); }

# FULL=1 and the candidate plan are unchanged by the docs-only inputs' absence.
out=$(FULL=1 sh tools/ci-plan.sh 2>/dev/null)
echo "$out" | grep -q '^docs_only=0$' || { echo "check-ci-plan: FULL=1 gives docs_only=0" >&2; fail=$((fail + 1)); }

if [ "$fail" -gt 0 ]; then
	echo "check-ci-plan: $fail failed" >&2
	exit 1
fi
echo "check-ci-plan: the docs-only rule and the tree-proof rule hold"
