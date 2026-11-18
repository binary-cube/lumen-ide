//! Operations that lose data, found before they run: the UI asks to confirm each — the
//! changes the app writes ([`schema_dangers`], [`change_dangers`], [`script_dangers`]) and
//! the statements the user writes ([`statement_dangers`]).

use crate::sql::classify::kind_at;
use crate::sql::{Flavor, StatementKind, Token, TokenKind, WriteKind, qualified_name, significant};
use crate::sqlite::{ObjectKind, Stale};
use crate::transaction::Script;
use crate::{ChangeSet, ColumnDef, ColumnInfo, SchemaChange, Structure, TableRef, TypeClass};

/// How many deleted rows make a mass delete (unless they're most of a small table).
pub const MASS_DELETE: usize = 100;

/// Something that loses data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Danger {
    /// The table and all its rows.
    DropTable { table: TableRef, rows: Option<u64> },
    /// A column of a table that has (or may have) rows.
    DropColumn { table: TableRef, column: String, rows: Option<u64> },
    /// A column's type changed: values may be converted or refused.
    AlterColumn { table: TableRef, column: String, loss: AlterColumnLoss },
    /// An index dropped (no data, but what it made fast or unique).
    DropIndex { table: TableRef, name: String },
    /// Many rows deleted at once.
    MassDelete { table: TableRef, rows: usize },
    /// An index, a trigger or a view dropped by a table's rebuild and not made again: it
    /// names a column dropped (or a view that goes).
    LostObject { table: TableRef, kind: ObjectKind, name: String },
    /// A statement's `UPDATE` or `DELETE` without a `WHERE` of its own (or with one that's
    /// always true, `WHERE 1 = 1`): every row of the table.
    UnfilteredWrite { kind: WriteKind, table: TableRef },
    /// A statement's `DROP`; with `cascade`, what depends on it goes too.
    DropObject { kind: DropKind, name: String, cascade: bool },
    /// A statement's `TRUNCATE`: every row of the table.
    Truncate { table: TableRef },
    /// A statement's `ALTER TABLE … DROP [COLUMN]`: the column and its values.
    AlterDrop { table: TableRef, column: String },
}

/// What a statement's `DROP` removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    /// A table (a foreign or temporary one too).
    Table,
    /// A view, materialized or not.
    View,
    Index,
    Schema,
    Database,
    /// A function, an aggregate or a routine.
    Function,
    Procedure,
    Trigger,
    Sequence,
    /// A type or a domain.
    Type,
    /// Anything else (a role, an extension, a server…).
    Other,
}

/// How changing a column's definition may lose its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlterColumnLoss {
    /// Another class of type: values are converted, or refused.
    Conversion,
    /// Another type of the same class (`varchar(255)` to `varchar(10)`): values may be
    /// cut or refused.
    Resize,
    /// It becomes `NOT NULL`: the rows holding null refuse it.
    NotNull,
}

impl AlterColumnLoss {
    /// What `old` becoming `to` may lose (`None`: nothing).
    pub fn of(old: &ColumnInfo, to: &ColumnDef) -> Option<Self> {
        let same = |a: &str, b: &str| {
            let norm = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
            norm(a).eq_ignore_ascii_case(&norm(b))
        };
        if TypeClass::of(&to.declared_type) != old.class {
            Some(Self::Conversion)
        } else if !same(&old.declared_type, &to.declared_type) {
            Some(Self::Resize)
        } else if old.nullable && !to.nullable {
            Some(Self::NotNull)
        } else {
            None
        }
    }
}

/// What `changes` lose. `rows` says how many rows a table holds (`None`: unknown, which
/// counts as having some); `structure` gives a table's structure, when known (a column
/// whose definition doesn't change its values loses nothing).
pub fn schema_dangers(
    changes: &[SchemaChange],
    rows: &dyn Fn(&TableRef) -> Option<u64>,
    structure: &dyn Fn(&TableRef) -> Option<Structure>,
) -> Vec<Danger> {
    let has_rows = |table: &TableRef| {
        let n = rows(table);
        (n != Some(0)).then_some(n)
    };
    let mut dangers = Vec::new();
    for change in changes {
        match change {
            SchemaChange::DropTable { table } => {
                dangers.push(Danger::DropTable { table: table.clone(), rows: rows(table) });
            }
            SchemaChange::DropColumn { table, column } => {
                if let Some(n) = has_rows(table) {
                    dangers.push(Danger::DropColumn {
                        table: table.clone(),
                        column: column.clone(),
                        rows: n,
                    });
                }
            }
            SchemaChange::DropIndex { table, name } => {
                dangers.push(Danger::DropIndex { table: table.clone(), name: name.clone() });
            }
            SchemaChange::AlterColumn { table, column, to } => {
                if has_rows(table).is_none() {
                    continue;
                }
                let before = structure(table);
                let loss = match before.as_ref().and_then(|s| s.column(column)) {
                    Some(old) => AlterColumnLoss::of(old, to),
                    None => Some(AlterColumnLoss::Conversion),
                };
                if let Some(loss) = loss {
                    dangers.push(Danger::AlterColumn {
                        table: table.clone(),
                        column: column.clone(),
                        loss,
                    });
                }
            }
            _ => {}
        }
    }
    dangers
}

/// What running `script` loses beyond its changes: the objects its rebuilds drop and don't
/// make again ([`Script::stale`]).
pub fn script_dangers(script: &Script) -> Vec<Danger> {
    script
        .stale
        .iter()
        .map(|Stale { table, kind, name }| Danger::LostObject {
            table: table.clone(),
            kind: *kind,
            name: name.clone(),
        })
        .collect()
}

/// What deleting `set`'s rows loses: a mass delete when it's at least [`MASS_DELETE`] rows,
/// or every row of a table of more than one (`total`: the table's rows, when known).
pub fn change_dangers(set: &ChangeSet, total: Option<u64>) -> Vec<Danger> {
    let deletes = set.deletes();
    let all = total.is_some_and(|t| t > 1 && deletes as u64 >= t);
    if deletes >= MASS_DELETE || all {
        vec![Danger::MassDelete { table: set.table.clone(), rows: deletes }]
    } else {
        Vec::new()
    }
}

/// What running the statement made of `tokens` (read from `text`, written for `flavor`)
/// loses: its `UPDATE`s and `DELETE`s without a filter, its `DROP`s, `TRUNCATE`s and dropped
/// columns — in a `WITH`'s parts and under `EXPLAIN ANALYZE` too (which runs them), not in
/// a trigger's body (made, not run) nor in a plain `EXPLAIN`.
pub fn statement_dangers(text: &str, tokens: &[Token], flavor: Flavor) -> Vec<Danger> {
    let sig = significant(tokens);
    if let StatementKind::Explain { analyze: false, .. } = kind_at(text, &sig, 0, flavor) {
        return Vec::new();
    }
    let mut dangers = Vec::new();
    for i in 0..sig.len() {
        let (token, _) = sig[i];
        if token.kind != TokenKind::Word || !starts_statement(text, &sig, i) {
            continue;
        }
        let word = token.text(text).to_ascii_uppercase();
        match word.as_str() {
            "UPDATE" => unfiltered(text, &sig, i, WriteKind::Update, &mut dangers),
            "DELETE" => unfiltered(text, &sig, i, WriteKind::Delete, &mut dangers),
            "TRUNCATE" => truncate(text, &sig, i, &mut dangers),
            "DROP" => drop(text, &sig, i, &mut dangers),
            "ALTER" => alter_drop(text, &sig, i, &mut dangers),
            _ => {}
        }
    }
    dangers
}

/// Whether `sig[i]` starts a statement that runs: the statement itself, a part of a `WITH`,
/// the main one after its parts, a subquery, or what `EXPLAIN ANALYZE` runs.
fn starts_statement(text: &str, sig: &[(Token, usize)], i: usize) -> bool {
    let Some(prev) = i.checked_sub(1).map(|p| sig[p].0) else {
        return true;
    };
    match prev.kind {
        TokenKind::OpenParen | TokenKind::CloseParen => true,
        TokenKind::Word => {
            let explained = |w: &str| prev.is_word(text, w);
            explained("ANALYZE")
                || explained("ANALYSE")
                || (explained("VERBOSE")
                    && i >= 2
                    && (sig[i - 2].0.is_word(text, "ANALYZE")
                        || sig[i - 2].0.is_word(text, "ANALYSE")))
        }
        _ => false,
    }
}

fn word_at<'a>(text: &'a str, sig: &[(Token, usize)], i: usize) -> Option<&'a str> {
    sig.get(i).filter(|(t, _)| t.kind == TokenKind::Word).map(|(t, _)| t.text(text))
}

fn is_at(text: &str, sig: &[(Token, usize)], i: usize, keyword: &str) -> bool {
    word_at(text, sig, i).is_some_and(|w| w.eq_ignore_ascii_case(keyword))
}

/// Past the words of `words` at `sig[i]` (any number, in any order).
fn skip_words(text: &str, sig: &[(Token, usize)], mut i: usize, words: &[&str]) -> usize {
    while word_at(text, sig, i).is_some_and(|w| words.iter().any(|k| w.eq_ignore_ascii_case(k))) {
        i += 1;
    }
    i
}

/// Past `IF EXISTS`.
fn skip_if_exists(text: &str, sig: &[(Token, usize)], i: usize) -> usize {
    if is_at(text, sig, i, "IF") && is_at(text, sig, i + 1, "EXISTS") { i + 2 } else { i }
}

/// The end of the statement starting at `sig[i]`: where its depth drops (its `)`), or the
/// end.
fn statement_end(sig: &[(Token, usize)], i: usize) -> usize {
    let depth = sig[i].1;
    sig.iter().skip(i + 1).position(|(_, d)| *d < depth).map_or(sig.len(), |p| i + 1 + p)
}

/// An `UPDATE` or a `DELETE` at `sig[i]` without a `WHERE` of its own that filters.
fn unfiltered(
    text: &str,
    sig: &[(Token, usize)],
    i: usize,
    kind: WriteKind,
    dangers: &mut Vec<Danger>,
) {
    let depth = sig[i].1;
    let end = statement_end(sig, i);
    let target = match kind {
        // `UPDATE [OR IGNORE] [LOW_PRIORITY] [IGNORE] [ONLY] t`.
        WriteKind::Update => {
            let mut j = i + 1;
            if is_at(text, sig, j, "OR") {
                j += 2;
            }
            j = skip_words(text, sig, j, &["LOW_PRIORITY", "IGNORE", "ONLY"]);
            qualified_name(text, sig, j)
        }
        // `DELETE [LOW_PRIORITY] [QUICK] [IGNORE] FROM [ONLY] t`, MySQL's `DELETE t FROM …`.
        _ => {
            let j = skip_words(text, sig, i + 1, &["LOW_PRIORITY", "QUICK", "IGNORE"]);
            if is_at(text, sig, j, "FROM") {
                let j = skip_words(text, sig, j + 1, &["ONLY"]);
                qualified_name(text, sig, j)
            } else {
                qualified_name(text, sig, j)
            }
        }
    };
    let Some((table, _)) = target else { return };
    let filter = (i + 1..end).find(|&j| sig[j].1 == depth && sig[j].0.is_word(text, "WHERE"));
    let filtered = filter.is_some_and(|w| {
        let clauses = ["RETURNING", "ORDER", "LIMIT", "OFFSET", "GROUP", "WINDOW"];
        let stop = (w + 1..end)
            .find(|&j| sig[j].1 == depth && clauses.iter().any(|c| sig[j].0.is_word(text, c)))
            .unwrap_or(end);
        !always_true(text, &sig[w + 1..stop])
    });
    if !filtered {
        dangers.push(Danger::UnfilteredWrite { kind, table });
    }
}

/// Whether the condition made of `cond` keeps every row: empty, `TRUE`, a number other than
/// zero, a literal equal to itself (`1 = 1`, `'a' = 'a'`), `NOT FALSE`, or any of these
/// `OR`ed with anything.
fn always_true(text: &str, cond: &[(Token, usize)]) -> bool {
    let mut cond = cond;
    // `(…)` around it all.
    while cond.len() >= 2
        && cond[0].0.kind == TokenKind::OpenParen
        && cond.last().is_some_and(|(t, d)| t.kind == TokenKind::CloseParen && *d == cond[0].1)
        && cond[1..cond.len() - 1].iter().all(|(_, d)| *d > cond[0].1)
    {
        cond = &cond[1..cond.len() - 1];
    }
    let Some(&(_, depth)) = cond.first() else {
        return true;
    };
    let parts: Vec<&[(Token, usize)]> =
        cond.split(|(t, d)| *d == depth && t.is_word(text, "OR")).collect();
    if parts.len() > 1 {
        return parts.iter().any(|p| !p.is_empty() && always_true(text, p));
    }
    let literal = |t: &Token| matches!(t.kind, TokenKind::Number | TokenKind::String(_));
    match cond {
        [(t, _)] => {
            t.is_word(text, "TRUE")
                || (t.kind == TokenKind::Number
                    && t.text(text).parse::<f64>().is_ok_and(|n| n != 0.0))
        }
        [(not, _), (f, _)] => not.is_word(text, "NOT") && f.is_word(text, "FALSE"),
        [(a, _), (op, _), (b, _)] => {
            literal(a)
                && literal(b)
                && op.kind == TokenKind::Operator
                && op.text(text) == "="
                && a.text(text) == b.text(text)
        }
        _ => false,
    }
}

fn truncate(text: &str, sig: &[(Token, usize)], i: usize, dangers: &mut Vec<Danger>) {
    let end = statement_end(sig, i);
    let mut j = skip_words(text, sig, i + 1, &["TABLE"]);
    while j < end {
        j = skip_words(text, sig, j, &["ONLY"]);
        let Some((table, next)) = qualified_name(text, sig, j) else { break };
        dangers.push(Danger::Truncate { table });
        j = next;
        // `t *` (its descendants too).
        if sig.get(j).is_some_and(|(t, _)| t.kind == TokenKind::Operator && t.text(text) == "*") {
            j += 1;
        }
        if sig.get(j).is_some_and(|(t, _)| t.kind == TokenKind::Comma) {
            j += 1;
        } else {
            break;
        }
    }
}

fn drop(text: &str, sig: &[(Token, usize)], i: usize, dangers: &mut Vec<Danger>) {
    let depth = sig[i].1;
    let end = statement_end(sig, i);
    let start = i + 1;
    let first_kind = word_at(text, sig, start).unwrap_or_default().to_ascii_uppercase();
    let second_kind = word_at(text, sig, start + 1).unwrap_or_default().to_ascii_uppercase();
    let (kind, kind_len) = match (first_kind.as_str(), second_kind.as_str()) {
        ("FOREIGN" | "TEMPORARY" | "TEMP", "TABLE") => (DropKind::Table, 2),
        ("TABLE", _) => (DropKind::Table, 1),
        ("MATERIALIZED", "VIEW") => (DropKind::View, 2),
        ("VIEW", _) => (DropKind::View, 1),
        ("INDEX", _) => (DropKind::Index, 1),
        ("SCHEMA", _) => (DropKind::Schema, 1),
        ("DATABASE", _) => (DropKind::Database, 1),
        ("FUNCTION" | "AGGREGATE" | "ROUTINE", _) => (DropKind::Function, 1),
        ("PROCEDURE", _) => (DropKind::Procedure, 1),
        ("TRIGGER", _) => (DropKind::Trigger, 1),
        ("SEQUENCE", _) => (DropKind::Sequence, 1),
        ("TYPE" | "DOMAIN", _) => (DropKind::Type, 1),
        _ => {
            // `DROP EVENT TRIGGER x`, `DROP TEXT SEARCH CONFIGURATION x`, `DROP OWNED BY r`…
            let other_words = [
                "EXTENSION",
                "ROLE",
                "USER",
                "GROUP",
                "OWNED",
                "SERVER",
                "FOREIGN",
                "DATA",
                "WRAPPER",
                "EVENT",
                "TRIGGER",
                "TEXT",
                "SEARCH",
                "CONFIGURATION",
                "DICTIONARY",
                "PARSER",
                "TEMPLATE",
                "ACCESS",
                "METHOD",
                "COLLATION",
                "CONVERSION",
                "LANGUAGE",
                "PROCEDURAL",
                "TRUSTED",
                "OPERATOR",
                "CLASS",
                "FAMILY",
                "POLICY",
                "PUBLICATION",
                "RULE",
                "STATISTICS",
                "SUBSCRIPTION",
                "TABLESPACE",
                "TRANSFORM",
                "MAPPING",
                "LOGIN",
            ];
            (DropKind::Other, skip_words(text, sig, start, &other_words) - start)
        }
    };
    let after_kind = start + kind_len;
    let cascade = (i + 1..end).any(|j| sig[j].1 == depth && sig[j].0.is_word(text, "CASCADE"));
    let mut j = skip_words(text, sig, after_kind, &["CONCURRENTLY", "BY"]);
    j = skip_if_exists(text, sig, j);
    let mut found = false;
    while j < end {
        let Some((name, next)) = qualified_name(text, sig, j) else { break };
        let stop = ["CASCADE", "RESTRICT", "ON", "FORCE"];
        if stop.iter().any(|w| sig[j].0.is_word(text, w)) {
            break;
        }
        dangers.push(Danger::DropObject { kind, name: name.to_string(), cascade });
        found = true;
        j = next;
        // A routine's arguments: `f(int, text)`.
        if sig.get(j).is_some_and(|(t, _)| t.kind == TokenKind::OpenParen) {
            let close =
                (j + 1..end).find(|&k| sig[k].1 == depth && sig[k].0.kind == TokenKind::CloseParen);
            j = close.map_or(end, |k| k + 1);
        }
        if sig.get(j).is_some_and(|(t, d)| t.kind == TokenKind::Comma && *d == depth) {
            j += 1;
        } else {
            break;
        }
    }
    if !found {
        dangers.push(Danger::DropObject { kind, name: String::new(), cascade });
    }
}

/// `ALTER TABLE t … DROP [COLUMN] [IF EXISTS] c, …`.
fn alter_drop(text: &str, sig: &[(Token, usize)], i: usize, dangers: &mut Vec<Danger>) {
    if !is_at(text, sig, i + 1, "TABLE") {
        return;
    }
    let depth = sig[i].1;
    let end = statement_end(sig, i);
    let j = skip_if_exists(text, sig, i + 2);
    let j = skip_words(text, sig, j, &["ONLY"]);
    let Some((table, mut j)) = qualified_name(text, sig, j) else { return };
    if sig.get(j).is_some_and(|(t, _)| t.kind == TokenKind::Operator && t.text(text) == "*") {
        j += 1;
    }
    // Not columns: what else `DROP` names in an `ALTER TABLE`.
    let others = [
        "CONSTRAINT",
        "INDEX",
        "KEY",
        "PRIMARY",
        "FOREIGN",
        "CHECK",
        "PARTITION",
        "DEFAULT",
        "NOT",
        "EXPRESSION",
        "IDENTITY",
        "PERIOD",
        "SYSTEM",
        "TRIGGER",
        "ROW",
        "OIDS",
    ];
    let mut action_start = true;
    while j < end {
        let (token, d) = sig[j];
        if d == depth && token.kind == TokenKind::Comma {
            action_start = true;
            j += 1;
            continue;
        }
        if action_start && d == depth && token.is_word(text, "DROP") {
            let mut k = j + 1;
            let said_column = is_at(text, sig, k, "COLUMN");
            if said_column {
                k += 1;
            }
            let other = !said_column && others.iter().any(|w| is_at(text, sig, k, w));
            if !other {
                k = skip_if_exists(text, sig, k);
                if let Some((t, _)) = sig.get(k).filter(|(t, _)| t.is_name()) {
                    let column = crate::sql::identifier(text, t).into_owned();
                    dangers.push(Danger::AlterDrop { table: table.clone(), column });
                }
            }
        }
        action_start = false;
        j += 1;
    }
}

#[cfg(test)]
#[path = "../tests/unit/danger.rs"]
mod tests;
