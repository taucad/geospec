/* global addToLibrary, ENVIRONMENT_IS_NODE, HEAPU8 */
/**
 * Emscripten JS library linked into the engine's own WASM glue by `scripts/build-mixed-wasm.mts`.
 *
 * `geospec_host_sha256` is the import behind the core crate's `runtime::host_digest` on Emscripten:
 * under Node it digests the module's own bytes with `node:crypto` and writes the 32 digest bytes
 * back into the module's memory. Callers never supply a digest. Elsewhere (browsers, workers
 * without Node), or if Node refuses, it returns 0 and the engine computes the digest itself.
 * Both builds cap memory at 2 GiB, so every address arrives as a non-negative number.
 */
addToLibrary({
  geospec_host_sha256: (data, length, digest) => {
    if (!ENVIRONMENT_IS_NODE) {
      return 0;
    }
    try {
      // oxlint-disable-next-line unicorn/prefer-module -- The glue's own createRequire, defined only under Node.
      const value = require('node:crypto')
        .createHash('sha256')
        .update(HEAPU8.subarray(data, data + length))
        .digest();
      if (value.length !== 32) {
        return 0;
      }
      HEAPU8.set(value, digest);
      return 1;
    } catch {
      return 0;
    }
  },
});
