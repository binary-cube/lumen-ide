//! The functions offered and their signatures: the source's routines and the ones every
//! engine (or one) has built in.

use crate::RoutineKind;
use crate::schema_index::SchemaIndex;
use crate::sql::Flavor;

use super::keywords::{Of, fits_flavor};
use super::{Completer, Suggestion, SuggestionKind};

impl<'a> Completer<'a> {
    pub(super) fn functions(&mut self, group: u8) {
        let flavor = self.req.flavor;
        let mut named: Vec<String> = Vec::new();
        if let Some(index) = self.index {
            for i in index.routine_candidates(&self.query, 120) {
                let r = &index.routines()[i];
                if r.kind == RoutineKind::Procedure
                    || named.iter().any(|n| n.eq_ignore_ascii_case(&r.name))
                {
                    continue;
                }
                named.push(r.name.clone());
                let signature = routine_signature(r, builtin_signature(&r.name, flavor));
                let shown =
                    if self.lower || !r.builtin { r.name.clone() } else { r.name.to_uppercase() };
                let mut item = Suggestion::new(shown.clone(), SuggestionKind::Function)
                    .insert(format!(
                        "{}()",
                        if r.builtin { shown.clone() } else { self.name(&r.name) }
                    ))
                    .detail(signature);
                item.cursor_back = 1;
                let key = r.name.to_lowercase();
                self.push_by(group, &key, item);
            }
        }
        for (name, signature, f) in BUILTINS {
            if !fits_flavor(*f, flavor) || named.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                continue;
            }
            let shown = if self.lower { name.to_lowercase() } else { name.to_uppercase() };
            let mut item = Suggestion::new(shown.clone(), SuggestionKind::Function)
                .insert(format!("{shown}()"))
                .detail(*signature);
            item.cursor_back = 1;
            self.push_by(group, &name.to_lowercase(), item);
        }
    }
}

/// The functions every engine (or one) has, with their signatures: offered without a
/// connection, and their signatures given where the engine lists only names.
const BUILTINS: &[(&str, &str, Of)] = &[
    ("count", "count(expression) → integer", Of::All),
    ("sum", "sum(expression)", Of::All),
    ("avg", "avg(expression)", Of::All),
    ("min", "min(expression)", Of::All),
    ("max", "max(expression)", Of::All),
    ("coalesce", "coalesce(value, …)", Of::All),
    ("nullif", "nullif(a, b)", Of::All),
    ("lower", "lower(text) → text", Of::All),
    ("upper", "upper(text) → text", Of::All),
    ("length", "length(text) → integer", Of::All),
    ("trim", "trim(text) → text", Of::All),
    ("replace", "replace(text, from, to) → text", Of::All),
    ("round", "round(number, digits)", Of::All),
    ("abs", "abs(number)", Of::All),
    ("substr", "substr(text, start, length) → text", Of::All),
    ("ifnull", "ifnull(value, other)", Of::Sqlite),
    ("iif", "iif(condition, then, else)", Of::Sqlite),
    ("group_concat", "group_concat(expression, separator) → text", Of::Sqlite),
    ("date", "date(time, modifier, …) → text", Of::Sqlite),
    ("datetime", "datetime(time, modifier, …) → text", Of::Sqlite),
    ("strftime", "strftime(format, time, modifier, …) → text", Of::Sqlite),
    ("julianday", "julianday(time, modifier, …) → real", Of::Sqlite),
    ("json_extract", "json_extract(json, path, …)", Of::Sqlite),
    ("printf", "printf(format, …) → text", Of::Sqlite),
    ("instr", "instr(text, part) → integer", Of::Sqlite),
    ("typeof", "typeof(value) → text", Of::Sqlite),
    ("random", "random() → integer", Of::Sqlite),
    ("now", "now() → timestamp with time zone", Of::Postgres),
    ("string_agg", "string_agg(expression, delimiter) → text", Of::Postgres),
    ("array_agg", "array_agg(expression) → array", Of::Postgres),
    ("json_agg", "json_agg(expression) → json", Of::Postgres),
    ("jsonb_build_object", "jsonb_build_object(key, value, …) → jsonb", Of::Postgres),
    ("to_char", "to_char(value, format) → text", Of::Postgres),
    ("to_date", "to_date(text, format) → date", Of::Postgres),
    ("date_trunc", "date_trunc(field, source) → timestamp", Of::Postgres),
    ("extract", "extract(field FROM source) → numeric", Of::Postgres),
    ("generate_series", "generate_series(start, stop, step) → setof", Of::Postgres),
    ("concat", "concat(value, …) → text", Of::NotSqlite),
    ("substring", "substring(text FROM start FOR length) → text", Of::NotSqlite),
    ("position", "position(part IN text) → integer", Of::NotSqlite),
    ("greatest", "greatest(value, …)", Of::NotSqlite),
    ("least", "least(value, …)", Of::NotSqlite),
    ("row_number", "row_number() over (…) → bigint", Of::All),
    ("rank", "rank() over (…) → bigint", Of::All),
    ("dense_rank", "dense_rank() over (…) → bigint", Of::All),
    ("lag", "lag(value, offset, default) over (…)", Of::All),
    ("lead", "lead(value, offset, default) over (…)", Of::All),
    ("first_value", "first_value(value) over (…)", Of::All),
    ("last_value", "last_value(value) over (…)", Of::All),
    ("ifnull", "ifnull(value, other)", Of::MySql),
    ("if", "if(condition, then, else)", Of::MySql),
    ("group_concat", "group_concat(expression SEPARATOR separator) → text", Of::MySql),
    ("concat_ws", "concat_ws(separator, value, …) → text", Of::NotSqlite),
    ("date_format", "date_format(date, format) → text", Of::MySql),
    ("str_to_date", "str_to_date(text, format) → datetime", Of::MySql),
    ("date_add", "date_add(date, INTERVAL amount unit) → datetime", Of::MySql),
    ("date_sub", "date_sub(date, INTERVAL amount unit) → datetime", Of::MySql),
    ("datediff", "datediff(end, start) → integer", Of::MySql),
    ("timestampdiff", "timestampdiff(unit, start, end) → integer", Of::MySql),
    ("json_extract", "json_extract(json, path, …)", Of::MySql),
    ("json_object", "json_object(key, value, …) → json", Of::MySql),
    ("left", "left(text, length) → text", Of::NotSqlite),
    ("right", "right(text, length) → text", Of::NotSqlite),
    ("lpad", "lpad(text, length, fill) → text", Of::NotSqlite),
    ("rpad", "rpad(text, length, fill) → text", Of::NotSqlite),
    ("split_part", "split_part(text, delimiter, n) → text", Of::Postgres),
    ("regexp_replace", "regexp_replace(text, pattern, replacement, flags) → text", Of::NotSqlite),
    ("age", "age(timestamp, timestamp) → interval", Of::Postgres),
    ("make_date", "make_date(year, month, day) → date", Of::Postgres),
    ("jsonb_set", "jsonb_set(target, path, value, create_missing) → jsonb", Of::Postgres),
];

/// The signature of `name` as the engine of `flavor` has it built in, if it's one of the
/// functions known here (`substr(text, start, length) → text`).
pub fn builtin_signature(name: &str, flavor: Flavor) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|(n, _, of)| n.eq_ignore_ascii_case(name) && fits_flavor(*of, flavor))
        .map(|(_, signature, _)| *signature)
}

/// The signature of the function `name` (its parameters, what it gives): the source's own
/// or its engine's from the schema's routines when they say their arguments, else one known
/// here, else what the routine's arity says (`f(…, …)`).
pub fn signature(name: &str, flavor: Flavor, index: Option<&SchemaIndex>) -> Option<String> {
    let routine =
        index.and_then(|index| index.routines().iter().find(|r| r.name.eq_ignore_ascii_case(name)));
    match routine {
        Some(r) => Some(routine_signature(r, builtin_signature(&r.name, flavor))),
        None => builtin_signature(name, flavor).map(str::to_owned),
    }
}

/// A routine's signature: its arguments and result as the engine says, else one known
/// here, else its arity's.
fn routine_signature(r: &crate::schema_index::IndexRoutine, known: Option<&str>) -> String {
    if !r.arguments.is_empty() || r.result.is_some() {
        let result = r.result.as_deref().map(|x| format!(" → {x}")).unwrap_or_default();
        format!("{}({}){result}", r.name, r.arguments)
    } else if let Some(sig) = known {
        sig.to_owned()
    } else {
        match r.arity {
            Some(0) => format!("{}()", r.name),
            Some(n) => format!("{}({})", r.name, vec!["…"; n as usize].join(", ")),
            None => format!("{}(…)", r.name),
        }
    }
}
