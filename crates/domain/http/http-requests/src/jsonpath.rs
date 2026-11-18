//! JSONPath, as chained requests use it (`{{login.response.body.$.data.items[0].id}}`):
//! `$` the root; `.name` and `['name']` a member; `[0]`, `[-1]` an element; `[*]` / `.*`
//! every one; `..name` a member at any depth; `[?(@.x == 1)]` the elements whose member
//! compares (`==`, `!=`, `<`, `<=`, `>`, `>=` with a number, a string in quotes, `true`,
//! `false`, `null`; `[?(@.x)]`: it's there). What a path selects, in document order.

use serde_json::Value;

/// A step of a path.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Member(String),
    Index(i64),
    All,
    /// `..name`: the member at any depth.
    Descend(String),
    Filter(Filter),
}

#[derive(Debug, Clone, PartialEq)]
struct Filter {
    /// The member compared (`@.price` → `["price"]`; `@` alone → empty).
    member: Vec<String>,
    /// `None`: only that it's there.
    compare: Option<(Op, Value)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Why a path can't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathError(pub String);

/// `path` read into its steps (`$` first, optional).
fn steps(path: &str) -> Result<Vec<Step>, PathError> {
    let path = path.trim();
    let mut rest = path.strip_prefix('$').unwrap_or(path);
    let mut out = Vec::new();
    let bad = |why: &str| PathError(format!("{why}: {path}"));
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("..") {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let name = &after[..end];
            if name.is_empty() {
                return Err(bad("a name after `..`"));
            }
            out.push(Step::Descend(name.to_owned()));
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let name = &after[..end];
            out.push(if name == "*" { Step::All } else { Step::Member(name.to_owned()) });
            if name.is_empty() {
                return Err(bad("a name after `.`"));
            }
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('[') {
            let close = closing(after).ok_or_else(|| bad("a `]`"))?;
            out.push(bracket(after[..close].trim()).ok_or_else(|| bad("a step"))?);
            rest = &after[close + 1..];
        } else {
            // `a.b` without the `$.` (a name first).
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            out.push(Step::Member(rest[..end].to_owned()));
            rest = &rest[end..];
        }
    }
    Ok(out)
}

/// Where the `]` closing a bracket opened before `text` is (quotes and parentheses skipped).
fn closing(text: &str) -> Option<usize> {
    let mut quote = None;
    let mut depth = 0i32;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ']') if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn unquote(text: &str) -> Option<&str> {
    let text = text.trim();
    for q in ['\'', '"'] {
        if let Some(inner) = text.strip_prefix(q).and_then(|t| t.strip_suffix(q)) {
            return Some(inner);
        }
    }
    None
}

/// What's between `[` and `]`.
fn bracket(inner: &str) -> Option<Step> {
    if inner == "*" {
        return Some(Step::All);
    }
    if let Some(name) = unquote(inner) {
        return Some(Step::Member(name.to_owned()));
    }
    if let Ok(n) = inner.parse::<i64>() {
        return Some(Step::Index(n));
    }
    let expression = inner.strip_prefix('?')?.trim().strip_prefix('(')?.strip_suffix(')')?.trim();
    filter(expression).map(Step::Filter)
}

fn filter(expression: &str) -> Option<Filter> {
    const OPS: [(&str, Op); 6] = [
        ("==", Op::Eq),
        ("!=", Op::Ne),
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("<", Op::Lt),
        (">", Op::Gt),
    ];
    let (left, compare) = match OPS
        .iter()
        .find_map(|(text, op)| expression.split_once(text).map(|(l, r)| (l, *op, r)))
    {
        Some((left, op, right)) => (left.trim(), Some((op, literal(right.trim())?))),
        None => (expression, None),
    };
    let member = left.strip_prefix('@')?;
    let member = member.split('.').filter(|m| !m.is_empty()).map(str::to_owned).collect();
    Some(Filter { member, compare })
}

fn literal(text: &str) -> Option<Value> {
    if let Some(s) = unquote(text) {
        return Some(Value::String(s.to_owned()));
    }
    serde_json::from_str(text).ok()
}

/// What `path` selects in `value` (none: an empty list).
pub fn select<'a>(value: &'a Value, path: &str) -> Result<Vec<&'a Value>, PathError> {
    let mut current = vec![value];
    for step in steps(path)? {
        let mut next = Vec::new();
        for node in current {
            apply(&step, node, &mut next);
        }
        current = next;
    }
    Ok(current)
}

fn apply<'a>(step: &Step, node: &'a Value, out: &mut Vec<&'a Value>) {
    match step {
        Step::Member(name) => out.extend(node.get(name)),
        Step::Index(i) => {
            if let Value::Array(items) = node {
                let at = if *i < 0 { items.len() as i64 + i } else { *i };
                out.extend(usize::try_from(at).ok().and_then(|at| items.get(at)));
            }
        }
        Step::All => match node {
            Value::Array(items) => out.extend(items),
            Value::Object(members) => out.extend(members.values()),
            _ => {}
        },
        Step::Descend(name) => descend(node, name, out),
        Step::Filter(filter) => {
            let items: Box<dyn Iterator<Item = &Value>> = match node {
                Value::Array(items) => Box::new(items.iter()),
                Value::Object(members) => Box::new(members.values()),
                _ => Box::new(std::iter::empty()),
            };
            out.extend(items.filter(|item| matches(filter, item)));
        }
    }
}

fn descend<'a>(node: &'a Value, name: &str, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(members) => {
            if let Some(found) = members.get(name) {
                out.push(found);
            }
            for child in members.values() {
                descend(child, name, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                descend(child, name, out);
            }
        }
        _ => {}
    }
}

fn matches(filter: &Filter, item: &Value) -> bool {
    let mut at = Some(item);
    for member in &filter.member {
        at = at.and_then(|v| v.get(member));
    }
    let Some(value) = at else { return false };
    let Some((op, wanted)) = &filter.compare else { return true };
    match (value, wanted) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap_or(f64::NAN), b.as_f64().unwrap_or(f64::NAN));
            match op {
                Op::Eq => a == b,
                Op::Ne => a != b,
                Op::Lt => a < b,
                Op::Le => a <= b,
                Op::Gt => a > b,
                Op::Ge => a >= b,
            }
        }
        (Value::String(a), Value::String(b)) => match op {
            Op::Eq => a == b,
            Op::Ne => a != b,
            Op::Lt => a < b,
            Op::Le => a <= b,
            Op::Gt => a > b,
            Op::Ge => a >= b,
        },
        (a, b) => match op {
            Op::Eq => a == b,
            Op::Ne => a != b,
            _ => false,
        },
    }
}

/// A selected value as it goes into a request: a string as it is (no quotes), the rest as
/// JSON (`42`, `true`, `{"a":1}`).
pub fn as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The keys (an object's names, an array's indexes) at `path` in `value`, for completion: a
/// member as `.name` (`['a b']` when it isn't a plain name), an element as `[0]`, with a
/// short view of its value.
pub fn children(value: &Value, path: &str) -> Vec<(String, String)> {
    let Ok(found) = select(value, path) else { return Vec::new() };
    let Some(node) = found.first() else { return Vec::new() };
    let preview = |v: &Value| {
        let text = match v {
            Value::Object(m) => format!("{{…}} {} keys", m.len()),
            Value::Array(a) => format!("[…] {} items", a.len()),
            other => as_text(other),
        };
        text.chars().take(60).collect::<String>()
    };
    match node {
        Value::Object(members) => members
            .iter()
            .map(|(name, v)| {
                let plain =
                    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
                let step = if plain { format!(".{name}") } else { format!("['{name}']") };
                (step, preview(v))
            })
            .collect(),
        Value::Array(items) => {
            items.iter().take(50).enumerate().map(|(i, v)| (format!("[{i}]"), preview(v))).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/jsonpath.rs"]
mod tests;
