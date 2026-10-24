"""TASK-02b: M5 (syscalls and argument patterns under parallelism) and M6 (event sequences) from raw-par/.

Inputs : raw-par/{strace,dynamic}/ (remote/parallel.sh), raw/{strace,dynamic}/ (TASK-02 serial runs, read only)
Outputs: results/par/syscall-delta.csv   syscall, workload, serial_count, parallel_count, notable_args
         results/par/syscall-reps.tsv    per-run counts behind the means (run-to-run variation)
         results/par/parallel-tables.md  status, concurrency, jobserver, new libc symbols, signals, EINTR, W9 outcomes
         results/par/timelines.md        M6 timelines (W9 Ctrl-C, W9 SIGTERM, W10) and W9 stability across repetitions
Counts are over all processes of a run (strace -f), `*-check` runs excluded. serial_count/parallel_count are means over
repetitions; the counts inside notable_args are sums over the parallel repetitions.
"""
import sys, os, re, glob, csv, collections, statistics
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import runs as R

W = sys.argv[1]
RAW, RAWP = os.path.join(W, "raw"), os.path.join(W, "raw-par")
RES = os.path.join(W, "results", "par")
os.makedirs(RES, exist_ok=True)
M = R.Mapper(W)
PARAMS = dict(l.rstrip("\n").split("\t", 1) for l in open(os.path.join(RES, "params.tsv")))
JOBS, STYLES = PARAMS["jobs"].split(), PARAMS["styles"].split()
W9_DELAYS = PARAMS["w9_delays"].split()

# ---------------------------------------------------------------- trace parsing
LINE = re.compile(r"^(\d+)\s+(?:(\d+\.\d+)\s+)?(.*)$")
CALL = re.compile(r"^([a-z_0-9]+)\((.*)$")
RESUMED = re.compile(r"^<\.\.\. ([a-z_0-9]+) resumed>(.*)$")
SIGL = re.compile(r"^--- (SIG\w+) \{si_signo=\w+, si_code=(\w+)(?:, si_pid=(\d+))?")
EXITL = re.compile(r"^\+\+\+ (exited with (\d+)|killed by (SIG\w+))")
QSTR = re.compile(r'"((?:[^"\\]|\\.)*)"')

RETSEP = re.compile(r"\)\s+=\s")
def ret_of(text):
    """return value after the last ') = ' (strace pads short lines: ')    = 0')"""
    m = None
    for m in RETSEP.finditer(text):
        pass
    return text[m.end():].strip() if m else "?"

def strlist(text, start):
    """quoted strings of the array that starts at text[start] == '['"""
    out, i = [], start + 1
    while i < len(text) and text[i] != "]":
        m = QSTR.match(text, i)
        if not m:
            break
        out.append(m.group(1))
        i = m.end()
        if text.startswith(", ", i):
            i += 2
    return out, i

class Trace:
    """One strace -f file: calls (pid, ts, name, full text, ret), signals, exits, process table."""
    def __init__(self, path):
        self.path, self.calls, self.sigs, self.exits = path, [], [], {}
        self.parent, self.argv, self.exe, self.start, self.end = {}, {}, {}, {}, {}
        self.exec_ts = {}       # pid -> time its (last) execve returned: calls before it ran the parent's image
        pend = {}
        for l in open(path, errors="replace"):
            m = LINE.match(l.rstrip("\n"))
            if not m:
                continue
            pid, ts, rest = int(m.group(1)), float(m.group(2)) if m.group(2) else None, m.group(3)
            self.start.setdefault(pid, ts)
            if ts is not None:
                self.end[pid] = ts
            if rest.startswith("---"):
                s = SIGL.match(rest)
                if s:
                    self.sigs.append((pid, ts, s.group(1), s.group(2), int(s.group(3)) if s.group(3) else None))
                continue
            if rest.startswith("+++"):
                e = EXITL.match(rest)
                if e:
                    self.exits[pid] = (ts, ("exit", int(e.group(2))) if e.group(2) else ("killed", e.group(3)))
                continue
            r = RESUMED.match(rest)
            if r:
                if pid in pend:
                    i = pend.pop(pid)
                    name, full = self.calls[i][2], self.calls[i][3].rstrip() + r.group(2)
                    self.calls[i] = (pid, self.calls[i][1], name, full, ret_of(full), ts)
                    self._note(self.calls[i])
                continue
            c = CALL.match(rest)
            if not c:
                continue
            name = c.group(1)
            if rest.endswith("<unfinished ...>"):
                pend[pid] = len(self.calls)
                self.calls.append((pid, ts, name, rest[: -len("<unfinished ...>")], "?", None))
            else:
                self.calls.append((pid, ts, name, rest, ret_of(rest), ts))
                self._note(self.calls[-1])

    def _note(self, c):
        pid, ts, name, full, ret, ets = c
        if name in ("clone", "clone3", "vfork", "fork") and ret.split()[0].isdigit():
            self.parent[int(ret.split()[0])] = pid
        elif name == "execve" and ret.startswith("0"):
            m = QSTR.match(full, len("execve("))
            if m:
                self.exe[pid] = m.group(1)
                self.exec_ts[pid] = ets
                b = full.find("[", m.end())
                if b >= 0:
                    self.argv[pid] = strlist(full, b)[0]

    def image(self, pid, ts=None):
        """pid whose execve defines the image running in pid (at time ts, if given)"""
        while pid is not None:
            if pid in self.exe and (ts is None or self.exec_ts.get(pid) is None or ts >= self.exec_ts[pid]):
                return pid
            pid = self.parent.get(pid)
        return None

    def prog(self, pid, ts=None):
        ip = self.image(pid, ts)
        if ip is None:
            return "?"
        av = self.argv.get(ip, [""])
        return M.program(av[0] if av else "", M.resolve(self.exe[ip]))

    def role(self, pid, ts=None):
        """short label: make@<dir> for makes, otherwise program + argument, @ the nearest make's dir"""
        ip = self.image(pid, ts)
        if ip is None:
            return f"?{pid}"
        av = self.argv.get(ip, [])
        p = self.prog(pid, ts)
        if p == "gmake" or p == "pdpmake":
            return f"{p}@{self.makedir(ip)}"
        base = os.path.basename(av[0]) if av else p
        if base in ("sh", "dash") and len(av) > 2 and av[1] == "-c":
            what = f"sh -c '{av[2][:28]}'"
        elif base == "mktab":
            what = f"mktab {av[1] if len(av) > 1 else ''}"
        else:
            what = " ".join([base] + av[1:2])
        mk = self.nearest_make(pid)
        return f"{what}" + (f" @{self.makedir(mk)}" if mk else "")

    def makedir(self, pid):
        parts, p = [], pid
        while p is not None:
            if self.prog(p) in ("gmake", "pdpmake"):
                av = self.argv.get(self.image(p), [])
                if "-C" in av and av.index("-C") + 1 < len(av):
                    parts.append(av[av.index("-C") + 1])
            p = self.parent.get(p)
        return "/".join(reversed(parts)) or "top"

    def _makedepth(self, pid):
        """number of distinct make images above pid (vfork/fork children of a make share its image)"""
        seen, q = set(), self.parent.get(pid)
        mine = self.image(pid)
        while q is not None:
            iq = self.image(q)
            if iq is not None and iq != mine and self.prog(q) in ("gmake", "pdpmake"):
                seen.add(iq)
            q = self.parent.get(q)
        return len(seen)

    def nearest_make(self, pid):
        p = self.parent.get(pid)
        while p is not None:
            if self.prog(p) in ("gmake", "pdpmake"):
                return p
            p = self.parent.get(p)
        return None

    def rclass(self, pid, ts=None):
        """role class used for stability signatures"""
        if ts is not None and self.image(pid, ts) != self.image(pid):
            return self.rclass(self.parent[pid], ts) + "(child before exec)" if pid in self.parent else "?"
        p = self.prog(pid)
        if p in ("gmake", "pdpmake"):
            # level = number of make ancestors (recursion through -C or through `cd dir && $(MAKE)`)
            return ["make-top", "make-l1", "make-l2"][min(self._makedepth(pid), 2)]
        av = self.argv.get(self.image(pid), [""])
        b = os.path.basename(av[0]) if av else ""
        if b in ("sh", "dash"):
            return "recipe-sh" if self.nearest_make(pid) else "shell"
        if b == "mktab":
            return "mktab"
        if b == "tcc":
            return "tcc"
        return b or p

# ---------------------------------------------------------------- argument patterns
SIGNUM = re.compile(r"SIG[A-Z0-9]+")
def patterns(name, full, ret):
    out = []
    if ret.startswith("? ERESTART") or "EINTR" in ret:
        out.append("intr=" + ret.split()[1 if ret.startswith("?") else 1] if len(ret.split()) > 1 else "intr")
    a = full[len(name) + 1:]
    if name in ("clone", "clone3"):
        out.append("kind=" + ("thread" if "CLONE_THREAD" in a else "vfork" if "CLONE_VFORK" in a else "fork"))
    elif name == "wait4":
        m = re.match(r"(-?\d+),\s*(?:NULL|0x[0-9a-f]+|\[.*?\]),\s*([A-Z_|]+|0)", a)
        if m:
            out.append(f"pid={'-1' if m.group(1) == '-1' else '>0'};opts={m.group(2)}")
    elif name in ("pselect6", "ppoll", "poll", "select", "epoll_wait", "epoll_pwait"):
        rs = re.match(r"\d+, \[([\d ]*)\]", a)
        tmo = "timeout=NULL" if re.search(r"NULL, \{sigmask|NULL\)|, NULL, NULL, NULL", a) else "timeout=set"
        sm = re.search(r"sigmask=\[([^\]]*)\]", a)
        out.append(f"{'fds=[' + rs.group(1) + ']' if rs else 'fds'};{tmo};sigmask=[{sm.group(1) if sm else '-'}]")
        out.append("result=" + ("ready" if ret[:1].isdigit() and ret[0] != "0" else ret.split()[0] if ret else "?"))
    elif name == "rt_sigaction":
        m = re.match(r"(SIG\w+), (NULL|\{sa_handler=(\w+)[^}]*\})", a)
        if m:
            if m.group(2) == "NULL":
                out.append(f"{m.group(1)}:query")
            else:
                h = m.group(3)
                h = h if h in ("SIG_DFL", "SIG_IGN") else "handler"
                fl = "+SA_RESTART" if "SA_RESTART" in m.group(2) else ""
                out.append(f"{m.group(1)}:{h}{fl}")
    elif name == "rt_sigprocmask":
        m = re.match(r"(SIG_\w+), (NULL|\[([^\]]*)\]|~\[([^\]]*)\])", a)
        if m:
            out.append(f"{m.group(1)} {m.group(2)}")
    elif name in ("kill", "tgkill", "tkill"):
        nums = re.findall(r"-?\d+", a.split("SIG")[0])
        sig = SIGNUM.search(a)
        tgt = nums[0] if nums else "?"
        k = "pgrp" if tgt.startswith("-") else ("own-pgrp" if tgt == "0" else "pid")
        out.append(f"{sig.group(0) if sig else a.split(',')[-1].strip(') ')}->{k}" if name == "kill" else f"{sig.group(0) if sig else '?'}->self/thread")
    elif name == "pipe2":
        m = re.search(r"\], (\w[\w|]*)\)", a)
        out.append("flags=" + (m.group(1) if m else "0"))
    elif name == "mknodat":
        out.append("S_IFIFO" if "S_IFIFO" in a else "other")
    elif name == "openat":
        if "GMfifo" in a:
            fl = re.search(r'", (O_[\w|]+)', a)
            out.append("jobserver-fifo " + (fl.group(1) if fl else ""))
    elif name in ("read", "write"):
        if re.match(r'\d+,\s*"\+", 1\)', a) and ret.startswith("1"):
            out.append("token")
        elif ret.startswith("?") or "EAGAIN" in ret:
            out.append("result=" + ret.split()[1] if len(ret.split()) > 1 else ret)
    elif name == "fcntl":
        m = re.match(r"\d+, (F_\w+)(?:, (O_[\w|]+))?", a)
        if m:
            out.append(m.group(1) + (f" {m.group(2)}" if m.group(2) and m.group(1) == "F_SETFL" else ""))
    elif name == "unlinkat":
        m = QSTR.search(a)
        if m:
            out.append("path=" + re.sub(r"\d", "?", os.path.basename(m.group(1))))
    elif name == "setpgid":
        out.append("setpgid")
    elif name == "ioctl":
        m = re.match(r"\d+, (\w+)", a)
        if m:
            out.append("req=" + m.group(1))
    return out

# ---------------------------------------------------------------- runs
def strace_file(base, rid):
    return os.path.join(base, "strace", rid + ".txt")

def par_runs(prefix):
    return sorted(f[:-4] for f in map(os.path.basename, glob.glob(os.path.join(RAWP, "strace", prefix + "*.txt")))
                  if not f[:-4].endswith("-check"))

CACHE = {}
def trace(path):
    if path not in CACHE:
        CACHE[path] = Trace(path)
    return CACHE[path]

def counts(path):
    t = trace(path)
    return collections.Counter(c[2] for c in t.calls)

def pats(paths):
    pc = collections.defaultdict(collections.Counter)
    for p in paths:
        for pid, ts, name, full, ret, ets in trace(p).calls:
            for x in patterns(name, full, ret):
                pc[name][x] += 1
    return pc

# comparisons: (workload label, [serial trace files], [parallel trace files])
COMP = []
def ser(*ids):
    return [f for f in ids if os.path.exists(f)]
for j in JOBS:
    for st in STYLES:
        COMP.append((f"W2 gmake -j{j} {st}", ser(strace_file(RAW, "W2-make-g")),
                     [strace_file(RAWP, r) for r in par_runs(f"W2j-g-{st}-j{j}-r")]))
    COMP.append((f"W2 pdpmake -j{j}", ser(strace_file(RAW, "W2-make-p")),
                 [strace_file(RAWP, r) for r in par_runs(f"W2j-p-j{j}-r")]))
for w in ("W3", "W8"):
    gs = [strace_file(RAWP, r) for r in par_runs(f"{w}j-g-j1-r")]
    ps = [strace_file(RAWP, r) for r in par_runs(f"{w}j-p-j1-r")]
    for j in JOBS:
        for st in STYLES:
            COMP.append((f"{w} gmake -j{j} {st}", gs, [strace_file(RAWP, r) for r in par_runs(f"{w}j-g-{st}-j{j}-r")]))
        COMP.append((f"{w} pdpmake -j{j}", ps, [strace_file(RAWP, r) for r in par_runs(f"{w}j-p-j{j}-r")]))
COMP.append(("W10 dash", [strace_file(RAWP, r) for r in par_runs("W10-serial-r")],
             [strace_file(RAWP, r) for r in par_runs("W10-parallel-r")]))
W9BASE = [strace_file(RAWP, r) for r in par_runs("W8j-g-fifo-j4-r")]
for c in ("int", "term"):
    for d in W9_DELAYS:
        COMP.append((f"W9 {'Ctrl-C' if c == 'int' else 'SIGTERM'} at {d}s (vs W8 gmake -j4 fifo)", W9BASE,
                     [strace_file(RAWP, r) for r in par_runs(f"W9-{c}-d{d}-r")]))

def mean(xs):
    return round(statistics.mean(xs), 1) if xs else ""

with open(os.path.join(RES, "syscall-delta.csv"), "w", newline="") as f, \
     open(os.path.join(RES, "syscall-reps.tsv"), "w") as fr:
    w = csv.writer(f)
    w.writerow(["syscall", "workload", "serial_count", "parallel_count", "notable_args"])
    fr.write("workload\tside\trun\tsyscall\tcount\n")
    for label, sp, pp in COMP:
        if not pp:
            continue
        sc = [counts(p) for p in sp]
        pc = [counts(p) for p in pp]
        for side, files, cs in (("serial", sp, sc), ("parallel", pp, pc)):
            for p, c in zip(files, cs):
                for k, v in sorted(c.items()):
                    fr.write(f"{label}\t{side}\t{os.path.basename(p)[:-4]}\t{k}\t{v}\n")
        pa = pats(pp)
        for name in sorted(set().union(*sc, *pc) if sc else set().union(*pc)):
            notable = ";".join(f"{k}({v})" for k, v in pa[name].most_common(10))
            w.writerow([name, label, mean([c[name] for c in sc]) if sc else "", mean([c[name] for c in pc]), notable])

# ---------------------------------------------------------------- tables
md = ["<!-- generated by tools/posix-surface/analysis/parallel.py; do not edit -->", "",
      f"VM cores {PARAMS['ncpu']}; job counts {' '.join(JOBS)} (-jN = -j{PARAMS['ncpu']}); GNU make jobserver styles "
      f"{' '.join(STYLES)}; {PARAMS['reps']} repetitions of W2-j/W3-j/W8/W10; W9: delays {' '.join(W9_DELAYS)} s, "
      f"{PARAMS['w9_reps']} repetitions each.", ""]

# status
md += ["### Exit status of every run", ""]
for mode in ("dynamic", "strace"):
    st = [l.rstrip("\n").split("\t") for l in open(os.path.join(RES, f"{mode}-status.tsv"))]
    bad = [f"{a}={c}" for a, b, c in st if c != "0" and not a.startswith("W9-")]
    md.append(f"- {mode}: {len(st)} runs; non-zero outside W9: {', '.join(bad) or 'none'}")
md.append("")

# concurrency: recipe processes (children of a make that are not makes) alive at the same time
def concurrency(t):
    ev = []
    for pid, par in t.parent.items():
        if t.prog(par) in ("gmake", "pdpmake") and t.prog(pid) not in ("gmake", "pdpmake") and t.start.get(pid):
            e = t.exits.get(pid, (t.end.get(pid),))[0] or t.end.get(pid)
            ev += [(t.start[pid], 1), (e, -1)]
    cur = mx = 0
    for ts, d in sorted(ev):
        cur += d
        mx = max(mx, cur)
    return mx
def duration(t):
    ts = [c[1] for c in t.calls if c[1]]
    return max(ts) - min(ts) if ts else 0

md += ["### Concurrency and duration (strace runs)", "",
       "Max recipe processes alive at once = children of any make that are not makes themselves, from clone and exit "
       "timestamps. Duration = first to last traced syscall. Values per repetition.", "",
       "| Runs | Max concurrent recipe processes | Duration (s) |", "|---|---|---|"]
groups = collections.OrderedDict()
for f in sorted(glob.glob(os.path.join(RAWP, "strace", "*.txt"))):
    rid = os.path.basename(f)[:-4]
    if rid.endswith("-check") or rid.startswith(("W9-", "W10-")):
        continue
    groups.setdefault(re.sub(r"-r\d+$", "", rid), []).append(f)
for g, fs in groups.items():
    md.append(f"| {g} | {', '.join(str(concurrency(trace(f))) for f in fs)} | "
              f"{', '.join(f'{duration(trace(f)):.1f}' for f in fs)} |")
md.append("")

# jobserver mechanics
md += ["### Jobserver mechanics (GNU make, strace)", "",
       "`auth` = `--jobserver-auth=` values seen in MAKEFLAGS of executed processes. Tokens are 1-byte `+` reads and "
       "writes by make processes. `pselect6` = jobserver waits.", "",
       "| Runs | auth values | FIFO created | token reads | token writes | pselect6 calls (interrupted) | makes |",
       "|---|---|---|---|---|---|---|"]
for g, fs in groups.items():
    if "-g-" not in g or "-j1" in g:
        continue
    auth, fifo, tr, tw, ps, pi, mk = collections.Counter(), 0, [], [], [], [], []
    for f in fs:
        t = trace(f)
        a = collections.Counter()
        for pid, ts, name, full, ret, ets in t.calls:
            if name == "execve":
                for m in re.finditer(r"MAKEFLAGS=[^\"]*?--jobserver-auth=([^\" ]+)(?: --jobserver-auth=([^\" ]+))?", full):
                    k = m.group(1) if not m.group(2) else f"{m.group(1)} then {m.group(2)}"
                    auth[re.sub(r"GMfifo[0-9]+", "GMfifoN", k)] += 1
        fifo += sum(1 for c in t.calls if c[2] == "mknodat" and "S_IFIFO" in c[3])
        tr.append(sum(1 for c in t.calls if c[2] == "read" and "token" in patterns("read", c[3], c[4])))
        tw.append(sum(1 for c in t.calls if c[2] == "write" and "token" in patterns("write", c[3], c[4])))
        ps.append(sum(1 for c in t.calls if c[2] == "pselect6"))
        pi.append(sum(1 for c in t.calls if c[2] == "pselect6" and c[4].startswith("? ERESTART")))
        mk.append(len({p for p in t.exe if t.prog(p) == "gmake"}))
    au = "; ".join(f"`{k}` ×{v}" for k, v in auth.most_common(3))
    md.append(f"| {g} | {au} | {fifo} | {tr} | {tw} | {ps} ({pi}) | {mk} |")
md.append("")

# new libc symbols vs TASK-02 dynamic
def hits(base):
    out = collections.defaultdict(set)
    for f in glob.glob(os.path.join(base, "dynamic", "*.tsv")):
        if os.path.basename(f)[:-4].endswith("-check"):
            continue
        for l in open(f, errors="replace"):
            p = l.rstrip("\n").split("\t")
            if len(p) >= 4:
                prog = M.program(p[-3], M.resolve(p[-2]))
                if not prog.startswith(("built", "?")):
                    out[prog.split(":")[0] if prog.startswith(("toybox", "busybox")) else prog].add(p[-1])
    return out
H2, HP = hits(RAW), hits(RAWP)
all2 = set().union(*H2.values())
md += ["### libc functions called under parallelism that TASK-02 never saw (interposer)", "",
       "| Program | New for this program | Of those, new to TASK-02 altogether |", "|---|---|---|"]
newall = set()
for prog in sorted(HP):
    n = sorted(HP[prog] - H2.get(prog, set()))
    nn = sorted(set(n) - all2)
    newall |= set(nn)
    md.append(f"| {prog} | {', '.join(n) or '—'} | {', '.join(nn) or '—'} |")
md += ["", f"New to the TASK-02 lower bound altogether: {len(newall)} ({', '.join(sorted(newall)) or 'none'}).", ""]

# signals delivered / sent, EINTR, per workload family
def family(rid):
    if rid.startswith("W9-"):
        return "W9 " + ("Ctrl-C" if "-int-" in rid else "SIGTERM")
    if rid.startswith("W10-"):
        return "W10 " + rid.split("-")[1]
    m = re.match(r"(W\dj)-(g|p)-(?:(pipe|fifo)-)?j(\d+)", rid)
    return f"{m.group(1)} {'gmake' if m.group(2) == 'g' else 'pdpmake'} -j{m.group(4)}" if m else rid
dl, sd, intr, pg = (collections.defaultdict(collections.Counter) for _ in range(4))
nrun = collections.Counter()
for f in sorted(glob.glob(os.path.join(RAWP, "strace", "*.txt"))):
    rid = os.path.basename(f)[:-4]
    if rid.endswith("-check"):
        continue
    t, fam = trace(f), family(rid)
    nrun[fam] += 1
    for pid, ts, sig, code, spid in t.sigs:
        dl[fam][f"{sig} ({code}) -> {t.rclass(pid)}" + (f" from {t.rclass(spid)}" if spid and spid in t.start else "")] += 1
    for pid, ts, name, full, ret, ets in t.calls:
        if name in ("kill", "tgkill", "tkill"):
            sd[fam][f"{t.rclass(pid, ts)} {name}: {patterns(name, full, ret)[0]}"] += 1
        if ret.startswith("? ERESTART") or "EINTR" in ret:
            intr[fam][f"{name} {ret.split()[1]} in {t.rclass(pid, ts)}"] += 1
        if name in ("setpgid",) or (name == "ioctl" and "TIOCSPGRP" in full) or (name == "kill" and re.match(r"kill\(-\d", full)):
            pg[fam][f"{t.rclass(pid, ts)} {name}" + (" TIOCSPGRP" if "TIOCSPGRP" in full else "")] += 1
for title, d in (("Signals delivered (receiver role, sender role when si_pid names a traced process)", dl),
                 ("Signals sent by the processes themselves (kill/tgkill/tkill)", sd),
                 ("System calls interrupted by a signal (EINTR / ERESTART*)", intr),
                 ("Process-group operations", pg)):
    md += [f"### {title}", "", "Totals over all strace repetitions of the family; runs per family in brackets.", ""]
    for fam in sorted(d):
        md.append(f"- **{fam}** [{nrun[fam]}]: " + "; ".join(f"{k} ×{v}" for k, v in sorted(d[fam].items())))
    if not d:
        md.append("- none")
    md.append("")

# W9 outcomes from the logs, both modes
md += ["### W9 outcomes per run (both modes; `dynamic` = interposer only, no strace)", "",
       "| Run | Mode | make status | tables complete /32 | 'Deleting file' messages | partial targets left | processes left |",
       "|---|---|---|---|---|---|---|"]
W9OUT = []
for mode in ("dynamic", "strace"):
    for f in sorted(glob.glob(os.path.join(RAWP, mode, "W9-*.log"))):
        rid, txt = os.path.basename(f)[:-4], open(f, errors="replace").read()
        st = re.search(r"MAKE_STATUS=(\d+)", txt)
        tc = re.search(r"tables complete: (\d+)/32; partial targets left: (.*)", txt)
        left = re.search(r"processes left in build tree after 0.5s: (.*)", txt)
        dels = len(re.findall(r"\*\*\* Deleting file", txt))
        row = (rid, mode, st.group(1) if st else "?", tc.group(1) if tc else "?", dels,
               tc.group(2).strip() if tc else "?", left.group(1).strip() if left else "?")
        W9OUT.append(row)
        md.append("| " + " | ".join(map(str, row)) + " |")
md.append("")
open(os.path.join(RES, "parallel-tables.md"), "w").write("\n".join(md) + "\n")

# ---------------------------------------------------------------- M6 timelines
def harness_events(rid):
    f = os.path.join(RAWP, "strace", rid + ".log")
    return [(float(m.group(1)), m.group(2)) for m in
            re.finditer(r"HARNESS t=(\d+\.\d+) (.*)", open(f, errors="replace").read())] if os.path.exists(f) else []

KEEP = ("kill", "tgkill", "tkill", "unlinkat", "wait4", "exit_group", "rt_sigaction", "ioctl", "setpgid",
        "clone", "execve", "pselect6", "rt_sigsuspend", "pipe2", "write", "read", "rt_sigreturn", "dup3")
def tl_lines(rid, t0, since=None, until=None, full=False):
    """ordered events of one strace run, times in ms relative to t0"""
    t = trace(strace_file(RAWP, rid))
    ev = []
    for pid, ts, sig, code, spid in t.sigs:
        ev.append((ts, pid, f"receives {sig} ({code}" + (f", from {t.role(spid)}" if spid and spid in t.start else "") + ")"))
    for pid, (ts, (k, v)) in t.exits.items():
        ev.append((ts, pid, f"exits {v}" if k == "exit" else f"killed by {v}"))
    for pid, ts0, name, a, ret, ets in t.calls:
        # a call that blocked is logged twice (unfinished / resumed): its result happened at the resume time
        ts = ets if ets is not None else ts0
        if name not in KEEP or ts is None:
            continue
        p = patterns(name, a, ret)
        txt = None
        if name in ("kill", "tgkill", "tkill"):
            tgt = re.findall(r"-?\d+", a[len(name) + 1:].split("SIG")[0])
            tp = int(tgt[-1]) if name == "tgkill" else int(tgt[0]) if tgt else 0
            who = t.role(abs(tp)) if abs(tp) in t.start else str(tp)
            txt = f"{name}({'pgrp ' if tp < 0 else ''}{who}, {SIGNUM.search(a).group(0) if SIGNUM.search(a) else '?'}) = {ret}"
        elif name == "unlinkat":
            txt = f"unlinkat({QSTR.search(a).group(1) if QSTR.search(a) else '?'}) = {ret}"
        elif name == "wait4" and ret.split()[0].lstrip("-").isdigit() and int(ret.split()[0]) > 0:
            st = re.search(r"\[\{(.*?)\}\]", a)
            txt = f"wait4 reaps {t.role(int(ret.split()[0]))}: {st.group(1) if st else ''}"
        elif name == "wait4" and full:
            txt = f"wait4({'WNOHANG' if 'WNOHANG' in a else 'blocking'}) = {ret}"
        elif name == "rt_sigaction" and p and not p[0].endswith(":query"):
            txt = f"rt_sigaction {p[0]}"
        elif name == "ioctl" and ("TIOCSPGRP" in a):
            txt = "ioctl TIOCSPGRP (foreground process group)"
        elif name == "setpgid":
            txt = f"setpgid{a[a.find('('):] if '(' in a else '(' + a}"[:60]
        elif name in ("clone",):
            r0 = ret.split()[0]
            if r0.isdigit():
                txt = f"{p[0]} -> child {r0}"
        elif name == "execve" and ret.startswith("0"):
            txt = f"execve -> {t.role(pid)}"
        elif name in ("pselect6", "rt_sigsuspend") and ret.startswith("? ERESTART"):
            txt = f"{name} interrupted ({ret.split()[1]})"
        elif name in ("pselect6", "rt_sigsuspend") and full:
            txt = f"{name} -> {ret}"
        elif name in ("read", "write") and "token" in p:
            txt = f"{name} jobserver token"
        elif name in ("write",) and "EPIPE" in ret:
            txt = f"write -> EPIPE"
        elif name == "pipe2" and full:
            txt = f"pipe2 -> {re.search(r'\[(\d+, \d+)\]', a).group(1) if re.search(r'\[(\d+, \d+)\]', a) else '?'}"
        elif name == "rt_sigreturn" and full:
            txt = "returns from signal handler"
        if txt:
            ev.append((ts, pid, txt))
    ev.sort(key=lambda e: (e[0] or 0, e[1]))
    # a process that resets many dispositions in a row (toybox start-up, musl posix_spawn child) -> one line
    resets = collections.Counter(pid for ts, pid, txt in ev if txt.startswith("rt_sigaction") and ":SIG_DFL" in txt)
    shown, comp = set(), []
    for ts, pid, txt in ev:
        if txt.startswith("rt_sigaction") and ":SIG_DFL" in txt and resets[pid] > 3:
            if pid not in shown:
                shown.add(pid)
                comp.append((ts, pid, f"resets {resets[pid]} signal dispositions to SIG_DFL"))
            continue
        comp.append((ts, pid, txt))
    ev = comp
    out = []
    for ts, pid, txt in ev:
        if ts is None or (since and ts < since) or (until and ts > until):
            continue
        out.append((ts, f"| {1000 * (ts - t0):+9.1f} | {pid} {t.role(pid, ts)} | {txt} |"))
    return out, t

def alive_at(t, ts):
    return sorted(t.role(p) for p in t.start if (t.start[p] or 0) <= ts and (t.exits.get(p, (t.end.get(p),))[0] or 0) >= ts)

tl = ["<!-- generated by tools/posix-surface/analysis/parallel.py; do not edit -->", "",
      "Times are milliseconds relative to the harness action (Ctrl-C written to the pty, or SIGTERM sent), on the "
      "same clock as strace -ttt. Roles: `gmake@top` is the top-level make, `gmake@a` a level-1 make, `gmake@a/1` a "
      "leaf make; `@dir` after a recipe names the make that ran it. strace slows every process, so absolute gaps are "
      "longer than without tracing; the order of events within one process is exact, across processes it is the "
      "order in which strace observed them. A call that blocked is placed at the time it returned.", ""]
def rep_run(c):
    mid = W9_DELAYS[len(W9_DELAYS) // 2]
    rs = par_runs(f"W9-{c}-d{mid}-r")
    return rs[0] if rs else None
for c, title in (("int", "W9a: Ctrl-C during `make -j4` (W8) in an interactive dash"),
                 ("term", "W9b: SIGTERM to the top-level make only")):
    rid = rep_run(c)
    if not rid:
        continue
    he = harness_events(rid)
    t0 = next((ts for ts, m in he if m.startswith(("write VINTR", "kill SIGTERM"))), None)
    if t0 is None:
        continue
    lines, t = tl_lines(rid, t0, since=t0 - 0.002)
    tl += [f"## {title}", "", f"Run `{rid}`. Harness: " + "; ".join(f"{1000 * (ts - t0):+.1f} ms {m}" for ts, m in he), "",
           "Alive at the harness action: " + ", ".join(f"`{r}`" for r in alive_at(t, t0)), "",
           "| ms | process | event |", "|---|---|---|"] + [l for ts, l in lines] + [""]
rid = (par_runs("W10-parallel-r") or [None])[0]
if rid:
    t = trace(strace_file(RAWP, rid))
    t0 = min(c[1] for c in t.calls if c[1])
    lines, _ = tl_lines(rid, t0, full=True)
    tl += ["## W10: background jobs, wait, concurrent pipelines in dash (`sh w10.sh parallel`)", "",
           f"Run `{rid}`. Times relative to the first traced syscall. Includes blocking/non-blocking waits, pipes and "
           "handler returns.", "", "| ms | process | event |", "|---|---|---|"] + [l for ts, l in lines] + [""]

# stability across W9 repetitions (strace)
def sig9(rid):
    t = trace(strace_file(RAWP, rid))
    f = set()
    for pid, ts, sig, code, spid in t.sigs:
        if sig in ("SIGINT", "SIGTERM"):
            f.add(f"{t.rclass(pid)} receives {sig} ({code})")
        if sig == "SIGCHLD":
            f.add(f"{t.rclass(pid)} receives SIGCHLD")
    for pid, ts, name, a, ret, ets in t.calls:
        if name in ("kill", "tgkill", "tkill"):
            f.add(f"{t.rclass(pid)} {name} {patterns(name, a, ret)[0]}")
        if name == "unlinkat" and re.search(r"t\d\.h", a):
            f.add(f"{t.rclass(pid)} deletes a partial table")
        if ret.startswith("? ERESTART") or "EINTR" in ret:
            f.add(f"{name} interrupted in {t.rclass(pid)}")
    for pid, (ts, (k, v)) in t.exits.items():
        f.add(f"{t.rclass(pid)} {'exits ' + str(v) if k == 'exit' else 'killed by ' + v}")
    return f
for c, title in (("int", "Ctrl-C"), ("term", "SIGTERM")):
    rs = [r for d in W9_DELAYS for r in par_runs(f"W9-{c}-d{d}-r")]
    if not rs:
        continue
    sigs = {r: sig9(r) for r in rs}
    feats = collections.Counter(x for s in sigs.values() for x in s)
    tl += [f"## W9 stability, {title}: event kinds per repetition (strace, {len(rs)} runs)", "",
           "`all` = present in every run (stable); otherwise the number of runs, and the delays it occurred at.", "",
           "| Event kind | Runs |", "|---|---|"]
    for x, n in sorted(feats.items(), key=lambda kv: (-kv[1], kv[0])):
        at = sorted({re.search(r"-d([\d.]+)-", r).group(1) for r in rs if x in sigs[r]})
        tl.append(f"| {x} | {'all' if n == len(rs) else f'{n}/{len(rs)} (at {", ".join(at)} s)'} |")
    tl.append("")
open(os.path.join(RES, "timelines.md"), "w").write("\n".join(tl) + "\n")
print("parallel analysis written to", RES)
