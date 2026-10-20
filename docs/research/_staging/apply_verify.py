"""Apply verifier outputs (_staging/verify/V*-entries.md, V*-findings.md) to the final research files."""
import re, sys, pathlib

R = pathlib.Path(sys.argv[1])  # docs/research
V = R / "_staging" / "verify"
log = []

def split(text, pat):
    """Return {id: block} for blocks starting at headings matching pat (a regex with one group)."""
    out = {}
    ms = list(re.finditer(pat, text, re.M))
    for i, m in enumerate(ms):
        end = ms[i + 1].start() if i + 1 < len(ms) else len(text)
        out[m.group(1)] = text[m.start():end]
    return out

def strip_tail(block):
    # verifier files may end with stray headings or summaries; keep only the item itself
    return re.split(r"^## (?!#)", block, maxsplit=1, flags=re.M)[0].rstrip() + "\n\n"

def replace_items(path, pat, new):
    t = path.read_text()
    old = split(t, pat)
    n = 0
    for k, blk in new.items():
        if k not in old:
            log.append(f"WARN {path.name}: verified item {k} not in target")
            continue
        o = old[k]
        # keep the category heading that may follow the old block's item text
        tail = ""
        m = re.search(r"^## (?!#).*", o, re.M)
        if m:
            tail = o[m.start():]
        t = t.replace(o, strip_tail(blk) + tail, 1)
        n += 1
    missing = sorted(set(old) - set(new))
    path.write_text(t)
    return n, missing

dm_new, gr_new, pm_new = {}, {}, {}
for f in sorted(V.glob("V*-entries.md")):
    t = f.read_text()
    for k, b in split(t, r"^### ((?:DM|GR)-\d+)").items():
        (gr_new if k.startswith("GR") else dm_new)[k] = b
    if f.name.startswith(("V9-", "V10-")):
        pm_new.update(split(t, r"^## (.+?)\s*$"))

n, miss = replace_items(R / "design-mistakes.md", r"^### (DM-\d+)", dm_new)
log.append(f"design-mistakes.md: replaced {n}; not covered by a verifier: {miss}")
n, miss = replace_items(R / "held-up-well.md", r"^### (GR-\d+)", gr_new)
log.append(f"held-up-well.md: replaced {n}; not covered by a verifier: {miss}")

pm_path = R / "_staging" / "postmortem-merged.md"
pm = pm_path.read_text()
old = split(pm, r"^## (.+?)\s*$")
for k, b in pm_new.items():
    if k in old:
        pm = pm.replace(old[k], b.rstrip() + "\n\n", 1)
    else:
        log.append(f"WARN postmortem: verified section '{k}' not in draft")
log.append(f"postmortem: verified sections {len([k for k in pm_new if k in old])}/{len(old)}; "
           f"missing {sorted(set(old) - set(pm_new))}")
(R / "_staging" / "postmortem-verified.md").write_text(pm)

# sources directives
sp = R / "sources.md"
st = sp.read_text()
head, _, merged_tbl = st.partition("\n## Merged duplicate IDs")
blocks = split(head, r"^\[S(\d+)\]")
pre = head[: min(m.start() for m in re.finditer(r"^\[S\d+\]", head, re.M))]
new_lines = {}
for f in sorted(V.glob("V*-findings.md")):
    for line in f.read_text().splitlines():
        m = re.match(r"^\s*-?\s*`?SRC S(\d+):\s*(type=|unverified|note|new)\s*(?:—|-)?\s*(.*?)`?\s*$", line)
        if not m:
            continue
        sid, kind, rest = m.group(1), m.group(2), m.group(3)
        if kind == "new":
            entry, _, sup = rest.partition("| supports:")
            new_lines.setdefault(sid, [entry.strip(), []])[1].append(sup.strip())
            continue
        if sid not in blocks:
            log.append(f"WARN {f.name}: SRC S{sid} not found")
            continue
        b = blocks[sid]
        first, nl, restb = b.partition("\n")
        if kind == "type=":
            t2 = rest.split()[0].strip("`")
            first2 = re.sub(r",\s*(primary|secondary|weak)\b", f", {t2}", first, count=1)
            if first2 == first:
                log.append(f"WARN S{sid}: could not relabel")
            first = first2
        elif kind == "unverified":
            if "unverified" not in first:
                first = first + ", unverified"
            restb = restb + f"  - verifier ({f.stem.split('-')[0]}): unverified — {rest}\n"
        elif kind == "note":
            restb = restb + f"  - verifier ({f.stem.split('-')[0]}): {rest}\n"
        blocks[sid] = first + nl + restb
for sid, (entry, sups) in new_lines.items():
    if sid in blocks:
        log.append(f"WARN new source S{sid} collides with existing ID")
        continue
    if not entry.startswith(f"[S{sid}]"):
        entry = f"[S{sid}] " + entry
    blocks[sid] = entry + "\n" + "".join(f"  - supports: {s}\n" for s in sups if s)
out = pre + "".join(blocks[k] if blocks[k].endswith("\n") else blocks[k] + "\n"
                    for k in sorted(blocks, key=int))
sp.write_text(out.rstrip() + "\n" + ("\n## Merged duplicate IDs" + merged_tbl if merged_tbl else ""))
log.append(f"sources: {len(new_lines)} new sources added")
print("\n".join(log))
