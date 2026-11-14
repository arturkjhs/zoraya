#!/usr/bin/env python3
"""Compares the replayed grids with a second emulator on the host: tmux.

    compare-tmux.py [NAME...]        (from the repository's root)

For each mark of each recording in crates/zoraya-term/recordings, the bytes up
to the mark are shown in a tmux pane of the recording's size (TERM is
tmux's own; what matters is that tmux parses the same bytes), the pane's text is
captured, and each row is compared, trailing spaces ignored, with the row in the
committed .grids file. A difference is not an error of either side until
someone looks: tmux is not xterm (ADR-0332's open question asks for a second
emulator; this is one, by text only). Exit status 1 when any row differs.
"""
import os
import subprocess
import sys
import tempfile
import time

DIR = "crates/zoraya-term/recordings"


def grids(name):
    marks = []
    text = open(f"{DIR}/{name}.grids").read().split("\n@ ")[1:]
    for sec in text:
        lines = sec.split("\n")
        header = lines[0]
        rows = [l[1:-1] for l in lines[4:] if l.startswith("|")][0::2]
        marks.append((header, [r.rstrip() for r in rows]))
    return marks


# tmux's capture shows the line-drawing set as the ASCII letters that select
# it (it redraws them for its own client); ours is the characters. The same
# letters, for the comparison, and tabs as eight columns.
ACS = {"┌": "l", "─": "q", "┐": "k", "│": "x", "└": "m", "┘": "j", "├": "t", "┤": "u", "┴": "v", "┬": "w",
       "┼": "n", "▒": "a", "◆": "`", "°": "f", "±": "g", "␋": "i", "·": "~", "≥": "z", "≠": "|", "≤": "y",
       "£": "}", "π": "{", "⎺": "o", "⎻": "p", "⎼": "r", "⎽": "s", "▮": "0", "␉": "b", "␌": "c", "␍": "d",
       "␊": "e", "␤": "h", "←": ",", "→": "+", "↑": "-", "↓": "."}


def plain(text):
    return "".join(ACS.get(c, c) for c in text)


def tmux_rows(data, columns, rows):
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "rec")
        open(path, "wb").write(data)
        sock = os.path.join(d, "sock")
        base = ["tmux", "-S", sock, "-f", "/dev/null"]
        subprocess.run(base + ["new-session", "-d", "-x", str(columns), "-y", str(rows), f"cat {path}; sleep 30"], check=True)
        try:
            time.sleep(0.4)
            out = subprocess.run(base + ["capture-pane", "-p", "-t", "0"], capture_output=True, text=True, check=True).stdout
        finally:
            subprocess.run(base + ["kill-server"], capture_output=True)
        lines = out.split("\n")[:rows]
        return [l.rstrip() for l in lines] + [""] * (rows - len(lines))


def main():
    names = sys.argv[1:] or sorted(f[:-4] for f in os.listdir(DIR) if f.endswith(".rec"))
    bad = 0
    total = 0
    for name in names:
        rec = open(f"{DIR}/{name}.rec", "rb").read()
        size = open(f"{DIR}/{name}.marks").readline().split()
        columns, rows = int(size[1]), int(size[2])
        for header, ours in grids(name):
            offset = int(header.split()[0])
            theirs = tmux_rows(rec[:offset], columns, rows)
            total += 1
            diff = [
                (i, a, b)
                for i, (a, b) in enumerate(zip(ours, theirs))
                if a != b and plain(a) != b.expandtabs(8) and plain(a).replace("a", "") != b.replace("a", "")
            ]
            status = "same" if not diff else f"{len(diff)} rows differ"
            print(f"{name} @{header}: {status}")
            for i, a, b in diff[:3]:
                print(f"    row {i}: ours {a!r}\n            tmux {b!r}")
            bad += bool(diff)
    print(f"{total - bad} of {total} marks agree with tmux")
    sys.exit(1 if bad else 0)


main()
