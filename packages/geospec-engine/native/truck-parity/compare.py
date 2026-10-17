#!/usr/bin/env python3
"""Compares OCCT and Truck harness output (JSON lines) per corpus file.

Usage: python3 -I compare.py occt.jsonl truck.jsonl out.json

OCCT is the oracle (charter D3). Each file gets one row with admission,
topology, whole-shape, face and timing comparisons; the summary counts how
many files agree within each tolerance band.
"""

import json
import math
import sys
from collections import Counter


def load(path):
    rows = {}
    with open(path) as handle:
        for line in handle:
            row = json.loads(line)
            rows[row["file"]] = row
    return rows


def ok(row, key):
    value = row.get(key)
    return isinstance(value, (dict, list)) and not (isinstance(value, dict) and "error" in value)


def rel(a, b):
    if a is None or b is None:
        return None
    if not (math.isfinite(a) and math.isfinite(b)):
        return None
    scale = max(abs(a), abs(b))
    return 0.0 if scale == 0 else abs(a - b) / scale


def dist(p, q):
    return math.sqrt(sum((x - y) ** 2 for x, y in zip(p, q)))


def diag(shape):
    return dist(shape["min"], shape["max"])


def band(value, limits=(1e-9, 1e-6, 1e-4, 1e-3, 1e-2)):
    if value is None:
        return "n/a"
    for limit in limits:
        if value <= limit:
            return f"<={limit:g}"
    return f">{limits[-1]:g}"


def face_kinds(faces):
    return Counter(face["surface"]["kind"] for face in faces)


def compare_faces(occt_faces, truck_faces):
    """Index-aligned comparison; both connectors list whole faces in STEP order."""
    same_count = len(occt_faces) == len(truck_faces)
    kind_matches = 0
    radius_errors = []
    area_errors = []
    for a, b in zip(occt_faces, truck_faces):
        if a["surface"]["kind"] == b["surface"]["kind"]:
            kind_matches += 1
            if "radius" in a["surface"] and "radius" in b["surface"]:
                radius_errors.append(rel(a["surface"]["radius"], b["surface"]["radius"]))
        area_errors.append(rel(a["area"], b["area"]))
    finite = [e for e in area_errors if e is not None]
    finite_radius = [e for e in radius_errors if e is not None]
    return {
        "sameCount": same_count,
        "occtFaces": len(occt_faces),
        "truckFaces": len(truck_faces),
        "kindMatchRatio": kind_matches / len(occt_faces) if occt_faces else None,
        "occtKinds": dict(face_kinds(occt_faces)),
        "truckKinds": dict(face_kinds(truck_faces)),
        "maxFaceAreaRel": max(finite) if finite else None,
        "medianFaceAreaRel": sorted(finite)[len(finite) // 2] if finite else None,
        "maxRadiusRel": max(finite_radius) if finite_radius else None,
    }


def main():
    occt, truck, out = sys.argv[1], sys.argv[2], sys.argv[3]
    occt_rows, truck_rows = load(occt), load(truck)
    rows = []
    for file, a in sorted(occt_rows.items()):
        b = truck_rows.get(file, {"open": {"error": "missing"}})
        row = {
            "file": file,
            "bytes": a.get("bytes"),
            "occtOpen": a["open"] == "ok",
            "truckOpen": b["open"] == "ok",
            "truckError": None if b["open"] == "ok" else str(b["open"].get("error"))[:160],
            "occtError": None if a["open"] == "ok" else str(a["open"].get("error"))[:160],
        }
        for phase in ("openMs", "facesMs", "shapeMs", "validityMs", "occurrencesMs", "tessellateMs"):
            row[f"occt_{phase}"] = a.get(phase)
            row[f"truck_{phase}"] = b.get(phase)
        if row["occtOpen"] and row["truckOpen"]:
            if ok(a, "shape") and ok(b, "shape"):
                sa, sb = a["shape"], b["shape"]
                row["volumeRel"] = rel(sa["volume"], sb["volume"])
                row["areaRel"] = rel(sa["area"], sb["area"])
                d = diag(sa)
                row["comRelDiag"] = dist(sa["com"], sb["com"]) / d if d else None
                row["boundsRelDiag"] = max(
                    max(abs(x - y) for x, y in zip(sa["min"], sb["min"])),
                    max(abs(x - y) for x, y in zip(sa["max"], sb["max"])),
                ) / d if d else None
                row["countsEqual"] = {k: sa["counts"][k] == sb["counts"][k] for k in sa["counts"]}
                row["occtCounts"] = sa["counts"]
                row["truckCounts"] = sb["counts"]
            else:
                row["shapeError"] = {
                    "occt": a.get("shape", {}).get("error") if isinstance(a.get("shape"), dict) else None,
                    "truck": b.get("shape", {}).get("error") if isinstance(b.get("shape"), dict) else None,
                }
            if ok(a, "faces") and ok(b, "faces"):
                row["faces"] = compare_faces(a["faces"], b["faces"])
            if ok(a, "validity") and ok(b, "validity"):
                row["validity"] = {"occt": a["validity"]["valid"], "truck": b["validity"]["valid"]}
            if ok(a, "occurrences") and ok(b, "occurrences"):
                row["occurrences"] = {"occt": len(a["occurrences"]), "truck": len(b["occurrences"])}
            if ok(a, "tessellation") and ok(b, "tessellation"):
                row["triangles"] = {"occt": a["tessellation"]["triangles"], "truck": b["tessellation"]["triangles"]}
        rows.append(row)

    both = [r for r in rows if r["occtOpen"] and r["truckOpen"]]
    summary = {
        "files": len(rows),
        "occtAdmitted": sum(r["occtOpen"] for r in rows),
        "truckAdmitted": sum(r["truckOpen"] for r in rows),
        "bothAdmitted": len(both),
        "truckRefusedWhereOcctAdmitted": sum(r["occtOpen"] and not r["truckOpen"] for r in rows),
        "volumeRelBands": Counter(band(r.get("volumeRel")) for r in both),
        "areaRelBands": Counter(band(r.get("areaRel")) for r in both),
        "comRelDiagBands": Counter(band(r.get("comRelDiag")) for r in both),
        "boundsRelDiagBands": Counter(band(r.get("boundsRelDiag")) for r in both),
        "faceCountEqual": sum(1 for r in both if r.get("faces", {}).get("sameCount")),
        "allFaceKindsMatch": sum(1 for r in both if r.get("faces", {}).get("kindMatchRatio") == 1.0),
        "allCountsEqual": sum(1 for r in both if r.get("countsEqual") and all(r["countsEqual"].values())),
        "countFieldEqual": {
            k: sum(1 for r in both if r.get("countsEqual", {}).get(k)) for k in ("solids", "shells", "faces", "wires", "edges", "vertices")
        },
        "validityAgree": sum(1 for r in both if r.get("validity") and r["validity"]["occt"] == r["validity"]["truck"]),
        "occurrenceCountEqual": sum(1 for r in both if r.get("occurrences") and r["occurrences"]["occt"] == r["occurrences"]["truck"]),
        "truckErrors": Counter((r["truckError"] or "")[:90] for r in rows if r["occtOpen"] and not r["truckOpen"]),
    }
    json.dump({"summary": summary, "rows": rows}, open(out, "w"), indent=1, default=str)
    print(json.dumps(summary, indent=1, default=str))


if __name__ == "__main__":
    main()
