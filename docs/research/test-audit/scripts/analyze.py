#!/usr/bin/env python3
"""Statistics over the fetched run history (fetch.sh): runs.json and jobs/RUN-ATTEMPT.json.

Writes Markdown tables to out/*.md and prints a short summary.
"""
import collections
import datetime
import glob
import json
import os
import statistics
import sys

SETUP = {"image", "plan", "build"}
CHECK_PREFIXES = ("make fmt-check", "make clippy", "make adr-check", "make check-")


def ts(s):
    if not s:
        return None
    return datetime.datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ")


def kind_of(name):
    if name in SETUP:
        return name
    if name.startswith(CHECK_PREFIXES):
        return "check"
    if name.startswith("make "):
        return "test"
    return "other"


def load():
    runs = {r["databaseId"]: r for r in json.load(open("runs.json"))}
    jobs = collections.defaultdict(list)  # (run, attempt) -> jobs
    for f in glob.glob("jobs/*.json"):
        base = os.path.basename(f)[:-5]
        run, attempt = base.split("-")
        for line in open(f):
            line = line.strip()
            if not line:
                continue
            j = json.loads(line)
            j["start"] = ts(j.get("started_at"))
            j["end"] = ts(j.get("completed_at"))
            j["made"] = ts(j.get("created_at"))
            j["secs"] = (j["end"] - j["start"]).total_seconds() if j["start"] and j["end"] else None
            j["kind"] = kind_of(j["name"])
            jobs[(int(run), int(attempt))].append(j)
    return runs, jobs


def fmt(secs):
    if secs is None:
        return "-"
    secs = int(round(secs))
    return f"{secs // 60}:{secs % 60:02d}"


def run_class(js, r=None):
    tests = [j for j in js if j["kind"] == "test" and j["conclusion"] != "skipped"]
    n = len(tests)
    if n == 0:
        return "quick"
    if r is None:
        return "full" if n >= 45 else "candidate"
    ref, ev = r["headBranch"] or "", r["event"]
    if ref == "main" or ref.startswith("candidate-full/") or ev == "schedule":
        return "full"
    if ev == "workflow_dispatch":
        return "full" if n >= 40 else "candidate"
    if ref.startswith("candidate/") or ev == "pull_request":
        return "candidate"
    return "branch-push"


def main():
    runs, jobs = load()
    os.makedirs("out", exist_ok=True)

    # --- per run summary -------------------------------------------------
    rows = []
    for (rid, att), js in sorted(jobs.items()):
        r = runs.get(rid)
        if not r or not js:
            continue
        done = [j for j in js if j["secs"] is not None and j["conclusion"] not in ("skipped",)]
        if not done:
            continue
        first = min(j["made"] or j["start"] for j in done)
        last = max(j["end"] for j in done)
        total = sum(j["secs"] for j in done)
        billed = sum(-(-j["secs"] // 60) for j in done if j["secs"] > 0)
        build = [j for j in done if j["name"] == "build"]
        tests = [j for j in done if j["kind"] == "test"]
        rows.append(dict(
            run=rid, attempt=att, event=r["event"], branch=r["headBranch"], sha=r["headSha"][:8],
            concl=r["conclusion"] if att == r["attempt"] else "(earlier attempt)",
            cls=run_class(js, r), created=r["createdAt"], njobs=len(done), ntests=len(tests),
            wall=(last - first).total_seconds(), total=total, billed=billed,
            build=build[0]["secs"] if build else None,
            build_end=(build[0]["end"] - first).total_seconds() if build else None,
            tests_total=sum(j["secs"] for j in tests),
            tests_span=((max(j["end"] for j in tests) - min(j["made"] or j["start"] for j in tests)).total_seconds()
                        if tests else None),
            failed=[j["name"] for j in done if j["conclusion"] == "failure"],
            cancelled=sum(1 for j in js if j["conclusion"] == "cancelled"),
        ))
    json.dump(rows, open("out/runs-summary.json", "w"), indent=0, default=str)

    with open("out/runs.md", "w") as o:
        o.write("| run | attempt | event | ref | sha | class | conclusion | jobs | tests | wall | runner-min (sum) | billed min | build | failed jobs |\n")
        o.write("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n")
        for r in sorted(rows, key=lambda r: r["created"]):
            if r["cls"] == "quick":
                continue
            o.write(f"| {r['run']} | {r['attempt']} | {r['event']} | {r['branch']} | {r['sha']} | {r['cls']} | {r['concl']} | "
                    f"{r['njobs']} | {r['ntests']} | {fmt(r['wall'])} | {r['total'] / 60:.0f} | {r['billed']:.0f} | {fmt(r['build'])} | "
                    f"{'; '.join(r['failed'])[:300]} |\n")

    def stats(vals):
        vals = sorted(vals)
        if not vals:
            return None
        return dict(n=len(vals), med=statistics.median(vals), p90=vals[min(len(vals) - 1, int(0.9 * len(vals)))],
                    max=vals[-1], min=vals[0])

    with open("out/classes.md", "w") as o:
        o.write("| class | conclusion | runs | wall median | wall worst | runner-min median | runner-min worst | billed-min median | build median | build worst | time to build end median | tests span median |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n")
        for cls in ("full", "candidate", "branch-push", "quick"):
            for concl in ("success", "failure"):
                sel = [r for r in rows if r["cls"] == cls and r["concl"] == concl and r["cancelled"] == 0]
                if not sel:
                    continue
                w = stats([r["wall"] for r in sel])
                t = stats([r["total"] for r in sel])
                b = stats([r["billed"] for r in sel])
                bu = stats([r["build"] for r in sel if r["build"]])
                be = stats([r["build_end"] for r in sel if r["build_end"]])
                sp = stats([r["tests_span"] for r in sel if r["tests_span"]])
                o.write(f"| {cls} | {concl} | {len(sel)} | {fmt(w['med'])} | {fmt(w['max'])} | {t['med'] / 60:.0f} | {t['max'] / 60:.0f} | {b['med']:.0f} | "
                        f"{fmt(bu['med']) if bu else '-'} | {fmt(bu['max']) if bu else '-'} | {fmt(be['med']) if be else '-'} | {fmt(sp['med']) if sp else '-'} |\n")

    # --- per job statistics ----------------------------------------------
    per = collections.defaultdict(lambda: collections.defaultdict(list))
    for (rid, att), js in jobs.items():
        for j in js:
            if j["secs"] is None or j["conclusion"] == "skipped":
                continue
            per[j["name"]][j["conclusion"]].append(j["secs"])
    with open("out/jobs.md", "w") as o:
        o.write("| job | kind | passed | failed | cancelled | median | p90 | worst (passed) | worst failed | share of failures |\n|---|---|---|---|---|---|---|---|---|---|\n")
        for name in sorted(per, key=lambda n: -sum(per[n].get("success", [0])) ):
            p = per[name]
            s = stats(p.get("success", []))
            f = p.get("failure", [])
            c = p.get("cancelled", [])
            n_ok = len(p.get("success", []))
            o.write(f"| `{name}` | {kind_of(name)} | {n_ok} | {len(f)} | {len(c)} | {fmt(s['med']) if s else '-'} | {fmt(s['p90']) if s else '-'} | "
                    f"{fmt(s['max']) if s else '-'} | {fmt(max(f)) if f else '-'} | {100 * len(f) / max(1, n_ok + len(f)):.0f}% |\n")

    # --- step statistics inside build and a test job ---------------------
    steps = collections.defaultdict(lambda: collections.defaultdict(list))
    for (rid, att), js in jobs.items():
        for j in js:
            if j["conclusion"] != "success":
                continue
            key = j["name"] if j["kind"] in SETUP else j["kind"]
            for s in j.get("steps") or []:
                a, b = ts(s.get("started_at")), ts(s.get("completed_at"))
                if a and b and s.get("conclusion") == "success":
                    steps[key][s["name"]].append((b - a).total_seconds())
    with open("out/steps.md", "w") as o:
        for key in ("build", "test", "check", "plan", "image"):
            o.write(f"\n### steps of `{key}` jobs (successful jobs)\n\n| step | n | median | p90 | worst |\n|---|---|---|---|---|\n")
            for name, vals in sorted(steps[key].items(), key=lambda kv: -statistics.median(kv[1])):
                s = stats(vals)
                if s["max"] < 3:
                    continue
                o.write(f"| {name} | {s['n']} | {fmt(s['med'])} | {fmt(s['p90'])} | {fmt(s['max'])} |\n")

    # --- queueing: created -> started delays of test jobs in full runs ----
    with open("out/waves.md", "w") as o:
        o.write("| run | ref | tests | build end (from run start) | first test start | last test start | last test end | sum of test durations | peak concurrent tests |\n|---|---|---|---|---|---|---|---|---|\n")
        for (rid, att), js in sorted(jobs.items()):
            r = runs.get(rid)
            if not r or run_class(js, r) != "full":
                continue
            tests = [j for j in js if j["kind"] == "test" and j["secs"] is not None]
            if not tests:
                continue
            t0 = min((j["made"] or j["start"]) for j in js if j["start"])
            build = [j for j in js if j["name"] == "build" and j["end"]]
            ev = sorted([(j["start"], 1) for j in tests] + [(j["end"], -1) for j in tests])
            cur = peak = 0
            for _, d in ev:
                cur += d
                peak = max(peak, cur)
            o.write(f"| {rid} | {r['headBranch']} | {len(tests)} | {fmt((build[0]['end'] - t0).total_seconds()) if build else '-'} | "
                    f"{fmt((min(j['start'] for j in tests) - t0).total_seconds())} | {fmt((max(j['start'] for j in tests) - t0).total_seconds())} | "
                    f"{fmt((max(j['end'] for j in tests) - t0).total_seconds())} | {sum(j['secs'] for j in tests) / 60:.0f} min | {peak} |\n")

    # --- flake candidates --------------------------------------------------
    # (a) failed in one attempt of a run, passed in a later attempt of the same run
    # (b) failed in one run of a commit, passed in another run of the same commit
    by_sha = collections.defaultdict(list)
    for (rid, att), js in jobs.items():
        r = runs.get(rid)
        if not r:
            continue
        for j in js:
            if j["conclusion"] in ("success", "failure"):
                by_sha[(r["headSha"], j["name"])].append((r["createdAt"], rid, att, j["conclusion"], j["id"], r["headBranch"], r["event"], j["secs"]))
    flakes = []
    for (sha, name), lst in by_sha.items():
        concls = {x[3] for x in lst}
        if concls == {"success", "failure"}:
            for x in sorted(lst):
                if x[3] == "failure":
                    flakes.append(dict(sha=sha[:8], job=name, run=x[1], attempt=x[2], job_id=x[4], ref=x[5], event=x[6], secs=x[7], when=x[0],
                                       passes=[(y[1], y[2]) for y in lst if y[3] == "success"]))
    json.dump(flakes, open("out/flakes-same-sha.json", "w"), indent=1)
    with open("out/flakes-same-sha.md", "w") as o:
        o.write("Jobs that failed and passed on the same commit (another attempt or another run).\n\n| when | sha | job | failed in run (attempt) | job id | ref | took | passed in |\n|---|---|---|---|---|---|---|---|\n")
        for f in sorted(flakes, key=lambda f: f["when"]):
            o.write(f"| {f['when']} | {f['sha']} | `{f['job']}` | {f['run']} ({f['attempt']}) | {f['job_id']} | {f['ref']} | {fmt(f['secs'])} | "
                    f"{', '.join(f'{a} ({b})' for a, b in f['passes'][:4])} |\n")

    # (c) every failed test/build job, for triage: name, run, ref, sha, duration
    fails = []
    for (rid, att), js in jobs.items():
        r = runs.get(rid)
        if not r:
            continue
        for j in js:
            if j["conclusion"] == "failure" and j["kind"] in ("test", "build", "check", "plan", "image"):
                step = next((s["name"] for s in (j.get("steps") or []) if s.get("conclusion") == "failure"), "?")
                fails.append(dict(when=r["createdAt"], run=rid, attempt=att, ref=r["headBranch"], sha=r["headSha"][:8], event=r["event"],
                                  job=j["name"], job_id=j["id"], secs=j["secs"], step=step, kind=j["kind"]))
    json.dump(fails, open("out/failures.json", "w"), indent=1)
    cnt = collections.Counter(f["job"] for f in fails)
    with open("out/failures.md", "w") as o:
        o.write("| job | failures | refs |\n|---|---|---|\n")
        for name, n in cnt.most_common():
            refs = collections.Counter(f["ref"] for f in fails if f["job"] == name)
            o.write(f"| `{name}` | {n} | {', '.join(f'{k} x{v}' for k, v in refs.most_common(8))} |\n")

    print(f"runs with jobs: {len(rows)}; classes: {collections.Counter(r['cls'] for r in rows)}")
    print(f"same-commit flake candidates: {len(flakes)}; failed jobs: {len(fails)}")


if __name__ == "__main__":
    main()
