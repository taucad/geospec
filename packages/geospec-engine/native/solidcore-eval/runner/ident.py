#!/usr/bin/env python3
"""Strict fact identity between two run_corpus.py JSONL files.

Usage: ident.py [--ignore k1,k2.sub,...] [--skip-refused] <ref.jsonl> <cand.jsonl>

Every key except timing and process metadata must serialize identically (floats
compare through repr, so equality is bit equality, -0.0 included). `--ignore`
leaves out keys, dotted for nested ones (`import.admission`), on both sides: the
keys a change adds or renames. `--skip-refused` leaves out the files the candidate
refuses (`open.refusal` or a top-level `refusal`), counting them. Prints the
differing files, then a summary line with engine-time totals (phases.totalMs).
Exit 0 only when every compared file in ref is present in cand and identical.
From the W-SC lane's ident.py (2026-11-05).
"""
import argparse
import json
import sys

SKIP = {"phases", "listPath", "wallS", "cpuS", "peakRssMiB", "peakThreads", "exit", "capHit"}
ROOTS = ("corpus/step/", "packages/", "docs-site/")


def key(path):
    for r in ROOTS:
        i = path.find("/" + r)
        if i >= 0:
            return path[i + 1:]
    return path


def load(p):
    d = {}
    for line in open(p):
        if line.strip():
            r = json.loads(line)
            d[key(r["listPath"])] = r
    return d


def strip(row, ignore):
    out = {k: v for k, v in row.items() if k not in SKIP and k not in ignore}
    for dotted in ignore:
        head, _, tail = dotted.partition(".")
        if tail and isinstance(out.get(head), dict):
            out[head] = {k: v for k, v in out[head].items() if k != tail}
    return out


def refused(row):
    o = row.get("open")
    return bool(row.get("refusal")) or (isinstance(o, dict) and "refusal" in o)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ignore", default="")
    ap.add_argument("--skip-refused", action="store_true")
    ap.add_argument("ref")
    ap.add_argument("cand")
    a = ap.parse_args()
    ignore = {k for k in a.ignore.split(",") if k}
    ref, cand = load(a.ref), load(a.cand)
    diffs, missing, skipped, ta, tb = [], [], 0, 0.0, 0.0
    for f, ra in ref.items():
        rb = cand.get(f)
        if rb is None:
            missing.append(f)
            continue
        if a.skip_refused and refused(rb):
            skipped += 1
            continue
        ta += (ra.get("phases") or {}).get("totalMs", 0)
        tb += (rb.get("phases") or {}).get("totalMs", 0)
        ka, kb = strip(ra, ignore), strip(rb, ignore)
        if json.dumps(ka, sort_keys=True) != json.dumps(kb, sort_keys=True):
            keys = sorted(k for k in set(ka) | set(kb)
                          if json.dumps(ka.get(k), sort_keys=True) != json.dumps(kb.get(k), sort_keys=True))
            diffs.append((f, keys))
    for f, keys in diffs:
        print("DIFF", ",".join(keys), f)
    for f in missing:
        print("MISSING", f)
    n = len(ref) - skipped
    print(f"files {len(ref)} refused-skipped {skipped} compared {n} identical {n - len(diffs) - len(missing)} "
          f"diffs {len(diffs)} missing {len(missing)}; engine {ta / 1e3:.1f}s -> {tb / 1e3:.1f}s ({ta / max(tb, 1e-9):.3f}x)")
    sys.exit(0 if not diffs and not missing else 1)


if __name__ == "__main__":
    main()
