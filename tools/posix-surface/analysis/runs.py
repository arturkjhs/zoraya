"""Shared definitions: which workload runs count for which tier, and how executables map to programs."""
import os, glob, re

TIERS = ["T0", "T1", "T2", "T3"]
WL_TIER = {"W5": "T0", "W1": "T1", "W2": "T1", "W3": "T1", "W4": "T2", "W6": "T3"}

# Runs that are not part of the minimal configuration of their tier.
#   comparison: the same workload with an alternative program (pdpmake instead of GNU make, other shells)
#   probe: a deliberately failing attempt that records a missing tool (no separate ld, no ar)
COMPARISON = re.compile(r"^(W1-make-pdpmake|W2-(make|check)-p|W4p-.*|W5-(oksh|mrsh|busybox))$")
PROBE = re.compile(r"^(W4-configure-noLD|W4-configure-noAR|W4-make-noAR)$")

def run_kind(fid):
    if COMPARISON.match(fid):
        return "comparison"
    if PROBE.match(fid):
        return "probe"
    return "core"

def wl_of(fid):
    return fid.split("-")[0].replace("W4p", "W4")

def tier_of_run(fid):
    return WL_TIER[wl_of(fid)]

def tiers_upto(t):
    return TIERS[: TIERS.index(t) + 1] if t != "T3" else ["T3"]

class Mapper:
    def __init__(self, work):
        self.pfx = os.path.join(work, "prefix")
        self.run = os.path.join(work, "run")
        self.real = {os.path.realpath(b): os.path.basename(b) for b in glob.glob(os.path.join(self.pfx, "bin", "*"))}
        self.binmap = {}
        ov = os.path.join(work, "results", "bin-overrides.tsv")
        if os.path.exists(ov):
            for line in open(ov):
                parts = line.rstrip("\n").split("\t")
                if len(parts) == 3:
                    self.binmap[parts[1]] = parts[2]

    def resolve(self, path):
        """Executable path as seen inside a workload namespace -> real file outside it."""
        if path in self.binmap:
            return self.binmap[path]
        return os.path.realpath(path) if os.path.exists(path) else path

    def program(self, arg0, exe):
        name = self.real.get(exe)
        a0 = os.path.basename(arg0) if arg0 else ""
        if name in ("toybox", "busybox"):
            return f"{name}:{a0}" if a0 and a0 != name else name
        if name:
            return name
        b = os.path.basename(exe)
        if "/llvm" in exe and b.startswith("clang"):
            return "clang"
        if b in ("lld", "ld.lld") or "/lld" in exe:
            return "ld.lld"
        if exe.startswith(self.run) or exe.startswith("./") or exe.startswith("../"):
            return "built:" + b
        return "system:" + exe

def base(p):
    return p.split(":")[0]

def load_hits_pid(raw, sub="dynamic"):
    """yield (run_id, pid, arg0, exe, symbol)"""
    for f in sorted(glob.glob(os.path.join(raw, sub, "*.tsv"))):
        fid = os.path.basename(f)[:-4]
        for line in open(f, errors="replace"):
            parts = line.rstrip("\n").split("\t")
            if len(parts) == 4:
                yield (fid, *parts)

def load_hits(raw):
    """yield (run_id, arg0, exe, symbol) from raw/dynamic/*.tsv"""
    for f in sorted(glob.glob(os.path.join(raw, "dynamic", "*.tsv"))):
        fid = os.path.basename(f)[:-4]
        for line in open(f, errors="replace"):
            parts = line.rstrip("\n").split("\t")
            if len(parts) == 4:          # pid, argv0, exe, symbol
                yield (fid, *parts[1:])
            elif len(parts) == 3:        # older format without pid
                yield (fid, *parts)

def applets_by_tier(work, mapper=None):
    """{tier: {"toybox": set, "busybox": set}} from core runs, cumulative over T0..T2."""
    mapper = mapper or Mapper(work)
    per = {t: {"toybox": set(), "busybox": set()} for t in TIERS[:3]}
    for fid, a0, exe, sym in load_hits(os.path.join(work, "raw")):
        if run_kind(fid) != "core":
            continue
        t = tier_of_run(fid)
        if t == "T3":
            continue
        p = mapper.program(a0, exe)
        if ":" in p and base(p) in ("toybox", "busybox"):
            for tt in TIERS[TIERS.index(t):3]:
                per[tt][base(p)].add(p.split(":")[1])
    return per
