import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

import { readNames } from '#scripts/names.mjs';

const names = readNames();
const workflow = readFileSync(new URL('../../.github/workflows/licence-materials.yml', import.meta.url), 'utf8');
const checker = fileURLToPath(
  new URL(`../../${names.engine.root}/scripts/check-licence-materials.py`, import.meta.url),
);
const scratch = mkdtempSync(join(tmpdir(), 'licence-materials-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

/** @type {(name: string, options: {crate?: string, kit?: string, text?: string}) => string} */
const tree = (name, { crate = 'sha2@0.11.0', kit = 'a', text = 'MIT' }) => {
  const root = join(scratch, name);
  mkdirSync(join(root, 'cargo'), { recursive: true });
  writeFileSync(join(root, 'cargo/x-LICENSE-MIT'), text);
  writeFileSync(join(root, 'SOURCE-RELINK.json'), JSON.stringify({ artifact: { sha256: kit } }));
  const [crateName, version] = crate.split('@');
  writeFileSync(
    join(root, 'components.json'),
    JSON.stringify({
      cohort: 'node',
      cargoComponents: [{ cohort: 'node', name: crateName, version }],
      materialFiles: [
        { path: 'cargo/x-LICENSE-MIT', sha256: text },
        { path: 'SOURCE-RELINK.json', sha256: kit },
      ],
      sourceRelink: { artifact: { sha256: kit } },
      currentMixedWasm: { sourceRevision: kit },
    }),
  );
  return root;
};

/** @type {(generated: string, committed: string) => {status: number | null, stdout: string}} */
const check = (generated, committed) => {
  const result = spawnSync('python3', ['-I', checker, generated, committed], { encoding: 'utf8' });
  return { status: result.status, stdout: result.stdout };
};

describe('licence materials workflow', () => {
  it('runs each kit part on its producer host on dispatch and on changes to itself', () => {
    const job = (name) =>
      workflow.slice(
        workflow.indexOf(`\n  ${name}:\n`),
        workflow.indexOf('\n    steps:', workflow.indexOf(`\n  ${name}:\n`)),
      );
    assert.match(job('native-part'), /^ {4}runs-on: macos-15$/mu);
    assert.match(job('mixed-part'), /^ {4}runs-on: ubuntu-24\.04$/mu);
    assert.match(job('merge'), /^ {4}needs: \[native-part, mixed-part\]$/mu);
    assert.match(workflow, /^ {2}workflow_dispatch:$/mu);
    assert.match(workflow, /^ {6}- \.github\/workflows\/licence-materials\.yml$/mu);
  });

  it('reads only repository contents and pins every action by SHA', () => {
    assert.match(workflow, /^permissions:\n {2}contents: read\n\n/mu);
    assert.doesNotMatch(workflow, /write/u);
    const uses = [...workflow.matchAll(/uses: (\S+)/gu)].map((match) => match[1]);
    assert.ok(uses.length > 0);
    for (const action of uses) {
      assert.ok(action?.startsWith('./') || /@[0-9a-f]{40}$/u.test(action ?? ''), `${action} is not pinned`);
    }
  });

  it('generates both cohorts through the generator against one kit', () => {
    for (const route of ['native-part', 'mixed-part', 'merge-parts']) {
      assert.ok(workflow.includes(`/scripts/ci-artifacts.mjs" ${route}`), route);
    }
    assert.ok(workflow.includes('--cohort python'));
    assert.ok(workflow.includes('--relink-reference out/relink-merge/source-relink'));
  });
  it('prints the trees for transfer only on dispatch', () => {
    const step = workflow.slice(workflow.indexOf('- name: Print the generated trees for transfer'));
    assert.match(step, /^ {8}if: github\.event_name == 'workflow_dispatch'$/mu);
    assert.ok(step.indexOf('BEGIN LICENCE MATERIALS BASE64') < step.indexOf('- name: Require the committed trees'));
  });
});

describe('check-licence-materials.py', () => {
  it('accepts a tree whose only difference is the relink binding', () => {
    const generated = tree('binding-generated', { kit: 'b' });
    const committed = tree('binding-committed', { kit: 'a' });
    const result = check(generated, committed);
    assert.equal(result.status, 0, result.stdout);
    assert.match(result.stdout, /build binding \(not compared\)/u);
  });

  it('refuses a tree with a different crate closure', () => {
    const result = check(
      tree('crate-generated', { crate: 'sha2@0.11.0' }),
      tree('crate-committed', { crate: 'sha2@0.10.9' }),
    );
    assert.equal(result.status, 1);
    assert.match(result.stdout, /crate added: node:sha2@0\.11\.0/u);
    assert.match(result.stdout, /crate removed: node:sha2@0\.10\.9/u);
  });

  it('refuses changed, missing and extra licence texts', () => {
    const generated = tree('text-generated', { text: 'Apache' });
    const committed = tree('text-committed', {});
    cpSync(join(committed, 'cargo/x-LICENSE-MIT'), join(committed, 'cargo/y-LICENSE-MIT'));
    const result = check(generated, committed);
    assert.equal(result.status, 1);
    assert.match(result.stdout, /content differs: cargo\/x-LICENSE-MIT/u);
    assert.match(result.stdout, /no longer generated: cargo\/y-LICENSE-MIT/u);
  });

  it('refuses arguments that are not licence trees', () => {
    assert.equal(check(scratch, scratch).status, 2);
  });
});
