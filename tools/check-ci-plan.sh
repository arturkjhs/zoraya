#!/bin/sh
# Tests tools/ci-plan.sh and its helpers on a scratch repository (ADR-0348): a push
# to main plans the document checks only, whatever changed; a candidate runs the
# steps its change touches and the full set only for the kernel, libposix or the
# test runner; a change of the Makefile runs the steps whose recipes or variables
# it changes (tools/ci-makefile-steps.rb); the scheduled run's lookup of the last
# full run (tools/ci-last-full.rb); and the tree proof (tools/ci-tree-proof.rb,
# ADR-0345, kept as a way to show that a commit's tree went green on the full set).
#
#   tools/check-ci-plan.sh
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ci-plan-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
# The scratch repository is $tmp/repo; what the fixtures need (the fake API, its
# answers, the plan's messages) is in $aux, outside it: a commit of the
# scratch repository must not carry it and a checkout must not remove it.
aux="$tmp/aux"
mkdir -p "$aux" "$tmp/repo"
cd "$tmp/repo"
git init -q .
git config user.name ci-plan-test
git config user.email ci-plan-test@localhost
git config commit.gpgsign false
mkdir -p tools docs/adr docs/research/task-02 docs/research/desktop/mockups src
for f in ci-plan.sh plan-lib.sh ci-tests.txt test-map.txt ci-tree-proof.rb ci-makefile-steps.rb ci-last-full.rb ci-nightly-tests.txt ci-plan-step.sh; do cp "$here/tools/$f" tools/; done
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
mkdir -p kernel/src crates/zoraya-posix/src crates/zoraya-posix-extra/src crates/zoraya-proto-posix/src ports/musl tools/test-runner/src
for f in kernel/src/main.rs crates/zoraya-posix/src/lib.rs crates/zoraya-posix-extra/src/lib.rs crates/zoraya-proto-posix/src/lib.rs ports/musl/musl.mk tools/test-runner/src/main.rs; do printf 'x\n' > "$f"; done
# A small Makefile for tools/ci-makefile-steps.rb's cases.
printf '%s\n' '# a comment' '.PHONY: test-boot test-files test-gui test-net other' 'KERNEL := build/k.elf' 'SHARED := -x' 'ONLY_BOOT := b' 'DERIVED = $(ONLY_BOOT)' 'TIMEOUT ?= 180' 'export EXPORTED := 1' 'GEN := build/gen' 'LONGVAR := a \' > Makefile
printf '\tb\n$(KERNEL):\n\tcc -o $@ k\ntest-boot: $(KERNEL)\n\trun $(SHARED) $(ONLY_BOOT)\ntest-files: $(KERNEL)\n\trun $(SHARED) files\ntest-gui:\n\trun gui $(TIMEOUT) $(DERIVED)\ntest-net:\n\trun net $(LONGVAR)\n\trun net2\n\t@$(MAKE) --no-print-directory helper\nhelper:\n\techo h\nother:\n\techo hi\ncheck-x:\n\techo chk\n' >> Makefile
git add -A
git commit -q -m base
base=$(git rev-parse HEAD)

fail=0
step() { # NAME: commit the working tree
	git add -A
	git commit -q -m "$1"
}
reset_to() { git reset -q --hard "$1"; }
ALL=$(sed -n 's/^all="\(.*\)"/\1/p' tools/plan-lib.sh)
njobs=$(grep -c -E '^make ' tools/ci-tests.txt)

# A push to main: the document checks only, whatever changed (ADR-0348).
out=$(PUSH_MAIN=1 sh tools/ci-plan.sh origin/main 2>/dev/null)
for want in '^docs_only=1$' '^tests=\[\]$' '^build=false$' '^doc_checks=.*make adr-check'; do
	echo "$out" | grep -q "$want" || { echo "check-ci-plan: a push to main does not plan the document checks only ($want)" >&2; fail=$((fail + 1)); }
done
# A scheduled run with nothing new: no jobs, no checks.
out=$(CI_PLAN_SKIP="main did not change" sh tools/ci-plan.sh origin/main 2>/dev/null)
for want in '^docs_only=1$' '^tests=\[\]$' '^build=false$' '^doc_checks=$'; do
	echo "$out" | grep -q "$want" || { echo "check-ci-plan: a skipped scheduled run does not plan nothing ($want)" >&2; fail=$((fail + 1)); }
done
out=$(FULL=1 sh tools/ci-plan.sh 2>/dev/null)
echo "$out" | grep -q '^docs_only=0$' && echo "$out" | grep -q '^build=true$' || { echo "check-ci-plan: FULL=1 gives the full set" >&2; fail=$((fail + 1)); }

# A candidate: the full set for the kernel, libposix and the test runner, the steps
# of the map for the rest.
cand_summary() { sh tools/ci-plan.sh "$base" 2>"$aux/plan.err" | sed -n 's/^summary=//p'; }
for f in kernel/src/main.rs crates/zoraya-posix/src/lib.rs crates/zoraya-posix-extra/src/lib.rs ports/musl/musl.mk tools/test-runner/src/main.rs; do
	reset_to "$base"
	mkdir -p "$(dirname "$f")"
	printf 'y\n' >> "$f"
	step full-path
	case "$(cand_summary)" in
	"full set"*": $njobs jobs") ;;
	*) echo "check-ci-plan: a candidate that changes $f runs the full set (got: $(cand_summary))" >&2; fail=$((fail + 1)) ;;
	esac
done
for f in docs/design.md crates/zoraya-proto-posix/src/lib.rs; do
	reset_to "$base"
	printf 'y\n' >> "$f"
	step not-full-path
	case "$(cand_summary)" in
	"full set"*) echo "check-ci-plan: a candidate that changes $f runs the full set" >&2; fail=$((fail + 1)) ;;
	esac
done

# A change of the Makefile: the steps whose recipes or variables it changes.
mk_steps() { # NAME EXPECTED_LINE: what tools/ci-makefile-steps.rb says of the working tree against $base
	got=$(ruby tools/ci-makefile-steps.rb "$base" "$ALL" 2>/dev/null | grep -v '^unit=') || got=ERROR
	if [ "$got" != "$2" ]; then
		echo "check-ci-plan: Makefile: $1: got '$got', expected '$2'" >&2
		fail=$((fail + 1))
	fi
}
mk_edit() { # NAME SED_EXPRESSION: change the Makefile, commit
	reset_to "$base"
	sed -e "$2" Makefile > "$aux/mk" && cp "$aux/mk" Makefile
	step "mk $1"
}
mk_edit recipe 's/) files$/) files2/'
mk_steps "a one-line change of one recipe" "steps: test-files"
mk_edit shared 's/^SHARED := -x/SHARED := -y/'
mk_steps "a variable two recipes use" "steps: test-boot test-files"
mk_edit kernel-recipe 's/cc -o \$@ k/cc -o $@ k2/'
mk_steps "the recipe of a file the steps depend on" "steps: test-boot test-files"
mk_edit timeout 's/^TIMEOUT ?= 180/TIMEOUT ?= 181/'
mk_steps "a variable one recipe uses" "steps: test-gui"
mk_edit derived 's/^ONLY_BOOT := b/ONLY_BOOT := c/'
mk_steps "a variable used through another variable" "steps: test-boot test-gui"
mk_edit continuation 's/^	b$/	c/'
mk_steps "a continuation line of a variable" "steps: test-net"
mk_edit other 's/echo hi/echo ho/'
mk_steps "a recipe that no step and no root reaches is not known to be harmless" "full"
mk_edit check-x 's/echo chk/echo chk2/'
mk_steps "a recipe only a check target reaches" "steps:"
mk_edit make-edge 's/echo h$/echo h2/'
mk_steps "a recipe a step runs through \$(MAKE)" "steps: test-net"
mk_edit make-rename 's/^helper:/helper2:/'
mk_steps "a helper renamed while its caller still names the old one" "full"
mk_edit exported 's/^export EXPORTED := 1/export EXPORTED := 2/'
mk_steps "an exported variable" "full"
mk_edit shell 's/^TIMEOUT ?= 180/SHELL := \/bin\/bash\
TIMEOUT ?= 180/'
mk_steps "SHELL" "full"
mk_edit delheader '/^helper:/d'
mk_steps "a deleted rule header moves its recipe to another rule" "full"
mk_edit backslash 's/^LONGVAR := a \\$/LONGVAR := a/'
mk_steps "a removed trailing backslash" "full"
mk_edit comment 's/^# a comment/# another comment/'
mk_steps "a comment" "steps:"
mk_edit deleted '/run net2/d'
mk_steps "a deleted recipe line" "steps: test-net"
mk_edit phony 's/^\.PHONY: test-boot/.PHONY: other-new test-boot/'
mk_steps ".PHONY" "full"
mk_edit export 's/^TIMEOUT ?= 180/export TIMEOUT\
TIMEOUT ?= 180/'
mk_steps "an export" "full"
mk_edit conditional 's/^other:/ifeq ($(X),1)\
endif\
other:/'
mk_steps "a conditional" "full"
mk_edit include 's/^other:/include x.mk\
other:/'
mk_steps "an include" "full"
# Constructs the parser cannot follow must not read as "no step": each of these changes what test-a does.
adv() { # NAME EXPECTED OLD-TEXT NEW-TEXT (printf formats)
	printf "$3" > "$aux/adv.old"
	printf "$4" > "$aux/adv.new"
	got=$(ruby tools/ci-makefile-steps.rb --files "$aux/adv.old" "$aux/adv.new" "test-a test-b" 2>/dev/null) || got=ERROR
	[ "$got" = "$2" ] || { echo "check-ci-plan: Makefile: $1: got '$got', expected '$2'" >&2; fail=$((fail + 1)); }
}
adv "a substitution reference" "steps: test-a" 'X := a.c\ntest-a:\n\tcc $(X:.c=.o)\ntest-b:\n\tb\n' 'X := b.c\ntest-a:\n\tcc $(X:.c=.o)\ntest-b:\n\tb\n'
adv "a prerequisite through foreach" "steps: test-a" 'L := 1\ntest-a: $(foreach s,$(L),h-$(s))\n\ta\nh-1:\n\tx\ntest-b:\n\tb\n' 'L := 1\ntest-a: $(foreach s,$(L),h-$(s))\n\ta\nh-1:\n\ty\ntest-b:\n\tb\n'
adv "a prerequisite through if" "steps: test-a" 'test-a: $(if $(CI),h,h)\n\ta\nh:\n\tx\ntest-b:\n\tb\n' 'test-a: $(if $(CI),h,h)\n\ta\nh:\n\ty\ntest-b:\n\tb\n'
adv "a computed variable name" "steps: test-a" 'S := a\na_F := 1\ntest-a:\n\tcc $($(S)_F)\ntest-b:\n\tb\n' 'S := a\na_F := 2\ntest-a:\n\tcc $($(S)_F)\ntest-b:\n\tb\n'
adv "an eval template" "full" 'define T\n$(1):\n\tx\nendef\n$(eval $(call T,test-a))\ntest-b:\n\tb\n' 'define T\n$(1):\n\ty\nendef\n$(eval $(call T,test-a))\ntest-b:\n\tb\n'
adv "a variable used only in a target list" "full" 'GEN := g1\ntest-a: g1\n\ta\n$(GEN):\n\tx\ntest-b:\n\tb\n' 'GEN := g2\ntest-a: g1\n\ta\n$(GEN):\n\tx\ntest-b:\n\tb\n'
adv "a make word the tool cannot resolve" "steps: test-a" 'F = $(MAKE) $(1)\ntest-a:\n\t$(call F,x)\n\t$(MAKE) $(X)\nx:\n\ta\ntest-b:\n\tb\n' 'F = $(MAKE) $(1)\ntest-a:\n\t$(call F,x)\n\t$(MAKE) $(X)\nx:\n\tb\ntest-b:\n\tb\n'
adv "a tab comment that continues into a command" "steps: test-a" 'test-a:\n\t# c \\\n\techo one\n' 'test-a:\n\t# c \\\n\techo two\n'
adv "\${MAKE}" "steps: test-a" 'test-a:\n\t+${MAKE} h\nh:\n\tx\ntest-b:\n\tb\n' 'test-a:\n\t+${MAKE} h\nh:\n\ty\ntest-b:\n\tb\n'

# Through the plan: the one recipe's step and the smoke set; not test-net; not the full set.
mk_edit recipe 's/) files$/) files2/'
out=$(sh tools/ci-plan.sh "$base" 2>"$aux/plan.err")
echo "$out" | grep -q '"make test-files"' && ! echo "$out" | grep -q '"make test-net' && echo "$out" | grep -q '^summary=candidate' ||
	{ echo "check-ci-plan: Makefile: the plan of a one-line recipe change is its step and the smoke set" >&2; fail=$((fail + 1)); }
mk_edit phony 's/^\.PHONY: test-boot/.PHONY: other-new test-boot/'
case "$(cand_summary)" in
"full set"*) ;;
*) echo "check-ci-plan: Makefile: a change of .PHONY runs the full set" >&2; fail=$((fail + 1)) ;;
esac
reset_to "$base"


# The tree-proof rule (ADR-0345): a push to main that is not docs-only gets
# the document checks only when the merge commit's tree is the tree of a run of
# the full set that went green, every job present and successful (with its Run
# step), and the commit is a merge of two parents whose first is the previous
# head. The API is a directory of fixtures (a missing file is an API error).
# The run's jobs come from the files of the BASE commit (all_jobs), not from the
# tree under test, so that a list line or a matrix entry the run lacks shows.
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
fix="$aux/fixtures"
fixn=0
fakeapi="$aux/fakeapi"
cat > "$fakeapi" <<'FAKE'
#!/bin/sh
# $1: the path with its query. head_sha=X reads the file <key>_head, page=N
# (N > 1) the file <key>_pN. SLOW=hold never answers (and a child of its group
# would touch $MARKER after 3 s); SLOW=close closes its stdout and does not exit.
case "${SLOW:-}" in
hold) ( sleep 3; : > "$MARKER" ) & exec sleep 60 ;;
close) exec >/dev/null; exec sleep 60 ;;
esac
path=${1%%\?*}
query=${1#"$path"}
suffix=
case "$query" in *head_sha=*) suffix=_head ;; esac
case "$query" in *page=[2-9]*) suffix=_p$(echo "$query" | sed 's/.*page=\([0-9]*\).*/\1/') ;; esac
f="$FIXTURES/$(echo "$path" | tr -c 'A-Za-z0-9\n' _)$suffix"
[ -r "$f" ] || exit 1
cat "$f"
FAKE
chmod +x "$fakeapi"
repo=o/r
key() { echo "repos/$repo/$1" | tr -c 'A-Za-z0-9\n' _; }
all_jobs() { # the job names of a full run at the base commit, one a line
	echo build
	git show "$base:tools/ci-tests.txt" | grep -v -E '^#|^$'
	git show "$base:.github/workflows/test.yml" | sed -n -E 's/^[[:space:]]*- (make .*)$/\1/p'
}
# names on stdin -> the jobs API answer, each job with its Run step (build: make
# ci-build). BAD_JOB gets BAD_CONCLUSION, DROP_JOB is left out, SKIP_STEP has its
# step skipped, DUP_JOB comes twice (success, then failure); PAGE_FROM and
# PAGE_TO keep a range of the jobs (a page).
jobs_json() {
	awk -v bad="${BAD_JOB:-}" -v conc="${BAD_CONCLUSION:-success}" -v drop="${DROP_JOB:-}" \
		-v skip="${SKIP_STEP:-}" -v dup="${DUP_JOB:-}" -v lo="${PAGE_FROM:-1}" -v hi="${PAGE_TO:-100000}" -v total="$TOTAL_JOBS" '
		function job(name, c,   st) {
			st = (name == skip) ? "skipped" : "success"
			gsub(/\\/, "\\\\", name); gsub(/"/, "\\\"", name)
			return sprintf("{\"name\":\"%s\",\"conclusion\":\"%s\",\"steps\":[{\"name\":\"%s\",\"conclusion\":\"%s\"}]}", name, c, (name == "build" ? "make ci-build" : "Run"), st)
		}
		BEGIN { printf "{\"total_count\":%d,\"jobs\":[", total }
		$0 == drop { next }
		{ n++; if (n < lo || n > hi) next
		  printf "%s%s", (m++ ? "," : ""), job($0, ($0 == bad ? conc : "success"))
		  if ($0 == dup) printf ",%s", job($0, "failure") }
		END { print "]}" }'
}
TOTAL_JOBS=$(all_jobs | wc -l | tr -d ' ')
# NAME HEAD_SHA CONCLUSION [EVENT]: one run, id 100 (the variables of jobs_json)
fixture() {
	fixn=$((fixn + 1))
	[ ! -d "$fix" ] || mv "$fix" "$fix.old.$fixn"
	mkdir -p "$fix"
	printf '{"workflow_runs":[{"id":100,"event":"%s","conclusion":"%s","head_sha":"%s"}]}\n' "${4:-push}" "$3" "$2" > "$fix/$(key 'actions/workflows/test.yml/runs')"
	all_jobs | jobs_json > "$fix/$(key 'actions/runs/100/jobs')"
	unset BAD_JOB BAD_CONCLUSION DROP_JOB SKIP_STEP DUP_JOB PAGE_FROM PAGE_TO # an assignment before a function call may stay set
	: "$1"
}
# NAME EXPECTED_docs_only [BEFORE]: the proof of $merge (1: proven, 0: not); $proof_env is added to the environment;
# $reason, with 0, is text the proof's message must hold (a negative fixture fails for its own reason)
proof_expect() {
	out=$(env CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 BEFORE=${3:-$base} \
		SHA=$merge ${proof_env:-} ruby tools/ci-tree-proof.rb 2>"$aux/plan.err" </dev/null) && got=1 || got=0
	if [ "$got" != "$2" ]; then
		echo "check-ci-plan: tree proof: $1: proven=$got, expected $2" >&2
		grep "ci-" "$aux/plan.err" | head -8 >&2
		fail=$((fail + 1))
	elif [ "$2" = 0 ] && [ -n "${reason:-}" ] && ! grep -q -F -- "$reason" "$aux/plan.err"; then
		echo "check-ci-plan: tree proof: $1: not proven, but not for the reason: $reason" >&2
		grep "ci-" "$aux/plan.err" | head -8 >&2
		fail=$((fail + 1))
	elif [ "$2" = 1 ]; then
		echo "$out" | grep -q '^proof_run=100$' ||
			{ echo "check-ci-plan: tree proof: $1: the proof names the run" >&2; fail=$((fail + 1)); }
	fi
	reason=
}
[ "$(git rev-parse "$merge^{tree}")" = "$(git rev-parse "$cand^{tree}")" ] ||
	{ echo "check-ci-plan: the fixture's merge and candidate trees differ" >&2; fail=$((fail + 1)); }
runs_file() { echo "$fix/$(key 'actions/workflows/test.yml/runs')"; }
jobs_file() { echo "$fix/$(key 'actions/runs/100/jobs')"; }
gone() { mv "$1" "$1.gone"; } # an API error for that answer

fixture ok "$cand" success
proof_expect "same tree, every job, success" 1
reason="is not the previous head" proof_expect "first parent is not the previous head" 0 "$cand"
fixture failed "$cand" failure
reason="no green full run on tree" proof_expect "the run failed" 0
fixture other "$cand2" success
reason="no green full run on tree" proof_expect "a different tree" 0
DROP_JOB="make test-boot" fixture ok "$cand" success
reason="first: make test-boot" proof_expect "same tree, a job missing" 0
BAD_JOB="make test-boot" BAD_CONCLUSION=skipped fixture ok "$cand" success
reason="first: make test-boot" proof_expect "same tree, a job skipped" 0
BAD_JOB=build BAD_CONCLUSION=failure fixture ok "$cand" success
reason="first: build" proof_expect "same tree, the build failed" 0
SKIP_STEP="make adr-check" fixture ok "$cand" success
reason="first: make adr-check" proof_expect "a checks job whose Run step was skipped" 0
SKIP_STEP="make test-boot" fixture ok "$cand" success
reason="first: make test-boot" proof_expect "a tests job whose Run step was skipped" 0
SKIP_STEP=build fixture ok "$cand" success
reason="first: build" proof_expect "a build job whose make ci-build step was skipped" 0
DUP_JOB="make test-boot" fixture ok "$cand" success
reason="first: make test-boot" proof_expect "two jobs of one name, success then failure" 0
fixture ok "$cand" success
gone "$(jobs_file)"
reason="not the full set" proof_expect "API error reading the jobs" 0
fixture ok "$cand" success
gone "$(runs_file)"
reason="no list of runs can be read" proof_expect "API error listing the runs" 0
# The jobs come in two pages: all of them are read.
half=$((TOTAL_JOBS / 2))
PAGE_TO=$half fixture ok "$cand" success
all_jobs | PAGE_FROM=$((half + 1)) jobs_json > "$fix/$(key 'actions/runs/100/jobs')_p2"
proof_expect "the jobs in two pages" 1
gone "$fix/$(key 'actions/runs/100/jobs')_p2"
reason="not the full set" proof_expect "the second page missing" 0
# Events: a pull request's run was tested on the merge ref, an unknown event is
# not a known way to plan the full set.
fixture ok "$cand" success pull_request
reason="no green full run on tree" proof_expect "a pull_request run" 0
fixture ok "$cand" success pull_request_target
reason="no green full run on tree" proof_expect "an unknown event" 0
for ev in workflow_dispatch schedule; do
	fixture ok "$cand" success "$ev"
	proof_expect "a $ev run" 1
done
# A head commit this checkout does not have: its tree comes from the API.
unknown=1111111111111111111111111111111111111111
fixture ok "$unknown" success
reason="no green full run on tree" proof_expect "an unknown head, no tree from the API" 0
printf '{"tree":{"sha":"%s"}}\n' "$(git rev-parse "$merge^{tree}")" > "$fix/$(key "git/commits/$unknown")"
proof_expect "an unknown head, the API's tree is the same" 1
printf '{"tree":{"sha":"%s"}}\n' "$(git rev-parse "$cand2^{tree}")" > "$fix/$(key "git/commits/$unknown")"
reason="no green full run on tree" proof_expect "an unknown head, the API's tree differs" 0
# The runs of the second parent are asked for by head_sha first: a candidate
# older than the 100 latest runs is found, and the scan may fail.
fixture ok "$cand" success
mv "$(runs_file)" "$(runs_file)_head"
proof_expect "found by head_sha, the scan of the latest runs fails" 1
# A deadline: an API that does not answer (SLOW=hold: a child that holds the
# pipe for 60 s, with a grandchild of its group that would touch the marker after
# 3 s), or closes its stdout and does not exit (SLOW=close), gives the full set
# in a few seconds, not after 60 s, and the group is killed.
for mode in hold close; do
	fixture ok "$cand" success
	marker="$aux/survived"
	start=$(date +%s)
	proof_env="SLOW=$mode MARKER=$marker CI_TREE_PROOF_CALL_TIMEOUT=1 CI_TREE_PROOF_TIMEOUT=2" reason="did not answer" proof_expect "an API that does not answer ($mode)" 0
	[ $(($(date +%s) - start)) -le 10 ] || { echo "check-ci-plan: tree proof: the deadline did not stop a call ($mode)" >&2; fail=$((fail + 1)); }
done
proof_env=
sleep 4
[ ! -e "$marker" ] || { echo "check-ci-plan: tree proof: the process group of a call was not killed" >&2; fail=$((fail + 1)); }
# The run is never its own proof.
fixture ok "$cand" success
if CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=100 BEFORE=$base SHA=$merge ruby tools/ci-tree-proof.rb >/dev/null 2>&1; then
	echo "check-ci-plan: tree proof: a run proves itself" >&2; fail=$((fail + 1))
fi
# Not a merge: the candidate's own commit, one parent.
fixture ok "$cand" success
if CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 BEFORE=$base SHA=$cand ruby tools/ci-tree-proof.rb >/dev/null 2>&1; then
	echo "check-ci-plan: tree proof: a one-parent commit is proven" >&2; fail=$((fail + 1))
fi
# By hand (the checkout is another commit): the names are those of the commit
# asked about (git show), not of the checkout. BY_HAND_NAME COMMIT REASON; the
# run, from fixture(), has the base commit's jobs.
by_hand() {
	git checkout -q -f -B tp "$base"
	fixture ok "$2" success
	if env CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo CI_TREE_PROOF_ANY_COMMIT=1 SHA=$2 ruby tools/ci-tree-proof.rb > /dev/null 2> "$aux/hand.err"; then
		echo "check-ci-plan: tree proof: by hand, $1: names read from the checkout, not the commit" >&2; fail=$((fail + 1))
	elif ! grep -q -F -- "$3" "$aux/hand.err"; then
		echo "check-ci-plan: tree proof: by hand, $1: not proven, but not for the reason: $3" >&2; fail=$((fail + 1))
	fi
}

# The job names are those of the tree's own files: a tree whose list has a line
# the run lacks, and one whose checks matrix has an entry it lacks, are not proven.
git checkout -q -f -B tp "$base"
git checkout -q -b cand3
printf 'make test-extra\n' >> tools/ci-tests.txt
step list-line
cand3=$(git rev-parse HEAD)
git checkout -q tp
git merge -q --no-ff -m merge-cand3 cand3
merge=$(git rev-parse HEAD)
fixture ok "$cand3" success
reason="first: make test-extra" proof_expect "a list line the run lacks" 0
by_hand "a list line" "$cand3" "first: make test-extra"

git checkout -q -f -B tp "$base"
git checkout -q -b cand4
sed 's/^\(  *\)- make fmt-check$/&\n\1- make check-extra/' .github/workflows/test.yml > "$aux/wf" && cp "$aux/wf" .github/workflows/test.yml
grep -q 'make check-extra' .github/workflows/test.yml || { echo "check-ci-plan: fixture: no matrix entry added" >&2; fail=$((fail + 1)); }
step matrix-entry
cand4=$(git rev-parse HEAD)
git checkout -q tp
git merge -q --no-ff -m merge-cand4 cand4
merge=$(git rev-parse HEAD)
fixture ok "$cand4" success
reason="first: make check-extra" proof_expect "a checks matrix entry the run lacks" 0
by_hand "a matrix entry" "$cand4" "first: make check-extra"
# An empty list proves nothing.
git checkout -q -f -B tp "$base"
git checkout -q -b cand5
: > tools/ci-tests.txt
step empty-list
cand5=$(git rev-parse HEAD)
git checkout -q tp
git merge -q --no-ff -m merge-cand5 cand5
merge=$(git rev-parse HEAD)
fixture ok "$cand5" success
reason="the full list is empty" no_build_check=1 proof_expect "an empty list of tests" 0
no_build_check=
# A merge whose tree differs from the candidate's (main moved: more than the merge).
git checkout -q -f -B tp "$base"
git checkout -q -b moved
printf 'm\n' >> docs/design.md
step main-moved
before_moved=$(git rev-parse HEAD)
git merge -q --no-ff -m merge-moved cand
merge=$(git rev-parse HEAD)
fixture ok "$cand" success
reason="no green full run on tree" proof_expect "main moved: the merge's tree is not the candidate's" 0 "$before_moved"

# The scheduled run's lookup of the last full run (tools/ci-last-full.rb, ADR-0348).
# last_full RUNS_JSON JOBS...: the API answers a list of runs and, for each run id
# given as ID:BUILD_CONCLUSION, its jobs (a skipped build is a run that ran nothing).
lastfull() { # CONCLUSION-LIST: lines "ID CONCLUSION SHA BUILD"
	fixn=$((fixn + 1))
	[ ! -d "$fix" ] || mv "$fix" "$fix.old.$fixn"
	mkdir -p "$fix"
	awk 'BEGIN { printf "{\"workflow_runs\":[" } { printf "%s{\"id\":%s,\"conclusion\":\"%s\",\"head_sha\":\"%s\"}", (NR > 1 ? "," : ""), $1, $2, $3 } END { print "]}" }' > "$fix/$(key 'actions/workflows/test.yml/runs')" < "$aux/runs.txt"
	while read -r id _ _ build; do
		printf '{"jobs":[{"name":"plan","conclusion":"success"},{"name":"build","conclusion":"%s"}]}\n' "$build" > "$fix/$(key "actions/runs/$id/jobs")"
	done < "$aux/runs.txt"
}
lf() { # NAME EXPECTED_OUTPUT_LINE [SHA]: run the lookup; $2 must be one line of its output (or FAIL)
	if out=$(env CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 SHA=${3:-aaaa} ruby tools/ci-last-full.rb 2>"$aux/lf.err"); then
		[ "$2" != FAIL ] && echo "$out" | grep -q -x -F "$2" || { echo "check-ci-plan: last full run: $1: expected $2, got: $(echo "$out" | tr '\n' ' ')" >&2; fail=$((fail + 1)); }
	else
		[ "$2" = FAIL ] || { echo "check-ci-plan: last full run: $1: failed: $(cat "$aux/lf.err")" >&2; fail=$((fail + 1)); }
	fi
}
printf '%s\n' '201 success aaaa success' '200 success bbbb success' > "$aux/runs.txt"; lastfull
lf "the latest full run is on this commit: skip" skip=1
lf "a newer main: run" skip=0 cccc
printf '%s\n' '203 success aaaa skipped' '202 cancelled aaaa success' '201 failure aaaa success' '200 success bbbb success' > "$aux/runs.txt"; lastfull
lf "skipped and cancelled runs do not count, a red run on this commit does" skip=1
lf "the green run is the older one" green_sha=bbbb
printf '%s\n' '203 success dddd skipped' '202 success cccc success' > "$aux/runs.txt"; lastfull
lf "a run that skipped its build is not the last full run" skip=0 dddd
lf "a run that skipped its build is not the last full run (its predecessor is)" last_sha=cccc dddd
printf '%s\n' '203 success aaaa skipped' > "$aux/runs.txt"; lastfull
lf "no full run at all: run" skip=0
printf '%s\n' '201 success aaaa success' > "$aux/runs.txt"; lastfull
gone "$(runs_file)"
lf "an API error listing the runs: unknown, run" FAIL
lastfull
rm "$fix/$(key 'actions/runs/201/jobs')"
lf "an API error reading the jobs: unknown, run" FAIL
out=$(ruby tools/ci-last-full.rb --merges "$base" 2>/dev/null)
echo "$out" | grep -q 'merge-moved' || { echo "check-ci-plan: last full run: the merges since the last green run are listed" >&2; fail=$((fail + 1)); }
out=$(ruby tools/ci-last-full.rb --merges 1234567890abcdef1234567890abcdef12345678 2>/dev/null)
echo "$out" | grep -q 'No green full run is known' || { echo "check-ci-plan: last full run: an unknown last green run says so" >&2; fail=$((fail + 1)); }

# --green-for: the stage-tag rule (a green full run whose head has the tag's commit as an ancestor).
printf '%s\n' '203 success '"$merge"' skipped' '202 failure '"$merge"' success' '201 success '"$cand"' success' > "$aux/runs.txt"; lastfull
out=$(env CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 ruby tools/ci-last-full.rb --green-for "$base" 2>/dev/null) && echo "$out" | grep -q '^green_run=201$' ||
	{ echo "check-ci-plan: green-for: a skipped run and a red run do not count, an older green run on a descendant does" >&2; fail=$((fail + 1)); }
if env CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 ruby tools/ci-last-full.rb --green-for "$base" >/dev/null 2>&1; then :; else
	echo "check-ci-plan: green-for: $base is an ancestor of the green run's head" >&2; fail=$((fail + 1))
fi
if env CI_API="$fakeapi" FIXTURES="$fix" GITHUB_REPOSITORY=$repo RUN_ID=999 ruby tools/ci-last-full.rb --green-for "$merge" >/dev/null 2>&1; then
	echo "check-ci-plan: green-for: the merge is only on a red run and a skipped run, not on a green one" >&2; fail=$((fail + 1))
fi
printf '%s\n' '201 success '"$cand"' success' > "$aux/runs.txt"; lastfull
lf "last_conclusion of a green run" last_conclusion=success
printf '%s\n' '201 failure '"$cand"' success' > "$aux/runs.txt"; lastfull
lf "last_conclusion of a red run" last_conclusion=failure

# The plan step (tools/ci-plan-step.sh): what each event plans.
fakelast="$aux/fakelast"
cat > "$fakelast" <<'FAKE'
#!/bin/sh
[ "${1:-}" = --merges ] && { echo "merges since ${2:-none}"; exit 0; }
[ -z "${FAKE_FAIL:-}" ] || exit 1
cat "$FAKE_OUT"
FAKE
chmod +x "$fakelast"
step_plan() { # NAME EXPECTED_EXIT EXPECTED_REGEX; env of the event in $ev
	env CI_LAST_FULL="$fakelast" GITHUB_STEP_SUMMARY="$aux/summary" GITHUB_SHA=headsha SCHEDULE="${SCHEDULE:-}" $ev sh tools/ci-plan-step.sh > "$aux/step.out" 2>"$aux/step.err" && rc=0 || rc=$?
	[ "$rc" = "$2" ] || { echo "check-ci-plan: plan step: $1: exit $rc, expected $2" >&2; fail=$((fail + 1)); }
	grep -q -E "$3" "$aux/step.out" || { echo "check-ci-plan: plan step: $1: the plan lacks $3" >&2; fail=$((fail + 1)); }
}
: > "$aux/summary"
printf 'skip=1\nlast_run=7\nlast_sha=headsha\nlast_conclusion=success\ngreen_sha=g1\n' > "$aux/last.skip.green"
printf 'skip=1\nlast_run=7\nlast_sha=headsha\nlast_conclusion=failure\n' > "$aux/last.skip.red"
printf 'skip=0\nlast_run=7\nlast_sha=old\nlast_conclusion=failure\n' > "$aux/last.new"
ev="GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main" step_plan "a push to main" 0 '^docs_only=1$'
ev="GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/work/x" FAKE_OUT="$aux/last.new" step_plan "a push to a branch plans a candidate (no merge base here: full)" 0 '^tests='
ev="GITHUB_EVENT_NAME=schedule GITHUB_REF=refs/heads/main FAKE_OUT=$aux/last.skip.green SCHEDULE=4h" step_plan "the 4-hourly run, nothing new, last run green: nothing runs, green" 0 '^summary=nothing to run'
ev="GITHUB_EVENT_NAME=schedule GITHUB_REF=refs/heads/main FAKE_OUT=$aux/last.skip.red SCHEDULE=4h" step_plan "the 4-hourly run, nothing new, last run red: the run is red" 1 '^summary=nothing to run'
grep -q 'is red' "$aux/step.err" "$aux/summary" 2>/dev/null || grep -q 'is red' "$aux/step.out" "$aux/summary" || { echo "check-ci-plan: plan step: a red skip does not say why" >&2; fail=$((fail + 1)); }
ev="GITHUB_EVENT_NAME=schedule GITHUB_REF=refs/heads/main FAKE_OUT=$aux/last.new SCHEDULE=4h" step_plan "the 4-hourly run, main moved: the full set" 0 '^summary=full set: [0-9]+ jobs'
ev="GITHUB_EVENT_NAME=schedule GITHUB_REF=refs/heads/main FAKE_FAIL=1 SCHEDULE=4h" step_plan "the 4-hourly run, the lookup fails: the full set" 0 '^summary=full set: [0-9]+ jobs'
nightly_cron=$(sed -n "s/^NIGHTLY_CRON='\(.*\)'$/\1/p" tools/ci-plan-step.sh)
ev="GITHUB_EVENT_NAME=schedule GITHUB_REF=refs/heads/main FAKE_OUT=$aux/last.skip.green" SCHEDULE="$nightly_cron" step_plan "the nightly never skips" 0 "nightly's own jobs"
ev="GITHUB_EVENT_NAME=workflow_dispatch GITHUB_REF=refs/heads/work/x FULL_INPUT=true" step_plan "a manual full run" 0 '^summary=full set: [0-9]+ jobs'
grep -q 'merges since' "$aux/summary" || { echo "check-ci-plan: plan step: the summary of a scheduled run lists the merges" >&2; fail=$((fail + 1)); }

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
echo "check-ci-plan: the plans, the Makefile mapping, the scheduled lookup and the tree proof hold"
