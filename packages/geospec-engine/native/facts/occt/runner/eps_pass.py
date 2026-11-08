#!/usr/bin/env python3
"""Collects the Eps third reference for differential.py.

Usage: eps_pass.py <geospec-facts-occt> <corpus-root> <occt-runs.jsonl> <out.jsonl> [timeout-s]

Reruns `geospec-facts-occt --threads 1 --eps-reference 1e-11` (BRepGProp at
Eps 1e-11, M1's `volumeRef` method) on every file whose 1-thread 1e-7 row in
`occt-runs.jsonl` has a body GProp misses by more than 1e-7 relative, or a
tolerance gap. The runner prints no path, so each appended row gets the
file's `path` (relative to the corpus root, as in the runs). Resumable: paths
already in `out.jsonl` are skipped. With `GEOSPEC_CPU_LOCK` set, each run
holds that file's exclusive `flock`.
"""

import json
import os
import subprocess
import sys
import time


def wanted(row):
    for b in row.get("bodies") or []:
        f, g = b.get("facts"), b.get("reference") or {}
        if f and g.get("volume") is not None:
            m = max(abs(f["volume"]), abs(g["volume"]))
            if m and abs(f["volume"] - g["volume"]) / m > 1e-7:
                return True
        if (b.get("export") or {}).get("gapLines", 0) > 0:
            return True
    return False


def main():
    binary, root, runs, out = sys.argv[1:5]
    timeout = float(sys.argv[5]) if len(sys.argv) > 5 else 3600
    done = set()
    if os.path.exists(out):
        done = {json.loads(line)["path"] for line in open(out)}
    todo = []
    for line in open(runs):
        r = json.loads(line)
        if r.get("setting") == [1, 1e-7] and r["path"] not in done and wanted(r):
            todo.append(r["path"])
    print(len(todo), "files", flush=True)
    lock = os.environ.get("GEOSPEC_CPU_LOCK")
    for p in todo:
        t = time.time()
        cmd = [binary, "--threads", "1", "--eps-reference", "1e-11", os.path.join(root, p)]
        if lock:
            cmd = ["flock", "-x", lock] + cmd
        try:
            c = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
            row = json.loads(c.stdout) if c.returncode == 0 else {"error": c.stderr[-500:], "status": c.returncode}
        except subprocess.TimeoutExpired:
            row = {"error": "timeout"}
        except json.JSONDecodeError:
            row = {"error": "output is not JSON"}
        row["path"] = p
        row["wallS"] = time.time() - t
        with open(out, "a") as f:
            f.write(json.dumps(row) + "\n")
        print(p, round(row["wallS"], 1), flush=True)


if __name__ == "__main__":
    main()
