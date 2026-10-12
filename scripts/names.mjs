/**
 * Reads `names.json`, the single declaration of GeoSpec's published names,
 * engine location and NAPI platform rows. Scripts import it from here; YAML
 * reads the same values through `scripts/ci-plan.mjs` outputs.
 */
import { readFileSync } from 'node:fs';

export const NAMES_URL = new URL('../names.json', import.meta.url);

/** @returns {any} The parsed `names.json`. */
export const readNames = (url = NAMES_URL) => JSON.parse(readFileSync(url, 'utf8'));
