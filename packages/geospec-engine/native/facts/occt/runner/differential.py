#!/usr/bin/env python3
"""The OCCT adapter's differential against the solidcore adapter and OCCT's GProp.

Usage: differential.py <occt-runs.jsonl> <solidcore-runs.jsonl> <list> <out.json>

Both runs files come from `../../solidcore/runner/g3g4.py` (the OCCT runner
`geospec-facts-occt` prints the same row shape; its `reference` is the
bridge's BRepGProp of the solid, without Eps, and `shape` the bridge's
whole-shape facts). Only the 1-thread rows at accuracy 1e-7 are read.

- OCCT adapter vs solidcore adapter, per file, over the files where every
  body on both sides has facts and the closed bodies' counts agree: the summed
  volume and area, within the sum of both sides' bounds or not. Per body when
  each OCCT body pairs with a solidcore body whose volume lies within both
  bounds (greedy, in order), else by file only.
- OCCT adapter vs GProp, per body with facts: |V - V_gprop| against the
  bound; how many GProp volumes are off by more than 1e-7 relative.
- Whole-shape: the summed facts against the bridge's whole-shape volume where
  every solid has facts and the shape has no free faces.
- Export coverage: refusal codes, surface and trim kinds, reflected and
  shifted faces, seams, degenerate edges, trims past 2 pi.
- Time: the facts' integration against GProp's two calls, summed.
"""

import json
import sys
from collections import Counter


def rows(path):
    out = {}
    for line in open(path):
        r = json.loads(line)
        if r.get("setting") == [1, 1e-7]:
            out[r["path"]] = r
    return out


def complete(row):
    return bool(row.get("bodies")) and "open" not in row and "error" not in row and all(
        b.get("facts") is not None for b in row["bodies"]
    )


def closed(row):
    return [b for b in row["bodies"] if b.get("manifold") is True]


def main():
    occt_path, sc_path, lst, out = sys.argv[1:5]
    occt, sc = rows(occt_path), rows(sc_path)
    paths = [p.strip() for p in open(lst) if p.strip()]
    s = {
        "files": len(paths),
        "occt": {"opened": 0, "bodies": 0, "withFacts": 0, "refusals": Counter(), "refusedFiles": 0,
                 "completeFiles": 0},
        "solidcore": {"completeFiles": 0},
        "vsSolidcore": {"files": 0, "within": 0, "bodyCountDiffers": 0, "violations": [],
                        "bodies": 0, "bodiesWithin": 0, "maxRelVolume": 0.0},
        "vsGprop": {"bodies": 0, "withinBound": 0, "offOver1e7": 0, "maxRel": 0.0, "worst": []},
        "wholeShape": {"files": 0, "within": 0, "maxRel": 0.0},
        "export": {"surfaces": Counter(), "curves": Counter(), "reflectedFaces": 0, "movedOriginFaces": 0, "shiftedFaces": 0,
                   "seams": 0, "degenerate": 0, "gapLines": 0, "pastTwoPi": 0, "reversedFaces": 0,
                   "unsupportedSurfaceKinds": Counter()},
        "gapLineBodies": 0,
        "timeMs": {"integrate": 0.0, "gprop": 0.0, "export": 0.0, "open": 0.0},
    }
    for p in paths:
        o, c = occt.get(p), sc.get(p)
        if o and "open" not in o and "error" not in o:
            s["occt"]["opened"] += 1
            s["timeMs"]["integrate"] += o["phases"]["integrateMs"]
            s["timeMs"]["export"] += o["phases"]["exportPhaseMs"]
            s["timeMs"]["open"] += o["phases"]["openMs"]
            refused = False
            for b in o["bodies"]:
                s["occt"]["bodies"] += 1
                e = b.get("export") or {}
                for k in ("surfaces", "curves"):
                    s["export"][k].update(e.get(k, {}))
                for k in ("reflectedFaces", "movedOriginFaces", "shiftedFaces", "seams", "degenerate", "gapLines", "pastTwoPi", "reversedFaces"):
                    s["export"][k] += e.get(k, 0)
                s["gapLineBodies"] += e.get("gapLines", 0) > 0
                ref = b.get("reference") or {}
                s["timeMs"]["gprop"] += ref.get("ms", 0.0)
                r = b.get("refusal")
                if r:
                    refused = True
                    s["occt"]["refusals"][r["code"]] += 1
                    if r["code"] == "unsupported-surface":
                        s["export"]["unsupportedSurfaceKinds"][r["detail"]] += 1
                f = b.get("facts")
                if not f:
                    continue
                s["occt"]["withFacts"] += 1
                if ref.get("volume") is not None and b.get("manifold") is True:
                    g = s["vsGprop"]
                    g["bodies"] += 1
                    d = abs(f["volume"] - ref["volume"])
                    rel = d / abs(f["volume"]) if f["volume"] else 0.0
                    g["withinBound"] += d <= f["volumeBound"]
                    g["offOver1e7"] += rel > 1e-7
                    g["maxRel"] = max(g["maxRel"], rel)
                    g["worst"].append([rel, p, f["volume"], ref["volume"], f["volumeBound"]])
            s["occt"]["refusedFiles"] += refused
            if complete(o):
                s["occt"]["completeFiles"] += 1
                sh = o.get("shape") or {}
                if o.get("freeFaces") == 0 and sh.get("volume") is not None and all(
                    b.get("manifold") is True for b in o["bodies"]
                ):
                    v = sum(b["facts"]["volume"] for b in o["bodies"])
                    vb = sum(b["facts"]["volumeBound"] for b in o["bodies"])
                    w = s["wholeShape"]
                    w["files"] += 1
                    w["within"] += abs(v - sh["volume"]) <= vb
                    if v:
                        w["maxRel"] = max(w["maxRel"], abs(v - sh["volume"]) / abs(v))
        if c and complete(c):
            s["solidcore"]["completeFiles"] += 1
        if not (o and c and complete(o) and complete(c)):
            continue
        oc, cc = closed(o), closed(c)
        if not oc or len(oc) != len(cc):
            s["vsSolidcore"]["bodyCountDiffers"] += 1
            continue
        d = s["vsSolidcore"]
        d["files"] += 1
        sums = []
        for side in (oc, cc):
            sums.append([sum(b["facts"][k] for b in side) for k in ("volume", "volumeBound", "area", "areaBound")])
        (vo, vbo, ao, abo), (vc, vbc, ac, abc) = sums
        ok = abs(vo - vc) <= vbo + vbc and abs(ao - ac) <= abo + abc
        d["within"] += ok
        if not ok:
            d["violations"].append({"path": p, "volume": [vo, vc, vbo + vbc], "area": [ao, ac, abo + abc]})
        if vo:
            d["maxRelVolume"] = max(d["maxRelVolume"], abs(vo - vc) / abs(vo))
        free = list(cc)
        for b in oc:
            f = b["facts"]
            d["bodies"] += 1
            for i, m in enumerate(free):
                g = m["facts"]
                if abs(f["volume"] - g["volume"]) <= f["volumeBound"] + g["volumeBound"]:
                    d["bodiesWithin"] += 1
                    free.pop(i)
                    break
    s["vsGprop"]["worst"] = sorted(s["vsGprop"]["worst"], reverse=True)[:10]
    for k in ("surfaces", "curves", "unsupportedSurfaceKinds"):
        s["export"][k] = dict(s["export"][k].most_common())
    s["occt"]["refusals"] = dict(s["occt"]["refusals"].most_common())
    with open(out, "w") as f:
        json.dump(s, f, indent=1)
    print(json.dumps({k: v for k, v in s.items() if k != "export"}, default=str)[:4000])


if __name__ == "__main__":
    main()
