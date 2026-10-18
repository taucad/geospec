import { describe, expect, it } from 'vitest';

import { nameWords } from '@/lib/name-breaks.ts';

describe('nameWords', () => {
  it('should split a matcher or API name before each capital that starts a word', () => {
    expect(nameWords('toHaveNoComponentInterference')).toStrictEqual(['to', 'Have', 'No', 'Component', 'Interference']);
    expect(nameWords('createGeoSpecAssertionClient')).toStrictEqual(['create', 'Geo', 'Spec', 'Assertion', 'Client']);
    expect(nameWords('GeoSpecModelLoadError')).toStrictEqual(['Geo', 'Spec', 'Model', 'Load', 'Error']);
  });

  it('should keep a name with no inner capital, or a run of capitals, whole', () => {
    expect(nameWords('Variations')).toStrictEqual(['Variations']);
    expect(nameWords('WASM')).toStrictEqual(['WASM']);
  });

  it('should keep text with a space whole, so its spaces stay the only breaks', () => {
    expect(nameWords('Run the spec in TypeScript')).toStrictEqual(['Run the spec in TypeScript']);
  });
});
