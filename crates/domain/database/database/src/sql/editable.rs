//! What a query's result is made of, for editing it row by row: a
//! `SELECT` of one table alone (no join, no second table, no subquery or function in its
//! `FROM`, no `UNION`, no `WITH`: [`super::query_table`]'s), whose rows are the table's own —
//! no `DISTINCT`, no `GROUP BY` or `HAVING`, no aggregate and no window in its select list.
//! Each item of the select list is told apart ([`SelectItem`]): every column of the table
//! (`*`, `t.*`), one column (by name, perhaps qualified, perhaps renamed), or an expression
//! (shown, never written). Which result column is which table column, and the key its rows
//! are found by, is the table's structure's to say ([`crate::edits::EditableResult`]).

use super::lex::{Token, TokenKind};
use super::tables::{CLAUSES, name_at};
use super::{Flavor, identifier, significant};
use crate::TableRef;
use crate::edits::NotEditable;

/// An item of a query's select list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectItem {
    /// Every column of the table, in its order (`*`, `t.*`).
    All,
    /// One column of the table (its name as the engine names it), shown under its own name
    /// or another (`AS x`).
    Column(String),
    /// Anything else (`price * 2`, `'x'`, `lower(name)`): read only.
    Expression,
}

/// A query of one table, as its result is edited: the table, its alias, what each item of
/// its select list is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryShape {
    pub table: TableRef,
    pub alias: Option<String>,
    pub items: Vec<SelectItem>,
}

/// Functions whose rows are a group's, not the table's (by name, any case).
const AGGREGATES: &[&str] = &[
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "total",
    "group_concat",
    "string_agg",
    "array_agg",
    "json_agg",
    "jsonb_agg",
    "json_object_agg",
    "jsonb_object_agg",
    "json_group_array",
    "json_group_object",
    "bool_and",
    "bool_or",
    "every",
    "bit_and",
    "bit_or",
    "bit_xor",
    "stddev",
    "stddev_pop",
    "stddev_samp",
    "variance",
    "var_pop",
    "var_samp",
    "percentile_cont",
    "percentile_disc",
    "mode",
    "xmlagg",
];

/// The shape of `text`'s query (see the module's doc), or why its result can't be edited.
pub fn query_shape(
    text: &str,
    tokens: &[Token],
    flavor: Flavor,
) -> Result<QueryShape, NotEditable> {
    let sig = significant(tokens);
    let Some((first, _)) = sig.first() else { return Err(NotEditable::NotQuery) };
    if !first.is_word(text, "SELECT") {
        return Err(NotEditable::NotQuery);
    }
    if super::query_table(text, tokens, flavor).is_none() {
        return Err(NotEditable::NotOneTable);
    }
    let top = |i: usize| sig.get(i).filter(|(_, d)| *d == 0).map(|(t, _)| *t);
    let top_word = |i: usize, w: &str| top(i).is_some_and(|t| t.is_word(text, w));
    let Some(from) = (1..sig.len()).find(|&i| top_word(i, "FROM")) else {
        return Err(NotEditable::NotOneTable);
    };
    // Its rows are groups, or each other's duplicates gone.
    if (from..sig.len()).any(|i| top_word(i, "GROUP") || top_word(i, "HAVING")) {
        return Err(NotEditable::Grouped);
    }
    // It locks its rows: never run again (its rows read again once the changes are made).
    if !super::rerunnable(text, tokens, flavor) {
        return Err(NotEditable::Locking);
    }
    let mut start = 1;
    if top_word(start, "DISTINCT") {
        return Err(NotEditable::Distinct);
    }
    if top_word(start, "ALL") {
        start += 1;
    }
    // The table and its alias (as `query_table` reads them).
    let (table, at) = name_at(text, &sig, from + 1, flavor).ok_or(NotEditable::NotOneTable)?;
    let alias = if top_word(at, "AS") {
        sig.get(at + 1).map(|(t, _)| identifier(text, t).into_owned())
    } else {
        top(at)
            .filter(|t| {
                t.kind == TokenKind::QuotedIdent
                    || (t.kind == TokenKind::Word && !CLAUSES.iter().any(|c| t.is_word(text, c)))
            })
            .map(|t| identifier(text, &t).into_owned())
    };
    let list = &sig[start..from];
    if list.is_empty() {
        return Err(NotEditable::NotQuery);
    }
    // An aggregate or a window anywhere in the list: one row per group.
    for (i, (token, _)) in list.iter().enumerate() {
        if token.is_word(text, "OVER") {
            return Err(NotEditable::Aggregate("over".to_owned()));
        }
        let call = list.get(i + 1).is_some_and(|(t, _)| t.kind == TokenKind::OpenParen);
        if token.kind == TokenKind::Word && call {
            let name = token.text(text).to_ascii_lowercase();
            if AGGREGATES.contains(&name.as_str()) {
                return Err(NotEditable::Aggregate(name));
            }
        }
    }
    let depth0 = list.first().map_or(0, |(_, d)| *d);
    let mut items = Vec::new();
    let mut part: Vec<Token> = Vec::new();
    for (token, depth) in list {
        if token.kind == TokenKind::Comma && *depth == depth0 {
            items.push(item(text, &part, &table, alias.as_deref(), flavor));
            part.clear();
        } else {
            part.push(*token);
        }
    }
    items.push(item(text, &part, &table, alias.as_deref(), flavor));
    Ok(QueryShape { table, alias, items })
}

/// What one item of the select list is (its tokens, without the comma).
fn item(
    text: &str,
    tokens: &[Token],
    table: &TableRef,
    alias: Option<&str>,
    flavor: Flavor,
) -> SelectItem {
    let star = |t: &Token| t.kind == TokenKind::Operator && t.text(text) == "*";
    let name = |t: &Token| {
        let name = identifier(text, t);
        if t.kind == TokenKind::Word && flavor.is_postgres() {
            name.to_lowercase()
        } else {
            name.into_owned()
        }
    };
    // Its qualifier, when it's the table's (its name, or its alias): what follows it.
    let qualified = |q: &Token| {
        let q = identifier(text, q);
        alias.is_some_and(|a| a.eq_ignore_ascii_case(&q)) || table.name.eq_ignore_ascii_case(&q)
    };
    let rest = match tokens {
        [one] if star(one) => return SelectItem::All,
        [q, dot, s] if dot.kind == TokenKind::Dot && star(s) && q.is_name() => {
            return if qualified(q) { SelectItem::All } else { SelectItem::Expression };
        }
        // `schema.table.*`
        [_, d1, q, d2, s] if d1.kind == TokenKind::Dot && d2.kind == TokenKind::Dot && star(s) => {
            return if qualified(q) { SelectItem::All } else { SelectItem::Expression };
        }
        [q, dot, column, rest @ ..] if dot.kind == TokenKind::Dot && q.is_name() => {
            if !qualified(q) || !column.is_name() {
                return SelectItem::Expression;
            }
            (column, rest)
        }
        [column, rest @ ..] => (column, rest),
        [] => return SelectItem::Expression,
    };
    let (column, after) = rest;
    // A word that's a value, not a column (`NULL`, `TRUE`, `CURRENT_DATE`): an expression.
    if column.kind == TokenKind::Word
        && ["NULL", "TRUE", "FALSE", "CURRENT_DATE", "CURRENT_TIME", "CURRENT_TIMESTAMP"]
            .iter()
            .any(|w| column.is_word(text, w))
    {
        return SelectItem::Expression;
    }
    if !column.is_name() {
        return SelectItem::Expression;
    }
    // What may follow it: nothing, `AS x`, or a bare alias.
    let renamed = match after {
        [] => true,
        [as_word, a] => as_word.is_word(text, "AS") && a.is_name(),
        [a] => a.is_name() && !a.is_word(text, "AS"),
        _ => false,
    };
    if renamed { SelectItem::Column(name(column)) } else { SelectItem::Expression }
}

#[cfg(test)]
#[path = "../../tests/unit/sql/editable.rs"]
mod tests;
