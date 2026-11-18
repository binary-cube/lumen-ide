//! What a statement does, from its words: whether it reads, writes, changes the schema,
//! controls a transaction or the session. What isn't known is [`StatementKind::Other`], and
//! counts as writing.

use super::lex::{Token, TokenKind};
use super::{Flavor, significant};

/// What a statement does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// Reads only: `SELECT`, `WITH … SELECT`, `VALUES`, `TABLE x`, `SHOW`, a `PRAGMA` that
    /// reads, `FETCH`.
    Query,
    /// Changes rows (a `WITH` whose parts write included).
    Write(WriteKind),
    /// Changes the schema or the rights.
    Ddl(DdlKind),
    /// Starts or ends a transaction, or a savepoint.
    Transaction(TxControl),
    /// `EXPLAIN`: with `analyze` it runs the statement, which `writes` or not.
    Explain { analyze: bool, writes: bool },
    /// Changes the session: `SET`, `USE`, `ATTACH`, a `PRAGMA` that sets.
    Session,
    /// Not known (`CALL`, `DO`, `COPY`, `VACUUM`…): treated as a write.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    Insert,
    Update,
    Delete,
    Merge,
    /// An insert that may update (`ON CONFLICT … DO UPDATE`, `INSERT OR REPLACE`,
    /// `REPLACE`, `ON DUPLICATE KEY UPDATE`).
    Upsert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DdlKind {
    Create,
    Alter,
    Drop,
    Truncate,
    Rename,
    Comment,
    /// `GRANT`, `REVOKE`.
    Grant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxControl {
    Begin,
    Commit,
    Rollback,
    Savepoint,
    Release,
    RollbackTo,
}

impl StatementKind {
    /// Whether running it may change something: a write, DDL, an `EXPLAIN ANALYZE` of a
    /// write, or what isn't known.
    pub fn writes(self) -> bool {
        match self {
            StatementKind::Write(_) | StatementKind::Ddl(_) | StatementKind::Other => true,
            StatementKind::Explain { analyze, writes } => analyze && writes,
            StatementKind::Query | StatementKind::Transaction(_) | StatementKind::Session => false,
        }
    }

    /// Whether it changes the schema (what's known of it must be read again).
    pub fn touches_schema(self) -> bool {
        matches!(self, StatementKind::Ddl(_))
    }

    /// Whether it only reads, so running it again (for another page) changes nothing.
    pub fn is_query(self) -> bool {
        self == StatementKind::Query
    }
}

/// What the statement made of `tokens` (read from `text`) does.
pub fn classify(text: &str, tokens: &[Token], flavor: Flavor) -> StatementKind {
    let sig = significant(tokens);
    kind_at(text, &sig, 0, flavor)
}

/// The words of `sig` at the depth of `sig[from]`, from there, as long as they stay at it.
struct Words<'a> {
    text: &'a str,
    sig: &'a [(Token, usize)],
}

impl Words<'_> {
    fn word(&self, i: usize) -> Option<&str> {
        let (t, _) = self.sig.get(i)?;
        (t.kind == TokenKind::Word).then(|| t.text(self.text))
    }

    fn is(&self, i: usize, keyword: &str) -> bool {
        self.word(i).is_some_and(|w| w.eq_ignore_ascii_case(keyword))
    }

    /// The index of the first word `keyword` at `depth`, from `from` on, before the depth
    /// drops under it.
    fn find(&self, from: usize, depth: usize, keyword: &str) -> Option<usize> {
        for (i, (t, d)) in self.sig.iter().enumerate().skip(from) {
            if *d < depth {
                return None;
            }
            if *d == depth && t.is_word(self.text, keyword) {
                return Some(i);
            }
        }
        None
    }
}

/// The kind of the statement starting at `sig[at]` (a `(` before it skipped).
pub(crate) fn kind_at(
    text: &str,
    sig: &[(Token, usize)],
    at: usize,
    flavor: Flavor,
) -> StatementKind {
    let w = Words { text, sig };
    let mut i = at;
    while sig.get(i).is_some_and(|(t, _)| t.kind == TokenKind::OpenParen) {
        i += 1;
    }
    let Some(first) = w.word(i) else {
        return StatementKind::Other;
    };
    let depth = sig[i].1;
    let first = first.to_ascii_uppercase();
    match first.as_str() {
        "SELECT" => {
            // PostgreSQL's `SELECT … INTO t` makes a table (MySQL's is into variables).
            let into = !matches!(flavor, Flavor::Sqlite | Flavor::MySql)
                && w.find(i + 1, depth, "INTO").is_some();
            if into { StatementKind::Ddl(DdlKind::Create) } else { StatementKind::Query }
        }
        "VALUES" | "TABLE" | "SHOW" | "DESCRIBE" | "DESC" | "FETCH" | "MOVE" => {
            StatementKind::Query
        }
        "WITH" => with(text, sig, i, flavor),
        "INSERT" => insert(&w, i, depth),
        "REPLACE" => StatementKind::Write(WriteKind::Upsert),
        "UPDATE" => StatementKind::Write(WriteKind::Update),
        "DELETE" => StatementKind::Write(WriteKind::Delete),
        "MERGE" => StatementKind::Write(WriteKind::Merge),
        "CREATE" | "IMPORT" => StatementKind::Ddl(DdlKind::Create),
        "ALTER" => StatementKind::Ddl(DdlKind::Alter),
        "DROP" => StatementKind::Ddl(DdlKind::Drop),
        "TRUNCATE" => StatementKind::Ddl(DdlKind::Truncate),
        "RENAME" => StatementKind::Ddl(DdlKind::Rename),
        "COMMENT" => StatementKind::Ddl(DdlKind::Comment),
        "GRANT" | "REVOKE" => StatementKind::Ddl(DdlKind::Grant),
        "REASSIGN" => StatementKind::Ddl(DdlKind::Alter),
        "BEGIN" | "START" => StatementKind::Transaction(TxControl::Begin),
        "COMMIT" | "END" => StatementKind::Transaction(TxControl::Commit),
        "ROLLBACK" | "ABORT" => {
            let to = w.find(i + 1, depth, "TO").is_some();
            StatementKind::Transaction(if to { TxControl::RollbackTo } else { TxControl::Rollback })
        }
        "SAVEPOINT" => StatementKind::Transaction(TxControl::Savepoint),
        "RELEASE" => StatementKind::Transaction(TxControl::Release),
        "EXPLAIN" => explain(text, sig, i, flavor),
        "PRAGMA" => pragma(&w, i),
        "SET" | "RESET" | "USE" | "ATTACH" | "DETACH" | "DISCARD" | "LISTEN" | "UNLISTEN"
        | "DECLARE" | "CLOSE" | "PREPARE" | "DEALLOCATE" | "LOAD" => StatementKind::Session,
        _ => StatementKind::Other,
    }
}

fn insert(w: &Words<'_>, i: usize, depth: usize) -> StatementKind {
    // SQLite's `INSERT OR REPLACE`.
    if w.is(i + 1, "OR") && w.is(i + 2, "REPLACE") {
        return StatementKind::Write(WriteKind::Upsert);
    }
    let mut from = i + 1;
    while let Some(on) = w.find(from, depth, "ON") {
        let conflict = w.is(on + 1, "CONFLICT");
        let duplicate = w.is(on + 1, "DUPLICATE");
        if duplicate {
            return StatementKind::Write(WriteKind::Upsert);
        }
        if conflict {
            let update = w.find(on + 2, depth, "UPDATE");
            let doing = update.is_some_and(|u| w.is(u - 1, "DO"));
            if doing {
                return StatementKind::Write(WriteKind::Upsert);
            }
        }
        from = on + 1;
    }
    StatementKind::Write(WriteKind::Insert)
}

/// A `WITH`: the kind of its main statement, or a write when one of its parts writes.
fn with(text: &str, sig: &[(Token, usize)], i: usize, flavor: Flavor) -> StatementKind {
    let depth = sig[i].1;
    let mut part_writes: Option<WriteKind> = None;
    let mut main = None;
    let mut j = i + 1;
    while j < sig.len() {
        let (t, d) = sig[j];
        if d < depth {
            break;
        }
        if d == depth && t.kind == TokenKind::OpenParen {
            // A part's body: `name AS [NOT] [MATERIALIZED] (…)`.
            if let StatementKind::Write(kind) = kind_at(text, sig, j + 1, flavor) {
                part_writes.get_or_insert(kind);
            }
        }
        if d == depth && t.kind == TokenKind::Word {
            let word = t.text(text);
            let main_word = ["SELECT", "INSERT", "UPDATE", "DELETE", "MERGE", "VALUES", "TABLE"]
                .iter()
                .any(|k| word.eq_ignore_ascii_case(k));
            if main_word {
                main = Some(kind_at(text, sig, j, flavor));
                break;
            }
        }
        j += 1;
    }
    match (main, part_writes) {
        (Some(StatementKind::Write(kind)), _) => StatementKind::Write(kind),
        (_, Some(kind)) => StatementKind::Write(kind),
        (Some(kind), None) => kind,
        // Unfinished: nothing said yet beyond its parts.
        (None, None) => StatementKind::Query,
    }
}

/// `EXPLAIN [QUERY PLAN] [ANALYZE] [VERBOSE] …` or `EXPLAIN (ANALYZE[, …]) …`.
fn explain(text: &str, sig: &[(Token, usize)], i: usize, flavor: Flavor) -> StatementKind {
    let w = Words { text, sig };
    let depth = sig[i].1;
    let mut analyze = false;
    let mut j = i + 1;
    let options = [
        "ANALYZE",
        "ANALYSE",
        "VERBOSE",
        "QUERY",
        "PLAN",
        "COSTS",
        "BUFFERS",
        "FORMAT",
        "SETTINGS",
        "WAL",
        "TIMING",
        "SUMMARY",
        "GENERIC_PLAN",
        "MEMORY",
        "SERIALIZE",
        "EXTENDED",
    ];
    while let Some((t, d)) = sig.get(j) {
        if *d == depth && t.kind == TokenKind::OpenParen {
            // `(ANALYZE [true | on], BUFFERS, …)`, unless it's the statement's own `(`.
            let opens_options = w
                .word(j + 1)
                .is_some_and(|word| options.iter().any(|o| word.eq_ignore_ascii_case(o)));
            if !opens_options {
                break;
            }
            let mut k = j + 1;
            while let Some((t, d)) = sig.get(k) {
                if *d <= depth {
                    break;
                }
                if t.is_word(text, "ANALYZE") || t.is_word(text, "ANALYSE") {
                    let off = w.word(k + 1).is_some_and(|v| {
                        ["FALSE", "OFF", "0"].iter().any(|o| v.eq_ignore_ascii_case(o))
                    }) || sig.get(k + 1).is_some_and(|(n, _)| n.text(text) == "0");
                    analyze = !off;
                }
                k += 1;
            }
            j = k + 1;
            continue;
        }
        let Some(word) = w.word(j) else { break };
        if !options.iter().any(|o| word.eq_ignore_ascii_case(o)) {
            break;
        }
        if word.eq_ignore_ascii_case("ANALYZE") || word.eq_ignore_ascii_case("ANALYSE") {
            analyze = true;
        }
        j += 1;
    }
    let writes = sig.get(j).is_some() && kind_at(text, sig, j, flavor).writes();
    StatementKind::Explain { analyze, writes }
}

/// SQLite's `PRAGMA`: setting one (`= v`, or a function form of one that sets) changes the
/// session; reading one is a query.
fn pragma(w: &Words<'_>, i: usize) -> StatementKind {
    // `PRAGMA [schema.]name`.
    let mut name_at = i + 1;
    if w.sig.get(i + 2).is_some_and(|(t, _)| t.kind == TokenKind::Dot) {
        name_at = i + 3;
    }
    let name = w.word(name_at).unwrap_or_default().to_ascii_lowercase();
    let next = w.sig.get(name_at + 1).map(|(t, _)| (t.kind, t.text(w.text)));
    // What runs something rather than reading a value.
    let acts = ["optimize", "incremental_vacuum", "wal_checkpoint", "shrink_memory"];
    // What reads, given an argument.
    let reads_with_argument = [
        "table_info",
        "table_xinfo",
        "table_list",
        "index_info",
        "index_xinfo",
        "index_list",
        "foreign_key_list",
        "foreign_key_check",
        "integrity_check",
        "quick_check",
        "function_list",
        "module_list",
        "pragma_list",
        "collation_list",
        "database_list",
    ];
    match next {
        Some((TokenKind::Operator, "=")) => StatementKind::Session,
        Some((TokenKind::OpenParen, _)) if !reads_with_argument.contains(&name.as_str()) => {
            StatementKind::Session
        }
        _ if acts.contains(&name.as_str()) => StatementKind::Session,
        _ => StatementKind::Query,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/sql/classify.rs"]
mod tests;
