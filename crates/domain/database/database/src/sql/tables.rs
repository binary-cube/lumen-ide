//! The tables a statement names where the console needs them: the one a query reads alone
//! (its result's tab is named after it, `database.schema.table`), and those a statement
//! changes (the grids open on them read their rows again once it ran), with whether their
//! structure changed. Names are read as the engine reads them: an unquoted name is folded to
//! lower case by PostgreSQL, kept as written elsewhere.

use super::lex::{Token, TokenKind};
use super::{Flavor, identifier, significant};
use crate::TableRef;

/// The words after a table's name that aren't its alias.
pub(crate) const CLAUSES: &[&str] = &[
    "WHERE",
    "GROUP",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "FOR",
    "WINDOW",
    "HAVING",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "CROSS",
    "NATURAL",
    "STRAIGHT_JOIN",
    "ON",
    "USING",
    "RETURNING",
    "TABLESAMPLE",
    "INDEXED",
    "NOT",
];

/// Words joining another table in.
const JOINS: &[&str] =
    &["JOIN", "INNER", "LEFT", "RIGHT", "FULL", "CROSS", "NATURAL", "STRAIGHT_JOIN", "LATERAL"];

/// The table a `SELECT` reads by itself (`SELECT … FROM [schema.]table [alias] …`): no join,
/// no second table, no subquery or function in its `FROM`, no `UNION`, no `WITH`. `None`
/// for anything else.
pub fn query_table(text: &str, tokens: &[Token], flavor: Flavor) -> Option<TableRef> {
    let sig = significant(tokens);
    let first = sig.first()?.0;
    if !first.is_word(text, "SELECT") {
        return None;
    }
    let top = |i: usize| sig.get(i).filter(|(_, d)| *d == 0).map(|(t, _)| *t);
    let is_top_word =
        |i: usize, words: &[&str]| top(i).is_some_and(|t| words.iter().any(|w| t.is_word(text, w)));
    if (0..sig.len()).any(|i| is_top_word(i, &["UNION", "INTERSECT", "EXCEPT"])) {
        return None;
    }
    let from = (1..sig.len()).find(|&i| is_top_word(i, &["FROM"]))?;
    let (table, mut at) = name_at(text, &sig, from + 1, flavor)?;
    // A table-valued function (`generate_series(1, 3)`), not a table.
    if sig.get(at).is_some_and(|(t, _)| t.kind == TokenKind::OpenParen) {
        return None;
    }
    // Its alias (`AS x`, or a bare name that isn't the next clause).
    if is_top_word(at, &["AS"]) {
        at += 2;
    } else if top(at).is_some_and(|t| {
        t.kind == TokenKind::QuotedIdent
            || (t.kind == TokenKind::Word && !CLAUSES.iter().any(|c| t.is_word(text, c)))
    }) {
        at += 1;
    }
    match top(at) {
        Some(t) if t.kind == TokenKind::Comma => None,
        Some(_) if is_top_word(at, JOINS) => None,
        _ => Some(table),
    }
}

/// The tables `tokens`'s statement changes (`INSERT INTO`, `UPDATE`, `DELETE FROM`,
/// `MERGE INTO`, `REPLACE INTO`, `TRUNCATE`; and, their structure too, `ALTER TABLE` and
/// `DROP TABLE`), in a `WITH`'s parts as well; each once, in the order named.
pub fn written_tables(text: &str, tokens: &[Token], flavor: Flavor) -> Vec<(TableRef, bool)> {
    let sig = significant(tokens);
    let mut out: Vec<(TableRef, bool)> = Vec::new();
    let mut push =
        |table: TableRef, structure: bool| match out.iter_mut().find(|(t, _)| *t == table) {
            Some(found) => found.1 |= structure,
            None => out.push((table, structure)),
        };
    let word = |i: usize, w: &str| sig.get(i).is_some_and(|(t, _)| t.is_word(text, w));
    let skip = |mut i: usize, words: &[&str]| {
        while words.iter().any(|w| word(i, w)) {
            i += 1;
        }
        i
    };
    for i in 0..sig.len() {
        let (token, _) = sig[i];
        if token.kind != TokenKind::Word || !starts(&sig, i) {
            continue;
        }
        let w = token.text(text).to_ascii_uppercase();
        let (at, structure, many) = match w.as_str() {
            "INSERT" | "REPLACE" => {
                let mut j = i + 1;
                if word(j, "OR") {
                    j += 2;
                }
                let j = skip(j, &["LOW_PRIORITY", "DELAYED", "HIGH_PRIORITY", "IGNORE"]);
                if word(j, "INTO") {
                    (j + 1, false, false)
                } else if w == "REPLACE" {
                    (j, false, false)
                } else {
                    continue;
                }
            }
            "UPDATE" => {
                let mut j = i + 1;
                if word(j, "OR") {
                    j += 2;
                }
                (skip(j, &["LOW_PRIORITY", "IGNORE", "ONLY"]), false, false)
            }
            "DELETE" => {
                let j = skip(i + 1, &["LOW_PRIORITY", "QUICK", "IGNORE"]);
                if !word(j, "FROM") {
                    continue;
                }
                (skip(j + 1, &["ONLY"]), false, false)
            }
            "MERGE" if word(i + 1, "INTO") => (i + 2, false, false),
            "TRUNCATE" => (skip(i + 1, &["TABLE", "ONLY"]), false, true),
            "ALTER" if word(i + 1, "TABLE") => {
                let mut j = i + 2;
                if word(j, "IF") && word(j + 1, "EXISTS") {
                    j += 2;
                }
                (skip(j, &["ONLY"]), true, false)
            }
            "DROP" if word(i + 1, "TABLE") => {
                let mut j = i + 2;
                if word(j, "IF") && word(j + 1, "EXISTS") {
                    j += 2;
                }
                (j, true, true)
            }
            _ => continue,
        };
        let mut at = at;
        while let Some((table, next)) = name_at(text, &sig, at, flavor) {
            push(table, structure);
            if !many || !sig.get(next).is_some_and(|(t, _)| t.kind == TokenKind::Comma) {
                break;
            }
            at = next + 1;
        }
    }
    out
}

/// The tables, views and sequences `tokens` creates (`CREATE [TEMP] TABLE [IF NOT EXISTS]
/// t`, `CREATE [MATERIALIZED] VIEW v`, `CREATE SEQUENCE s`, `CREATE VIRTUAL TABLE t`,
/// `SELECT … INTO t`, the new name of `ALTER TABLE … RENAME TO t`), each once, in the order
/// written: names a script makes for itself (the schema doesn't know them yet). Read over
/// a whole text, every statement's.
pub fn created_tables(text: &str, tokens: &[Token], flavor: Flavor) -> Vec<TableRef> {
    let sig = significant(tokens);
    let mut out: Vec<TableRef> = Vec::new();
    let word = |i: usize, w: &str| sig.get(i).is_some_and(|(t, _)| t.is_word(text, w));
    let skip = |mut i: usize, words: &[&str]| {
        while words.iter().any(|w| word(i, w)) {
            i += 1;
        }
        i
    };
    for i in 0..sig.len() {
        let at = if word(i, "CREATE") {
            let mut j = i + 1;
            if word(j, "OR") && word(j + 1, "REPLACE") {
                j += 2;
            }
            j = skip(
                j,
                &[
                    "GLOBAL",
                    "LOCAL",
                    "TEMP",
                    "TEMPORARY",
                    "UNLOGGED",
                    "MATERIALIZED",
                    "RECURSIVE",
                    "VIRTUAL",
                    "FOREIGN",
                ],
            );
            if !["TABLE", "VIEW", "SEQUENCE"].iter().any(|w| word(j, w)) {
                continue;
            }
            j += 1;
            if word(j, "IF") && word(j + 1, "NOT") && word(j + 2, "EXISTS") {
                j += 3;
            }
            j
        } else if word(i, "RENAME")
            && word(i + 1, "TO")
            && sig[..i].iter().any(|(t, _)| t.is_word(text, "ALTER"))
        {
            i + 2
        } else if word(i, "INTO")
            && !word(i.wrapping_sub(1), "INSERT")
            && !word(i.wrapping_sub(1), "REPLACE")
            && !word(i.wrapping_sub(1), "MERGE")
            && flavor.is_postgres()
            && sig[..i].iter().any(|(t, _)| t.is_word(text, "SELECT"))
        {
            skip(i + 1, &["TEMP", "TEMPORARY", "UNLOGGED", "TABLE"])
        } else {
            continue;
        };
        if let Some((table, _)) = name_at(text, &sig, at, flavor)
            && !out.contains(&table)
        {
            out.push(table);
        }
    }
    out
}

/// Whether `sig[i]` starts a statement: the first word, a part of a `WITH` (after its `(`),
/// or the main statement after the parts (after a `)`).
fn starts(sig: &[(Token, usize)], i: usize) -> bool {
    match i.checked_sub(1).map(|p| sig[p].0.kind) {
        None => true,
        Some(kind) => matches!(kind, TokenKind::OpenParen | TokenKind::CloseParen),
    }
}

/// The qualified name at `sig[at]` (`t`, `schema.t`, `db.schema.t`: the last two parts
/// kept), its unquoted parts folded as `flavor` folds them, and the index after it.
pub(crate) fn name_at(
    text: &str,
    sig: &[(Token, usize)],
    at: usize,
    flavor: Flavor,
) -> Option<(TableRef, usize)> {
    let part = |token: &Token| {
        let name = identifier(text, token);
        if token.kind == TokenKind::Word && flavor.is_postgres() {
            name.to_lowercase()
        } else {
            name.into_owned()
        }
    };
    let (first, _) = sig.get(at)?;
    if !first.is_name() {
        return None;
    }
    let mut parts = vec![part(first)];
    let mut i = at + 1;
    while let (Some((dot, _)), Some((next, _))) = (sig.get(i), sig.get(i + 1)) {
        if dot.kind != TokenKind::Dot || !next.is_name() {
            break;
        }
        parts.push(part(next));
        i += 2;
    }
    let name = parts.pop()?;
    let table = match parts.pop() {
        Some(schema) => TableRef::in_schema(schema, name),
        None => TableRef::new(name),
    };
    Some((table, i))
}

/// Whether a statement only reads and can be run again over its result (`SELECT`,
/// `WITH … SELECT`, `VALUES`, `TABLE x`; none locking rows with `FOR UPDATE` / `FOR SHARE`):
/// its next page, or its rows sorted and filtered, read by running it again, wrapped.
pub fn rerunnable(text: &str, tokens: &[Token], flavor: Flavor) -> bool {
    if !super::classify(text, tokens, flavor).is_query() {
        return false;
    }
    let sig = significant(tokens);
    let Some((first, _)) = sig.first() else { return false };
    if !["SELECT", "WITH", "VALUES", "TABLE"].iter().any(|w| first.is_word(text, w)) {
        return false;
    }
    let locks = sig.windows(2).any(|pair| {
        pair[0].0.is_word(text, "FOR")
            && ["UPDATE", "SHARE", "NO", "KEY"].iter().any(|w| pair[1].0.is_word(text, w))
    });
    !locks
}

#[cfg(test)]
#[path = "../../tests/unit/sql/tables.rs"]
mod tests;
