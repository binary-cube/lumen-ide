//! A statement's fingerprint: what it says once its values are taken out — literals and
//! parameters become `?`, a list of them (`IN (1, 2, 3)`) one `?`, comments go, spaces are
//! normalized, unquoted words lower-cased (SQL reads them without case), a last `;` dropped —
//! and its hash. Two runs of the same query with other values have the same fingerprint:
//! what finds a query run over and over (N+1) and the profiles of one query to compare.

use super::Flavor;
use super::lex::{Token, TokenKind, lex};

/// A statement without its values, and its hash (FNV-1a, stable across runs and machines).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    pub text: String,
    pub hash: u64,
}

/// `text`'s fingerprint, read as `flavor` writes SQL.
pub fn fingerprint(text: &str, flavor: Flavor) -> Fingerprint {
    let tokens = lex(text, flavor);
    let significant: Vec<&Token> = tokens.iter().filter(|t| !t.is_trivia()).collect();
    // Each token as it's written in the fingerprint (`None`: dropped).
    let mut parts: Vec<(String, TokenKind)> = Vec::with_capacity(significant.len());
    for (i, token) in significant.iter().enumerate() {
        let kind = token.kind;
        let raw = token.text(text);
        let value = match kind {
            TokenKind::String(_) | TokenKind::Number | TokenKind::Parameter(_) => "?".to_owned(),
            TokenKind::DollarBody => "?".to_owned(),
            TokenKind::Word => {
                // `TRUE` / `FALSE` are values too; `NULL` isn't (`IS NULL` is another query).
                let lower = raw.to_ascii_lowercase();
                if lower == "true" || lower == "false" { "?".to_owned() } else { lower }
            }
            TokenKind::Semicolon if i + 1 == significant.len() => continue,
            _ => raw.to_owned(),
        };
        // A unary minus before a number belongs to the value (`= -5`).
        if value == "?"
            && kind == TokenKind::Number
            && parts.last().is_some_and(|(p, k)| *k == TokenKind::Operator && p == "-")
            && unary(&parts[..parts.len() - 1])
        {
            parts.pop();
        }
        parts.push((value, kind));
    }
    collapse_lists(&mut parts);
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<TokenKind> = None;
    for (part, kind) in &parts {
        if let Some(p) = prev
            && space_between(p, *kind)
        {
            out.push(' ');
        }
        out.push_str(part);
        prev = Some(*kind);
    }
    let hash = fnv1a(out.as_bytes());
    Fingerprint { text: out, hash }
}

/// The minus after these is a sign, not a subtraction (after a word it's left as written:
/// `x - 1` and `WHERE -1 < x` both read the same each time).
fn unary(before: &[(String, TokenKind)]) -> bool {
    before.last().is_none_or(|(_, kind)| {
        matches!(kind, TokenKind::Operator | TokenKind::OpenParen | TokenKind::Comma)
    })
}

/// `( ?, ?, ? )` → `( ? )`: a list of values of any length is one.
fn collapse_lists(parts: &mut Vec<(String, TokenKind)>) {
    let mut out: Vec<(String, TokenKind)> = Vec::with_capacity(parts.len());
    let mut i = 0;
    while i < parts.len() {
        if parts[i].1 == TokenKind::OpenParen {
            // Values separated by commas up to the closing parenthesis.
            let mut j = i + 1;
            let mut values = 0;
            let mut list = true;
            while j < parts.len() && parts[j].1 != TokenKind::CloseParen {
                let expected_value = (j - i) % 2 == 1;
                let ok =
                    if expected_value { parts[j].0 == "?" } else { parts[j].1 == TokenKind::Comma };
                if !ok {
                    list = false;
                    break;
                }
                if expected_value {
                    values += 1;
                }
                j += 1;
            }
            if list && values > 1 && j < parts.len() {
                out.push(parts[i].clone());
                out.push(("?".to_owned(), TokenKind::Number));
                out.push(parts[j].clone());
                i = j + 1;
                continue;
            }
        }
        out.push(parts[i].clone());
        i += 1;
    }
    *parts = out;
}

/// Whether a space goes between tokens of these kinds (none around `.`, `(`, `)`, `,`, `::`).
fn space_between(prev: TokenKind, next: TokenKind) -> bool {
    !matches!(prev, TokenKind::Dot | TokenKind::OpenParen | TokenKind::Cast)
        && !matches!(
            next,
            TokenKind::Dot
                | TokenKind::CloseParen
                | TokenKind::Comma
                | TokenKind::Cast
                | TokenKind::Semicolon
        )
        && !(next == TokenKind::OpenParen
            && matches!(prev, TokenKind::Word | TokenKind::QuotedIdent))
}

/// FNV-1a, 64 bits.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
#[path = "../../tests/unit/sql/fingerprint.rs"]
mod tests;
