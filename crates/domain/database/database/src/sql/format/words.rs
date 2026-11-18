//! The words the formatter recognizes: the keywords whose case it changes (reserved words of
//! the SQL standard, SQLite and PostgreSQL — names that can't be written unquoted anyway, so
//! changing their case never changes what they name), the words only keywords in a place
//! (`KEY` after `PRIMARY`), the type names (written as keywords where a type goes) and the
//! keywords a parenthesis may follow without being a call. All uppercase, sorted (searched
//! by bisection).

/// Keywords wherever they are (unless a name: after or before a `.`).
const KEYWORDS: &[&str] = &[
    "ADD",
    "AFTER",
    "ALL",
    "ALTER",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ATOMIC",
    "ATTACH",
    "AUTOINCREMENT",
    "BEFORE",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASCADE",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "COMMIT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "DEFAULT",
    "DEFERRABLE",
    "DEFERRED",
    "DELETE",
    "DESC",
    "DETACH",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "ESCAPE",
    "EXCEPT",
    "EXCLUSIVE",
    "EXISTS",
    "EXPLAIN",
    "FALSE",
    "FETCH",
    "FOLLOWING",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "FUNCTION",
    "GLOB",
    "GRANT",
    "GROUP",
    "HAVING",
    "IF",
    "ILIKE",
    "IMMEDIATE",
    "IN",
    "INDEX",
    "INITIALLY",
    "INNER",
    "INSERT",
    "INSTEAD",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "MATERIALIZED",
    "MERGE",
    "NATURAL",
    "NOT",
    "NOTNULL",
    "NULL",
    "NULLS",
    "OF",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "PRAGMA",
    "PRECEDING",
    "PRIMARY",
    "PROCEDURE",
    "RECURSIVE",
    "REFERENCES",
    "REGEXP",
    "RELEASE",
    "RENAME",
    "REPLACE",
    "RESTRICT",
    "RETURNING",
    "RETURNS",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "SAVEPOINT",
    "SELECT",
    "SEQUENCE",
    "SET",
    "SIMILAR",
    "SOME",
    "TABLE",
    "TEMP",
    "TEMPORARY",
    "THEN",
    "TO",
    "TRANSACTION",
    "TRIGGER",
    "TRUE",
    "TRUNCATE",
    "UNBOUNDED",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VACUUM",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
    "WITHIN",
    "WITHOUT",
];

/// Keywords only after one of their words (`PRIMARY KEY`, `NO ACTION`, `WITHOUT ROWID`…):
/// elsewhere they're often names (a column `key`, `action`, `first`).
const AFTER: &[(&str, &[&str])] = &[
    ("ABORT", &["OR"]),
    ("ACTION", &["NO"]),
    ("CONFLICT", &["ON", "OR"]),
    ("CURRENT", &["AND", "BETWEEN"]),
    ("DATABASE", &["ATTACH", "DETACH", "CREATE", "DROP"]),
    ("DUPLICATE", &["ON"]),
    ("EACH", &["FOR"]),
    ("FAIL", &["OR"]),
    ("FIRST", &["FETCH", "NULLS"]),
    ("IGNORE", &["OR", "INSERT"]),
    ("KEY", &["PRIMARY", "FOREIGN", "DUPLICATE"]),
    ("LAST", &["NULLS"]),
    ("LOCKED", &["SKIP"]),
    ("MATCHED", &["WHEN", "NOT"]),
    ("NEXT", &["FETCH"]),
    ("NO", &["ON", "DELETE", "UPDATE"]),
    ("NOTHING", &["DO"]),
    ("NOWAIT", &["UPDATE", "SHARE"]),
    ("PLAN", &["QUERY"]),
    ("QUERY", &["EXPLAIN"]),
    ("ROW", &["EACH", "CURRENT"]),
    ("ROWID", &["WITHOUT"]),
    ("ROWS", &["FIRST", "NEXT", "UNBOUNDED"]),
    ("SHARE", &["FOR"]),
    ("SKIP", &["UPDATE", "SHARE"]),
    ("STATEMENT", &["EACH"]),
    ("TIES", &["WITH"]),
    ("TIME", &["WITH", "WITHOUT"]),
    ("ZONE", &["TIME"]),
];

/// Type names, written as keywords where a type goes (after `::`, in a column's definition,
/// in `CAST(… AS type)`).
const TYPES: &[&str] = &[
    "BIGINT",
    "BIGSERIAL",
    "BINARY",
    "BIT",
    "BLOB",
    "BOOL",
    "BOOLEAN",
    "BYTEA",
    "CHAR",
    "CHARACTER",
    "CIDR",
    "CLOB",
    "DATE",
    "DATETIME",
    "DEC",
    "DECIMAL",
    "DOUBLE",
    "FLOAT",
    "INET",
    "INT",
    "INT2",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERVAL",
    "JSON",
    "JSONB",
    "MONEY",
    "NCHAR",
    "NUMERIC",
    "NVARCHAR",
    "PRECISION",
    "REAL",
    "SERIAL",
    "SMALLINT",
    "SMALLSERIAL",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "TIMETZ",
    "TINYINT",
    "UNSIGNED",
    "UUID",
    "VARBINARY",
    "VARCHAR",
    "VARYING",
    "XML",
    "ZONE",
];

/// Keywords a `(` may follow without being a call, a space between (`IN (`, `EXISTS (`,
/// `VALUES (`, `OVER (`): before a `(` any other word is a function's name.
const BEFORE_PAREN: &[&str] = &[
    "ALL",
    "ANALYZE",
    "AND",
    "AS",
    "BETWEEN",
    "BY",
    "CHECK",
    "CONFLICT",
    "DEFAULT",
    "DISTINCT",
    "ELSE",
    "EXCEPT",
    "EXISTS",
    "EXPLAIN",
    "FILTER",
    "FROM",
    "GROUP",
    "IN",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LATERAL",
    "LIKE",
    "MATERIALIZED",
    "NOT",
    "ON",
    "OR",
    "OVER",
    "RETURNING",
    "SELECT",
    "SET",
    "THEN",
    "UNION",
    "UNIQUE",
    "USING",
    "VACUUM",
    "VALUES",
    "WHEN",
    "WHERE",
    "WITH",
    "WITHIN",
];

/// Keywords written like a call, the `(` right after them (`CAST(x AS int)`, `ANY(…)`).
const CALL_LIKE: &[&str] = &["ANY", "ARRAY", "CAST", "SOME"];

fn has(list: &[&str], upper: &str) -> bool {
    list.binary_search(&upper).is_ok()
}

/// `upper` is a keyword wherever it stands.
pub(super) fn keyword(upper: &str) -> bool {
    has(KEYWORDS, upper)
}

/// `upper` is a keyword after `previous` (uppercase).
pub(super) fn keyword_after(upper: &str, previous: &str) -> bool {
    AFTER
        .binary_search_by(|(word, _)| word.cmp(&upper))
        .is_ok_and(|i| AFTER[i].1.contains(&previous))
}

pub(super) fn type_name(upper: &str) -> bool {
    has(TYPES, upper)
}

/// `upper` before a `(` is still a keyword (a space between).
pub(super) fn before_paren(upper: &str) -> bool {
    has(BEFORE_PAREN, upper)
}

/// `upper` before a `(` is a keyword written like a call (no space).
pub(super) fn call_like(upper: &str) -> bool {
    has(CALL_LIKE, upper)
}

#[cfg(test)]
pub(super) fn lists() -> [&'static [&'static str]; 4] {
    [KEYWORDS, TYPES, BEFORE_PAREN, CALL_LIKE]
}

#[cfg(test)]
pub(super) fn after_words() -> Vec<&'static str> {
    AFTER.iter().map(|(w, _)| *w).collect()
}
