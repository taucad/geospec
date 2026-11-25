#!/usr/bin/env python3
"""V1c driver: `geospec run` process wall and peak process-tree RSS, latest against next, store off / empty / warm,
with and without a sync shim. Measurement only; never merged.

usage:
  drive.py mkproj <out> <list> <repo root>
  drive.py run <plan> --work W --node NODE --inst a=DIR --inst c=DIR [--proj P] [--reps N] [--deadline EPOCH]
               [--count-shim LIB] [--nosync-shim LIB] [--preload-var DYLD_INSERT_LIBRARIES|LD_PRELOAD]
plans:
  warmup  one store-off run per version (not a measured row)
  313     per round (A/B order flips each round): off a|c, nosync-off c, empty a|c, nosync-empty c, count-empty c,
          warm a|c (the store this round's empty run filled), count-warm c (the store count-empty filled)
  full    next only, by priority, skipping a row that would not fit before --deadline: off, empty, nosync-empty,
          nosync-off
Rows -> W/rows.tsv (header in COLUMNS); reports -> W/raw/<label>.json.gz; masked sha256 in the row.
"""
import argparse, gzip, hashlib, json, os, shutil, signal, subprocess, sys, threading, time

COLUMNS = ['label', 'plan', 'ver', 'store', 'shim', 'round', 'wall_s', 'rc', 'load1', 'peak_rss_mib', 'procs',
           'fsync_n', 'fsync_ms', 'fdatasync_n', 'fdatasync_ms', 'fullfsync_n', 'fullfsync_ms', 'barrier_n',
           'barrier_ms', 'claims', 'masked_sha', 'utc']
MASK = {'durationMs', 'loadId', 'bundleSha256'}

SPEC = """import {{ describe, expectGeo, it }} from 'geospec';
import {{ loadModel }} from 'geospec/model';

const model = () => loadModel({{ source: '{f}', format: 'step' }});

describe('{n}', () => {{
  it('loads without error diagnostics', async () => {{ expectGeo(await model()).toHaveNoDiagnostics({{ severities: ['error'] }}); }});
  it('is a valid BRep with closed shells', async () => {{ expectGeo(await model()).toBeValidBrep({{ closedShells: true }}); }});
  it('has its bounding box', async () => {{ expectGeo(await model()).toHaveBoundingBox({{ size: {{ x: 1, y: 1, z: 1 }} }}); }});
  it('has its volume', async () => {{ expectGeo(await model()).toHaveVolume({{ value: 1 }}); }});
  it('has its surface area', async () => {{ expectGeo(await model()).toHaveSurfaceArea({{ value: 1 }}); }});
  it('has its centre of mass', async () => {{ expectGeo(await model()).toHaveCenterOfMass({{ point: [0, 0, 0] }}); }});
}});
"""


def mkproj(out, lst, root):
    """The M17 project: one byte-identical spec per STEP file (hard links), SOURCES.tsv maps mNNNN to the path."""
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, 'SOURCES.tsv'), 'w') as src:
        for i, line in enumerate(open(lst), 1):
            p = line.strip()
            if not p:
                continue
            n = 'm%04d' % i
            f = n + (os.path.splitext(p)[1].lower() or '.step')
            full = os.path.join(root, p)
            with open(full, 'rb') as fh:
                if fh.read(24).startswith(b'version https://git-lfs'):
                    sys.exit(f'LFS pointer, not content: {p}')
            dst = os.path.join(out, f)
            if not os.path.exists(dst):
                os.link(full, dst)
            with open(os.path.join(out, n + '.geospec.ts'), 'w') as spec:
                spec.write(SPEC.format(f=f, n=n))
            src.write(f'{n}\t{p}\n')


def norm(v):
    if isinstance(v, dict):
        return {k: (None if k in MASK else norm(x)) for k, x in v.items()}
    if isinstance(v, list):
        return [norm(x) for x in v]
    return v


def tree_rss_mib(root):
    out = subprocess.run(['ps', '-Ao', 'pid=,ppid=,rss='], capture_output=True, text=True).stdout.split('\n')
    kids, rss = {}, {}
    for line in out:
        p = line.split()
        if len(p) == 3:
            kids.setdefault(int(p[1]), []).append(int(p[0]))
            rss[int(p[0])] = int(p[2])
    tot, stack, seen = 0, [root], set()
    while stack:
        q = stack.pop()
        if q in seen:
            continue
        seen.add(q)
        tot += rss.get(q, 0)
        stack += kids.get(q, [])
    return tot / 1024


def count_claims(rep):
    n = 0
    def walk(v):
        nonlocal n
        if isinstance(v, dict):
            if 'status' in v and ('claim' in v or 'matcher' in v or 'title' in v):
                n += 1
            for x in v.values():
                walk(x)
        elif isinstance(v, list):
            for x in v:
                walk(x)
    walk(rep)
    return n


class Driver:
    def __init__(self, a):
        self.a = a
        self.w = a.work
        self.inst = dict(x.split('=', 1) for x in a.inst)
        for d in ('raw', 'stores', 'synclog'):
            os.makedirs(os.path.join(self.w, d), exist_ok=True)
        self.rows_path = os.path.join(self.w, 'rows.tsv')
        if not os.path.exists(self.rows_path):
            with open(self.rows_path, 'w') as f:
                f.write('\t'.join(COLUMNS) + '\n')

    def remaining(self):
        return (self.a.deadline - time.time()) if self.a.deadline else float('inf')

    def cli(self, ver):
        return os.path.join(self.inst[ver], 'node_modules', 'geospec', 'dist', 'cli', 'main.mjs')

    def run(self, label, plan, ver, store, shim, rnd, proj):
        """One measured `geospec run`. store: 'off' or a directory; shim: none|count|nosync."""
        env = dict(os.environ, GEOSPEC_FACTS_STORE=store)
        for k in ('DYLD_INSERT_LIBRARIES', 'LD_PRELOAD', 'V1C_SYNC_LOG'):
            env.pop(k, None)
        logdir = os.path.join(self.w, 'synclog', label)
        shutil.rmtree(logdir, ignore_errors=True)
        if shim != 'none':
            os.makedirs(logdir)
            env[self.a.preload_var] = self.a.count_shim if shim == 'count' else self.a.nosync_shim
            env['V1C_SYNC_LOG'] = logdir
        limit = self.remaining() - 90
        if limit < 60:
            return self.skip(label, plan, ver, store, shim, rnd, 'no time left')
        # Node directly, never through /usr/bin/env: SIP strips DYLD_* from protected binaries.
        cmd = [self.a.node, self.cli(ver), 'run', proj, '--json', '--shard-timeout', str(self.a.shard_timeout)]
        raw = os.path.join(self.w, 'raw', label + '.json')
        load = os.getloadavg()[0]
        t0 = time.monotonic()
        with open(raw, 'wb') as out, open(os.path.join(self.w, 'raw', label + '.err'), 'wb') as err:
            p = subprocess.Popen(cmd, cwd=self.inst[ver], env=env, stdout=out, stderr=err, start_new_session=True)
            peak = [0.0]
            def sample():
                while p.poll() is None:
                    peak[0] = max(peak[0], tree_rss_mib(p.pid))
                    time.sleep(0.5)
            sampler = threading.Thread(target=sample, daemon=True)
            sampler.start()
            killed = False
            try:
                p.wait(timeout=limit)
            except subprocess.TimeoutExpired:
                os.killpg(p.pid, signal.SIGKILL)
                killed = True
                p.wait()
        wall = time.monotonic() - t0
        sampler.join()
        peak = peak[0]
        rc = 'killed' if killed else str(p.returncode)
        sync = [0] * 8
        procs = 0
        if shim != 'none':
            for name in os.listdir(logdir):
                with open(os.path.join(logdir, name)) as f:
                    vals = f.read().split()
                if len(vals) == 8:
                    procs += 1
                    sync = [s + int(v) for s, v in zip(sync, vals)]
        claims, sha = '', ''
        try:
            with open(raw, 'rb') as f:
                rep = json.load(f)
            claims = str(count_claims(rep))
            sha = hashlib.sha256(json.dumps(norm(rep), sort_keys=True).encode()).hexdigest()[:16]
        except (ValueError, OSError) as e:
            sha = 'unparsed:' + type(e).__name__
        with open(raw, 'rb') as f, gzip.open(raw + '.gz', 'wb', compresslevel=6) as g:
            shutil.copyfileobj(f, g)
        os.remove(raw)
        syncs = []
        for i in range(4):
            syncs += [str(sync[2 * i]), f'{sync[2 * i + 1] / 1e6:.1f}']
        row = [label, plan, ver, 'off' if store == 'off' else 'store', shim, str(rnd), f'{wall:.2f}', rc, f'{load:.2f}',
               f'{peak:.0f}', str(procs), *syncs, claims, sha, time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())]
        self.emit(row)
        return wall

    def skip(self, label, plan, ver, store, shim, rnd, why):
        row = [label, plan, ver, 'off' if store == 'off' else 'store', shim, str(rnd), '', 'skipped:' + why]
        self.emit(row + [''] * (len(COLUMNS) - len(row) - 1) + [time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())])
        return None

    def emit(self, row):
        line = '\t'.join(row)
        print('ROW\t' + line, flush=True)
        with open(self.rows_path, 'a') as f:
            f.write(line + '\n')

    def fresh(self, name):
        st = os.path.join(self.w, 'stores', name)
        shutil.rmtree(st, ignore_errors=True)
        os.makedirs(st)
        return st

    def plan_warmup(self):
        for v in sorted(self.inst):
            self.run(f'warmup-{v}', 'warmup', v, 'off', 'none', 0, self.a.proj)

    def plan_313(self):
        proj = self.a.proj
        for r in range(1, self.a.reps + 1):
            order = ['a', 'c'] if r % 2 else ['c', 'a']
            p = f'313-r{r}'
            for v in order:
                self.run(f'{p}-off-{v}', '313', v, 'off', 'none', r, proj)
            self.run(f'{p}-nosync-off-c', '313', 'c', 'off', 'nosync', r, proj)
            stores = {}
            for v in order:
                stores[v] = self.fresh(f'{p}-{v}')
                self.run(f'{p}-empty-{v}', '313', v, stores[v], 'none', r, proj)
            st = self.fresh(f'{p}-nosync-c')
            self.run(f'{p}-nosync-empty-c', '313', 'c', st, 'nosync', r, proj)
            shutil.rmtree(st, ignore_errors=True)
            stc = self.fresh(f'{p}-count-c')
            self.run(f'{p}-count-empty-c', '313', 'c', stc, 'count', r, proj)
            for v in order:
                self.run(f'{p}-warm-{v}', '313', v, stores[v], 'none', r, proj)
                shutil.rmtree(stores[v], ignore_errors=True)
            self.run(f'{p}-count-warm-c', '313', 'c', stc, 'count', r, proj)
            shutil.rmtree(stc, ignore_errors=True)

    def plan_full(self):
        proj = self.a.proj
        est_off = self.a.full_estimate
        walls = {}
        rows = [('off', 'none'), ('empty', 'none'), ('empty', 'nosync'), ('off', 'nosync')]
        for store, shim in rows:
            label = f'full-{"nosync-" if shim == "nosync" else ""}{store}-c'
            base = walls.get('off', est_off)
            est = walls.get('empty', base * self.a.empty_factor) if store == 'empty' else base
            if est * 1.1 > self.remaining() - 120:
                self.skip(label, 'full', 'c', store, shim, 1, f'estimate {est:.0f}s > remaining {self.remaining():.0f}s')
                continue
            st = 'off' if store == 'off' else self.fresh(label)
            wall = self.run(label, 'full', 'c', st, shim, 1, proj)
            if st != 'off':
                shutil.rmtree(st, ignore_errors=True)
            if wall is not None and shim == 'none':
                walls[store] = wall


def main():
    if len(sys.argv) > 1 and sys.argv[1] == 'mkproj':
        mkproj(*sys.argv[2:5])
        return
    ap = argparse.ArgumentParser()
    ap.add_argument('cmd', choices=['run'])
    ap.add_argument('plan', choices=['warmup', '313', 'full'])
    ap.add_argument('--work', required=True)
    ap.add_argument('--node', required=True)
    ap.add_argument('--inst', action='append', required=True)
    ap.add_argument('--proj', required=True)
    ap.add_argument('--reps', type=int, default=3)
    ap.add_argument('--shard-timeout', type=int, default=600000, help='ms; M17 used 120000, raised so a slow hosted core cannot time a shard out')
    ap.add_argument('--deadline', type=float, default=0)
    ap.add_argument('--count-shim', default='')
    ap.add_argument('--nosync-shim', default='')
    ap.add_argument('--preload-var', default='DYLD_INSERT_LIBRARIES')
    ap.add_argument('--full-estimate', type=float, default=1800.0, help='expected full store-off wall, seconds')
    ap.add_argument('--empty-factor', type=float, default=2.0, help='expected full empty / store-off')
    a = ap.parse_args()
    a.work = os.path.abspath(a.work)
    a.proj = os.path.abspath(a.proj)
    d = Driver(a)
    getattr(d, 'plan_' + {'warmup': 'warmup', '313': '313', 'full': 'full'}[a.plan])()


if __name__ == '__main__':
    main()
