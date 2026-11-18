//! Rows sorted and filtered by the model's rules ([`Filter`], [`Order`]), for a source that
//! writes no SQL (a CSV file) and so does it itself — with the answers an SQL engine gives:
//! `NULL` first and never equal to anything, numbers by their value (`9` before `10`, also
//! when written as text), the rest by its text.

use std::cmp::Ordering;

use crate::{CompareOp, Filter, Order, Value};

/// A value as a number, when it is one (an integer, a float, or text spelling one, its
/// decimal point a `.` or a single `,`).
pub fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Text(t) | Value::Other(t) => {
            let t = t.trim();
            // `inf`, `nan` and the like are words, not numbers.
            if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit() || b"+-.,eE".contains(&b)) {
                return None;
            }
            t.parse().ok().or_else(|| {
                (t.matches(',').count() == 1 && !t.contains('.'))
                    .then(|| t.replace(',', ".").parse().ok())
                    .flatten()
            })
        }
        _ => None,
    }
}

/// How two values sort: `NULL` first, then numbers by value, booleans, then text.
pub fn compare(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.display().cmp(&b.display()),
        },
    }
}

/// How two rows sort by `order`: `a` and `b` hold the order's columns' values, in its order.
pub fn compare_rows(order: &[Order], a: &[Value], b: &[Value]) -> Ordering {
    for (at, column) in order.iter().enumerate() {
        let (x, y) = (a.get(at).unwrap_or(&Value::Null), b.get(at).unwrap_or(&Value::Null));
        let ordering = compare(x, y);
        let ordering = if column.descending { ordering.reverse() } else { ordering };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

/// Whether a row passes `filter`, its cells found by column name (`None`: no such column,
/// read as `NULL`). A comparison with `NULL` is false, as in SQL.
pub fn matches(filter: &Filter, cell: &dyn Fn(&str) -> Option<Value>) -> bool {
    let value = |column: &str| cell(column).unwrap_or(Value::Null);
    match filter {
        Filter::Compare { column, op, value: wanted } => {
            let have = value(column);
            if have == Value::Null || *wanted == Value::Null {
                return false;
            }
            let ordering = match (number(&have), number(wanted)) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                _ => compare(&have, wanted),
            };
            match op {
                CompareOp::Eq => ordering == Ordering::Equal,
                CompareOp::NotEq => ordering != Ordering::Equal,
                CompareOp::Less => ordering == Ordering::Less,
                CompareOp::LessEq => ordering != Ordering::Greater,
                CompareOp::Greater => ordering == Ordering::Greater,
                CompareOp::GreaterEq => ordering != Ordering::Less,
            }
        }
        Filter::IsNull(column) => value(column) == Value::Null,
        Filter::NotNull(column) => value(column) != Value::Null,
        Filter::Contains { column, text } => match value(column) {
            Value::Null => false,
            have => have.display().to_lowercase().contains(&text.to_lowercase()),
        },
        Filter::And(all) => all.iter().all(|f| matches(f, cell)),
        Filter::Or(any) => any.iter().any(|f| matches(f, cell)),
        Filter::Not(inner) => !matches(inner, cell),
    }
}

/// The columns `filter` names, each once, in the order met.
pub fn filter_columns(filter: &Filter) -> Vec<String> {
    fn walk(filter: &Filter, out: &mut Vec<String>) {
        let mut add = |c: &String| {
            if !out.contains(c) {
                out.push(c.clone());
            }
        };
        match filter {
            Filter::Compare { column, .. }
            | Filter::IsNull(column)
            | Filter::NotNull(column)
            | Filter::Contains { column, .. } => add(column),
            Filter::And(all) | Filter::Or(all) => all.iter().for_each(|f| walk(f, out)),
            Filter::Not(inner) => walk(inner, out),
        }
    }
    let mut out = Vec::new();
    walk(filter, &mut out);
    out
}

#[cfg(test)]
#[path = "../tests/unit/eval.rs"]
mod tests;
