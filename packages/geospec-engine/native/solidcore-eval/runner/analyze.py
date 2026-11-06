#!/usr/bin/env python3
"""Compare the solidcore harness and the OCCT engine over the corpus.

Usage: analyze.py <manifest.jsonl> <solidcore.jsonl...> -- <occt.jsonl...> > summary.md
Writes summary.json and per-file.jsonl next to the first solidcore file.
The manifest is the 2026-10-18 per-file table (path, producer, schema, bytes);
files not in it are classed by directory.
"""
import json, math, os, sys, statistics as st
from collections import Counter, defaultdict

args = sys.argv[1:]
manifest_path = args[0]
split = args.index("--")
sc_files, oc_files = args[1:split], args[split + 1:]


def load(paths):
    out = {}
    for p in paths:
        for line in open(p):
            r = json.loads(line)
            out[r["listPath"]] = r
    return out


sc, oc = load(sc_files), load(oc_files)
meta = {}
for line in open(manifest_path):
    r = json.loads(line)
    p = r["path"]
    fp = "/home/user/geospec/" + p[8:] if p.startswith("geospec:") else "/home/user/geospec/corpus/" + p
    meta[fp] = {"producer": r.get("producer", "unknown"), "schema": r.get("schema", "other"), "bytes": r.get("bytes")}
paths = sorted(set(sc) | set(oc))
for p in paths:
    if p not in meta:
        prod = "GeoSpec fixtures" if "/packages/" in p or "/docs-site/" in p else "unknown"
        meta[p] = {"producer": prod, "schema": "other", "bytes": os.path.getsize(p)}


def sc_class(r):
    if r is None:
        return "missing"
    if r.get("capHit"):
        return "time cap" if "time" in r["capHit"] else "memory cap"
    if r.get("exit", 0) != 0:
        return "crash"
    if isinstance(r.get("open"), dict):
        return "refused (Part 21)"
    imp = r.get("import", {})
    if imp.get("built", 0) == 0:
        return "refused (no body built)" if imp.get("skipped") or imp.get("buildErrors") else "opened, no body"
    if r.get("volume") is not None and r.get("solids", 0) > 0:
        return "solid"
    return "opened, no solid"


def oc_class(r):
    if r is None:
        return "missing"
    if r.get("capHit"):
        return "time cap" if "time" in r["capHit"] else "memory cap"
    if r.get("exit", 0) != 0:
        return "crash"
    if r.get("open") != "ok":
        return "refused"
    v = r.get("checks", {}).get("volume", {})
    t = r.get("checks", {}).get("topologyCounts", {}).get("measured") or {}
    if isinstance(v.get("measured"), (int, float)) and t.get("solids", 0) > 0:
        return "solid"
    return "opened, no solid"


def rel(a, b):
    if a is None or b is None:
        return None
    d = max(abs(a), abs(b))
    return 0.0 if d == 0 else abs(a - b) / d


def oc_bounds(r):
    m = r.get("checks", {}).get("boundingBox", {}).get("measured")
    if not isinstance(m, dict) or "min" not in m:
        return None
    return m["min"] + m["max"]


def bounds_off(a, b):
    if not a or not b:
        return None
    diag = math.dist(b[:3], b[3:])
    if diag == 0:
        return None
    return max(math.dist(a[:3], b[:3]), math.dist(a[3:], b[3:])) / diag


def size_bin(n):
    return "<100 KB" if n < 1e5 else "100 KB-1 MB" if n < 1e6 else "1-10 MB" if n < 1e7 else ">10 MB"


rows = []
for p in paths:
    s, o = sc.get(p), oc.get(p)
    m = meta[p]
    row = {"path": p.replace("/home/user/geospec/", ""), **m, "sizeBin": size_bin(m["bytes"] or 0),
           "sc": sc_class(s), "occt": oc_class(o)}
    if s:
        row["scWallS"], row["scCpuS"], row["scRss"], row["scThreads"] = s["wallS"], s.get("cpuS"), s["peakRssMiB"], s.get("peakThreads")
        ph = s.get("phases", {})
        row["scPhases"] = {k: ph.get(k) for k in ("readMs", "parseMs", "buildMsSum", "factsMsSum", "totalMs")}
        imp = s.get("import", {})
        row["scSkipped"] = imp.get("skipped", [])[:5]
        row["scBuildErrors"] = imp.get("buildErrors", [])[:3]
        row["scFactsErrors"] = imp.get("factsErrors", [])[:3]
        row["scDiagnostics"] = imp.get("diagnostics", [])[:3]
        row["scAssemblyInstances"] = imp.get("assemblyInstances", 0)
        row["scOpenError"] = s["open"]["error"] if isinstance(s.get("open"), dict) else None
        for k in ("volume", "area", "centroid", "bounds", "solids", "faces", "valid", "integrity"):
            row["sc_" + k] = s.get(k)
    if o:
        row["occtWallS"], row["occtCpuS"], row["occtRss"], row["occtThreads"] = o["wallS"], o.get("cpuS"), o["peakRssMiB"], o.get("peakThreads")
        row["occtAdmissionMs"] = o.get("phases", {}).get("admission", {}).get("wallMs")
        row["occtLoadMs"] = o.get("phases", {}).get("load", {}).get("wallMs")
        row["occtOpenError"] = o["open"]["error"] if isinstance(o.get("open"), dict) else None
        ch = o.get("checks", {})
        row["occtCheckMs"] = {k: v.get("wallMs") for k, v in ch.items()}
        meas = lambda k: ch.get(k, {}).get("measured")
        t = meas("topologyCounts") or {}
        row.update({"oc_volume": meas("volume"), "oc_area": meas("surfaceArea"), "oc_centroid": meas("centerOfMass"),
                    "oc_bounds": oc_bounds(o), "oc_solids": t.get("solids"), "oc_faces": t.get("faces"),
                    "oc_valid": ch.get("validBrep", {}).get("status")})
    if row["sc"] == "solid" and row["occt"] == "solid":
        row["dVolume"] = rel(row.get("sc_volume"), row.get("oc_volume"))
        row["dArea"] = rel(row.get("sc_area"), row.get("oc_area"))
        row["dBounds"] = bounds_off(row.get("sc_bounds"), row.get("oc_bounds"))
        if row.get("sc_centroid") and isinstance(row.get("oc_centroid"), list) and row.get("oc_bounds"):
            diag = math.dist(row["oc_bounds"][:3], row["oc_bounds"][3:])
            row["dCentroid"] = math.dist(row["sc_centroid"], row["oc_centroid"]) / diag if diag else None
    rows.append(row)

out_dir = os.path.dirname(os.path.abspath(sc_files[0]))
with open(os.path.join(out_dir, "per-file.jsonl"), "w") as f:
    for r in rows:
        f.write(json.dumps(r) + "\n")


def pct(n, d):
    return f"{n} ({100 * n / d:.1f}%)" if d else "0"


def med(xs):
    xs = [x for x in xs if x is not None]
    return st.median(xs) if xs else float("nan")


def q(xs, p):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return float("nan")
    return xs[min(len(xs) - 1, int(p * len(xs)))]


def gmean(xs):
    xs = [x for x in xs if x and x > 0]
    return math.exp(sum(map(math.log, xs)) / len(xs)) if xs else float("nan")


N = len(rows)
print(f"# solidcore versus OCCT engine 0.5.1: corpus summary\n\n{N} files.\n")
print("## Admission outcomes\n")
classes = ["solid", "opened, no solid", "opened, no body", "refused (no body built)", "refused (Part 21)", "refused", "crash", "time cap", "memory cap", "missing"]
print("| Engine | " + " | ".join(classes) + " |")
print("| --- |" + " --: |" * len(classes))
for name, key in (("solidcore 1.0.1 (step-import + step-brep)", "sc"), ("OCCT engine 0.5.1 (GeoSpec profile)", "occt")):
    c = Counter(r[key] for r in rows)
    print(f"| {name} | " + " | ".join(str(c.get(k, 0)) for k in classes) + " |")
print()
cross = Counter((r["occt"], r["sc"]) for r in rows)
print("## Cross-tabulation (OCCT row, solidcore column)\n")
print("| OCCT | solidcore | Files |\n| --- | --- | --: |")
for (a, b), n in cross.most_common():
    print(f"| {a} | {b} | {n} |")
print()

print("## Solid admission by producer\n")
print("| Producer | Files | OCCT solid | solidcore solid | Both |")
print("| --- | --: | --: | --: | --: |")
byp = defaultdict(list)
for r in rows:
    byp[r["producer"]].append(r)
for p, rs in sorted(byp.items(), key=lambda kv: -len(kv[1])):
    n = len(rs)
    a = sum(r["occt"] == "solid" for r in rs)
    b = sum(r["sc"] == "solid" for r in rs)
    both = sum(r["occt"] == "solid" and r["sc"] == "solid" for r in rs)
    print(f"| {p} | {n} | {pct(a, n)} | {pct(b, n)} | {both} |")
print()

print("## Solidcore failure reasons\n")
reasons = Counter()
for r in rows:
    if r["sc"] in ("solid", "missing"):
        continue
    if r.get("scOpenError"):
        msg = r["scOpenError"]
        msg = msg.split(":", 2)
        reasons["Part 21: " + (msg[2].strip() if len(msg) > 2 else ":".join(msg))[:60]] += 1
    elif r["sc"] in ("time cap", "memory cap", "crash"):
        reasons[r["sc"]] += 1
    else:
        rs = r.get("scSkipped") or []
        be = r.get("scBuildErrors") or []
        fe = r.get("scFactsErrors") or []
        if rs:
            reasons["skipped body: " + rs[0][:60]] += 1
        elif be:
            reasons["build error: " + be[0][:60]] += 1
        elif fe:
            reasons["facts error: " + fe[0][:60]] += 1
        elif r.get("sc_solids", 0) == 0:
            reasons["built, not a closed solid (sheet or open shell)"] += 1
        else:
            reasons["other"] += 1
print("| Reason | Files |\n| --- | --: |")
for k, n in reasons.most_common(30):
    print(f"| {k} | {n} |")
print()

both = [r for r in rows if r["sc"] == "solid" and r["occt"] == "solid"]
print(f"## Agreement on {len(both)} files both admit with a solid\n")
print("Relative difference; bounds and centre of mass as offset over OCCT's diagonal.\n")
print("| Measure | ≤1e-9 | ≤1e-6 | ≤1e-3 | ≤1e-2 | >1e-2 | n |\n| --- | --: | --: | --: | --: | --: | --: |")
for k in ("dVolume", "dArea", "dBounds", "dCentroid"):
    xs = [r.get(k) for r in both if r.get(k) is not None]
    n = len(xs)
    cells = [pct(sum(x <= t for x in xs), n) for t in (1e-9, 1e-6, 1e-3, 1e-2)] + [str(sum(x > 1e-2 for x in xs))]
    print(f"| {k[1:]} | " + " | ".join(cells) + f" | {n} |")
sol_eq = sum(r.get("sc_solids") == r.get("oc_solids") for r in both)
print(f"\nSolid count equal on {pct(sol_eq, len(both))}. Files with assembly instancing (solidcore does not apply placements): {sum(1 for r in both if r.get('scAssemblyInstances'))}.\n")
noasm = [r for r in both if not r.get("scAssemblyInstances")]
print(f"Without assembly instancing ({len(noasm)} files):\n")
print("| Measure | ≤1e-6 | ≤1e-3 | ≤1e-2 | n |\n| --- | --: | --: | --: | --: |")
for k in ("dVolume", "dArea", "dBounds", "dCentroid"):
    xs = [r.get(k) for r in noasm if r.get(k) is not None]
    n = len(xs)
    print(f"| {k[1:]} | " + " | ".join(pct(sum(x <= t for x in xs), n) for t in (1e-6, 1e-3, 1e-2)) + f" | {n} |")
print()

print("## Process wall time, CPU and peak memory on files both admit\n")
print("Process wall includes process start; OCCT's includes Node.js start and module load. Engine-only times follow.\n")
print("| Size | Files | OCCT wall s (median / p90) | solidcore wall s (median / p90) | Ratio sc/OCCT (geo-mean) | OCCT peak MiB (median / max) | solidcore peak MiB (median / max) |")
print("| --- | --: | --: | --: | --: | --: | --: |")
for b in ("<100 KB", "100 KB-1 MB", "1-10 MB", ">10 MB", "all"):
    rs = [r for r in both if b == "all" or r["sizeBin"] == b]
    if not rs:
        continue
    ow, sw = [r["occtWallS"] for r in rs], [r["scWallS"] for r in rs]
    ratio = gmean([r["scWallS"] / r["occtWallS"] for r in rs if r["occtWallS"]])
    print(f"| {b} | {len(rs)} | {med(ow):.3f} / {q(ow, .9):.3f} | {med(sw):.3f} / {q(sw, .9):.3f} | {ratio:.2f}x | {med([r['occtRss'] for r in rs]):.0f} / {max(r['occtRss'] for r in rs):.0f} | {med([r['scRss'] for r in rs]):.0f} / {max(r['scRss'] for r in rs):.0f} |")
print()
print("Engine-only time: OCCT admission plus its checks (from inside the process) against solidcore read, parse, build and facts.\n")
print("| Size | Files | OCCT engine ms (median / p90) | solidcore engine ms (median / p90) | Ratio (geo-mean) |")
print("| --- | --: | --: | --: | --: |")
for b in ("<100 KB", "100 KB-1 MB", "1-10 MB", ">10 MB", "all"):
    rs = [r for r in both if b == "all" or r["sizeBin"] == b]
    if not rs:
        continue
    oe = [(r.get("occtAdmissionMs") or 0) + sum(v or 0 for v in (r.get("occtCheckMs") or {}).values()) for r in rs]
    se = [r["scPhases"].get("totalMs") for r in rs]
    ratio = gmean([s / o for s, o in zip(se, oe) if s and o])
    print(f"| {b} | {len(rs)} | {med(oe):.1f} / {q(oe, .9):.1f} | {med(se):.1f} / {q(se, .9):.1f} | {ratio:.2f}x |")
print()

print("## solidcore time by phase (all files that opened)\n")
tot = Counter()
for r in rows:
    ph = r.get("scPhases") or {}
    for k in ("readMs", "parseMs", "buildMsSum", "factsMsSum"):
        tot[k] += ph.get(k) or 0
T = sum(tot.values())
print("| Phase | Total s | Share |\n| --- | --: | --: |")
for k in ("readMs", "parseMs", "buildMsSum", "factsMsSum"):
    print(f"| {k} | {tot[k] / 1e3:.1f} | {100 * tot[k] / T:.1f}% |")
print()

print("## Thread use\n")
for name, tk, ck, wk in (("OCCT (Node.js process)", "occtThreads", "occtCpuS", "occtWallS"), ("solidcore", "scThreads", "scCpuS", "scWallS")):
    rs = [r for r in both if r.get(tk)]
    th = Counter(r[tk] for r in rs)
    util = [r[ck] / r[wk] for r in rs if r.get(wk)]
    print(f"- {name}: peak threads {dict(sorted(th.items()))}; CPU/wall median {med(util):.2f}, p90 {q(util, .9):.2f}")
print()

print("## Whole-corpus totals (every file, caps included)\n")
print(f"- solidcore: {sum(r.get('scWallS') or 0 for r in rows):.0f} s wall, {sum(r.get('scCpuS') or 0 for r in rows):.0f} s CPU")
print(f"- OCCT engine: {sum(r.get('occtWallS') or 0 for r in rows):.0f} s wall, {sum(r.get('occtCpuS') or 0 for r in rows):.0f} s CPU")

json.dump({"files": N, "sc": Counter(r["sc"] for r in rows), "occt": Counter(r["occt"] for r in rows)},
          open(os.path.join(out_dir, "summary.json"), "w"), indent=1)
