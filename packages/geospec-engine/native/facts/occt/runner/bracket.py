#!/usr/bin/env python3
"""The box with a bracket (P5) against M3's samples and OCCT's AddOptimal.

Usage: bracket.py <box-runs.jsonl> <m3-full.jsonl.gz> <out.json> [<other-runs.jsonl>...]

The runs files (plain or gzipped JSON lines) hold one row per file of `geospec-facts-occt --box --no-facts`
with its corpus `path` added. M3's rows hold per file the sampled edge and
surface boxes (`edgeBox`, `surfBox`) and the per-face AddOptimal fold
(`fold`), each as min xyz then max xyz.

- Containment: over files where every solid has a box, the shape has no free
  faces, edges or vertices and the solid count matches M3's, the union of the
  outer boxes must hold M3's edge and surface samples; reported untoleranced
  and widened by each body's largest tolerance.
- Width: each body's bound against its epsilon.
- AddOptimal: M3's fold and the runner's fold against the union of the inner
  boxes (under: a side short of the attained extreme by more than the
  bound) and the outer boxes (over: the slack beyond the bracket, absolute
  and relative to the box's largest coordinate).
- Time: the bracket's milliseconds against the runner's AddOptimal fold, per
  file, direction only.
- Identity: each further runs file (other thread counts) must hold identical
  box values.
"""

import gzip
import json
import sys
from collections import Counter


def lines(path):
    return gzip.open(path, "rt") if path.endswith(".gz") else open(path)


def union(boxes):
    lo = [min(b[0][k] for b in boxes) for k in range(3)]
    hi = [max(b[1][k] for b in boxes) for k in range(3)]
    return lo, hi


def sides_of(lo, hi):
    return [-lo[0], -lo[1], -lo[2], hi[0], hi[1], hi[2]]


def quant(xs, qs=(0.5, 0.9, 0.99, 1.0)):
    xs = sorted(xs)
    if not xs:
        return {}
    return {str(q): xs[min(len(xs) - 1, int(q * (len(xs) - 1) + 0.5))] for q in qs}


def main():
    runs_path, m3_path, out = sys.argv[1:4]
    others = sys.argv[4:]
    runs = [json.loads(l) for l in lines(runs_path)]
    m3 = {}
    for l in gzip.open(m3_path, "rt"):
        r = json.loads(l)
        m3[r["path"]] = r
    s = {
        "files": len(runs),
        "errors": [],
        "bodies": 0,
        "boxed": 0,
        "refusals": Counter(),
        "fallbackBodies": 0,
        "cappedBodies": 0,
        "extrapolatedBodies": 0,
        "containment": {"files": 0, "excluded": Counter(), "edge": 0, "surf": 0,
                        "violations": [], "violationsTolerance": []},
        "width": {"withinEpsilon": 0, "above": [], "ratio": None, "bound": None},
        "addOptimal": {"m3Files": 0, "m3Under": [], "m3Over": None, "runnerUnder": [], "runnerOver": None,
                       "m3UnderMax": 0.0, "m3OverFiles": [], "runnerOverFiles": 0},
        "time": {"files": 0, "boxMs": 0.0, "addOptimalMs": 0.0, "slower": []},
        "identity": {},
    }
    ratios, bounds, m3_over, run_over = [], [], [], []
    for r in runs:
        p = r["path"]
        if "error" in r or "bodies" not in r:
            s["errors"].append([p, str(r.get("error", r.get("open")))[:200]])
            continue
        bodies = r["bodies"]
        boxed = [b for b in bodies if b.get("box")]
        s["bodies"] += len(bodies)
        s["boxed"] += len(boxed)
        for b in bodies:
            if b.get("boxRefusal"):
                s["refusals"][b["boxRefusal"]["code"]] += 1
        for b in boxed:
            x = b["box"]
            s["fallbackBodies"] += bool(x["fallbackFaces"])
            s["cappedBodies"] += bool(x["cappedFaces"])
            s["extrapolatedBodies"] += bool(x["extrapolatedFaces"])
            ratios.append(x["bound"] / x["epsilon"] if x["epsilon"] > 0 else 0.0)
            bounds.append(x["bound"])
            if x["bound"] <= x["epsilon"]:
                s["width"]["withinEpsilon"] += 1
            else:
                s["width"]["above"].append([p, bodies.index(b), x["bound"], x["epsilon"],
                                            len(x["fallbackFaces"]), len(x["extrapolatedFaces"]),
                                            len(x["cappedFaces"]), b.get("maxTolerance")])
        ph = r.get("phases", {})
        if boxed and "boxMs" in ph and "addOptimalMs" in ph:
            s["time"]["files"] += 1
            s["time"]["boxMs"] += ph["boxMs"]
            s["time"]["addOptimalMs"] += ph["addOptimalMs"]
            if ph["boxMs"] > ph["addOptimalMs"] and ph["boxMs"] > 5:
                s["time"]["slower"].append([p, round(ph["boxMs"], 1), round(ph["addOptimalMs"], 1)])
        if not boxed or len(boxed) != len(bodies):
            if p in m3:
                s["containment"]["excluded"]["refusedBody" if bodies else "noBodies"] += 1
            continue
        outer = union([b["box"]["outer"] for b in boxed])
        inner = union([b["box"]["inner"] for b in boxed])
        tol = max(b.get("maxTolerance") or 0.0 for b in boxed)
        bound = max(b["box"]["bound"] for b in boxed)
        so, si = sides_of(*outer), sides_of(*inner)
        # Runner's own AddOptimal fold.
        ao = [b["addOptimal"]["bounds"] for b in boxed if b.get("addOptimal", {}).get("bounds")]
        if len(ao) == len(boxed):
            sa = sides_of(*union(ao))
            under = max(si[k] - sa[k] for k in range(6))
            run_over.append(max(sa[k] - so[k] for k in range(6)))
            if run_over[-1] > 1e-12 * max(1.0, max(abs(x) for x in so)):
                s["addOptimal"]["runnerOverFiles"] += 1
            if under > bound:
                s["addOptimal"]["runnerUnder"].append([p, under])
        m = m3.get(p)
        if not m:
            s["containment"]["excluded"]["notInM3"] += 1
            continue
        fc = r.get("freeCounts") or {}
        if any(fc.get(k) for k in ("faces", "edges", "vertices")) or r.get("freeFaces"):
            s["containment"]["excluded"]["freeGeometry"] += 1
            continue
        if m.get("solids") != len(bodies):
            s["containment"]["excluded"]["solidCount"] += 1
            continue
        s["containment"]["files"] += 1
        scale = max(abs(x) for x in so)
        slack = 1e-12 * max(1.0, scale)
        for key in ("edgeBox", "surfBox"):
            b = m.get(key)
            if not b or b[0] is None:
                continue
            s["containment"]["edge" if key == "edgeBox" else "surf"] += 1
            sb = sides_of(b[:3], b[3:])
            over = max(sb[k] - so[k] for k in range(6))
            if over > slack:
                row = [p, key, over, tol, bound]
                s["containment"]["violations"].append(row)
                if over > tol + slack:
                    s["containment"]["violationsTolerance"].append(row)
        f = m.get("fold")
        if f and f[0] is not None:
            s["addOptimal"]["m3Files"] += 1
            sf = sides_of(f[:3], f[3:])
            under = max(si[k] - sf[k] for k in range(6))
            over = max(sf[k] - so[k] for k in range(6))
            m3_over.append(over / max(1.0, scale))
            if over > slack:
                s["addOptimal"]["m3OverFiles"].append([p, over, over / max(1.0, scale)])
            if under > bound:
                s["addOptimal"]["m3Under"].append([p, under])
                s["addOptimal"]["m3UnderMax"] = max(s["addOptimal"]["m3UnderMax"], under)
    s["width"]["ratio"] = quant(ratios)
    s["width"]["bound"] = quant(bounds)
    s["addOptimal"]["m3Over"] = quant(m3_over)
    s["addOptimal"]["runnerOver"] = quant(run_over)
    s["time"]["slower"].sort(key=lambda x: x[2] - x[1])
    base = {r["path"]: [b.get("box") or b.get("boxRefusal") for b in r.get("bodies", [])] for r in runs}
    for o in others:
        diff = []
        n = 0
        for l in lines(o):
            r = json.loads(l)
            if r["path"] in base:
                n += 1
                v = [b.get("box") or b.get("boxRefusal") for b in r.get("bodies", [])]
                if v != base[r["path"]]:
                    diff.append(r["path"])
        s["identity"][o] = {"files": n, "differ": diff}
    s["refusals"] = dict(s["refusals"])
    s["containment"]["excluded"] = dict(s["containment"]["excluded"])
    json.dump(s, open(out, "w"), indent=1)
    c = s["containment"]
    print(json.dumps({k: s[k] for k in ("files", "bodies", "boxed", "refusals", "fallbackBodies", "cappedBodies",
                                        "extrapolatedBodies")}))
    print("containment files", c["files"], "excluded", c["excluded"], "violations", len(c["violations"]),
          "beyond tolerance", len(c["violationsTolerance"]))
    print("width within eps", s["width"]["withinEpsilon"], "above", len(s["width"]["above"]), "ratio", s["width"]["ratio"])
    a = s["addOptimal"]
    print("AddOptimal m3 files", a["m3Files"], "under", len(a["m3Under"]), "max", a["m3UnderMax"], "over files",
          len(a["m3OverFiles"]), "over(rel)", a["m3Over"])
    print("runner AO under", len(a["runnerUnder"]), "over files", a["runnerOverFiles"], "over(abs)", a["runnerOver"])
    t = s["time"]
    print("time files", t["files"], "box ms", round(t["boxMs"]), "AO ms", round(t["addOptimalMs"]), "slower", len(t["slower"]))
    for k, v in s["identity"].items():
        print("identity", k, v["files"], "differ", len(v["differ"]))


if __name__ == "__main__":
    main()
