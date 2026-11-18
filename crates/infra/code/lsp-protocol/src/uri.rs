//! Paths as `file://` URIs and back, the way servers write them: percent-encoded, a
//! Windows drive as `/C:/…` (or `/c%3A/…`, as `vscode-uri` encodes it).

use std::path::{Path, PathBuf};

/// The URI of `path` (absolute).
pub fn from_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::with_capacity(text.len() + 8);
    out.push_str("file://");
    if !text.starts_with('/') {
        // `C:/x` → `/C:/x`.
        out.push('/');
    }
    for (i, b) in text.bytes().enumerate() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~')
            // A drive's colon stays as it is (`/C:/`).
            || (b == b':' && i == 1 && text.as_bytes()[0].is_ascii_alphabetic());
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path a `file://` URI names (`None`: another scheme, or not one).
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://host/share` (a UNC path) is kept as the host's path.
    let (host, path) = match rest.find('/') {
        Some(0) => ("", rest),
        Some(i) => (&rest[..i], &rest[i..]),
        None => return None,
    };
    let decoded = decode(path)?;
    let bytes = decoded.as_bytes();
    // `/C:/x` (or `/c:/x`) on Windows: the drive, without the slash before it.
    let drive =
        bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    if drive {
        let text = format!("{}{}", decoded[1..2].to_ascii_uppercase(), &decoded[2..]);
        return Some(PathBuf::from(if cfg!(windows) { text.replace('/', "\\") } else { text }));
    }
    if !host.is_empty() && host != "localhost" {
        let unc = format!("//{host}{decoded}");
        return Some(PathBuf::from(if cfg!(windows) { unc.replace('/', "\\") } else { unc }));
    }
    Some(PathBuf::from(decoded))
}

/// `%xx` escapes decoded (`None`: a bad escape, or not UTF-8).
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
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
#[path = "../tests/unit/uri.rs"]
mod tests;
