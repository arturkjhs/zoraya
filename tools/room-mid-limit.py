#!/usr/bin/env python3
"""Plan part F10 (proposed ADR-0443, INV-0443-5): prints the memory limit, in kilobytes with a
K, of the room tests/posix-core/room-mid.script makes: the room's reading with `t_shared keep`
and its launcher alone (the first "L alone" of the log of tests/posix-core/top.script), less
half of what an image keeps (printed by the program). No number is pinned: both come from the
same run."""
import re
import sys

log = open(sys.argv[1], errors="replace").read().splitlines()
kept = None
marker = False
reading = None
for line in log:
    m = re.search(r"an image keeps exactly (\d+) bytes", line)
    if m:
        kept = int(m.group(1))
    if "keep: L alone" in line:
        marker = True
        continue
    if marker:
        m = re.search(r"\btab\s+(\d+)K of \d+M", line)
        if m:
            reading = int(m.group(1))
            break
if kept is None or reading is None:
    sys.exit("room-mid-limit: kept=%r reading=%r" % (kept, reading))
limit = reading - kept // 2048
if limit <= kept // 1024:
    sys.exit("room-mid-limit: limit %dK is not above an image (%dK)" % (limit, kept // 1024))
print("%dK" % limit)
