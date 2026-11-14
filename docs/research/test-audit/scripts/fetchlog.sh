#!/bin/sh
f=logs/fail-$1.log
[ -s "$f" ] && exit 0
gh api --allow-escape-sequences "repos/{owner}/{repo}/actions/jobs/$1/logs" > "$f.tmp" 2>>fetchlog.err && mv "$f.tmp" "$f" || rm -f "$f.tmp"
