#!/usr/bin/env python3
"""Per-check paired timing, thread scaling and fixture relationships.

Usage: analyze_paired.py <run-dir>
Reads paired-sc.jsonl, paired-occt.jsonl, threads-sc-{1,2,4}.jsonl,
threads-occt-{1,4}.jsonl and rel-occt.jsonl from <run-dir>; prints Markdown.
"""
import json, math, os, sys, statistics as st
from collections import Counter, defaultdict

d = sys.argv[1]


def load(name):
    p = os.path.join(d, name)
    if not os.path.exists(p):
        return {}
    return {json.loads(l)["listPath"]: json.loads(l) for l in open(p)}


def size_bin(p):
    n = os.path.getsize(p)
    return "<100 KB" if n < 1e5 else "100 KB-1 MB" if n < 1e6 else "1-10 MB" if n < 1e7 else ">10 MB"


def med(xs):
    xs = [x for x in xs if x is not None]
    return st.median(xs) if xs else float("nan")


def p90(xs):
    xs = sorted(x for x in xs if x is not None)
    return xs[min(len(xs) - 1, int(0.9 * len(xs)))] if xs else float("nan")


def gmean(xs):
    xs = [x for x in xs if x and x > 0]
    return math.exp(sum(map(math.log, xs)) / len(xs)) if xs else float("nan")


sc, oc = load("paired-sc.jsonl"), load("paired-occt.jsonl")
common = [p for p in sc if p in oc and sc[p].get("volume") is not None and oc[p].get("open") == "ok"]
print(f"## Paired per-check times ({len(common)} files, serial, idle host)\n")
print("solidcore times each query on its own; the OCCT engine reuses its admitted subject and cached mass properties across claims.\n")
checks = [("admission", None), ("topologyCounts", "topologyCounts"), ("boundingBox", "boundingBox"), ("volume", "volume"),
          ("surfaceArea", "surfaceArea"), ("centerOfMass", "centerOfMass"), ("validBrep", "validBrep")]
print("| Step | OCCT ms (median / p90 / total s) | solidcore ms (median / p90 / total s) | solidcore ÷ OCCT (geometric mean) |")
print("| --- | --: | --: | --: |")
for name, key in checks:
    if name == "admission":
        o = [oc[p]["phases"]["admission"]["wallMs"] for p in common]
        s = [sc[p]["phases"]["readMs"] + sc[p]["phases"]["parseMs"] + sc[p]["phases"]["buildMsSum"] for p in common]
        label = "Admission (OCCT) / read, parse and build (solidcore)"
    else:
        o = [oc[p]["checks"].get(key, {}).get("wallMs") for p in common]
        s = [sc[p]["phases"]["checkMs"].get(key) for p in common]
        label = f"`{key}`"
    r = gmean([b / a for a, b in zip(o, s) if a and b])
    print(f"| {label} | {med(o):.2f} / {p90(o):.2f} / {sum(x or 0 for x in o) / 1e3:.1f} | {med(s):.2f} / {p90(s):.2f} / {sum(x or 0 for x in s) / 1e3:.1f} | {r:.2f}x |")
print()
print("By size, end-to-end engine time (admission plus all checks):\n")
print("| Size | Files | OCCT ms (median / p90) | solidcore ms (median / p90) | Ratio (geometric mean) | OCCT CPU/wall | solidcore CPU/wall |")
print("| --- | --: | --: | --: | --: | --: | --: |")
for b in ("<100 KB", "100 KB-1 MB", "1-10 MB", ">10 MB", "all"):
    ps = [p for p in common if b == "all" or size_bin(p) == b]
    if not ps:
        continue
    o = [oc[p]["phases"]["admission"]["wallMs"] + sum(v.get("wallMs") or 0 for v in oc[p]["checks"].values()) for p in ps]
    s = [sc[p]["phases"]["totalMs"] for p in ps]
    print(f"| {b} | {len(ps)} | {med(o):.1f} / {p90(o):.1f} | {med(s):.1f} / {p90(s):.1f} | {gmean([y / x for x, y in zip(o, s)]):.2f}x | "
          f"{med([oc[p]['cpuS'] / oc[p]['wallS'] for p in ps]):.2f} | {med([sc[p]['cpuS'] / sc[p]['wallS'] for p in ps]):.2f} |")
print()

t = {n: load(f"threads-sc-{n}.jsonl") for n in (1, 2, 4)}
to = {n: load(f"threads-occt-{n}.jsonl") for n in (1, 4)}
paths = [p for p in t[1] if all(p in t[n] and t[n][p].get("phases") for n in (1, 2, 4))]
if paths:
    print(f"## Thread scaling ({len(paths)} multi-body files)\n")
    print("| Configuration | Engine ms (median / total s) | Speed-up over 1 thread (geometric mean) | Peak threads (median) | CPU/wall (median) | Peak MiB (median) | Results identical to 1 thread |")
    print("| --- | --: | --: | --: | --: | --: | --: |")
    keys = ("volume", "area", "centroid", "bounds", "faces", "solids")
    for n in (1, 2, 4):
        ms = [t[n][p]["phases"]["totalMs"] for p in paths]
        sp = gmean([t[1][p]["phases"]["totalMs"] / t[n][p]["phases"]["totalMs"] for p in paths])
        same = sum(all(t[n][p].get(k) == t[1][p].get(k) for k in keys) for p in paths)
        print(f"| solidcore, {n} thread{'s' if n > 1 else ''} | {med(ms):.0f} / {sum(ms) / 1e3:.1f} | {sp:.2f}x | {med([t[n][p]['peakThreads'] for p in paths]):.0f} | "
              f"{med([t[n][p]['cpuS'] / t[n][p]['wallS'] for p in paths]):.2f} | {med([t[n][p]['peakRssMiB'] for p in paths]):.0f} | {same} of {len(paths)} |")
    op = [p for p in paths if p in to[1] and p in to[4] and to[1][p].get("open") == "ok" and to[4][p].get("open") == "ok"]
    for n in (1, 4):
        ms = [to[n][p]["phases"]["admission"]["wallMs"] + sum(v.get("wallMs") or 0 for v in to[n][p]["checks"].values()) for p in op]
        sp = gmean([(to[1][p]["phases"]["admission"]["wallMs"] + sum(v.get("wallMs") or 0 for v in to[1][p]["checks"].values())) /
                    (to[n][p]["phases"]["admission"]["wallMs"] + sum(v.get("wallMs") or 0 for v in to[n][p]["checks"].values())) for p in op])
        same = sum(all(to[n][p]["checks"].get(k, {}).get("measured") == to[1][p]["checks"].get(k, {}).get("measured") for k in ("volume", "surfaceArea", "centerOfMass", "boundingBox")) for p in op)
        print(f"| OCCT engine, {n} permit{'s' if n > 1 else ''} | {med(ms):.0f} / {sum(ms) / 1e3:.1f} | {sp:.2f}x | {med([to[n][p]['peakThreads'] for p in op]):.0f} | "
              f"{med([to[n][p]['cpuS'] / to[n][p]['wallS'] for p in op]):.2f} | {med([to[n][p]['peakRssMiB'] for p in op]):.0f} | {same} of {len(op)} |")
    print()
    print("solidcore phase split at 1 and 4 threads (sum over files):\n")
    for n in (1, 4):
        ph = Counter()
        for p in paths:
            for k in ("parseMs", "bodiesMs", "totalMs"):
                ph[k] += t[n][p]["phases"][k]
        print(f"- {n} thread(s): parse {ph['parseMs'] / 1e3:.1f} s, bodies (build and facts) {ph['bodiesMs'] / 1e3:.1f} s, total {ph['totalMs'] / 1e3:.1f} s")
    print()

rel = os.path.join(d, "rel-occt.jsonl")
if os.path.exists(rel):
    rows = [json.loads(l) for l in open(rel) if l.startswith("{")]
    print(f"## Fixture relationships on the OCCT engine ({len(rows)} manifests)\n")
    by = defaultdict(list)
    adm = []
    for r in rows:
        adm.append(r.get("admissionMs"))
        for x in r.get("relationships", []):
            by[x["kind"]].append(x)
    print("| Relationship | Claims | Matches manifest verdict | Refused or thrown | ms (median / max) |")
    print("| --- | --: | --: | --: | --: |")
    for k, xs in sorted(by.items()):
        ok = sum((x.get("status") == "passed") == (x.get("expected") == "pass") and "thrown" not in x for x in xs)
        bad = sum("thrown" in x or x.get("status") == "refused" for x in xs)
        print(f"| {k} | {len(xs)} | {ok} | {bad} | {med([x['ms'] for x in xs]):.2f} / {max(x['ms'] for x in xs):.2f} |")
    print(f"\nAdmission per fixture: median {med(adm):.1f} ms, max {max(a for a in adm if a):.1f} ms.\n")
