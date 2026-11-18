//! Passwords written in statements, hidden before the statements are logged or kept in the
//! history: the literal after `PASSWORD` (`WITH PASSWORD '…'`, `ENCRYPTED PASSWORD '…'`,
//! `PASSWORD = '…'`, `PASSWORD('…')`, a foreign server's `password '…'` option) or after
//! `IDENTIFIED BY` / `IDENTIFIED WITH plugin BY | AS` (MySQL) becomes `'••••'`.

use std::borrow::Cow;

use super::Flavor;
use super::lex::{Token, TokenKind, lex};

/// What a hidden secret is shown as.
pub const MASK: &str = "'••••'";

/// `text` with its password literals hidden (borrowed when it holds none).
pub fn mask_secrets(text: &str, flavor: Flavor) -> Cow<'_, str> {
    let tokens = lex(text, flavor);
    let sig: Vec<&Token> = tokens.iter().filter(|t| !t.is_trivia()).collect();
    let secret = |t: &Token| matches!(t.kind, TokenKind::String(_) | TokenKind::DollarBody);
    let mut hidden: Vec<(usize, usize)> = Vec::new();
    for (i, token) in sig.iter().enumerate() {
        let at = if token.is_word(text, "PASSWORD") {
            Some(i + 1)
        } else if token.is_word(text, "IDENTIFIED") {
            // `IDENTIFIED BY [PASSWORD] '…'`, `IDENTIFIED WITH plugin BY | AS '…'`.
            let mut j = i + 1;
            if sig.get(j).is_some_and(|t| t.is_word(text, "WITH")) {
                j += 2;
            }
            let by = sig.get(j).is_some_and(|t| t.is_word(text, "BY") || t.is_word(text, "AS"));
            by.then_some(j + 1)
        } else {
            None
        };
        let Some(mut j) = at else { continue };
        // Past `=` and `(` (`PASSWORD = '…'`, `PASSWORD('…')`, `BY PASSWORD '…'`).
        while sig.get(j).is_some_and(|t| {
            t.kind == TokenKind::OpenParen
                || (t.kind == TokenKind::Operator && t.text(text) == "=")
                || t.is_word(text, "PASSWORD")
        }) {
            j += 1;
        }
        if let Some(t) = sig.get(j).filter(|t| secret(t))
            && hidden.last().is_none_or(|&(_, end)| end <= t.start)
        {
            hidden.push((t.start, t.end));
        }
    }
    if hidden.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end) in hidden {
        out.push_str(&text[at..start]);
        out.push_str(MASK);
        at = end;
    }
    out.push_str(&text[at..]);
    Cow::Owned(out)
}

#[cfg(test)]
#[path = "../../tests/unit/sql/secrets.rs"]
mod tests;
