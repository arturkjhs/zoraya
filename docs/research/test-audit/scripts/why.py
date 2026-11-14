#!/usr/bin/env python3
"""why.py JOBID...: the verdict lines of a failed job's log."""
import re, sys, json
T = re.compile(r'^﻿?\d{4}-\d\d-\d\dT[\d:.]+Z ')
meta = {f['job_id']: f for f in json.load(open('out/failures.json'))}
KEY = re.compile(r'(ZORAYA-TEST fail|user panic|panicked at|timed out|\] FAIL|FAIL:|FAILED|##\[error\]|curl: \(|No space|shutdown signal|exceeded|NoMemory|OverQuota|did not|error:)')
DROP = re.compile(r'(fault_report_for_|harness_reports_|--expect|echo |then |if \[|grep -)')
for jid in sys.argv[1:]:
    f = meta.get(int(jid), {})
    print(f"== job {jid} run {f.get('run')} ({f.get('attempt')}) {f.get('when','')[:16]} {f.get('ref')} {f.get('sha')} `{f.get('job','')[:80]}` {int(f.get('secs') or 0)} s")
    seen = 0
    for line in open(f'logs/fail-{jid}.log', errors='replace'):
        txt = re.sub(r'\x1b\[[0-9;]*m', '', T.sub('', line.rstrip('\n')))
        if KEY.search(txt) and not DROP.search(txt):
            print('   ', txt[:300]); seen += 1
            if seen >= 5: break
