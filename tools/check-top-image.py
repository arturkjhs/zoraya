#!/usr/bin/env python3
"""Plan part F6's "done when" (ADR-0415 item 2, ADR-0438, INV-0415-2): reads the log of
tests/posix-core/top.script, where `top tab` was run in a room around `t_shared keep`,
twice, and checks the room's memory against itself (no number is pinned). Per run:

  P0   before any child (L, the launcher, alone with its buffer; its job settled)
  P3   after three spawns of L's own image
  P4   after one more spawn, of the file
  Pend after everything ended (the last of three readings: the reaper has finished)

  a spawn of the image costs the room its child's own pages and the pipe L keeps for it,
  never an image: so it is far less than the child's own job plus the image's pages
  (printed by the program as "an image keeps exactly N bytes"); a spawn of the file costs
  the same and the image's pages, and a few for the record and the tables. The room also
  shows the image of the program itself, which the shell made for it (plan part F10, proposed
  ADR-0443: it is charged to the room's job, not the shell's); it is in P0 and in every later
  reading alike, so none of the differences above sees it.

  The room gets everything back: each run ends within 16K of the empty room's reading, and
  the second run reads as the first did, so an image or a transfer buffer kept after a run
  (956K, 1024K) fails.

`top` prints kilobytes, so the comparisons have a few of slack."""
import re
import sys

log = open(sys.argv[1], errors="replace").read().splitlines()
runs = []        # one dict per run: marker -> last reading in K
kept = None
l_job = None
children = None
marker = "start"
cur = {"start": []}
for line in log:
    m = re.search(r"an image keeps exactly (\d+) bytes(?:; L's job holds (\d+) bytes)?", line)
    if m:
        kept = int(m.group(1))
        if m.group(2) and l_job is None:
            l_job = int(m.group(2))
    if "keep: L alone" in line:
        runs.append({})
        marker = "p0"
        continue
    if "keep: three children" in line:
        marker = "p3"
        continue
    if "keep: and one child" in line:
        marker = "p4"
        continue
    m = re.search(r"children's own jobs: (\d+) (\d+) (\d+) (\d+) bytes", line)
    if m:
        children = [int(x) for x in m.groups()]
    if "t_shared: passed" in line:
        marker = "end"
        continue
    m = re.search(r"\btab\s+(\d+)K of \d+M", line)
    if m:
        if not runs:
            cur["start"].append(int(m.group(1)))
        else:
            runs[-1][marker] = int(m.group(1))   # the last reading of the marker
if kept is None or children is None or len(runs) != 2 or not cur["start"] \
        or any(set(r) != {"p0", "p3", "p4", "end"} for r in runs):
    sys.exit("check-top-image: found kept=%r, children=%r, runs=%r, start=%r" % (kept, children, runs, cur["start"]))
empty = cur["start"][-1]
kk = kept / 1024
child = sum(children[:3]) / 3 / 1024
errors = []
# The positive control of the shell's charge (plan part F10, INV-0443-5): the room holds the image
# of the program the shell started in it. What a room holds at P0 is at least the launcher's own job,
# a child's own job (the driver is the same program) and an image's pages; a shell that charged the
# image to its own job leaves the room about an image short (845K here).
if l_job is None:
    errors.append("no 'L's job holds' reading: the program does not print it")
else:
    need = (l_job + children[0]) / 1024 + kk * 0.75
    for n, r in enumerate(runs, 1):
        if r["p0"] < need:
            errors.append("run %d: the room reads %dK alone, less than L's job (%.0fK) and a child's (%.0fK) with three quarters of the image (%.0fK) it holds: the program's image is not charged to the room"
                          % (n, r["p0"], l_job / 1024, children[0] / 1024, kk * 0.75))
for n, r in enumerate(runs, 1):
    own = (r["p3"] - r["p0"]) / 3
    file_child = r["p4"] - r["p3"]
    print("check-top-image: run %d: room %dK alone, %dK with three children of one image (%.0fK each), "
          "%dK with a child of the file (+%dK), %dK at the end; an image keeps %dK; empty room %dK"
          % (n, r["p0"], r["p3"], own, r["p4"], file_child, r["end"], kk, empty))
    if own <= 0:
        errors.append("run %d: three children added nothing to the room" % n)
    if not (kk - 4 <= file_child - own <= kk + 64):
        errors.append("run %d: a child of the file cost %dK, a child of the image %.0fK: the difference %.0fK is not the image's %.0fK"
                      % (n, file_child, own, file_child - own, kk))
    if own - child > kk / 2:
        errors.append("run %d: a spawn of the image cost the room %.0fK, its child's own job holds %.0fK: %.0fK more, an image keeps %.0fK"
                      % (n, own, child, own - child, kk))
    if own < child - 4:
        errors.append("run %d: the room holds less (%.0fK) per child than the child's own job (%.0fK)" % (n, own, child))
print("check-top-image: a child of the image holds %.0fK in its own job" % child)
a, b = runs
# The room gets everything back: each run ends where the room began, and the
# second run is the first again, so an image or a buffer kept (956K, 1024K) fails.
for n, r in enumerate(runs, 1):
    if r["end"] > empty + 16:
        errors.append("run %d: the room holds %dK at the end, %dK when it was empty" % (n, r["end"], empty))
if b["p0"] > a["p0"] + 16 or b["end"] > a["end"] + 16:
    errors.append("the room grew across runs: %dK then %dK at the end, %dK then %dK alone"
                  % (a["end"], b["end"], a["p0"], b["p0"]))
if errors:
    sys.exit("check-top-image: " + "; ".join(errors))
print("check-top-image: ok")
