import { describe, expect, it } from 'vitest';
import { bytesEqual } from '#support/bytes.js';

describe('bytesEqual', () => {
  const bytes = Uint8Array.from({ length: 1031 }, (_, index) => (index * 31) % 251);

  it('is true for equal bytes, aligned or not', () => {
    expect(bytesEqual(bytes, Uint8Array.from(bytes))).toBe(true);
    const shifted = new Uint8Array(bytes.byteLength + 1);
    shifted.set(bytes, 1);
    expect(bytesEqual(bytes, shifted.subarray(1))).toBe(true);
    expect(bytesEqual(new Uint8Array(0), new Uint8Array(0))).toBe(true);
  });

  it('finds a difference in any byte, including the unaligned tail', () => {
    for (const position of [0, 3, 4, 512, 1027, 1028, 1030]) {
      const other = Uint8Array.from(bytes);
      other[position] = (other[position]! + 1) % 256;
      expect(bytesEqual(bytes, other)).toBe(false);
      const shifted = new Uint8Array(other.byteLength + 3);
      shifted.set(other, 3);
      expect(bytesEqual(bytes, shifted.subarray(3))).toBe(false);
    }
  });

  it('is false for different lengths', () => {
    expect(bytesEqual(bytes, bytes.subarray(1))).toBe(false);
  });
});
