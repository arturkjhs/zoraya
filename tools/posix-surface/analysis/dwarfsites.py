"""DWARF line-table helpers shared by the call-site extractors (callsites.py for M3, callsites_par.py for M7)."""
import os, re, subprocess, collections

def exec_ranges(binary):
    """address ranges of executable sections (.init/.text/.fini)"""
    out = subprocess.run(["readelf", "-SW", binary], capture_output=True, text=True).stdout
    rs = []
    for line in out.splitlines():
        m = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\S+\s+(\S+)", line)
        if m and "X" in m.group(4) and m.group(1) != ".plt":
            a, n = int(m.group(2), 16), int(m.group(3), 16)
            rs.append((a, a + n))
    return rs

def line_table(binary):
    """(file basename -> set of lines) for line-table rows whose address is inside an executable section.
    Functions removed by --gc-sections keep their DWARF rows but at addresses outside .text; they are dropped."""
    rs = exec_ranges(binary)
    out = subprocess.run(["objdump", "--dwarf=decodedline", binary], capture_output=True, text=True).stdout
    have = collections.defaultdict(set)
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[1].isdigit() and parts[2].startswith("0x"):
            addr = int(parts[2], 16)
            if any(lo <= addr < hi for lo, hi in rs):
                have[os.path.basename(parts[0])].add(int(parts[1]))
    return have

def functions(path):
    out = subprocess.run(["ctags", "-x", "--c-kinds=f", "--language-force=C", path],
                         capture_output=True, text=True).stdout
    fs = []
    for line in out.splitlines():
        p = line.split()
        if len(p) >= 3 and p[2].isdigit():
            fs.append((int(p[2]), p[0]))
    return sorted(fs)

def enclosing(fs, ln):
    name = "?"
    for start, n in fs:
        if start <= ln:
            name = n
        else:
            break
    return name
