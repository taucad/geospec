/**
 * Byte equality for retained model inputs and bundler reads.
 *
 * @module
 */

/**
 * Whether two byte arrays hold the same bytes. Compares four bytes at a time where both views are aligned, which is
 * about ten times faster than a per-byte callback on a model of tens of megabytes; the answer is the same.
 *
 * @param left - The first bytes.
 * @param right - The second bytes.
 * @returns True when both have the same length and the same bytes.
 * @internal
 */
export const bytesEqual = (left: Uint8Array, right: Uint8Array): boolean => {
  const length = left.byteLength;
  if (length !== right.byteLength) {
    return false;
  }
  if (left.buffer === right.buffer && left.byteOffset === right.byteOffset) {
    return true;
  }
  let index = 0;
  if (left.byteOffset % 4 === 0 && right.byteOffset % 4 === 0) {
    const words = Math.floor(length / 4);
    const leftWords = new Uint32Array(left.buffer, left.byteOffset, words);
    const rightWords = new Uint32Array(right.buffer, right.byteOffset, words);
    for (; index < words; index += 1) {
      if (leftWords[index] !== rightWords[index]) {
        return false;
      }
    }
    index = words * 4;
  }
  for (; index < length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }
  return true;
};
