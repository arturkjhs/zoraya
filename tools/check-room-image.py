#!/usr/bin/env python3
"""Plan part F10 (proposed ADR-0443, INV-0443-5): reads the log of tests/posix-core/room-image.script,
where a room at 1M was asked twice to start a program whose image is larger than the room
(`/bin/t_unix pair`), and checks that the shell said "over quota" both times and that the
room's row of `top tight` is the same before, between and after the two refusals: a refusal at
the image charged the room nothing, and left nothing in it. No number is pinned; `top` prints
kilobytes, so the readings are compared to each other. The room's own count of refused memory
requests goes 1, 2, 2 over the last three readings: the refusals were the room's."""
import re
import sys

log = open(sys.argv[1], errors="replace").read()
refused = len(re.findall(r"t_unix[^\n]*over quota|over quota[^\n]*", log))
rows = [int(m.group(1)) for m in re.finditer(r"\btight\s+(\d+)K of 1M", log)]
if len(rows) < 4:
    sys.exit("check-room-image: found %d readings of the room's row, wanted 4: %r" % (len(rows), rows))
if "t_unix: pair passed" not in log:
    sys.exit("check-room-image: the same program did not start in the roomy room")
if refused < 2:
    sys.exit("check-room-image: the shell said over quota %d times, wanted 2" % refused)
counts = [int(x) for x in re.findall(r"\(refused (\d+) mem\)", log)]
if counts[:3] != [1, 2, 2]:
    sys.exit("check-room-image: the room's own refusal counts were %r, wanted 1, 2, 2" % counts)
print("check-room-image: the room's row read %s K around two refusals" % rows[:4])
if len(set(rows[:4])) != 1:
    sys.exit("check-room-image: the room's row changed across the refusals: %r" % rows[:4])
