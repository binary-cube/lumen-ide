//! A manifest's signature: Ed25519 over its bytes as published, both halves in hex (as
//! `cargo xtask release-manifest` writes them). One check for every signed list the app
//! reads (its own releases, its web engine's packages).

use ring::signature::{ED25519, UnparsedPublicKey};

/// The project's public key (Ed25519, hex): what the app downloads from the project (its
/// own releases, its web engine) is trusted only when its list is signed by the secret half
/// (made once by `cargo xtask release-key`, held by the release workflow).
pub const PUBLIC_KEY: &str = include_str!("../../../../../packaging/release-key.pub");

/// Bytes from hex (whitespace around it ignored); `None` unless it's exactly `N` bytes of
/// hex digits (no sign, nothing else a number's parse would take).
pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
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
    UnparsedPublicKey::new(&ED25519, public).verify(bytes, &signature).is_ok()
}

#[cfg(test)]
#[path = "../tests/unit/signature.rs"]
mod tests;
