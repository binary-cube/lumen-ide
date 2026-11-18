//! PostgreSQL's SQL, for every server from 9.2 to the newest: one dialect that knows the
//! server's version (`server_version_num`, read when a session connects) and never writes
//! what that server lacks — an identity column on a 9.x server is a `SERIAL`, a generated
//! column before 12 is refused, a virtual one before 18. [`Feature`] says what each version
//! has, so a driver's catalog queries and its UI ask the same rules. Pure: no connection.

use crate::dialect::{
    Dialect, check_type, quote_text, standard_column_definition, standard_literal,
};
use crate::sql::Flavor;
use crate::{ColumnDef, DbError, KeyKind, Value};

/// The oldest server the driver is made for (9.2: `pg_stat_activity.pid`, `SMALLSERIAL`,
/// `json`); an older one may work, its catalog read the 9.2 way.
pub const OLDEST: u32 = 90200;

/// What a PostgreSQL version may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `jsonb` and `to_regclass` (9.4).
    Jsonb,
    /// `INSERT … ON CONFLICT` and `CREATE INDEX … IF NOT EXISTS` (9.5).
    OnConflict,
    /// `ALTER TABLE … ADD COLUMN IF NOT EXISTS`, `current_setting(…, true)` (9.6).
    AddColumnIfNotExists,
    /// Identity columns (`pg_attribute.attidentity`), partitioned tables (`relkind` `p`,
    /// `relispartition`), the `pg_sequence` catalog (10).
    Identity,
    /// Procedures (`pg_proc.prokind`), indexes with `INCLUDE` columns (`indnkeyatts`) (11).
    Procedures,
    /// Generated columns (`attgenerated`), no more `WITH OIDS` (`relhasoids` gone) (12).
    Generated,
    /// `reltuples` of a table never analyzed is -1 (0 before) (14).
    UnknownReltuples,
    /// Virtual generated columns (18).
    VirtualGenerated,
}

impl Feature {
    /// The first `server_version_num` having it.
    pub fn since(self) -> u32 {
        match self {
            Self::Jsonb => 90400,
            Self::OnConflict => 90500,
            Self::AddColumnIfNotExists => 90600,
            Self::Identity => 100000,
            Self::Procedures => 110000,
            Self::Generated => 120000,
            Self::UnknownReltuples => 140000,
            Self::VirtualGenerated => 180000,
        }
    }
}

/// Whether a server of `version` (`server_version_num`) has `feature`.
pub fn supports(version: u32, feature: Feature) -> bool {
    version >= feature.since()
}

/// `server_version_num`'s text (`90624`, `170002`) as a number; `None` if it isn't one.
pub fn parse_version_num(text: &str) -> Option<u32> {
    text.trim().parse().ok().filter(|v| *v >= 70000)
}

/// A version shown as people write it: `9.6.24`, `17.2`.
pub fn version_text(version: u32) -> String {
    if version >= 100000 {
        format!("{}.{}", version / 10000, version % 10000)
    } else {
        format!("{}.{}.{}", version / 10000, version / 100 % 100, version % 100)
    }
}

/// How a server of `version` estimates a table's rows from `pg_class`: `reltuples` and
/// `relpages` as read; `None` when it doesn't know (never analyzed).
pub fn estimated_rows(version: u32, reltuples: f64, relpages: i64) -> Option<u64> {
    if reltuples < 0.0 {
        return None;
    }
    // Before 14, an empty `relpages` with 0 tuples meant "never counted" too.
    if !supports(version, Feature::UnknownReltuples) && reltuples == 0.0 && relpages == 0 {
        return None;
    }
    Some(reltuples.round() as u64)
}

/// PostgreSQL's dialect, for a server of `version` (`None`: not connected yet — what every
/// supported version accepts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Postgres {
    pub version: Option<u32>,
}

impl Postgres {
    pub fn new(version: u32) -> Self {
        Self { version: Some(version) }
    }

    /// Whether the server has `feature` (unknown: only what the oldest has).
    pub fn has(&self, feature: Feature) -> bool {
        supports(self.version.unwrap_or(OLDEST), feature)
    }
}

/// `text` as a literal any server reads the same, whatever its
/// `standard_conforming_strings`: with a backslash, an escape string (`E'…'`, the
/// backslashes doubled), else a plain one.
pub fn pg_text(text: &str) -> Result<String, DbError> {
    if text.contains('\\') {
        let quoted = quote_text(&text.replace('\\', "\\\\"))?;
        Ok(format!("E{quoted}"))
    } else {
        quote_text(text)
    }
}

/// The `SERIAL` type numbering a column declared `declared` (an older server's identity).
fn serial_of(declared: &str) -> &'static str {
    match declared.trim().to_ascii_lowercase().as_str() {
        "bigint" | "int8" | "bigserial" => "BIGSERIAL",
        "smallint" | "int2" | "smallserial" => "SMALLSERIAL",
        _ => "SERIAL",
    }
}

impl Dialect for Postgres {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn flavor(&self) -> Flavor {
        Flavor::Postgres { version: self.version }
    }

    fn literal(&self, value: &Value) -> Result<String, DbError> {
        match value {
            Value::Text(t) | Value::Other(t) => pg_text(t),
            Value::Json(t) => pg_text(t),
            Value::Float(f) if f.is_nan() => Ok("'NaN'::float8".to_owned()),
            Value::Float(f) if f.is_infinite() => {
                Ok(if *f > 0.0 { "'Infinity'::float8" } else { "'-Infinity'::float8" }.to_owned())
            }
            // Hex through `decode`: read the same by every server and setting.
            Value::Bytes(b) => {
                let hex: String = b.iter().map(|b| format!("{b:02x}")).collect();
                Ok(format!("decode('{hex}', 'hex')"))
            }
            v => standard_literal(v),
        }
    }

    fn type_name(&self, declared: &str) -> Result<String, DbError> {
        check_type(declared)
    }

    fn type_suggestions(&self) -> &'static [&'static str] {
        &[
            "integer",
            "bigint",
            "smallint",
            "serial",
            "bigserial",
            "varchar(255)",
            "text",
            "boolean",
            "numeric(10, 2)",
            "real",
            "double precision",
            "date",
            "time",
            "timestamp",
            "timestamptz",
            "interval",
            "json",
            "jsonb",
            "uuid",
            "bytea",
            "inet",
            "text[]",
        ]
    }

    fn collations(&self) -> &'static [&'static str] {
        &["C", "POSIX", "default", "und-x-icu"]
    }

    fn auto_increment(&self) -> &'static str {
        "GENERATED BY DEFAULT AS IDENTITY"
    }

    fn column_definition(&self, column: &ColumnDef) -> Result<String, DbError> {
        if let Some(generated) = &column.generated {
            if !self.has(Feature::Generated) {
                return Err(DbError::Unsupported(
                    "generated columns (PostgreSQL 12 or newer)".to_owned(),
                ));
            }
            if !generated.stored && !self.has(Feature::VirtualGenerated) {
                return Err(DbError::Unsupported(
                    "virtual generated columns (PostgreSQL 18 or newer; a stored one works)"
                        .to_owned(),
                ));
            }
        }
        // Before identity columns: a `SERIAL` (its own sequence), as 9.x numbered them.
        if column.auto_increment && !self.has(Feature::Identity) {
            let mut serial = column.clone();
            serial.declared_type = serial_of(&column.declared_type).to_owned();
            serial.auto_increment = false;
            return standard_column_definition(self, &serial);
        }
        standard_column_definition(self, column)
    }

    fn returning(&self, key: &KeyKind, _row_id: Option<&str>) -> Result<Option<String>, DbError> {
        Ok(match key {
            KeyKind::Primary(columns) => {
                let list = self.name_list(columns)?;
                Some(format!(" RETURNING {}", &list[1..list.len() - 1]))
            }
            _ => None,
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/postgres.rs"]
mod tests;
