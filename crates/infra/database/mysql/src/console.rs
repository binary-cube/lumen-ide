//! What a console's run writes and decides for MySQL and MariaDB, pure (tested without a
//! server): what a version's console can do ([`caps`]), a statement read by MySQL's rules
//! ([`statement`]: its body without comments and `;`, what it is), the transactions'
//! statements by isolation, an error's place taken back from MySQL's words (`near '…' at
//! line N`: [`near_position`]) and from a statement wrapped in another ([`shifted`]), and the
//! server's errors and warnings as the domain says them.

use database::mysql::ServerVersion;
use database::sql::{self, Flavor, SplitOptions, StatementKind, TokenKind};
use database::{
    ConsoleCaps, DbError, ExplainKinds, Isolation, Isolations, Notice, SchemaSwitch, SqlError,
    TxOptions,
};

/// What its console can do before a session says its own server's ([`caps`]): no cursor
/// (a query's next page is the query run again from the row after, which the engine
/// sorts and filters too), plans, the three isolation levels the domain knows and
/// read-only transactions, the schema by `USE`, errors with their place, warnings.
pub const CONSOLE: ConsoleCaps = caps(ServerVersion::mysql(database::mysql::OLDEST_MYSQL));

/// What `version`'s console can do (a plan as it ran from MySQL 8.0.18's `EXPLAIN ANALYZE`
/// and MariaDB's `ANALYZE FORMAT=JSON`).
pub const fn caps(version: ServerVersion) -> ConsoleCaps {
    ConsoleCaps {
        cursors: false,
        explain: ExplainKinds { plan: true, analyze: crate::profile::runs(version) },
        isolation: Isolations::of(&[
            Isolation::ReadCommitted,
            Isolation::RepeatableRead,
            Isolation::Serializable,
        ]),
        read_only: true,
        switch_schema: SchemaSwitch::Statement,
        // Its databases are its schemas: one session reads them all.
        databases: false,
        error_positions: true,
        notices: true,
        limited: false,
        profile: crate::profile::caps(version),
    }
}

/// The error a statement killed by `KILL QUERY` ends with (`ER_QUERY_INTERRUPTED`).
pub const INTERRUPTED: u16 = 1317;
/// MySQL's `max_execution_time` reached (`ER_QUERY_TIMEOUT`).
pub const TIMED_OUT: u16 = 3024;
/// A write refused in a read-only transaction (`ER_CANT_EXECUTE_IN_READ_ONLY_TRANSACTION`).
pub const READ_ONLY_TRANSACTION: u16 = 1792;

/// The statement of a console's run read: the text the engine gets (without the `;` and the
/// comments around it) and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement<'a> {
    pub body: &'a str,
    /// The characters before `body` in the text.
    pub leading: usize,
    pub kind: StatementKind,
    /// More than one statement in the text.
    pub several: bool,
    /// Its first word, upper-case.
    pub first: String,
    /// It locks the rows it reads (`FOR UPDATE`, `FOR SHARE`, `LOCK IN SHARE MODE`: no
    /// read-only transaction).
    pub locks: bool,
}

/// `text` read by MySQL's rules.
pub fn statement(text: &str) -> Statement<'_> {
    let flavor = Flavor::MySql;
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
    let locks = words.windows(2).any(|w| {
        (w[0] == "FOR" && matches!(w[1].as_str(), "UPDATE" | "SHARE"))
            || (w[0] == "SHARE" && w[1] == "MODE")
    });
    Statement {
        body: &text[first.range.clone()],
        leading: text[..first.range.start].chars().count(),
        kind: sql::classify(text, tokens, flavor),
        several: parsed.statements.len() > 1,
        first: words.first().cloned().unwrap_or_default(),
        locks,
    }
}

/// What begins a transaction of `options`: its isolation set for the next transaction
/// first (MySQL takes no level in `START TRANSACTION`), then `START TRANSACTION [READ
/// ONLY]`.
pub fn begin_statements(options: &TxOptions) -> Result<Vec<String>, DbError> {
    let mut out = Vec::new();
    match options.isolation {
        None => {}
        Some(kind @ (Isolation::Deferred | Isolation::Immediate | Isolation::Exclusive)) => {
            return Err(DbError::Unsupported(format!("{} in MySQL", kind.id())));
        }
        Some(level) => out.push(format!("SET TRANSACTION {}", level.sql())),
    }
    out.push(if options.read_only {
        "START TRANSACTION READ ONLY".to_owned()
    } else {
        "START TRANSACTION".to_owned()
    });
    Ok(out)
}

/// A read-only source's sessions refuse every write, whatever the app checks before.
pub const READ_ONLY_SESSION: &str = "SET SESSION TRANSACTION READ ONLY";

/// Where MySQL's words put an error in `statement` (`… near 'FROMM t' at line 2`): the
/// character (from 1) where the quoted text starts on or after that line; at its end for
/// `near ''`. `None`: the message names no place, or that text isn't there.
pub fn near_position(statement: &str, message: &str) -> Option<usize> {
    let start = message.find("near '")? + "near '".len();
    let end = message.rfind("' at line ")?;
    if end < start {
        return None;
    }
    let line: usize = message[end + "' at line ".len()..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    let near = &message[start..end];
    if near.is_empty() {
        return Some(statement.trim_end().chars().count() + 1);
    }
    // The line's first byte (lines counted from 1).
    let mut from = 0;
    for _ in 1..line.max(1) {
        from += statement[from..].find('\n')? + 1;
    }
    // The server cuts its quote at 80 characters (and may end mid-character).
    let probe: String = near.chars().take(40).collect();
    let at = from + statement[from..].find(probe.as_str())?;
    Some(statement[..at].chars().count() + 1)
}

/// `error` with its place counted in the text the user's run was given: the `prefix`
/// characters of a wrapping statement left out (a place inside those: none), the `leading`
/// ones before the statement in that text counted.
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

/// A server's error as the domain says it: its message, SQLSTATE, the place its words name
/// in `statement` (the text sent), its code as the detail.
pub fn sql_error(code: u16, state: &str, message: &str, statement: Option<&str>) -> SqlError {
    SqlError {
        message: message.to_owned(),
        sqlstate: (!state.is_empty() && state != "HY000").then(|| state.to_owned()),
        position: statement
            .and_then(|s| near_position(s, message))
            .map(sql::EnginePosition::Character),
        detail: Some(format!("error {code}")),
        hint: None,
    }
}

/// A row of `SHOW WARNINGS` (`Level`, `Code`, `Message`) as a notice.
pub fn warning(level: &str, code: &str, message: &str) -> Notice {
    Notice {
        severity: level.to_ascii_uppercase(),
        message: message.to_owned(),
        detail: (!code.is_empty()).then(|| format!("error {code}")),
        hint: None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/console.rs"]
mod tests;
