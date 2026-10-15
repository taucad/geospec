import { createRequire } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

import { describe, expect, it } from 'vitest';

import { formatSize, sizeCells } from '@/components/size-strip.tsx';
import sizes from '@/lib/sizes.json';

const site = resolve(import.meta.dirname, '..');
const require = createRequire(resolve(site, 'package.json'));
const manifest = JSON.parse(readFileSync(require.resolve('geospec-engine-published/package.json'), 'utf8')) as {
  readonly name: string;
  readonly version: string;
  readonly optionalDependencies?: Readonly<Record<string, string>>;
};
const compatibility = readFileSync(resolve(site, '..', 'compatibility.md'), 'utf8');
const documented = (pattern: RegExp): string[] => [
  ...new Set([...compatibility.matchAll(pattern)].map(([, name]) => name ?? '')),
];

describe('published size figures', () => {
  it('should quote the WASM module the site serves, under the compression the strip names', () => {
    const wasm = readFileSync(require.resolve('geospec-engine-published/wasm-binary'));
    expect(sizes.wasm).toEqual({
      raw: wasm.byteLength,
      gzip: gzipSync(wasm, { level: 9 }).byteLength,
      brotli: brotliCompressSync(wasm, {
        params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
      }).byteLength,
    });
  }, 120_000);

  it('should measure the release the demos run', () => {
    expect(sizes.version).toBe(manifest.version);
  });

  it('should quote every prebuild the published manifest lists, and only platforms compatibility.md documents', () => {
    const prefix = `${manifest.name}-`;
    const published = Object.keys(manifest.optionalDependencies ?? {})
      .map((name) => name.slice(prefix.length))
      .toSorted();
    expect(published).not.toEqual([]);
    expect(Object.keys(sizes.native).toSorted()).toEqual(published);
    const platforms = documented(/`geospec-engine-([\w-]+)`/gu);
    for (const platform of published) {
      expect(platforms).toContain(platform);
    }
    for (const bytes of Object.values(sizes.native)) {
      expect(bytes).toBeGreaterThan(1_000_000);
    }
  });

  it('should quote only wheels compatibility.md documents', () => {
    const wheels = documented(/`(cp\d+-[\w.]+)`/gu);
    expect(wheels).not.toEqual([]);
    for (const tag of Object.keys(sizes.wheels)) {
      // The file names the wheel `cp313-cp313-<platform>`; the table drops the repeated ABI tag.
      expect(wheels).toContain(tag.replace(/^(cp\d+)-cp\d+-/u, '$1-'));
    }
    for (const bytes of Object.values(sizes.wheels)) {
      expect(bytes).toBeGreaterThan(1_000_000);
    }
  });

  it('should have a measured figure behind every cell the strip shows', () => {
    expect(sizeCells.map(({ label }) => label)).toEqual(['Browser', 'Node', 'Python']);
    for (const { bytes } of sizeCells) {
      expect(bytes).toBeGreaterThan(1_000_000);
    }
  });

  it('should format bytes the way the strip prints them', () => {
    expect([252_013, 4_013_601, 34_726_619, 325].map((bytes) => formatSize(bytes))).toEqual([
      '252 KB',
      '4.0 MB',
      '34.7 MB',
      '0.3 KB',
    ]);
  });
});

// The export is only there after a build; the build's own test run then holds the page to the file.
const out = resolve(site, 'out/index.html');
const exportedTest = existsSync(out) ? it : it.skip;

describe('static home page', () => {
  exportedTest('should print the measured sizes', () => {
    const html = readFileSync(out, 'utf8');
    for (const { bytes } of sizeCells) {
      expect(html).toContain(formatSize(bytes));
    }
  });
});
