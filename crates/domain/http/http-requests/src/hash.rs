//! Hashes and HMACs as the HTTP client needs them (plan 77): Digest authentication (MD5,
//! SHA-256), the scripts' `crypto` (MD5, SHA-1, SHA-256, SHA-384, SHA-512, their HMACs),
//! OAuth's PKCE (SHA-256). Pure: bytes in, bytes out.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Digest;

/// The hashes known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
    Sha3_256,
    Sha3_384,
    Sha3_512,
}

impl Algorithm {
    /// By its name as scripts and Digest write it (`md5`, `SHA-256`, `sha256`…).
    pub fn named(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().replace('-', "").as_str() {
            "md5" => Some(Self::Md5),
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            "sha384" => Some(Self::Sha384),
            "sha512" => Some(Self::Sha512),
            "sha3256" | "sha3_256" => Some(Self::Sha3_256),
            "sha3384" | "sha3_384" => Some(Self::Sha3_384),
            "sha3512" | "sha3_512" => Some(Self::Sha3_512),
            _ => None,
        }
    }
}

/// `data` hashed by `algorithm`.
pub fn digest(algorithm: Algorithm, data: &[u8]) -> Vec<u8> {
    match algorithm {
        Algorithm::Md5 => md5::Md5::digest(data).to_vec(),
        Algorithm::Sha1 => sha1::Sha1::digest(data).to_vec(),
        Algorithm::Sha256 => sha2::Sha256::digest(data).to_vec(),
        Algorithm::Sha384 => sha2::Sha384::digest(data).to_vec(),
        Algorithm::Sha512 => sha2::Sha512::digest(data).to_vec(),
        Algorithm::Sha3_256 => sha3::Sha3_256::digest(data).to_vec(),
        Algorithm::Sha3_384 => sha3::Sha3_384::digest(data).to_vec(),
        Algorithm::Sha3_512 => sha3::Sha3_512::digest(data).to_vec(),
    }
}

/// `data`'s HMAC by `algorithm` with `key`.
pub fn hmac(algorithm: Algorithm, key: &[u8], data: &[u8]) -> Vec<u8> {
    fn run<M: Mac + KeyInit>(key: &[u8], data: &[u8]) -> Vec<u8> {
        let Ok(mut mac) = <M as KeyInit>::new_from_slice(key) else { return Vec::new() };
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    }
    match algorithm {
        Algorithm::Md5 => run::<Hmac<md5::Md5>>(key, data),
        Algorithm::Sha1 => run::<Hmac<sha1::Sha1>>(key, data),
        Algorithm::Sha256 => run::<Hmac<sha2::Sha256>>(key, data),
        Algorithm::Sha384 => run::<Hmac<sha2::Sha384>>(key, data),
        Algorithm::Sha512 => run::<Hmac<sha2::Sha512>>(key, data),
        Algorithm::Sha3_256 => run::<hmac::SimpleHmac<sha3::Sha3_256>>(key, data),
        Algorithm::Sha3_384 => run::<hmac::SimpleHmac<sha3::Sha3_384>>(key, data),
        Algorithm::Sha3_512 => run::<hmac::SimpleHmac<sha3::Sha3_512>>(key, data),
    }
}

/// Hexadecimal as bytes (`None`: not hexadecimal).
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

/// Bytes as lowercase hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Bytes as Base64 (`url`: the URL-safe alphabet, unpadded — PKCE's).
pub fn base64(bytes: &[u8], url: bool) -> String {
    let text = crate::curl::base64(bytes);
    if url { text.trim_end_matches('=').replace('+', "-").replace('/', "_") } else { text }
}

#[cfg(test)]
#[path = "../tests/unit/hash.rs"]
mod tests;
