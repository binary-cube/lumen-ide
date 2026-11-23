//! Stable hashes: the same value for the same bytes in every run and every build (unlike
//! the standard hasher's random seed), for what's kept or compared across runs — a file
//! name made from a path, a key a state is kept under, a fingerprint of a text. Not for
//! security: anything hostile can collide them.

/// FNV-1a's 64-bit offset basis and prime.
const FNV64: (u64, u64) = (0xcbf2_9ce4_8422_2325, 0x0100_0000_01b3);
/// FNV-1a's 32-bit offset basis and prime.
const FNV32: (u32, u32) = (2_166_136_261, 16_777_619);

/// 64-bit FNV-1a of `bytes`.
pub fn fnv64(bytes: impl AsRef<[u8]>) -> u64 {
    bytes.as_ref().iter().fold(FNV64.0, |h, b| (h ^ u64::from(*b)).wrapping_mul(FNV64.1))
}

/// 32-bit FNV-1a of `bytes` (a choice among a few: a color for a name).
pub fn fnv32(bytes: impl AsRef<[u8]>) -> u32 {
    bytes.as_ref().iter().fold(FNV32.0, |h, b| (h ^ u32::from(*b)).wrapping_mul(FNV32.1))
}

#[cfg(test)]
#[path = "../tests/unit/hash.rs"]
mod tests;
