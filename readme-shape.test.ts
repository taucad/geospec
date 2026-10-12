import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const readme = readFileSync(resolve(import.meta.dirname, 'README.md'), 'utf8');
const names = JSON.parse(readFileSync(resolve(import.meta.dirname, 'names.json'), 'utf8')) as {
  engine: { npm: string; pypi: string };
};
const lineBudget = 220; // Origin: bounded persona-routed top page in opencascade.js.
const wordBudget = 400; // A landing page: what it is, install, one quick start, hosts.
const requiredSections = [
  '## Install',
  '## Quick start',
  '## Compatibility',
  '## Versioning and stability',
  '## Security and provenance',
  '## Links',
  '## License',
] as const;

describe('README shape', () => {
  it('should stay within the persona-routed line budget', () => {
    expect(readme.split('\n').length).toBeLessThanOrEqual(lineBudget);
  });

  it('should stay within the word budget', () => {
    expect(readme.split(/\s+/u).filter(Boolean).length).toBeLessThanOrEqual(wordBudget);
  });

  it('should describe GeoSpec in its descriptive form before the first section', () => {
    const description = readme.indexOf('GeoSpec, geometry specifications for CAD');
    expect(description).toBeGreaterThanOrEqual(0);
    expect(description).toBeLessThan(readme.search(/^## /mu));
  });

  it('should route contributors and maintainers before the first section', () => {
    const firstSection = readme.search(/^## /mu);
    expect(readme.indexOf('(CONTRIBUTING.md)')).toBeLessThan(firstSection);
    expect(readme.indexOf('(MAINTAINER.md)')).toBeLessThan(firstSection);
  });

  it.each(requiredSections)('should contain %s', (section) => {
    expect(readme).toContain(section);
  });

  it('should install and import the names declared in names.json', () => {
    expect(readme).toContain(`npm install ${names.engine.npm}`);
    expect(readme).toContain(`pip install ${names.engine.pypi}`);
    expect(readme).toContain(`from '${names.engine.npm}'`);
  });
});
