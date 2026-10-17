#!/usr/bin/env python3
"""Runs one harness command per corpus file with a time and RSS cap and
records the exact peak RSS (ru_maxrss of the child) in each JSON line.
Usage: run_corpus.py <out.jsonl> <timeout-s> <rss-cap-mib> -- <cmd...> (file appended)"""
import json, os, resource, subprocess, sys, time
out, limit, cap = sys.argv[1], float(sys.argv[2]), int(sys.argv[3])
cmd = sys.argv[sys.argv.index("--") + 1:]
here = os.path.dirname(os.path.abspath(__file__))
corpus = os.environ.get("CORPUS", os.path.join(here, "corpus.txt"))
files = [l.strip() for l in open(corpus) if l.strip()]
out = os.path.abspath(out)
os.chdir(os.path.join(here, "../../../../.."))
with open(out, "w") as sink:
    for f in files:
        start = time.monotonic()
        capture = open(out + ".stdout", "w+b")
        proc = subprocess.Popen(cmd + [f], stdout=capture, stderr=subprocess.DEVNULL)
        reason = None
        usage = None
        while True:
            pid, status, ru = os.wait4(proc.pid, os.WNOHANG)
            if pid:
                usage, code = ru, os.waitstatus_to_exitcode(status)
                break
            try:
                rss = int(open(f"/proc/{proc.pid}/statm").read().split()[1]) * 4096 >> 20
            except OSError:
                rss = 0
            if rss > cap:
                reason = f"rss cap {cap} MiB"; proc.kill()
            elif time.monotonic() - start > limit:
                reason = f"time cap {limit:g} s"; proc.kill()
            time.sleep(0.05)
        wall = time.monotonic() - start
        capture.seek(0)
        stdout = capture.read().decode(errors="replace").strip()
        capture.close()
        try:
            # OCCT prints parser messages on stdout after the harness line.
            row = json.loads(next(l for l in reversed(stdout.splitlines()) if l.startswith('{"')))
        except Exception:
            row = {"file": f, "open": {"error": reason or f"process exit {code}"}}
        row["wallS"] = round(wall, 3)
        row["exit"] = code
        row["peakRssMiB"] = round(usage.ru_maxrss / 1024, 1)
        sink.write(json.dumps(row) + "\n"); sink.flush()
