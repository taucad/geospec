#!/usr/bin/env python3
"""Compares the solidcore runs with the OCCT run of the Truck spike.

Usage: python3 -I compare.py occt.jsonl solidcore.jsonl hybrid.jsonl out.json

OCCT is the oracle. `solidcore.jsonl` is `solidcore-parity solidcore` over the
corpus (connector admission); `hybrid.jsonl` is `solidcore-hybrid` (Truck's
STEP reader feeding solidcore's face integrals). Each hybrid row the OCCT run
admitted is compared on volume, area and centre of mass.
"""

import json
import math
import sys
from collections import Counter


def load(path):
    with open(path) as handle:
        return {row["file"]: row for row in map(json.loads, handle)}


def admitted(row):
    return row is not None and row.get("open") == "ok"


def rel(a, b):
    scale = max(abs(a), abs(b))
    return 0.0 if scale == 0 else abs(a - b) / scale


def band(value, limits=(1e-12, 1e-9, 1e-7, 1e-6, 1e-3)):
    if value is None:
        return "n/a"
    for limit in limits:
        if value <= limit:
            return f"<={limit:g}"
    return f">{limits[-1]:g}"


def main():
    occt, solid, hybrid, out = sys.argv[1:5]
    occt, solid, hybrid = load(occt), load(solid), load(hybrid)
    files = sorted(occt)
    connector = Counter()
    for name in files:
        row = solid.get(name)
        state = "admitted" if admitted(row) else (row or {}).get("open", {}).get("error", "no row")
        connector[state.split(":")[0]] += 1

    rows = []
    for name in files:
        h, o = hybrid.get(name), occt.get(name)
        if not admitted(h):
            rows.append({"file": name, "hybrid": (h or {}).get("open", {"error": "no row"})["error"],
                         "occtAdmits": admitted(o)})
            continue
        if not admitted(o) or not isinstance(o.get("shape"), dict) or "volume" not in o["shape"]:
            rows.append({"file": name, "hybrid": "ok", "occtAdmits": False})
            continue
        shape = o["shape"]
        diagonal = math.dist(shape["min"], shape["max"])
        # A zero summed volume leaves no centre of mass.
        finite = all(isinstance(c, (int, float)) for c in h["com"])
        com_error = math.dist(h["com"], shape["com"]) if finite else None
        rows.append({
            "file": name, "hybrid": "ok", "occtAdmits": True,
            "occtOccurrences": o.get("admission", {}).get("occurrences"),
            "volumeRel": rel(h["volume"], shape["volume"]),
            "areaRel": rel(h["area"], shape["area"]),
            "comOverDiagonal": com_error / diagonal if diagonal and com_error is not None else None,
            "comMm": com_error,
            "solidcoreMs": h["totalMs"], "integrateMs": h["integrateMs"],
            "occtShapeMs": o.get("shapeMs"), "occtOpenMs": o.get("openMs"),
        })

    compared = [r for r in rows if "volumeRel" in r]
    summary = {
        "corpusFiles": len(files),
        "connector": dict(connector),
        "hybridRefusals": dict(Counter(r["hybrid"] for r in rows if r["hybrid"] != "ok")),
        "hybridAdmitted": sum(r["hybrid"] == "ok" for r in rows),
        "hybridAdmittedOcctRefused": sum(r["hybrid"] == "ok" and not r["occtAdmits"] for r in rows),
        "compared": len(compared),
        "volumeRel": dict(Counter(band(r["volumeRel"]) for r in compared)),
        "areaRel": dict(Counter(band(r["areaRel"]) for r in compared)),
        "comOverDiagonal": dict(Counter(band(r["comOverDiagonal"]) for r in compared)),
        "solidcoreMsTotal": sum(r["solidcoreMs"] for r in compared),
        "integrateMsTotal": sum(r["integrateMs"] for r in compared),
        "occtShapeMsTotal": sum(r["occtShapeMs"] or 0 for r in compared),
        "occtOpenMsTotal": sum(r["occtOpenMs"] or 0 for r in compared),
    }
    with open(out, "w") as sink:
        json.dump({"summary": summary, "rows": rows}, sink, indent=1)
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
