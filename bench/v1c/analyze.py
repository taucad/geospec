#!/usr/bin/env python3
"""V1c analysis: per-arm wall table, ratios, sync counts and report identity from drive.py's rows and reports.

usage: analyze.py <work dir> [<markdown out>]
Prints TSV blocks (SUMMARY, RATIOS, IDENTITY) and writes <work>/summary.tsv, <work>/identity.txt and, when
given, a Markdown summary for GITHUB_STEP_SUMMARY.
Identity: reports are compared with durationMs, loadId and bundleSha256 masked; within a plan every report is
compared test by test with that plan's first `off-c` report; each differing test is listed with its status and
the status and diagnostic codes of each claim report (a refusal difference shows there).
"""
import csv, gzip, json, os, statistics, sys

MASK = {'durationMs', 'loadId', 'bundleSha256'}


def norm(v):
    if isinstance(v, dict):
        return {k: (None if k in MASK else norm(x)) for k, x in v.items()}
    if isinstance(v, list):
        return [norm(x) for x in v]
    return v


def codes(diags):
    out = []
    for d in diags or []:
        if isinstance(d, dict):
            out.append(str(d.get('code') or d.get('reason') or d.get('kind') or d.get('severity')))
    return out


def tests(path):
    with gzip.open(path) as f:
        rep = json.load(f)
    out = {}
    for fl in rep.get('files', []):
        for t in fl.get('tests', []):
            key = (os.path.basename(fl.get('file', '')), t.get('ordinal'), t.get('name'))
            reports = [(r.get('status'), (r.get('result') or {}).get('status'), codes(r.get('diagnostics')) +
                        codes((r.get('result') or {}).get('diagnostics'))) for r in t.get('reports', [])]
            out[key] = (t.get('status'), reports, codes(t.get('diagnostics')),
                        json.dumps(norm(t), sort_keys=True))
    return out


def main():
    w = sys.argv[1]
    md_out = sys.argv[2] if len(sys.argv) > 2 else None
    rows = list(csv.DictReader(open(os.path.join(w, 'rows.tsv')), delimiter='\t'))
    measured = [r for r in rows if r['wall_s'] and r['plan'] != 'warmup']
    groups = {}
    for r in measured:
        groups.setdefault((r['plan'], r['ver'], r['store'], r['shim'], 'warm' if '-warm-' in r['label'] else
                           ('empty' if r['store'] == 'store' else 'off')), []).append(r)
    lines = ['plan\tver\trow\tshim\tn\tbest_s\tmedian_s\tmax_s\tpeak_rss_mib_max\tfullfsync_n\tfullfsync_ms\t'
             'fsync_n\tfsync_ms\tbarrier_n\tbarrier_ms\trc']
    best = {}
    order = {'off': 0, 'empty': 1, 'warm': 2}
    for k in sorted(groups, key=lambda k: (k[0], k[1], order[k[4]], k[3])):
        g = groups[k]
        walls = [float(r['wall_s']) for r in g]
        plan, ver, _, shim, row = k
        best[(plan, ver, row, shim)] = min(walls)
        med = lambda c: statistics.median(float(r[c] or 0) for r in g)
        lines.append('\t'.join(map(str, [plan, ver, row, shim, len(g), f'{min(walls):.2f}',
                                         f'{statistics.median(walls):.2f}', f'{max(walls):.2f}',
                                         max(int(r['peak_rss_mib'] or 0) for r in g),
                                         int(med('fullfsync_n')), f"{med('fullfsync_ms'):.0f}", int(med('fsync_n')),
                                         f"{med('fsync_ms'):.0f}", int(med('barrier_n')), f"{med('barrier_ms'):.0f}",
                                         ','.join(sorted({r['rc'] for r in g}))])))
    summary = '\n'.join(lines)
    open(os.path.join(w, 'summary.tsv'), 'w').write(summary + '\n')
    print('SUMMARY\n' + summary)

    rat = ['plan\tratio\tvalue']
    for plan in sorted({k[0] for k in best}):
        b = lambda ver, row, shim='none': best.get((plan, ver, row, shim))
        for ver in ('a', 'c'):
            if b(ver, 'off') and b(ver, 'empty'):
                rat.append(f'{plan}\t{ver} empty/off\t{b(ver, "empty") / b(ver, "off"):.3f}')
            if b(ver, 'off') and b(ver, 'warm'):
                rat.append(f'{plan}\t{ver} warm/off\t{b(ver, "warm") / b(ver, "off"):.3f}')
        for row in ('off', 'empty', 'warm'):
            if b('a', row) and b('c', row):
                rat.append(f'{plan}\tc/a {row}\t{b("c", row) / b("a", row):.3f}')
        off, emp, noff, nemp = b('c', 'off'), b('c', 'empty'), b('c', 'off', 'nosync'), b('c', 'empty', 'nosync')
        if nemp and off:
            rat.append(f'{plan}\tc nosync-empty/off\t{nemp / off:.3f}')
        if nemp and noff:
            rat.append(f'{plan}\tc nosync-empty/nosync-off\t{nemp / noff:.3f}')
        if off and emp and nemp and emp > off:
            rat.append(f'{plan}\tc empty-store overhead s (empty - off)\t{emp - off:.2f}')
            rat.append(f'{plan}\tc share of empty-store overhead removed by nosync\t{(emp - nemp) / (emp - off):.3f}')
        cemp = b('c', 'empty', 'count')
        if cemp:
            rat.append(f'{plan}\tc count-empty/empty (shim overhead)\t{cemp / emp:.3f}' if emp else '')
    ratios = '\n'.join(x for x in rat if x)
    print('RATIOS\n' + ratios)

    ident = []
    for plan in sorted({r['plan'] for r in measured}):
        pr = [r for r in measured if r['plan'] == plan and r['masked_sha']]
        shas = {}
        for r in pr:
            shas.setdefault(r['masked_sha'], []).append(r['label'])
        ident.append(f'{plan}: {len(pr)} reports, {len(shas)} distinct masked digests')
        for s, labels in shas.items():
            ident.append(f'  {s}  {len(labels)}  {" ".join(labels)}')
        refs = [r for r in pr if r['label'].endswith('-off-c') and r['shim'] == 'none']
        if len(shas) <= 1 or not refs:
            continue
        ref = refs[0]
        ref_tests = tests(os.path.join(w, 'raw', ref['label'] + '.json.gz'))
        done = {ref['masked_sha']}
        for r in pr:
            if r['masked_sha'] in done:
                continue
            done.add(r['masked_sha'])
            other = tests(os.path.join(w, 'raw', r['label'] + '.json.gz'))
            diff = [k for k in sorted(set(ref_tests) | set(other), key=str)
                    if (ref_tests.get(k) or (None,) * 4)[3] != (other.get(k) or (None,) * 4)[3]]
            ident.append(f'  {r["label"]} vs {ref["label"]}: {len(diff)} tests differ')
            for k in diff:
                a, o = ref_tests.get(k), other.get(k)
                tag = 'STATUS/REFUSAL' if (a and o and (a[0], a[1], a[2]) != (o[0], o[1], o[2])) or not (a and o) else 'values'
                ident.append(f'    {tag}  {k[0]} #{k[1]} {k[2]}: {a[:3] if a else None} -> {o[:3] if o else None}')
    identity = '\n'.join(ident)
    open(os.path.join(w, 'identity.txt'), 'w').write(identity + '\n')
    print('IDENTITY\n' + identity)

    if md_out:
        def table(tsv):
            ls = tsv.split('\n')
            hdr = ls[0].split('\t')
            return '\n'.join(['| ' + ' | '.join(hdr) + ' |', '|' + '---|' * len(hdr)] +
                             ['| ' + ' | '.join(l.split('\t')) + ' |' for l in ls[1:]])
        with open(md_out, 'a') as f:
            f.write('## V1c results\n\n' + table(summary) + '\n\n' + table(ratios) + '\n\n### Identity\n\n```\n' +
                    identity[:60000] + '\n```\n')


if __name__ == '__main__':
    main()
