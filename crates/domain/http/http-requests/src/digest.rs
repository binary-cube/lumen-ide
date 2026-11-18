//! Digest authentication (RFC 7616, RFC 2617), written as `Authorization: Digest user
//! password`: the server's challenge (`WWW-Authenticate: Digest realm=…, nonce=…`) read,
//! the answer to it computed (MD5 or SHA-256, their `-sess`, `qop=auth`). Pure: the client
//! nonce is given.

use crate::hash::{Algorithm, digest, hex};

/// A server's Digest challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// It asks for `qop=auth` (else the old form, without it).
    pub auth_qop: bool,
    pub algorithm: Algorithm,
    /// `MD5-sess` / `SHA-256-sess`.
    pub session: bool,
    /// As the server wrote it, to be said back.
    pub algorithm_name: Option<String>,
}

/// A `WWW-Authenticate` header's Digest challenge (`None`: not one, or one asking what's
/// unknown).
pub fn challenge(header: &str) -> Option<Challenge> {
    let rest =
        header.trim().strip_prefix("Digest").or_else(|| header.trim().strip_prefix("digest"))?;
    let mut fields = std::collections::HashMap::new();
    let mut text = rest.trim();
    while !text.is_empty() {
        let (key, after) = text.split_once('=')?;
        let key = key.trim().trim_start_matches(',').trim().to_ascii_lowercase();
        let after = after.trim_start();
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = after.find(',').unwrap_or(after.len());
            (after[..end].trim(), &after[end..])
        };
        fields.insert(key, value.to_owned());
        text = next.trim_start().trim_start_matches(',').trim_start();
    }
    let name = fields.get("algorithm").cloned();
    let (algorithm, session) = match name.as_deref().map(str::to_ascii_uppercase).as_deref() {
        None | Some("MD5") => (Algorithm::Md5, false),
        Some("MD5-SESS") => (Algorithm::Md5, true),
        Some("SHA-256") => (Algorithm::Sha256, false),
        Some("SHA-256-SESS") => (Algorithm::Sha256, true),
        Some(_) => return None,
    };
    let auth_qop = fields.get("qop").is_some_and(|q| q.split(',').any(|q| q.trim() == "auth"));
    Some(Challenge {
        realm: fields.get("realm")?.clone(),
        nonce: fields.get("nonce")?.clone(),
        opaque: fields.get("opaque").cloned(),
        auth_qop,
        algorithm,
        session,
        algorithm_name: name,
    })
}

/// The `Authorization` header answering `challenge` for `user`, `password`, request
/// `method` of `uri` (its path and query), with client nonce `cnonce`, the `nc`th use of the
/// server's nonce.
pub fn authorization(
    challenge: &Challenge,
    (user, password): (&str, &str),
    (method, uri): (&str, &str),
    cnonce: &str,
    nc: u32,
) -> String {
    let h = |text: &str| hex(&digest(challenge.algorithm, text.as_bytes()));
    let mut ha1 = h(&format!("{user}:{}:{password}", challenge.realm));
    if challenge.session {
        ha1 = h(&format!("{ha1}:{}:{cnonce}", challenge.nonce));
    }
    let ha2 = h(&format!("{method}:{uri}"));
    let nc = format!("{nc:08x}");
    let response = if challenge.auth_qop {
        h(&format!("{ha1}:{}:{nc}:{cnonce}:auth:{ha2}", challenge.nonce))
    } else {
        h(&format!("{ha1}:{}:{ha2}", challenge.nonce))
    };
    let mut header = format!(
        "Digest username=\"{user}\", realm=\"{}\", nonce=\"{}\", uri=\"{uri}\", response=\"{response}\"",
        challenge.realm, challenge.nonce
    );
    if let Some(name) = &challenge.algorithm_name {
        header.push_str(&format!(", algorithm={name}"));
    }
    if challenge.auth_qop {
        header.push_str(&format!(", qop=auth, nc={nc}, cnonce=\"{cnonce}\""));
    }
    if let Some(opaque) = &challenge.opaque {
        header.push_str(&format!(", opaque=\"{opaque}\""));
    }
    header
}

/// `Digest user password` as a request writes it: the two (`None`: not that).
pub fn credentials(value: &str) -> Option<(String, String)> {
    let rest = value.trim().strip_prefix("Digest ")?;
    let (user, password) = rest.trim().split_once(char::is_whitespace)?;
    // A header already answering a challenge (`username="…", realm=…`) isn't the short form
    // (a password may hold a `=`: still the short form, never sent as it is).
    if user.contains('=') {
        return None;
    }
    Some((user.to_owned(), password.trim().to_owned()))
}

#[cfg(test)]
#[path = "../tests/unit/digest.rs"]
mod tests;
