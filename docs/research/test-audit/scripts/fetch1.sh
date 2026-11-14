#!/bin/sh
# One run attempt's jobs: fetch1.sh RUN ATTEMPT
f=jobs/$1-$2.json
[ -e "$f" ] && exit 0
gh api --paginate "repos/{owner}/{repo}/actions/runs/$1/attempts/$2/jobs?per_page=100" --jq '.jobs[] | {id, name, status, conclusion, started_at, completed_at, created_at, run_attempt, runner_name, steps: [.steps[]? | {name, conclusion, started_at, completed_at}]}' > "$f.tmp.$$" 2>>fetch.err && mv "$f.tmp.$$" "$f"
