//! What a console's run writes and decides for PostgreSQL, pure (tested without a server):
//! which statements read from a cursor (`DECLARE … NO SCROLL CURSOR FOR`, fetched a page at
//! a time — a real server-side cursor, in the session's transaction or in a read-only one
//! of its own), the statements around them, an error's place taken back from a statement
//! wrapped in another, and the server's errors and notices as the domain says them.

use database::sql::{self, Flavor, SplitOptions, StatementKind, TokenKind};
use database::{
    ConsoleCaps, DbError, ExplainKinds, Isolation, Isolations, Notice, SchemaSwitch, SqlError,
};
use tokio_postgres::error::ErrorPosition;

/// What its console can do, on every supported version: cursors, both plans and profiles, the three
/// isolation levels and read-only transactions, the schema by `search_path` and another
/// database by connecting again, errors with their place, notices.
pub const CONSOLE: ConsoleCaps = ConsoleCaps {
    cursors: true,
    explain: ExplainKinds { plan: true, analyze: true },
    isolation: Isolations::of(&[
        Isolation::ReadCommitted,
        Isolation::RepeatableRead,
        Isolation::Serializable,
    ]),
    read_only: true,
    switch_schema: SchemaSwitch::Statement,
    databases: true,
    error_positions: true,
    notices: true,
    limited: false,
    // The oldest version's: a session says its own server's (`Session::console`).
    profile: crate::profile::caps(database::postgres::OLDEST),
};

/// The most notices kept between two runs (a session the console doesn't read from keeps
/// no more).
pub const NOTICES: usize = 1000;

/// The SQLSTATE of a write refused in a read-only transaction.
pub const READ_ONLY_TRANSACTION: &str = "25006";

/// The statement of a console's run read: the text the engine gets (without the `;` and the
/// comments around it) and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement<'a> {
    pub body: &'a str,
    /// The characters before `body` in the text.
    pub leading: usize,
    pub kind: StatementKind,
    /// More than one statement in the text (run as one simple query: no cursor).
    pub several: bool,
    /// Its first word, upper-case.
    pub first: String,
    /// It locks the rows it reads (`FOR UPDATE`, `FOR SHARE`…: no read-only transaction).
    pub locks: bool,
}

/// `text` read by `version`'s rules.
pub fn statement(text: &str, version: u32) -> Statement<'_> {
    let flavor = Flavor::Postgres { version: Some(version) };
    let parsed = sql::parse(text, flavor, &SplitOptions::default());
    let Some(first) = parsed.statements.first() else {
        return Statement {
            body: text.trim(),
            leading: 0,
            kind: StatementKind::Other,
            several: false,
            first: String::new(),
            locks: false,
        };
    };
    let tokens = parsed.tokens_of(first);
    let words: Vec<String> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.text(text).to_ascii_uppercase())
        .collect();
    let locks = words
        .windows(2)
        .any(|w| w[0] == "FOR" && matches!(w[1].as_str(), "UPDATE" | "SHARE" | "NO" | "KEY"));
    Statement {
        body: &text[first.range.clone()],
        leading: text[..first.range.start].chars().count(),
        kind: sql::classify(text, tokens, flavor),
        several: parsed.statements.len() > 1,
        first: words.first().cloned().unwrap_or_default(),
        locks,
    }
}

impl Statement<'_> {
    /// It can be read through a cursor: one query a `DECLARE` takes (`SELECT`, `WITH …
    /// SELECT`, `VALUES`, `TABLE x`; not `SHOW`, `FETCH`, `EXPLAIN`), locking no row (its
    /// own cursor's transaction is read-only).
    pub fn cursorable(&self) -> bool {
        !self.several
            && !self.locks
            && self.kind == StatementKind::Query
            && matches!(self.first.as_str(), "SELECT" | "WITH" | "VALUES" | "TABLE")
    }
}

/// A cursor's name (a plain identifier: no quoting needed).
pub fn cursor_name(n: u64) -> String {
    format!("console_cursor_{n}")
}

/// What declares cursor `name` over `body`, and how many characters come before `body` in
/// it (to take an error's place back).
pub fn declare(name: &str, body: &str) -> (String, usize) {
    let prefix = format!("DECLARE {name} NO SCROLL CURSOR FOR ");
    let chars = prefix.chars().count();
    (format!("{prefix}{body}"), chars)
}

/// The next `n` rows of cursor `name`.
pub fn fetch(name: &str, n: u64) -> String {
    format!("FETCH FORWARD {n} FROM {name}")
}

pub fn close(name: &str) -> String {
    format!("CLOSE {name}")
}

/// Unqualified names looked for in `schema`, then `public` (`quoted`: its name quoted).
pub fn search_path(schema: &str, quoted: &str) -> String {
    if schema == "public" {
        "SET search_path TO public".to_owned()
    } else {
        format!("SET search_path TO {quoted}, public")
    }
}

/// A read-only source's sessions refuse every write, whatever the app checks before.
pub const READ_ONLY_SESSION: &str = "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY";

/// `error` with its place counted in the text the user's run was given: the `prefix`
/// characters of a wrapping statement left out (a place inside those: none), the `leading`
/// ones before the statement in that text (comments, spaces) counted.
pub fn shifted(error: DbError, prefix: usize, leading: usize) -> DbError {
    match error {
        DbError::Sql(mut e) => {
            e.position = match e.position {
                Some(sql::EnginePosition::Character(n)) if n > prefix => {
                    Some(sql::EnginePosition::Character(n - prefix + leading))
                }
                Some(sql::EnginePosition::Character(_)) => None,
                other => other,
            };
            DbError::Sql(e)
        }
        other => other,
    }
}

/// A server's error as the domain says it: its message, code, place (in the statement sent;
/// one inside a query the server made itself is no place in the user's), detail and hint.
pub fn sql_error(
    message: &str,
    code: &str,
    position: Option<&ErrorPosition>,
    detail: Option<&str>,
    hint: Option<&str>,
) -> SqlError {
    SqlError {
        message: message.to_owned(),
        sqlstate: (!code.is_empty()).then(|| code.to_owned()),
        position: match position {
            Some(ErrorPosition::Original(n)) => {
                usize::try_from(*n).ok().filter(|n| *n > 0).map(sql::EnginePosition::Character)
            }
            _ => None,
        },
        detail: detail.map(str::to_owned),
        hint: hint.map(str::to_owned),
    }
}

/// A server's notice.
pub fn notice(error: &tokio_postgres::error::DbError) -> Notice {
    Notice {
        severity: error.severity().to_owned(),
        message: error.message().to_owned(),
        detail: error.detail().map(str::to_owned),
        hint: error.hint().map(str::to_owned),
    }
}

#[cfg(test)]
#[path = "../tests/unit/console.rs"]
mod tests;
