//! SHA-256 of admitted subject bytes, always computed by the engine from the bytes themselves.
//!
//! The Emscripten module asks its own glue (`bindings/emscripten/host-sha256.js`) to digest
//! large inputs with the host's `node:crypto`, which under Node is far faster than the
//! portable code before the runtime tiers it up. The glue reads the bytes from the module's
//! memory and writes the 32 digest bytes back; when it has no synchronous digest (browsers,
//! workers) the portable implementation runs. Both spell the digest as
//! [`crate::identity::sha256_hex`] does.

use sha2::{Digest, Sha256};

/// Lowercase hexadecimal SHA-256 of `bytes`, equal to [`crate::identity::sha256_hex`].
pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    crate::identity::hex(&sha256(bytes))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    #[cfg(target_os = "emscripten")]
    if bytes.len() >= HOST_SHA256_MIN_BYTES {
        let mut digest = [0_u8; 32];
        // SAFETY: the glue reads exactly `bytes.len()` bytes from `bytes` and writes at
        // most 32 bytes to `digest`, both live for the call; it keeps neither pointer.
        if unsafe { geospec_host_sha256(bytes.as_ptr(), bytes.len(), digest.as_mut_ptr()) } == 1 {
            return digest;
        }
    }
    Sha256::digest(bytes).into()
}

/// Below this length the call into the glue costs more than the portable digest saves.
#[cfg(target_os = "emscripten")]
const HOST_SHA256_MIN_BYTES: usize = 64 * 1024;

#[cfg(target_os = "emscripten")]
extern "C" {
    /// Defined by `bindings/emscripten/host-sha256.js`: 1 when it wrote the digest of
    /// `length` bytes at `data` to the 32 bytes at `digest`, 0 when it computed nothing.
    fn geospec_host_sha256(data: *const u8, length: usize, digest: *mut u8) -> i32;
}

#[cfg(test)]
mod tests {
    #[test]
    fn spells_the_identity_digest() {
        for length in [0, 1, 64 * 1024 - 1, 64 * 1024, 200_003] {
            let bytes = (0..length)
                .map(|index| (index * 31 % 251) as u8)
                .collect::<Vec<_>>();
            assert_eq!(
                super::sha256_hex(&bytes),
                crate::identity::sha256_hex(&bytes)
            );
        }
    }
}
