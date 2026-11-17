#!/bin/sh
# The plan job's step (.github/workflows/test.yml), as a script so that
# `make check-ci-plan` can test it (ADR-0348): turns the event into the plan.
# Prints the plan's key=value lines on stdout (for $GITHUB_OUTPUT), writes the
# summary to $GITHUB_STEP_SUMMARY, the reasoning to stderr.
#
#   GITHUB_EVENT_NAME GITHUB_REF GITHUB_SHA  the event
#   FULL_INPUT        `true` for a manual run with full=true
#   SCHEDULE          github.event.schedule: the cron of a scheduled run
#   CI_LAST_FULL      the lookup (default tools/ci-last-full.rb)
#
# - a push to main: the document checks only (PUSH_MAIN=1);
# - a scheduled run: the full set; the nightly (its cron is NIGHTLY_CRON below,
#   which `make check-ci-jobs` compares with the workflow's first cron) always
#   with its own jobs; the 2-hourly run only if the lookup does not say that the
#   latest full run was on this very commit (an answer it cannot give is no
#   answer: the full set runs). If it skips and that latest run was red, it
#   exits 1 (the run is red, with the reason in the summary), so that a skipping
#   run does not hide a red full run;
# - a manual full=true run and a tag candidate-full/NAME: the full set;
# - anything else (a candidate, a manual run without full): the plan of the change.
set -u
NIGHTLY_CRON='17 2 * * *'
lastfull=${CI_LAST_FULL:-tools/ci-last-full.rb}
summary_file=${GITHUB_STEP_SUMMARY:-/dev/null}
event=${GITHUB_EVENT_NAME:-}
ref=${GITHUB_REF:-}

full=0
case "$ref" in refs/tags/candidate-full/*) full=1 ;; esac
[ "${FULL_INPUT:-}" = true ] && full=1
nightly=0
skip=
red=
last=
if [ "$event" = schedule ]; then
    full=1
    last=$($lastfull) || last=
    if [ "${SCHEDULE:-}" = "$NIGHTLY_CRON" ]; then
        nightly=1
    elif echo "$last" | grep -q '^skip=1$'; then
        skip="main is at ${GITHUB_SHA:-HEAD}, the commit of the last full scheduled run ($(echo "$last" | sed -n 's/^last_run=//p'))"
        if [ "$(echo "$last" | sed -n 's/^last_conclusion=//p')" = failure ]; then
            red="the last full run on main (run $(echo "$last" | sed -n 's/^last_run=//p')) is red and main has not changed since: this run fails with it, so that a skipped run does not read as green"
        fi
    fi
fi

# A failing plan is a failing step: a run that planned nothing must not read as green.
planned=0
if [ "$event" = push ] && [ "$ref" = refs/heads/main ]; then
    PUSH_MAIN=1 tools/ci-plan.sh origin/main > plan.txt && planned=1
elif [ -n "$skip" ]; then
    CI_PLAN_SKIP="$skip" tools/ci-plan.sh origin/main > plan.txt && planned=1
else
    NIGHTLY=$nightly FULL=$full tools/ci-plan.sh origin/main > plan.txt && planned=1
fi
if [ "$planned" != 1 ] || ! grep -q '^tests=' plan.txt; then
    echo "::error::tools/ci-plan.sh failed or gave no plan" >&2
    exit 1
fi
cat plan.txt
grep '^summary=' plan.txt | sed 's/^summary=//' >> "$summary_file"
if [ "$event" = schedule ]; then
    # The commits since the last green full run: what a red run may be blamed on.
    { echo; $lastfull --merges "$(echo "$last" | sed -n 's/^green_sha=//p')"; } >> "$summary_file"
fi
if [ -n "$red" ]; then
    echo "::error::$red" >&2
    echo "$red" >> "$summary_file"
    exit 1
fi
exit 0
