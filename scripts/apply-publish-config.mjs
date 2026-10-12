#!/usr/bin/env node

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/** The publishConfig fields pnpm applies at publish time and npm pack ignores. */
const PUBLISH_FIELDS = ['exports', 'imports'];

/** Return the manifest with its publishConfig exports and imports promoted to the top level. */
export const applyPublishConfig = (manifest) => {
  const { publishConfig = {}, ...rest } = manifest;
  const remaining = { ...publishConfig };
  const promoted = {};
  for (const field of PUBLISH_FIELDS) {
    if (field in remaining) {
      promoted[field] = remaining[field];
      delete remaining[field];
    }
  }
  return { ...rest, ...promoted, publishConfig: remaining };
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [file] = process.argv.slice(2);
  if (!file) {
    console.error('Usage: apply-publish-config.mjs <package.json>');
    process.exit(2);
  }
  const manifest = JSON.parse(readFileSync(file, 'utf8'));
  writeFileSync(file, `${JSON.stringify(applyPublishConfig(manifest), null, 2)}\n`);
}
