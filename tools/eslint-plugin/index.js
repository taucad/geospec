import { jsdocQualityRule } from '#tools/eslint-plugin/jsdoc-quality.js';

export const plugin = {
  meta: { name: 'geospec', version: '1.0.0' },
  rules: { 'jsdoc-quality': jsdocQualityRule },
};
