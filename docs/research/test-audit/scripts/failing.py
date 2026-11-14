#!/usr/bin/env python3
"""The first failing lines of every failed job's log (logs/fail-ID.log) -> out/failing-lines.tsv/.md"""
import json, os, re, collections
fails = json.load(open('out/failures.json'))
T = re.compile(r'^﻿?\d{4}-\d\d-\d\dT[\d:.]+Z ')
PAT = re.compile(r'(\] FAIL|ZORAYA-TEST fail|panicked at|timed out|FAIL:|FAILED|error(\[E\d+\])?:|\*\*\* \[|##\[error\]|No space left|Operation was canceled|exceeded the maximum execution time|curl: \(|NoMemory|OverQuota)')
SKIP = re.compile(r'(fault_report_for_|harness_reports_|expected failure|--expect-panic|ZORAYA-TEST fail .* \(expected\)|warning:|echo "\[|FAIL: INV|then echo|\|\| \{ echo|if grep|##\[group\])')
rows = []
for f in sorted(fails, key=lambda f: f['when']):
    p = f"logs/fail-{f['job_id']}.log"
    hits = []
    if os.path.exists(p):
        for line in open(p, errors='replace'):
            txt = T.sub('', line.rstrip('\n'))
            if PAT.search(txt) and not SKIP.search(txt):
                hits.append(re.sub(r'\x1b\[[0-9;]*m', '', txt)[:230])
                if len(hits) >= 4:
                    break
    else:
        hits = ['(no log)']
    f['lines'] = hits
    rows.append(f)
json.dump(rows, open('out/failing-lines.json', 'w'), indent=1)
with open('out/failing-lines.md', 'w') as o:
    for f in rows:
        o.write(f"- {f['when'][:16]} run {f['run']} ({f['attempt']}) {f['ref']} {f['sha']} `{f['job'][:90]}` job {f['job_id']} ({int(f['secs'] or 0)} s)\n")
        for h in f['lines']:
            o.write(f"    - `{h}`\n")
# signature counts
sig = collections.Counter()
for f in rows:
    first = f['lines'][0] if f['lines'] else '(nothing matched)'
    first = re.sub(r'\d+', 'N', first)[:110]
    sig[first] += 1
for s, n in sig.most_common(60):
    print(n, s)
