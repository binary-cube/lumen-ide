//! What Symfony's YAML files declare, read line by line (no YAML parser: only the shapes its
//! configuration and translations use): keys with their scalar value, nested keys joined
//! with `.` (`messages.yaml`'s `user: { greeting: Hi }` → `user.greeting`).

/// A key declared: its full (dotted) name, its value when it's a scalar on the same line,
/// where its name starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub name: String,
    pub value: Option<String>,
    pub offset: usize,
}

/// Every key of `text`, nested ones joined with `.` (under `section`, if given: only the
/// keys under that top-level key, without it).
pub fn keys(text: &str, section: Option<&str>) -> Vec<Key> {
    let mut out = Vec::new();
    // The keys open at each indentation (their depth and name).
    let mut path: Vec<(usize, String)> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let body = line.trim_end();
        let content = body.trim_start();
        if content.is_empty() || content.starts_with('#') || content.starts_with('-') {
            continue;
        }
        let depth = body.len() - content.len();
        let Some((raw, value)) = split_key(content) else { continue };
        while path.last().is_some_and(|(d, _)| *d >= depth) {
            path.pop();
        }
        let quoted = raw.starts_with(['\'', '"']);
        let name = raw.trim_matches(['\'', '"']).to_owned();
        let at = start + depth + usize::from(quoted);
        let value = value.map(|v| v.trim_matches(['\'', '"']).to_owned());
        let full: Vec<&str> =
            path.iter().map(|(_, n)| n.as_str()).chain(std::iter::once(name.as_str())).collect();
        let keep = match section {
            Some(section) => full.len() > 1 && full[0] == section,
            None => true,
        };
        if keep && value.is_some() {
            let skip = usize::from(section.is_some());
            out.push(Key { name: full[skip..].join("."), value, offset: at });
        }
        if value_is_empty(content) {
            path.push((depth, name));
        }
    }
    out
}

/// `key: value` → the key and the value (`None`: nothing after it, a block below); not a
/// key (a list item, text): `None`.
fn split_key(content: &str) -> Option<(&str, Option<&str>)> {
    let (key, rest) =
        if let Some(quote) = content.chars().next().filter(|c| *c == '\'' || *c == '"') {
            let end = content[1..].find(quote)? + 2;
            (&content[..end], content[end..].strip_prefix(':')?)
        } else {
            let colon = content.find(": ").or_else(|| content.strip_suffix(':').map(str::len))?;
            (&content[..colon], &content[colon + 1..])
        };
    let value = rest.trim();
    let value = value.split(" #").next().unwrap_or(value).trim();
    Some((key, (!value.is_empty() && !value.starts_with(['|', '>'])).then_some(value)))
}

/// The line opens a block (nothing after its key's `:`).
fn value_is_empty(content: &str) -> bool {
    split_key(content).is_some_and(|(_, v)| v.is_none())
}

#[cfg(test)]
#[path = "../tests/unit/yaml.rs"]
mod tests;
