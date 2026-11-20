//! What never reaches the log whole: a secret is replaced by `***` on the thread that logs,
//! before the line is handed over (nothing whole reaches even the writer's memory).
//!
//! - **By a field's name** ([`secret_name`]): `password`, `token`, `cookie`, `api_key`… the
//!   whole value.
//! - **By a value's shape**, anywhere in a text (the message too, [`text`]): an
//!   `Authorization` scheme's credentials (`Bearer …`, `Basic …`), a URL's password
//!   (`scheme://user:***@host`), `password=…` in a connection string or a query, a private
//!   key's block, a JWT, the tokens of known shape (GitHub's, AWS keys, `sk-…`, Slack's).
//!
//! Paths stay: the local log needs them; the bug report's archive makes them anonymous.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

/// What a secret is replaced with.
pub const HIDDEN: &str = "***";

/// A field named `name` holds a secret (its case, `-` and `_` aside).
pub fn secret_name(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "password",
        "passwd",
        "pwd",
        "pass",
        "secret",
        "token",
        "accesstoken",
        "refreshtoken",
        "idtoken",
        "apikey",
        "authorization",
        "auth",
        "cookie",
        "setcookie",
        "privatekey",
        "clientsecret",
        "sessionkey",
        "passphrase",
        "credentials",
    ];
    let plain: String =
        name.chars().filter(|c| *c != '_' && *c != '-').flat_map(char::to_lowercase).collect();
    NAMES.contains(&plain.as_str())
        || plain.ends_with("password")
        || plain.ends_with("secret")
        || plain.ends_with("token")
        || plain.ends_with("apikey")
}

/// The shapes of a secret in a text, each with what keeps its context (`$1`, `$2`).
/// Written once, compiled once ([`SHAPES`]); a test sees that each one compiles.
const PATTERNS: &[(&str, &str)] = &[
    // A private key's block, whole.
    (
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)",
        "-----PRIVATE KEY ***-----",
    ),
    // An Authorization header's credentials.
    (r"(?i)\b(bearer|basic|digest)(\s+)[A-Za-z0-9._~+/=-]{6,}", "$1$2***"),
    // GitHub's `token …` scheme: only what's as long as a token (not "the token stream").
    (r"(?i)\b(token)(\s+)[A-Za-z0-9._~+/=-]{16,}", "$1$2***"),
    // A URL's password: `scheme://user:password@host`.
    (r"([a-zA-Z][a-zA-Z0-9+.-]*://[^\s:/@]*:)[^\s@/]+@", "${1}***@"),
    // `password=…`, `pwd: …` in a connection string, a query, a command line.
    (
        r#"(?i)\b((?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?token|client[_-]?secret)\s*[=:]\s*)("[^"]*"|'[^']*'|[^\s;&,"']+)"#,
        "${1}***",
    ),
    // A password in SQL: `IDENTIFIED BY '…'`, `WITH PASSWORD '…'`.
    (r#"(?i)\b(identified\s+by|password)(\s+)('[^']*'|"[^"]*")"#, "$1$2'***'"),
    // A JWT.
    (r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}", HIDDEN),
    // GitHub's tokens, AWS access keys, `sk-…` API keys, Slack's tokens.
    (r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})", HIDDEN),
    (r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b", HIDDEN),
    (r"\bsk-[A-Za-z0-9_-]{20,}", HIDDEN),
    (r"\bxox[abprs]-[A-Za-z0-9-]{10,}", HIDDEN),
];

static SHAPES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    PATTERNS.iter().filter_map(|(p, keep)| Some((Regex::new(p).ok()?, *keep))).collect()
});

/// How many shapes compiled (all of them: a test sees it).
pub fn shapes() -> usize {
    SHAPES.len()
}

/// `text` with every secret of a known shape hidden (borrowed when there's none).
pub fn text(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    for (shape, keep) in SHAPES.iter() {
        if shape.is_match(&out) {
            out = Cow::Owned(shape.replace_all(&out, *keep).into_owned());
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/redact.rs"]
mod tests;
