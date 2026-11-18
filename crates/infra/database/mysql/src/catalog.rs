//! The catalog's queries, pure (tested without a server): `information_schema` as every
//! supported version keeps it — MySQL 5.7 to the newest, MariaDB 10.2 to the newest — read
//! in bulk where the console asks (every table's columns, every foreign key, the routines
//! and their parameters in one query each). Names and values are written in as literals
//! ([`database::mysql::mysql_text`]), so every query runs through the text protocol.
//!
//! A database is a schema here: `TABLE_SCHEMA` is the database's name. The server's own
//! schemas (`information_schema`, `mysql`, `performance_schema`, `sys`) are never listed
//! as the user's. MySQL lists only what the user has some privilege on: what it may not
//! reach doesn't exist for it, whatever the query.

use database::mysql::{SYSTEM_SCHEMAS, ServerVersion, mysql_text};
use database::{FkAction, TableKind};

/// `schema = …` for `column`: a schema named, else the session's database.
fn schema_is(column: &str, schema: Option<&str>) -> String {
    match schema {
        Some(schema) => format!("{column} = {}", mysql_text(schema)),
        None => format!("{column} = DATABASE()"),
    }
}

/// `column` isn't one of the server's own schemas.
fn not_system(column: &str) -> String {
    let list: Vec<String> = SYSTEM_SCHEMAS.iter().map(|s| format!("'{s}'")).collect();
    format!("{column} NOT IN ({})", list.join(", "))
}

/// Every schema (database) of the user's.
pub fn schemas() -> String {
    format!(
        "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE {} ORDER BY SCHEMA_NAME",
        not_system("SCHEMA_NAME")
    )
}

/// The tables and views of every user schema, or of `schema` alone: its schema, name, type
/// and the engine's row estimate.
pub fn tables(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => schema_is("TABLE_SCHEMA", Some(schema)),
        None => not_system("TABLE_SCHEMA"),
    };
    format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, TABLE_ROWS FROM information_schema.TABLES \
         WHERE {filter} AND TABLE_TYPE <> 'SEQUENCE' ORDER BY TABLE_SCHEMA, TABLE_NAME"
    )
}

/// What a `TABLE_TYPE` is (`BASE TABLE`, `SYSTEM VERSIONED`: a table; `VIEW`, `SYSTEM VIEW`:
/// a view).
pub fn table_kind(table_type: &str) -> TableKind {
    if table_type.to_ascii_uppercase().contains("VIEW") {
        TableKind::View
    } else {
        TableKind::Table
    }
}

/// MariaDB's sequences (10.3+): relations a query may read that the tables don't list.
pub fn sequences() -> String {
    format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME FROM information_schema.TABLES \
         WHERE TABLE_TYPE = 'SEQUENCE' AND {} ORDER BY TABLE_SCHEMA, TABLE_NAME",
        not_system("TABLE_SCHEMA")
    )
}

/// A table's columns, in order: name, `COLUMN_TYPE`, nullable (`YES`), default (as the
/// catalog writes it), `EXTRA`, generation expression, collation, `DATA_TYPE`, comment.
pub fn columns(schema: Option<&str>, table: &str) -> String {
    format!(
        "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, \
         GENERATION_EXPRESSION, COLLATION_NAME, DATA_TYPE, COLUMN_COMMENT \
         FROM information_schema.COLUMNS WHERE {} AND TABLE_NAME = {} ORDER BY ORDINAL_POSITION",
        schema_is("TABLE_SCHEMA", schema),
        mysql_text(table)
    )
}

/// A table's indexes, a row per column in order (the primary key first): index, non-unique
/// (`1`), place, column (none for an expression), its order (`D` descending), its prefix,
/// its expression (MySQL 8.0.13+'s functional parts).
pub fn indexes(version: ServerVersion, schema: Option<&str>, table: &str) -> String {
    let expression =
        if !version.mariadb && version.number >= 80013 { "EXPRESSION" } else { "NULL" };
    format!(
        "SELECT INDEX_NAME, NON_UNIQUE, SEQ_IN_INDEX, COLUMN_NAME, COLLATION, SUB_PART, \
         {expression} FROM information_schema.STATISTICS WHERE {} AND TABLE_NAME = {} \
         ORDER BY INDEX_NAME <> 'PRIMARY', INDEX_NAME, SEQ_IN_INDEX",
        schema_is("TABLE_SCHEMA", schema),
        mysql_text(table)
    )
}

/// The foreign keys of a table (`table` given: `schema`'s, else the session's database's)
/// or of every table of `schema` (none: every schema), a row per column in order:
/// schema, table, constraint, the referenced schema and table, the column and the one it
/// references, `UPDATE_RULE`, `DELETE_RULE`.
pub fn foreign_keys(schema: Option<&str>, table: Option<&str>) -> String {
    let mut filter = match (schema, table) {
        (schema, Some(_)) => schema_is("k.TABLE_SCHEMA", schema),
        (Some(schema), None) => schema_is("k.TABLE_SCHEMA", Some(schema)),
        (None, None) => not_system("k.TABLE_SCHEMA"),
    };
    if let Some(table) = table {
        filter.push_str(&format!(" AND k.TABLE_NAME = {}", mysql_text(table)));
    }
    format!(
        "SELECT k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.REFERENCED_TABLE_SCHEMA, \
         k.REFERENCED_TABLE_NAME, k.COLUMN_NAME, k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, \
         r.DELETE_RULE FROM information_schema.KEY_COLUMN_USAGE k \
         JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
         ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
         AND r.TABLE_NAME = k.TABLE_NAME \
         WHERE {filter} AND k.REFERENCED_TABLE_NAME IS NOT NULL \
         ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION"
    )
}

/// A referential action's rule as the catalog writes it (`CASCADE`, `SET NULL`…).
pub fn action(rule: &str) -> FkAction {
    match rule.trim().to_ascii_uppercase().as_str() {
        "CASCADE" => FkAction::Cascade,
        "SET NULL" => FkAction::SetNull,
        "SET DEFAULT" => FkAction::SetDefault,
        "RESTRICT" => FkAction::Restrict,
        _ => FkAction::NoAction,
    }
}

/// Every table's and view's columns, of `schema` (none: every user schema), in one query:
/// schema, table, column, `COLUMN_TYPE`, nullable, default, comment, its place in the
/// primary key.
pub fn catalog_columns(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => schema_is("c.TABLE_SCHEMA", Some(schema)),
        None => not_system("c.TABLE_SCHEMA"),
    };
    format!(
        "SELECT c.TABLE_SCHEMA, c.TABLE_NAME, c.COLUMN_NAME, c.COLUMN_TYPE, c.IS_NULLABLE, \
         c.COLUMN_DEFAULT, c.COLUMN_COMMENT, s.SEQ_IN_INDEX, c.EXTRA, c.DATA_TYPE \
         FROM information_schema.COLUMNS c \
         LEFT JOIN information_schema.STATISTICS s ON s.TABLE_SCHEMA = c.TABLE_SCHEMA \
         AND s.TABLE_NAME = c.TABLE_NAME AND s.INDEX_NAME = 'PRIMARY' \
         AND s.COLUMN_NAME = c.COLUMN_NAME \
         WHERE {filter} ORDER BY c.TABLE_SCHEMA, c.TABLE_NAME, c.ORDINAL_POSITION"
    )
}

/// The functions and procedures of `schema` (none: every user schema): schema, name,
/// `FUNCTION` / `PROCEDURE`, the result's type.
pub fn routines(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => schema_is("ROUTINE_SCHEMA", Some(schema)),
        None => not_system("ROUTINE_SCHEMA"),
    };
    format!(
        "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, ROUTINE_TYPE, DTD_IDENTIFIER \
         FROM information_schema.ROUTINES WHERE {filter} \
         AND ROUTINE_TYPE IN ('FUNCTION', 'PROCEDURE') ORDER BY ROUTINE_SCHEMA, ROUTINE_NAME"
    )
}

/// Their parameters, in order: schema, routine, name, type, mode (`IN`, `OUT`, `INOUT`;
/// none for a function's).
pub fn parameters(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => schema_is("SPECIFIC_SCHEMA", Some(schema)),
        None => not_system("SPECIFIC_SCHEMA"),
    };
    format!(
        "SELECT SPECIFIC_SCHEMA, SPECIFIC_NAME, PARAMETER_NAME, DTD_IDENTIFIER, PARAMETER_MODE \
         FROM information_schema.PARAMETERS WHERE {filter} AND ORDINAL_POSITION > 0 \
         ORDER BY SPECIFIC_SCHEMA, SPECIFIC_NAME, ORDINAL_POSITION"
    )
}

/// A column's default as an SQL expression, from what the catalog wrote (`raw`) — MySQL
/// writes a literal bare (`M`, `0`) and an expression bare with `DEFAULT_GENERATED` in
/// `extra`; MariaDB (10.2.7+) quotes literals itself and writes `NULL` for none.
pub fn default_of(
    raw: Option<&str>,
    extra: &str,
    data_type: &str,
    mariadb: bool,
) -> Option<String> {
    let raw = raw?;
    if mariadb {
        return (!raw.eq_ignore_ascii_case("NULL")).then(|| raw.to_owned());
    }
    let extra = extra.to_ascii_uppercase();
    let upper = raw.to_ascii_uppercase();
    if extra.contains("DEFAULT_GENERATED") || upper.starts_with("CURRENT_TIMESTAMP") {
        return Some(raw.to_owned());
    }
    let numeric = matches!(
        data_type.to_ascii_lowercase().as_str(),
        "tinyint"
            | "smallint"
            | "mediumint"
            | "int"
            | "integer"
            | "bigint"
            | "decimal"
            | "numeric"
            | "float"
            | "double"
            | "real"
            | "bit"
            | "year"
    );
    if numeric && !raw.is_empty() { Some(raw.to_owned()) } else { Some(mysql_text(raw)) }
}

/// Whether `extra` makes a column numbered by itself.
pub fn auto_increment(extra: &str) -> bool {
    extra.to_ascii_lowercase().contains("auto_increment")
}

/// Whether `extra` makes a column generated, and if so whether it's stored.
pub fn generated(extra: &str) -> Option<bool> {
    let extra = extra.to_ascii_uppercase();
    if extra.contains("STORED GENERATED") || extra.contains("PERSISTENT") {
        Some(true)
    } else if extra.contains("VIRTUAL GENERATED") || extra.contains("VIRTUAL") {
        Some(false)
    } else {
        None
    }
}

/// The server's built-in functions offered by the console's completion (MySQL's and
/// MariaDB's reference manuals: the common ones, by name, their argument counts; `None`
/// for any number). `information_schema` lists only the user's own.
pub const BUILTINS: &[(&str, Option<u16>)] = &[
    ("ABS", Some(1)),
    ("ACOS", Some(1)),
    ("ADDDATE", Some(2)),
    ("ADDTIME", Some(2)),
    ("AES_DECRYPT", None),
    ("AES_ENCRYPT", None),
    ("ANY_VALUE", Some(1)),
    ("ASCII", Some(1)),
    ("ASIN", Some(1)),
    ("ATAN", None),
    ("AVG", Some(1)),
    ("BENCHMARK", Some(2)),
    ("BIN", Some(1)),
    ("BIT_AND", Some(1)),
    ("BIT_COUNT", Some(1)),
    ("BIT_LENGTH", Some(1)),
    ("BIT_OR", Some(1)),
    ("BIT_XOR", Some(1)),
    ("CAST", Some(1)),
    ("CEIL", Some(1)),
    ("CEILING", Some(1)),
    ("CHAR", None),
    ("CHAR_LENGTH", Some(1)),
    ("CHARACTER_LENGTH", Some(1)),
    ("CHARSET", Some(1)),
    ("COALESCE", None),
    ("COLLATION", Some(1)),
    ("COMPRESS", Some(1)),
    ("CONCAT", None),
    ("CONCAT_WS", None),
    ("CONNECTION_ID", Some(0)),
    ("CONV", Some(3)),
    ("CONVERT", Some(2)),
    ("CONVERT_TZ", Some(3)),
    ("COS", Some(1)),
    ("COT", Some(1)),
    ("COUNT", Some(1)),
    ("CRC32", Some(1)),
    ("CUME_DIST", Some(0)),
    ("CURDATE", Some(0)),
    ("CURRENT_DATE", Some(0)),
    ("CURRENT_TIME", None),
    ("CURRENT_TIMESTAMP", None),
    ("CURRENT_USER", Some(0)),
    ("CURTIME", None),
    ("DATABASE", Some(0)),
    ("DATE", Some(1)),
    ("DATE_ADD", Some(2)),
    ("DATE_FORMAT", Some(2)),
    ("DATE_SUB", Some(2)),
    ("DATEDIFF", Some(2)),
    ("DAY", Some(1)),
    ("DAYNAME", Some(1)),
    ("DAYOFMONTH", Some(1)),
    ("DAYOFWEEK", Some(1)),
    ("DAYOFYEAR", Some(1)),
    ("DEGREES", Some(1)),
    ("DENSE_RANK", Some(0)),
    ("ELT", None),
    ("EXP", Some(1)),
    ("EXPORT_SET", None),
    ("EXTRACT", Some(1)),
    ("FIELD", None),
    ("FIND_IN_SET", Some(2)),
    ("FIRST_VALUE", Some(1)),
    ("FLOOR", Some(1)),
    ("FORMAT", None),
    ("FOUND_ROWS", Some(0)),
    ("FROM_BASE64", Some(1)),
    ("FROM_DAYS", Some(1)),
    ("FROM_UNIXTIME", None),
    ("GET_LOCK", Some(2)),
    ("GREATEST", None),
    ("GROUP_CONCAT", None),
    ("HEX", Some(1)),
    ("HOUR", Some(1)),
    ("IF", Some(3)),
    ("IFNULL", Some(2)),
    ("INET_ATON", Some(1)),
    ("INET_NTOA", Some(1)),
    ("INSERT", Some(4)),
    ("INSTR", Some(2)),
    ("IS_FREE_LOCK", Some(1)),
    ("ISNULL", Some(1)),
    ("JSON_ARRAY", None),
    ("JSON_ARRAYAGG", Some(1)),
    ("JSON_CONTAINS", None),
    ("JSON_CONTAINS_PATH", None),
    ("JSON_EXTRACT", None),
    ("JSON_INSERT", None),
    ("JSON_KEYS", None),
    ("JSON_LENGTH", None),
    ("JSON_MERGE_PATCH", None),
    ("JSON_MERGE_PRESERVE", None),
    ("JSON_OBJECT", None),
    ("JSON_OBJECTAGG", Some(2)),
    ("JSON_QUOTE", Some(1)),
    ("JSON_REMOVE", None),
    ("JSON_REPLACE", None),
    ("JSON_SEARCH", None),
    ("JSON_SET", None),
    ("JSON_TABLE", None),
    ("JSON_TYPE", Some(1)),
    ("JSON_UNQUOTE", Some(1)),
    ("JSON_VALID", Some(1)),
    ("JSON_VALUE", None),
    ("LAG", None),
    ("LAST_DAY", Some(1)),
    ("LAST_INSERT_ID", None),
    ("LAST_VALUE", Some(1)),
    ("LCASE", Some(1)),
    ("LEAD", None),
    ("LEAST", None),
    ("LEFT", Some(2)),
    ("LENGTH", Some(1)),
    ("LN", Some(1)),
    ("LOCATE", None),
    ("LOG", None),
    ("LOG10", Some(1)),
    ("LOG2", Some(1)),
    ("LOWER", Some(1)),
    ("LPAD", Some(3)),
    ("LTRIM", Some(1)),
    ("MAKE_SET", None),
    ("MAKEDATE", Some(2)),
    ("MAKETIME", Some(3)),
    ("MAX", Some(1)),
    ("MD5", Some(1)),
    ("MICROSECOND", Some(1)),
    ("MID", Some(3)),
    ("MIN", Some(1)),
    ("MINUTE", Some(1)),
    ("MOD", Some(2)),
    ("MONTH", Some(1)),
    ("MONTHNAME", Some(1)),
    ("NOW", None),
    ("NTH_VALUE", Some(2)),
    ("NTILE", Some(1)),
    ("NULLIF", Some(2)),
    ("OCT", Some(1)),
    ("ORD", Some(1)),
    ("PERCENT_RANK", Some(0)),
    ("PERIOD_ADD", Some(2)),
    ("PERIOD_DIFF", Some(2)),
    ("PI", Some(0)),
    ("POSITION", Some(1)),
    ("POW", Some(2)),
    ("POWER", Some(2)),
    ("QUARTER", Some(1)),
    ("QUOTE", Some(1)),
    ("RADIANS", Some(1)),
    ("RAND", None),
    ("RANK", Some(0)),
    ("REGEXP_INSTR", None),
    ("REGEXP_LIKE", None),
    ("REGEXP_REPLACE", None),
    ("REGEXP_SUBSTR", None),
    ("RELEASE_LOCK", Some(1)),
    ("REPEAT", Some(2)),
    ("REPLACE", Some(3)),
    ("REVERSE", Some(1)),
    ("RIGHT", Some(2)),
    ("ROUND", None),
    ("ROW_COUNT", Some(0)),
    ("ROW_NUMBER", Some(0)),
    ("RPAD", Some(3)),
    ("RTRIM", Some(1)),
    ("SEC_TO_TIME", Some(1)),
    ("SECOND", Some(1)),
    ("SESSION_USER", Some(0)),
    ("SHA1", Some(1)),
    ("SHA2", Some(2)),
    ("SIGN", Some(1)),
    ("SIN", Some(1)),
    ("SLEEP", Some(1)),
    ("SOUNDEX", Some(1)),
    ("SPACE", Some(1)),
    ("SQRT", Some(1)),
    ("STD", Some(1)),
    ("STDDEV", Some(1)),
    ("STDDEV_POP", Some(1)),
    ("STDDEV_SAMP", Some(1)),
    ("STR_TO_DATE", Some(2)),
    ("STRCMP", Some(2)),
    ("SUBDATE", Some(2)),
    ("SUBSTR", None),
    ("SUBSTRING", None),
    ("SUBSTRING_INDEX", Some(3)),
    ("SUBTIME", Some(2)),
    ("SUM", Some(1)),
    ("SYSDATE", None),
    ("SYSTEM_USER", Some(0)),
    ("TAN", Some(1)),
    ("TIME", Some(1)),
    ("TIME_FORMAT", Some(2)),
    ("TIME_TO_SEC", Some(1)),
    ("TIMEDIFF", Some(2)),
    ("TIMESTAMP", None),
    ("TIMESTAMPADD", Some(3)),
    ("TIMESTAMPDIFF", Some(3)),
    ("TO_BASE64", Some(1)),
    ("TO_DAYS", Some(1)),
    ("TO_SECONDS", Some(1)),
    ("TRIM", Some(1)),
    ("TRUNCATE", Some(2)),
    ("UCASE", Some(1)),
    ("UNCOMPRESS", Some(1)),
    ("UNHEX", Some(1)),
    ("UNIX_TIMESTAMP", None),
    ("UPPER", Some(1)),
    ("USER", Some(0)),
    ("UTC_DATE", Some(0)),
    ("UTC_TIME", None),
    ("UTC_TIMESTAMP", None),
    ("UUID", Some(0)),
    ("UUID_SHORT", Some(0)),
    ("VALUES", Some(1)),
    ("VAR_POP", Some(1)),
    ("VAR_SAMP", Some(1)),
    ("VARIANCE", Some(1)),
    ("VERSION", Some(0)),
    ("WEEK", None),
    ("WEEKDAY", Some(1)),
    ("WEEKOFYEAR", Some(1)),
    ("YEAR", Some(1)),
    ("YEARWEEK", None),
];

/// Whether built-in `name` is an aggregate or a window function (its kind in the
/// completion).
pub fn builtin_kind(name: &str) -> database::RoutineKind {
    use database::RoutineKind as K;
    match name {
        "AVG" | "BIT_AND" | "BIT_OR" | "BIT_XOR" | "COUNT" | "GROUP_CONCAT" | "JSON_ARRAYAGG"
        | "JSON_OBJECTAGG" | "MAX" | "MIN" | "STD" | "STDDEV" | "STDDEV_POP" | "STDDEV_SAMP"
        | "SUM" | "VAR_POP" | "VAR_SAMP" | "VARIANCE" => K::Aggregate,
        "CUME_DIST" | "DENSE_RANK" | "FIRST_VALUE" | "LAG" | "LAST_VALUE" | "LEAD"
        | "NTH_VALUE" | "NTILE" | "PERCENT_RANK" | "RANK" | "ROW_NUMBER" => K::Window,
        _ => K::Function,
    }
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
