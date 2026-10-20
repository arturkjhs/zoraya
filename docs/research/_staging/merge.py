"""Merge TASK-01 stream staging files into the final docs/research outputs."""
import re, sys, pathlib, collections

R = pathlib.Path(sys.argv[1])  # docs/research
S = R / "_staging"

CATS = [
    "Process model and program execution",
    "Security, authority and identity",
    "Inter-process communication",
    "I/O, asynchrony and event handling",
    "Memory management",
    "Filesystems, namespaces and storage semantics",
    "Kernel structure, drivers and fault isolation",
    "Kernel–user interfaces and ABI design",
    "Concurrency and synchronization primitives",
    "Time, clocks and scheduling",
    "Terminals, sessions and consoles",
    "Networking APIs",
    "Graphics, input and window systems",
    "Configuration, observability and system management",
    "Boot, update and recovery",
    "Text, encoding and locale",
]

def norm_url(u):
    u = u.strip().rstrip(".,;)")
    u = re.sub(r"^http://", "https://", u)
    u = re.sub(r"^https://www\.", "https://", u)
    return u.rstrip("/").lower()

# ---- sources ----
src = {}          # id -> dict(line, supports[], url, type, unverified)
for f in sorted(S.glob("stream-*-sources.md")):
    cur = None
    for line in f.read_text().splitlines():
        m = re.match(r"^\[S(\d+)\]\s*(.*)$", line)
        if m:
            sid = int(m.group(1))
            body = m.group(2)
            urls = re.findall(r"https?://[^\s,]+", body)
            low = body.lower()
            typ = next((t for t in ("primary", "secondary", "weak") if re.search(rf",\s*{t}\b", low)), "?")
            if sid in src:
                print(f"WARN duplicate source id S{sid} in {f.name}", file=sys.stderr)
            cur = src[sid] = dict(body=body, supports=[], url=norm_url(urls[0]) if urls else None,
                                  type=typ, unverified="unverified" in low, stream=f.name)
        elif cur is not None and line.strip().startswith("- "):
            cur["supports"].append(line.strip()[2:])

remap = {}
by_url = {}
for sid in sorted(src):
    u = src[sid]["url"]
    if u and u in by_url:
        keep = by_url[u]
        remap[sid] = keep
        src[keep]["supports"] += src[sid]["supports"]
    elif u:
        by_url[u] = sid
canon = {sid: s for sid, s in src.items() if sid not in remap}

def rewrite(text):
    def r(m):
        n = int(m.group(1))
        return f"S{remap.get(n, n)}"
    return re.sub(r"(?<=\[)S(\d+)(?=\])", r, text)

# ---- entries ----
entries = {}   # id -> (cat_index or None, text)
notes = {}
def split_entries(f):
    t = f.read_text()
    m = re.search(r"^## Stream notes.*$", t, re.M)
    if m:
        notes[f.name] = t[m.start():]
        t = t[:m.start()]
    parts = re.split(r"(?=^### (?:DM|GR)-\d+)", t, flags=re.M)
    for p in parts:
        h = re.match(r"^### ((?:DM|GR)-\d+)", p)
        if not h:
            continue
        eid = h.group(1)
        body = re.sub(r"\n## [^\n]*\n", "\n", p).rstrip() + "\n"  # drop stray category headings
        cm = re.search(r"^- \*\*Category:\*\*\s*(.*)$", body, re.M)
        ci = None
        if cm:
            c = re.sub(r"^\d+\.\s*", "", cm.group(1)).strip()
            for i, name in enumerate(CATS):
                if c.lower().startswith(name.lower()):
                    ci = i
            if ci is not None and c != CATS[ci]:
                # keep note on the category choice, but normalise the field itself
                extra = c[len(CATS[ci]):].strip()
                body = body.replace(cm.group(0), f"- **Category:** {CATS[ci]}" + (f" {extra}" if extra else ""))
        if eid in entries:
            print(f"WARN duplicate entry {eid} in {f.name}", file=sys.stderr)
        entries[eid] = (ci, rewrite(body))

for f in sorted(S.glob("stream-*-entries.md")):
    split_entries(f)

def key(e):
    return int(e.split("-")[1])

# ---- checks ----
problems = []
for eid, (ci, body) in sorted(entries.items(), key=lambda kv: (kv[0][:2], key(kv[0]))):
    if "**Status:**" in body:
        continue
    if ci is None and eid.startswith("DM"):
        problems.append(f"{eid}: no recognised category")
    sm = re.search(r"^\*\*Sources\.\*\*(.*)$", body, re.M)
    cited = sorted({int(x) for x in re.findall(r"\[S(\d+)\]", sm.group(1) if sm else "")})
    inline = {int(x) for x in re.findall(r"\[S(\d+)\]", body)}
    missing = [x for x in inline if x not in canon]
    if missing:
        problems.append(f"{eid}: cites unknown sources {missing}")
    ok = [x for x in cited if x in canon and not canon[x]["unverified"]]
    prim = [x for x in ok if canon[x]["type"] == "primary"]
    if len(ok) < 2 or not prim:
        problems.append(f"{eid}: {len(ok)} verified sources, {len(prim)} primary")
    not_listed = sorted(inline - set(cited))
    if not_listed:
        problems.append(f"{eid}: inline refs not in Sources line {not_listed}")

# ---- write outputs ----
hdr_dm = """# Catalog of operating system design mistakes and tradeoffs

Part of [TASK-01](TASK-01-design-mistakes-research.md). Evidence base only: no design decisions are made here.
IDs are stable and are referenced by later ADRs; see [index.md](index.md) for the ID allocation. References
`[S<N>]` resolve in [sources.md](sources.md). Verification status of every entry is recorded in
[research-log.md](research-log.md).

"""
out = [hdr_dm]
for i, name in enumerate(CATS):
    out.append(f"## {i+1}. {name}\n\n")
    ids = sorted([e for e, (ci, _) in entries.items() if e.startswith("DM") and ci == i], key=key)
    if not ids:
        out.append("_No entries._\n\n")
    for e in ids:
        out.append(entries[e][1] + "\n")
orphans = [e for e, (ci, _) in entries.items() if e.startswith("DM") and ci is None]
if orphans:
    out.append("## Uncategorised\n\n" + "\n".join(entries[e][1] for e in sorted(orphans, key=key)))
(R / "design-mistakes.md").write_text("".join(out))

hdr_gr = """# Designs that held up well

Part of [TASK-01](TASK-01-design-mistakes-research.md). Counterweight to [design-mistakes.md](design-mistakes.md):
decisions worth considering for keeping in some form. Same evidence standard; IDs `GR-<NNN>` are stable.
Field names differ from DM entries where noted: **Why it held up** replaces "Why it is a problem" and
**Where it falls short** replaces "Why it was not fixed".

"""
gr = sorted([e for e in entries if e.startswith("GR")], key=key)
(R / "held-up-well.md").write_text(hdr_gr + "\n".join(entries[e][1] for e in gr))

hdr_s = """# Sources

Bibliography for TASK-01. Format: `[S<N>] Authors, Title, Venue/Publisher, Year, URL, type, checked <date>`.
IDs were allocated in blocks per research stream (S100–S199 stream A, … S1100–S1199 stream K), so gaps are
expected. Where two streams fetched the same URL the lower ID was kept and the other merged into it (see
the merge table at the end). Indented `supports:` lines record which claim each source was cited for, so the
citation can be audited.

"""
lines = [hdr_s]
for sid in sorted(canon):
    s = canon[sid]
    lines.append(f"[S{sid}] {s['body']}\n")
    for sup in s["supports"]:
        lines.append(f"  - {sup}\n")
if remap:
    lines.append("\n## Merged duplicate IDs\n\n")
    for a, b in sorted(remap.items()):
        lines.append(f"- S{a} → S{b}\n")
(R / "sources.md").write_text("".join(lines))

pm = []
for f in ("stream-J-postmortem.md", "stream-K-postmortem.md"):
    t = (S / f).read_text()
    m = re.search(r"^## Stream notes.*$", t, re.M)
    if m:
        notes[f] = t[m.start():]
        t = t[:m.start()]
    t = re.sub(r"^# .*\n", "", t, count=1)
    pm.append(rewrite(t).strip() + "\n")
(S / "postmortem-merged.md").write_text("\n\n".join(pm))

(S / "stream-notes-combined.md").write_text(
    "\n\n".join(f"# {k}\n\n{rewrite(v)}" for k, v in sorted(notes.items())))

# ---- stats ----
cls = collections.Counter()
percat = collections.Counter()
for e, (ci, b) in entries.items():
    if "**Status:**" in b:
        continue
    m = re.search(r"\*\*Classification:\*\*\s*([a-z-]+)", b)
    cls[(e[:2], m.group(1) if m else "?")] += 1
    if e.startswith("DM"):
        percat[ci] += 1
types = collections.Counter((s["type"], s["unverified"]) for s in canon.values())
print("entries:", collections.Counter(e[:2] for e in entries))
print("classification:", dict(cls))
print("per category:", {CATS[k] if k is not None else None: v for k, v in sorted(percat.items(), key=lambda kv: (kv[0] is None, kv[0] or 0))})
print("sources canonical:", len(canon), "merged dups:", len(remap), "types:", dict(types))
print("PROBLEMS:", len(problems))
for p in problems:
    print("  ", p)
