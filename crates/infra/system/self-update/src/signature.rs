//! A manifest's signature: Ed25519 over its bytes as published, both halves in hex (as
//! `cargo xtask release-manifest` writes them).

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Bytes from hex (whitespace around it ignored); `None` unless it's exactly `N` bytes.
fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Whether `signature` (hex) is `public`'s (hex) over `bytes`.
pub fn verify(bytes: &[u8], signature: &str, public: &str) -> bool {
    let (Some(public), Some(signature)) = (unhex::<32>(public), unhex::<64>(signature)) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&public) else { return false };
    key.verify(bytes, &Signature::from_bytes(&signature)).is_ok()
}

#[cfg(test)]
#[path = "../tests/unit/signature.rs"]
mod tests;
