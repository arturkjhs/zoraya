#!/usr/bin/env python3
"""Plan part F6's "done when" (ADR-0415 item 2, ADR-0438, INV-0415-2): reads the log of
tests/posix-core/top.script, where `top tab` was run in a room at four moments around
`t_shared keep`, and checks the room's memory against each other (no number is pinned):

  P0   before any child (L, the launcher, alone with its buffer)
  P3   after three spawns of L's own image
  P4   after one more spawn, of the file
  Pend after everything ended

  a spawn of the image costs the room its child's own pages and the pipe L keeps for it,
  never an image: so it is far less than the child's own job plus the image's pages
  (printed by the program as "an image keeps exactly N bytes"); a spawn of the file costs
  the same and the image's pages, and a few for the record and the tables; and the room
  gets everything back. What the room does not show is the image of the program itself,
  which the shell made: it is charged to the shell's job, outside every room.

`top` prints kilobytes, so the comparisons have a few of slack."""
import re
import sys

log = open(sys.argv[1], errors="replace").read().splitlines()
kept = None
children = None
seen = []  # the room's MEM in K, in order
want = False
for line in log:
    m = re.search(r"an image keeps exactly (\d+) bytes", line)
    if m:
        kept = int(m.group(1))
        want = True
        continue
    m = re.search(r"children's own jobs: (\d+) (\d+) (\d+) (\d+) bytes", line)
    if m:
        children = [int(x) for x in m.groups()]
    if "t_shared: passed" in line:
        want = True
    m = re.search(r"\btab\s+(\d+)K of \d+M", line)
    if m and want:
        seen.append(int(m.group(1)))
        want = False
if kept is None or children is None or len(seen) != 4:
    sys.exit("check-top-image: found kept=%r and %d readings of `top tab`, want 4: %r" % (kept, len(seen), seen))
p0, p3, p4, end = seen
kk = kept / 1024
own = (p3 - p0) / 3
file_child = p4 - p3
print("check-top-image: room memory %dK alone, %dK with three children of one image (%.0fK each), "
      "%dK with a child of the file (+%dK), %dK at the end; an image keeps %dK" % (p0, p3, own, p4, file_child, end, kk))
child = sum(children[:3]) / 3 / 1024
print("check-top-image: a child of the image holds %.0fK in its own job" % child)
errors = []
if own <= 0:
    errors.append("three children added nothing to the room")
# One spawn of the file = one spawn of the image + the image's pages.
if not (kk - 4 <= file_child - own <= kk + 64):
    errors.append("a child of the file cost %dK, a child of the image %.0fK: the difference %.0fK is not the image's %.0fK"
                  % (file_child, own, file_child - own, kk))
# A spawn of the image made no image: what it cost the room beyond the child's own job
# (the pipe) is far less than the image's pages.
child = sum(children[:3]) / 3 / 1024
if own - child > kk / 2:
    errors.append("a spawn of the image cost the room %.0fK, its child's own job holds %.0fK: %.0fK more, an image keeps %.0fK"
                  % (own, child, own - child, kk))
if own < child - 4:
    errors.append("the room holds less (%.0fK) per child than the child's own job (%.0fK)" % (own, child))
if end > p0 + 16:
    errors.append("the room holds %dK at the end, %dK at the start" % (end, p0))
if errors:
    sys.exit("check-top-image: " + "; ".join(errors))
print("check-top-image: ok")
