import { describe, expect, it } from 'vitest';
import { defaultGeoSpecSubjectRetention, retainedSubjectCount } from '#runner/node/subject-retention.js';

describe('retainedSubjectCount', () => {
  const { subjects, memoryBytes } = defaultGeoSpecSubjectRetention;

  it('should keep the count up to the memory threshold and none above it', () => {
    expect(retainedSubjectCount({ subjects: 3, memoryBytes: 10 }, () => 10)()).toBe(3);
    expect(retainedSubjectCount({ subjects: 3, memoryBytes: 10 }, () => 11)()).toBe(0);
  });

  it('should take the default for a field that is absent or undefined', () => {
    expect(retainedSubjectCount({}, () => memoryBytes)()).toBe(subjects);
    expect(retainedSubjectCount({ subjects: 2, memoryBytes: undefined }, () => memoryBytes)()).toBe(2);
    expect(retainedSubjectCount({ subjects: 2, memoryBytes: undefined }, () => memoryBytes + 1)()).toBe(0);
  });

  it('should keep none when the memory threshold is not a number', () => {
    expect(retainedSubjectCount({ subjects: 2, memoryBytes: Number.NaN }, () => 0)()).toBe(0);
  });
});
