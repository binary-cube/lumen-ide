//! What a value is: a boolean, a number, a URL, a path or text — and whether it looks like a
//! secret (by its key's name: `APP_SECRET`, `API_TOKEN`, `DB_PASSWORD`, `APP_KEY`; or a URL
//! holding credentials: `postgresql://app:pass@db/app`), with the part of it to hide. Pure.

use std::ops::Range;

/// What a value looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Empty,
    /// `true`, `false`, `yes`, `no`, `on`, `off` (any case).
    Boolean,
    /// `42`, `-1`, `3.14`, `0x1f`.
    Number,
    /// `scheme://…`.
    Url,
    /// `/var/…`, `./…`, `../…`, `~/…`, `%kernel.project_dir%/…`.
    Path,
    Text,
}

const BOOLEANS: &[&str] = &["true", "false", "yes", "no", "on", "off"];

pub fn kind(value: &str) -> ValueKind {
    let value = value.trim();
    if value.is_empty() {
        ValueKind::Empty
    } else if BOOLEANS.iter().any(|b| b.eq_ignore_ascii_case(value)) {
        ValueKind::Boolean
    } else if is_number(value) {
        ValueKind::Number
    } else if url_scheme(value).is_some() {
        ValueKind::Url
    } else if ["/", "./", "../", "~/", "%kernel.project_dir%"].iter().any(|p| value.starts_with(p))
    {
        ValueKind::Path
    } else {
        ValueKind::Text
    }
}

fn is_number(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    all_digits(whole) && all_digits(fraction)
}

/// The length of `value`'s `scheme://`, if it starts with one (`redis://`, `sqlite:///`).
fn url_scheme(value: &str) -> Option<usize> {
    let colon = value.find("://")?;
    let scheme = &value[..colon];
    let valid = scheme.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    valid.then_some(colon + 3)
}

/// The parts of a key's name that say its value is a secret.
const SECRET_WORDS: &[&str] =
    &["SECRET", "TOKEN", "PASSWORD", "PASSWD", "PASS", "PWD", "PRIVATE", "CREDENTIALS", "KEY"];

/// A key whose value is a secret by its name: one of its `_`-separated words is (or ends
/// with) a secret's word — `APP_SECRET`, `GITHUB_TOKEN`, `DB_PASS`, `APIKEY`, `JWT_PASSPHRASE`.
/// Not `KEYBOARD_LAYOUT`, `PASSENGERS`: a word must end with it.
pub fn secret_key(key: &str) -> bool {
    key.split('_').any(|word| {
        let word = word.to_ascii_uppercase();
        SECRET_WORDS.iter().any(|s| word.ends_with(s)) || word == "PASSPHRASE"
    })
}

/// The credentials' password in a URL (`scheme://user:password@host`): its range in
/// `value`.
pub fn url_password(value: &str) -> Option<Range<usize>> {
    let start = url_scheme(value)?;
    let authority_end = value[start..].find(['/', '?', '#']).map_or(value.len(), |i| start + i);
    let at = value[start..authority_end].rfind('@').map(|i| start + i)?;
    let colon = value[start..at].find(':').map(|i| start + i)?;
    (colon + 1 < at).then_some(colon + 1..at)
}

/// What to hide of `key`'s `value` (a range of the value): all of it for a secret's key,
/// the password of a URL holding credentials; nothing for an empty value.
pub fn hidden_part(key: &str, value: &str) -> Option<Range<usize>> {
    if value.trim().is_empty() {
        return None;
    }
    url_password(value).or_else(|| secret_key(key).then_some(0..value.len()))
}

/// `key`'s `value` as shown where it's quoted (a hover card, a list): its secret part as
/// `••••••`.
pub fn shown(key: &str, value: &str) -> String {
    match hidden_part(key, value) {
        Some(hidden) => format!("{}{MASK}{}", &value[..hidden.start], &value[hidden.end..]),
        None => value.to_owned(),
    }
}

/// What stands for a hidden secret (never its length).
const MASK: &str = "••••••";

#[cfg(test)]
#[path = "../tests/unit/values.rs"]
mod tests;
