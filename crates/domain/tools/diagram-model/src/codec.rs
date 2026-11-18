//! A page draw.io kept compressed: the text of `<diagram>` is
//! `base64(deflateRaw(encodeURIComponent(xml)))`. Read back the same way, the inflated size
//! bounded (a hostile file can't ask for gigabytes). Pages are written uncompressed (a diff
//! shows what changed), so only reading is here.

use crate::Error;

/// A page's compressed text read back into its XML; `limit`: the most bytes it may inflate
/// to.
pub(crate) fn decompress(text: &str, limit: usize) -> Result<String, Error> {
    let bytes = base64(text).ok_or_else(|| Error::Compressed("not base64".into()))?;
    let inflated =
        miniz_oxide::inflate::decompress_to_vec_with_limit(&bytes, limit).map_err(|e| {
            match e.status {
                miniz_oxide::inflate::TINFLStatus::HasMoreOutput => Error::TooLarge,
                _ => Error::Compressed("not deflate".into()),
            }
        })?;
    let encoded = String::from_utf8(inflated).map_err(|_| Error::Compressed("not text".into()))?;
    uri_decode(&encoded).ok_or_else(|| Error::Compressed("not URI-encoded".into()))
}

/// Standard base64 (whitespace skipped, padding optional); `None`: not base64.
fn base64(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' {
            break;
        }
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// JavaScript's `decodeURIComponent`: `%XX` sequences as UTF-8 bytes; `None`: a broken
/// sequence or bytes that aren't UTF-8.
fn uri_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_owned());
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
#[path = "../tests/unit/codec.rs"]
mod tests;
