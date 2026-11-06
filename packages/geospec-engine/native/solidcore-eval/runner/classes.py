#!/usr/bin/env python3
"""Admission outcome classes: OCCT against solidcore, before and after a change.

Usage: classes.py <occt.jsonl> <before.jsonl> <after.jsonl> > classes.md

Each input is a run_corpus.py JSONL (OCCT from occt/child.mjs, solidcore from the
harness). Paths are matched from their `corpus/step/`, `packages/` or `docs-site/`
component, so runs from different checkouts and hosts compare. Prints:

1. the OCCT outcome against solidcore's, before and after (the G1 matrix), with
   the parity count (same coarse class: solid, opened without a solid, refused,
   cap);
2. the S1 fail-open classes (units assumed, placements not applied, external
   references, OCCT strictness and Part 21 refusals read as solids, sentinel
   extents, hard-invariant violations) counted before and after;
3. the after-run refusal classes.
"""
import json
import math
import re
import sys
from collections import Counter

ROOTS = ("corpus/step/", "packages/", "docs-site/")


def key(path):
    for r in ROOTS:
        i = path.find("/" + r)
        if i >= 0:
            return path[i + 1:]
        if path.startswith(r):
            return path
    return path


def load(p):
    out = {}
    for line in open(p):
        if line.strip():
            r = json.loads(line)
            out[key(r["listPath"])] = r
    return out


def occt(r):
    """OCCT outcome: (coarse, fine)."""
    if r is None:
        return ("missing", "missing")
    if r.get("capHit"):
        return ("cap", "cap")
    if r.get("open") == "ok":
        v = (r.get("checks", {}).get("volume") or {}).get("measured")
        t = (r.get("checks", {}).get("topologyCounts") or {}).get("measured") or {}
        if isinstance(v, (int, float)) and t.get("solids", 0) > 0:
            return ("solid", "solid")
        return ("open", "opened, no solid")
    e = str((r.get("open") or {}).get("error", ""))
    if "Incorrect syntax" in e or "Undefined Parsing" in e or e == "STEP read failed.":
        fine = "Part 21 syntax"
    elif "for file_name" in e or "(author)" in e or "(authorisation)" in e:
        fine = "Part 21 header"
    elif "Unresolved Reference" in e:
        fine = "dangling reference"
    elif "external file references" in e:
        fine = "external reference"
    elif "XDE transfer" in e or "transfer lost" in e:
        fine = "XDE transfer"
    elif "RepresentationMap" in e:
        fine = "schema: REPRESENTATION_MAP"
    elif "Unit" in e and "Step" in e:
        fine = "schema: units"
    elif "RationalBSpline" in e:
        fine = "schema: rational B-spline"
    elif re.search(r"Step(Visual|AP242|AP203|Repr_(Mechanical|ShapeAspect|ProductDefinitionShape))", e):
        fine = "schema: PMI and presentation"
    elif "Step" in e:
        fine = "schema: geometry and topology"
    elif "subject limit" in e:
        fine = "size"
    else:
        fine = "other"
    return ("refused", "refused: " + fine)


def solidcore(r):
    """solidcore outcome: (coarse, fine)."""
    if r is None:
        return ("missing", "missing")
    if r.get("capHit"):
        return ("cap", "cap")
    if r.get("exit", 0) != 0:
        return ("crash", "crash")
    o = r.get("open")
    if isinstance(o, dict):
        ref = o.get("refusal")
        if ref:
            return ("refused", "refused: " + ref["class"])
        return ("refused", "refused: part21 parser")
    if r.get("refusal"):
        return ("refused", "refused: " + r["refusal"]["class"])
    imp = r.get("import", {})
    if imp.get("built", 0) == 0:
        if imp.get("skipped") or imp.get("buildErrors"):
            return ("refused", "refused: no body built")
        return ("open", "opened, no body")
    if partial(r):
        return ("open", "opened, not every stated solid measured")
    if r.get("volume") is not None and r.get("solids", 0) > 0:
        return ("solid", "solid")
    return ("open", "opened, no solid")


def partial(r):
    """A file whose volume leaves out a stated solid: a body failed to build or was
    skipped, or fewer bodies closed than the file states as solids."""
    imp = r.get("import", {})
    return (imp.get("built", 0) < imp.get("bodies", 0) or bool(imp.get("skipped"))
            or r.get("manifoldBodies", 0) < imp.get("solidBodies", 0))


MARKER = 1234567.0


def sentinel(b):
    """A box at solidcore's infinity marker, also scaled or rotated by a placement or
    unit (any coordinate at half the marker or more: 617 m), or not finite."""
    return bool(b) and any(not math.isfinite(x) or abs(x) >= MARKER / 2 for x in b)


def invariants(r):
    """The Mac fail-open scan's file-level hard invariants an admitted file breaks:
    a sentinel extent for any file with a box, the rest for a solid."""
    kind = solidcore(r)[0]
    if kind == "open" and sentinel((r or {}).get("bounds")):
        return ["sentinel extent"]
    if kind != "solid":
        return []
    v, b, c = r["volume"], r.get("bounds"), r.get("centroid")
    out = []
    bv = math.prod(b[i + 3] - b[i] for i in range(3)) if b else None
    dg = math.dist(b[:3], b[3:]) if b else 0
    if v <= 0:
        out.append("volume <= 0")
    if bv is not None and v > bv * (1 + 1e-6) and r["solids"] == 1:
        out.append("volume > box (1 solid)")
    if sentinel(b):
        out.append("sentinel extent")
    if r.get("import", {}).get("solidBodies") == 0:
        out.append("solids but no solid body")
    if b and c and not all(b[i] - 1e-6 * dg <= c[i] <= b[i + 3] + 1e-6 * dg for i in range(3)):
        out.append("centroid outside box")
    return out


def table(rows, head):
    print("| " + " | ".join(head) + " |")
    print("| " + " | ".join("---" if i == 0 else "--:" for i in range(len(head))) + " |")
    for r in rows:
        print("| " + " | ".join(str(x) for x in r) + " |")
    print()


def main():
    O, B, A = (load(p) for p in sys.argv[1:4])
    files = sorted(O)
    print(f"Files: {len(files)} (OCCT), before {sum(f in B for f in files)}, after {sum(f in A for f in files)}.\n")

    print("## G1: OCCT outcome against solidcore (coarse class)\n")
    for name, S in (("before", B), ("after", A)):
        c = Counter((occt(O[f])[0], solidcore(S.get(f))[0]) for f in files)
        same = sum(n for (o, s), n in c.items() if o == s)
        print(f"**{name}**: same coarse class on {same} of {len(files)} files.\n")
        table(sorted(((o, s, n) for (o, s), n in c.items()), key=lambda x: -x[2]), ["OCCT", "solidcore", "files"])

    print("## OCCT refusal class against solidcore, before → after\n")
    c = Counter()
    for f in files:
        o = occt(O[f])
        if o[0] != "solid":
            c[(o[1], solidcore(B.get(f))[0], solidcore(A.get(f))[0])] += 1
    table(sorted(((o, b, a, n) for (o, b, a), n in c.items()), key=lambda x: (x[0], -x[3])),
          ["OCCT", "solidcore before", "after", "files"])

    print("## OCCT solid against solidcore after (stricter than OCCT where refused)\n")
    c = Counter(solidcore(A.get(f))[1] for f in files if occt(O[f])[0] == "solid")
    table(sorted(c.items(), key=lambda x: -x[1]), ["solidcore after", "files"])

    print("## S1 classes, before → after\n")
    def diag(r, text):
        return any(text in d for d in (r or {}).get("import", {}).get("diagnostics", []))
    classes = [
        ("units assumed (no GLOBAL_UNIT_ASSIGNED_CONTEXT), solid", lambda f: diag(B.get(f), "no GLOBAL_UNIT_ASSIGNED_CONTEXT")),
        ("placements not applied (assembly instances), solid", lambda f: (B.get(f) or {}).get("import", {}).get("assemblyInstances", 0) > 0),
        ("placements not applied, OCCT refuses", lambda f: (B.get(f) or {}).get("import", {}).get("assemblyInstances", 0) > 0 and occt(O[f])[0] == "refused"),
        ("external reference (OCCT), solid", lambda f: occt(O[f])[1] == "refused: external reference"),
        ("OCCT schema strictness, solid", lambda f: occt(O[f])[1].startswith("refused: schema")),
        ("OCCT PMI and presentation strictness, solid", lambda f: occt(O[f])[1] == "refused: schema: PMI and presentation"),
        ("OCCT Part 21 syntax or header, solid or opened", lambda f: occt(O[f])[1] in ("refused: Part 21 syntax", "refused: Part 21 header")),
        ("OCCT XDE transfer, solid", lambda f: occt(O[f])[1] == "refused: XDE transfer"),
        ("OCCT refuses, solidcore solid", lambda f: occt(O[f])[0] == "refused"),
        ("sentinel or non-finite extent (marker scaled or rotated too), solid or opened", lambda f: "sentinel extent" in invariants(B.get(f))),
        ("partial volume: a stated solid not measured, read as solid", lambda f: partial(B.get(f) or {}) and (B.get(f) or {}).get("volume") is not None and (B.get(f) or {}).get("solids", 0) > 0),
        ("hard invariant broken (Mac scan rules), solid or opened", lambda f: bool(invariants(B.get(f)))),
    ]
    rows = []
    for name, pick in classes:
        sel = [f for f in files if pick(f)]
        def count(S, kind):
            return sum(solidcore(S.get(f))[0] == kind for f in sel)
        open_b = sum(solidcore(B.get(f))[0] == "open" for f in sel)
        open_a = sum(solidcore(A.get(f))[0] == "open" for f in sel)
        rows.append((name, len(sel), count(B, "solid"), open_b, count(A, "solid"), open_a, count(A, "refused")))
    table(rows, ["class", "files", "solid before", "opened before", "solid after", "opened after", "refused after"])

    bad_b = [f for f in files if invariants(B.get(f))]
    bad_a = [f for f in files if invariants(A.get(f))]
    vb = sum(bool((B.get(f) or {}).get("valid")) for f in bad_b)
    print(f"Hard-invariant files (Mac scan rules, file level): before {len(bad_b)} ({vb} `valid: true`), after {len(bad_a)}.\n")
    for f in bad_a:
        print(f"- after: {f} {invariants(A.get(f))}")
    print()

    print("## After: refusal classes\n")
    c = Counter(solidcore(A.get(f))[1] for f in files if solidcore(A.get(f))[0] == "refused")
    table(sorted(c.items(), key=lambda x: -x[1]), ["class", "files"])
    m = Counter((bool(r.get("manifold")), bool(r.get("oracle"))) for f, r in A.items() if solidcore(r)[0] == "solid")
    print("After, solids by `manifold` / `oracle`: " + ", ".join(f"manifold {a} oracle {b}: {n}" for (a, b), n in sorted(m.items())) + "\n")


if __name__ == "__main__":
    main()
