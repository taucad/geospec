#!/usr/bin/env python3
"""First G3/G4 check of the facts layer's solidcore adapter on a file list.

Usage: g3g4.py <binary> <corpus-root> <list> <files-v2.jsonl> <out-dir> [timeout-s]

Runs the binary once per file and setting: threads 1, 2 and 4 at accuracy
1e-7 (G4: the facts' bits identical), and threads 4 at accuracy 1e-10 (the
two-level convergence reference), with --reference (patch 0001's integrator
on the same B-rep). Writes runs.jsonl (one row per file and setting) and
summary.json:

- G4: per file, the bodies' facts and bounds identical at 1, 2 and 4 threads.
- G3 (two-level): |V(1e-7) - V(1e-10)| <= bound(1e-7), likewise area and each
  centroid coordinate, per body.
- G3 (truth): where R3v2's file-geometry truth is valid, |V - truth| <= bound
  on the file's summed volume and area (the adapter's B-rep is solidcore's,
  so a trim or build defect shows here as an excess over the bound).
- Differential: |V - V(patch 0001)| against the bound.
"""

import json
import os
import subprocess
import sys
import time


def run(binary, path, threads, accuracy, timeout):
    cmd = [binary, "--threads", str(threads), "--accuracy", repr(accuracy), "--reference", path]
    t = time.time()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        row = json.loads(p.stdout.strip().splitlines()[-1]) if p.stdout.strip() else {"error": p.stderr[-500:]}
    except subprocess.TimeoutExpired:
        row = {"error": "timeout"}
    except (json.JSONDecodeError, IndexError) as e:
        row = {"error": f"output: {e}"}
    row["wallS"] = time.time() - t
    return row


def facts_key(row):
    """The bodies' facts and refusals, without timings."""
    out = []
    for b in row.get("bodies", []):
        out.append([b.get("manifold"), b.get("facts"), b.get("refusal")])
    return json.dumps(out, sort_keys=True)


def file_sums(row):
    """Summed volume, area and bounds over manifold bodies; None when one refused."""
    v = a = vb = ab = 0.0
    n = 0
    for b in row.get("bodies", []):
        if b.get("manifold") is not True:
            continue
        f = b.get("facts")
        if f is None:
            return None
        v += f["volume"]
        a += f["area"]
        vb += f["volumeBound"]
        ab += f["areaBound"]
        n += 1
    return (v, vb, a, ab) if n else None


def main():
    binary, root, lst, files_v2, out = sys.argv[1:6]
    timeout = float(sys.argv[6]) if len(sys.argv) > 6 else 900
    os.makedirs(out, exist_ok=True)
    truth = {}
    for line in open(files_v2):
        r = json.loads(line)
        truth[r["path"]] = r
    paths = [p.strip() for p in open(lst) if p.strip()]
    settings = [(1, 1e-7), (2, 1e-7), (4, 1e-7), (4, 1e-10)]
    summary = {
        "files": len(paths), "g4Identical": 0, "g4Differ": [], "refusedFiles": {},
        "twoLevel": {"bodies": 0, "violations": []},
        "truth": {"files": 0, "within": 0, "violations": []},
        "reference": {"bodies": 0, "withinBound": 0, "maxRelDiff": 0.0},
        "refusalCodes": {},
    }
    with open(os.path.join(out, "runs.jsonl"), "w") as w:
        for i, rel in enumerate(paths):
            rows = {}
            for threads, acc in settings:
                row = run(binary, os.path.join(root, rel), threads, acc, timeout)
                row["path"] = rel
                row["setting"] = [threads, acc]
                w.write(json.dumps(row) + "\n")
                w.flush()
                rows[(threads, acc)] = row
            base = rows[(1, 1e-7)]
            keys = {facts_key(rows[(t, 1e-7)]) for t in (1, 2, 4)}
            if len(keys) == 1:
                summary["g4Identical"] += 1
            else:
                summary["g4Differ"].append(rel)
            for b in base.get("bodies", []):
                r = b.get("refusal")
                if r:
                    summary["refusalCodes"][r["code"]] = summary["refusalCodes"].get(r["code"], 0) + 1
                    if b.get("manifold") is True:
                        summary["refusedFiles"][rel] = r
            fine = rows[(4, 1e-10)]
            for b, f in zip(base.get("bodies", []), fine.get("bodies", [])):
                fb, ff = b.get("facts"), f.get("facts")
                if not (fb and ff):
                    continue
                summary["twoLevel"]["bodies"] += 1
                checks = [("volume", fb["volume"], ff["volume"], fb["volumeBound"]),
                          ("area", fb["area"], ff["area"], fb["areaBound"])]
                if fb.get("centroid") and ff.get("centroid"):
                    for k in range(3):
                        checks.append((f"c{k}", fb["centroid"][k], ff["centroid"][k], fb["centroidBound"][k]))
                for name, x, y, bound in checks:
                    if abs(x - y) > bound:
                        summary["twoLevel"]["violations"].append([rel, name, x, y, bound])
                ref = b.get("reference") or {}
                if "volume" in ref and ref["volume"] is not None:
                    summary["reference"]["bodies"] += 1
                    d = abs(fb["volume"] - ref["volume"])
                    if d <= fb["volumeBound"]:
                        summary["reference"]["withinBound"] += 1
                    if fb["volume"]:
                        summary["reference"]["maxRelDiff"] = max(summary["reference"]["maxRelDiff"], d / abs(fb["volume"]))
            t = truth.get(rel)
            sums = file_sums(base)
            if t and t.get("truth_ok") and sums:
                v, vb, a, ab = sums
                summary["truth"]["files"] += 1
                ev, ea = abs(v - t["truth_vol"]), abs(a - t["truth_area"])
                if ev <= vb and ea <= ab:
                    summary["truth"]["within"] += 1
                else:
                    summary["truth"]["violations"].append({
                        "path": rel, "volErr": ev, "volBound": vb, "areaErr": ea, "areaBound": ab,
                        "primary": t.get("primary"), "group": t.get("group"),
                    })
            print(f"{i + 1}/{len(paths)} {rel} {base.get('wallS', 0):.1f}s", flush=True)
    with open(os.path.join(out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps({k: v for k, v in summary.items() if not isinstance(v, (list, dict))}))


if __name__ == "__main__":
    main()
