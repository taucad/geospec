/**
 * Renders a CAP-0 capacity report (capacity.json) as a markdown summary.
 */

const mib = (bytes) => (bytes === null || bytes === undefined ? 'n/a' : `${(bytes / 1024 ** 2).toFixed(1)} MiB`);
const seconds = (ms) => (ms === null || ms === undefined ? 'n/a' : `${(ms / 1000).toFixed(2)} s`);
const count = (value) => (value === null || value === undefined ? 'n/a' : value.toLocaleString('en-US'));
const cell = (text) =>
  String(text ?? '')
    .replaceAll('|', String.raw`\|`)
    .replaceAll('\n', ' ');

/**
 * @param {any} claim Row claim.
 * @returns {string} Short outcome text.
 */
function outcomeText(claim) {
  if (claim.counts) {
    return Object.entries(claim.counts)
      .map(([key, value]) => `${value} ${key}`)
      .join(', ');
  }
  return claim.code ? `${claim.outcome} \`${claim.code}\`` : claim.outcome;
}

const coded = (outcome, code) => (code ? `${outcome} \`${code}\`` : outcome);

function header(report) {
  const { host } = report;
  return [
    '# CAP-0 capacity measurements',
    '',
    `Run from ${report.startedAt} to ${report.finishedAt}.`,
    '',
    `Route: ${report.route}. Host: ${host.platform}-${host.arch}, ${host.cpus} CPUs, ${mib(host.memoryBytes)} memory; each probe ran under a ${mib(host.probeAddressSpaceBytes)} address-space limit and a ${seconds(host.timeoutMs)} wall-clock limit.`,
    '',
    `Hard assertions: ${report.passed ? 'all held' : `${report.violations.length} violation(s)`}. Capacity outcomes below are measurements, not assertions.`,
    '',
  ];
}

function list(title, items) {
  return items.length === 0 ? [] : [`## ${title}`, '', ...items.map((item) => `- ${cell(item)}`), ''];
}

function fixtureTable(report) {
  return [
    '## Fixtures',
    '',
    '| Fixture | Entry limits crossed | Subjects | Largest primary | Closure | Vertices | Triangles | Occurrences | Temp disk | Generate |',
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |',
    ...report.fixtures.map(
      (f) =>
        `| ${f.id} | ${(f.crosses ?? []).join(', ') || 'none'} | ${f.subjects} | ${mib(f.maxPrimaryBytes)} | ${mib(f.closureBytes)} | ${count(f.vertices)} | ${count(f.triangles)} | ${count(f.occurrences)} | ${mib(f.tempDiskBytes)} | ${seconds(f.generateMs)} |`,
    ),
    '',
  ];
}

function outcomeTable(report) {
  const lines = [
    '## Outcomes',
    '',
    'Peak RSS is the probe process high-water mark (`VmHWM`) for admission plus that one claim; for a runner timeout it is read just before the kill. Work units are the units the engine charged in that probe (`logical.chargedUnits`).',
    '',
    '| Fixture | Profile | Stage | Outcome | Peak RSS | Time | Work units |',
    '| --- | --- | --- | --- | --- | --- | --- |',
  ];
  for (const row of report.rows) {
    const { admission } = row;
    lines.push(
      `| ${row.fixture} | ${row.profile} | admission | ${cell(coded(admission.outcome, admission.code))} | ${mib(admission.peakRssBytes)} | ${seconds(admission.admitMs ?? admission.wallMs)} | |`,
      ...row.claims.map(
        (claim) =>
          `| ${row.fixture} | ${row.profile} | ${claim.claimId} | ${cell(outcomeText(claim))} | ${mib(claim.peakRssBytes)} | ${seconds(claim.claimMs ?? claim.wallMs)} | ${claim.chargedUnits ? count(Number(claim.chargedUnits)) : 'n/a'} |`,
      ),
    );
  }
  lines.push('');
  return lines;
}

function messages(report) {
  const found = [];
  for (const row of report.rows) {
    if (row.admission.message) {
      found.push(`${row.fixture} / ${row.profile} / admission: ${row.admission.message}`);
    }
    for (const claim of row.claims) {
      const message = claim.stderrTail ?? (claim.outcome === 'pass' ? null : claim.message);
      if (message) {
        found.push(`${row.fixture} / ${row.profile} / ${claim.claimId}: ${message}`);
      }
    }
  }
  return list('Refusal and failure messages', found);
}

function admissionBoundaryRows(boundaries) {
  const admission = boundaries.filter((b) => b.kind === 'admission');
  const groups = Map.groupBy(admission, (b) => `${b.limit}::${b.fixture}`);
  return [...groups.values()].map((items) => {
    const at = (position) => {
      const item = items.find((b) => b.position === position);
      return item ? `${coded(item.outcome, item.code)} (${count(item.limitValue)})` : 'n/a';
    };
    return `| ${items[0].limit} | ${items[0].fixture} | ${count(items[0].measured)} | ${at('below')} | ${at('at')} | ${at('above')} |`;
  });
}

function retentionBoundaryRows(boundaries) {
  return boundaries
    .filter((b) => b.kind === 'retention')
    .map((item) =>
      item.at
        ? `| ${item.limit} (${item.claimId}) | ${item.fixture} | ${count(item.at.limitValue)} | ${coded(item.below.outcome, item.below.code)} | ${item.at.outcome} | ${item.above.outcome} |`
        : `| ${item.limit} (${item.claimId}) | ${item.fixture} | n/a | ${cell(item.note)} | | |`,
    );
}

function boundaryTable(report) {
  if (report.boundaries.length === 0) {
    return [];
  }
  return [
    '## Injected boundaries',
    '',
    '| Limit | Fixture | Measured | Below | At | Above |',
    '| --- | --- | --- | --- | --- | --- |',
    ...admissionBoundaryRows(report.boundaries),
    ...retentionBoundaryRows(report.boundaries),
    '',
  ];
}

/**
 * @param {any} report Capacity report.
 * @returns {string} Markdown.
 */
export function renderMarkdown(report) {
  const lines = [
    ...header(report),
    ...list('Violations', report.violations),
    ...fixtureTable(report),
    ...outcomeTable(report),
    ...messages(report),
    ...boundaryTable(report),
    ...list(
      'Not run',
      report.notRun.map((item) => `${item.id}: ${item.reason}`),
    ),
  ];
  return `${lines.join('\n')}\n`;
}
