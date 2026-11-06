#!/usr/bin/env python3
"""Run one harness process per STEP file with time and memory caps.

Records wall time, child CPU time (user + system from wait4), exact peak RSS
(ru_maxrss), the peak thread count sampled from /proc, exit status and the
harness's JSON line. Resumable: files already in <out.jsonl> are skipped.

Usage: run_corpus.py <list.txt> <out.jsonl> <timeout-s> <rss-cap-mib> -- <cmd...>
(the file path is appended to the command). Adapted from the 2026-10-18 corpus
runner (tau-brain research/artifacts/geospec-step-corpus-kernel-benchmark).
"""
import json, os, sys, time, subprocess

lst, out, limit, cap = sys.argv[1], sys.argv[2], float(sys.argv[3]), int(sys.argv[4])
cmd = sys.argv[sys.argv.index("--") + 1:]
files = [l.strip() for l in open(lst) if l.strip()]
done = set()
if os.path.exists(out):
    for l in open(out):
        try:
            done.add(json.loads(l)["listPath"])
        except Exception:
            pass
with open(out, "a") as sink:
    for f in files:
        if f in done:
            continue
        start = time.monotonic()
        cap_out = open(out + ".stdout", "w+b")
        proc = subprocess.Popen(cmd + [f], stdout=cap_out, stderr=subprocess.DEVNULL)
        reason = None
        peak_threads = 0
        while True:
            pid, status, ru = os.wait4(proc.pid, os.WNOHANG)
            if pid:
                usage, code = ru, os.waitstatus_to_exitcode(status)
                break
            rss = 0
            try:
                for line in open(f"/proc/{proc.pid}/status"):
                    if line.startswith("Threads:"):
                        peak_threads = max(peak_threads, int(line.split()[1]))
                    elif line.startswith("VmRSS:"):
                        rss = int(line.split()[1]) >> 10
            except OSError:
                pass
            if rss > cap and not reason:
                reason = f"rss cap {cap} MiB"
                proc.kill()
            elif time.monotonic() - start > limit and not reason:
                reason = f"time cap {limit:g} s"
                proc.kill()
            time.sleep(0.005)
        wall = time.monotonic() - start
        cap_out.seek(0)
        stdout = cap_out.read().decode(errors="replace").strip()
        cap_out.close()
        row = None
        for l in reversed(stdout.splitlines()):
            if l.startswith('{"'):
                try:
                    row = json.loads(l)
                    break
                except Exception:
                    pass
        if row is None:
            row = {"open": {"error": reason or (f"signal {-code}" if code < 0 else f"process exit {code}")}}
        if reason:
            row["capHit"] = reason
        row["listPath"] = f
        row["wallS"] = round(wall, 4)
        row["cpuS"] = round(usage.ru_utime + usage.ru_stime, 4)
        row["exit"] = code
        row["peakRssMiB"] = round(usage.ru_maxrss / 1024, 1)
        row["peakThreads"] = peak_threads
        sink.write(json.dumps(row) + "\n")
        sink.flush()
