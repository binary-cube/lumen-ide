//! The environments as a table to edit (plan 77l): each environment's variables (its own
//! and `$shared`'s) from the public and the private file together, a private one marked;
//! written back each into its file — what isn't a variable (`Security`,
//! `SSLConfiguration`, a top-level key that isn't an environment) kept as it was, a value
//! that was JSON (a number, an object) written as JSON again (on one line, as it's edited).
//! A file that isn't JSON is never written over: the user's text would be lost. An
//! environment left out is deleted from both files; one renamed keeps its settings.
//! [`valid_name`] says what a new name may be.

use serde_json::{Map, Value};

/// A variable as edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// A text as it is; anything else as its JSON.
    pub value: String,
    /// In the private file (never committed).
    pub private: bool,
    /// Its value is JSON (written back so), not a text.
    pub json: bool,
}

/// An environment's variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub rows: Vec<Row>,
    /// The name it has in the files (`None`: a new one): renamed, its settings
    /// (`Security`, `SSLConfiguration`) follow it.
    pub was: Option<String>,
}

/// Why `name` can't name an environment beside `others` (`None`: it can): empty, a
/// `$`-name (`$shared` is the files' own), a setting's name, or taken.
pub fn valid_name<'a>(name: &str, others: impl IntoIterator<Item = &'a str>) -> Option<NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Some(NameError::Empty);
    }
    if name.starts_with('$') || SETTINGS.contains(&name) {
        return Some(NameError::Reserved);
    }
    others.into_iter().any(|o| o == name).then_some(NameError::Taken)
}

/// Why a name can't be an environment's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    Reserved,
    Taken,
}

/// What isn't a variable of an environment (kept as it is).
const SETTINGS: &[&str] = &["Security", "SSLConfiguration"];

/// A file's object (`None`: no file, nothing in it); what isn't one, an error.
fn object(text: Option<&str>) -> Result<Map<String, Value>, String> {
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else { return Ok(Map::new()) };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(o)) => Ok(o),
        Ok(_) => Err("not a JSON object".to_owned()),
        Err(e) => Err(e.to_string()),
    }
}

/// The public and private files' texts (`None`: no such file) as tables: `$shared` first,
/// then the environments by name.
/// A file that isn't JSON reads as empty here (`write` refuses it).
pub fn tables(public: Option<&str>, private: Option<&str>) -> Vec<Table> {
    let (public, private) =
        (object(public).unwrap_or_default(), object(private).unwrap_or_default());
    let environment = |name: &String| {
        [&public, &private].iter().any(|f| matches!(f.get(name), Some(Value::Object(_))))
    };
    let mut names: Vec<String> =
        public.keys().chain(private.keys()).filter(|n| environment(n)).cloned().collect();
    names.sort();
    names.dedup();
    if let Some(at) = names.iter().position(|n| n == crate::env::SHARED) {
        let shared = names.remove(at);
        names.insert(0, shared);
    }
    names
        .into_iter()
        .map(|name| {
            let mut rows = Vec::new();
            for (file, is_private) in [(&public, false), (&private, true)] {
                let Some(Value::Object(vars)) = file.get(&name) else { continue };
                for (key, value) in vars {
                    if SETTINGS.contains(&key.as_str()) {
                        continue;
                    }
                    let (value, json) = match value {
                        Value::String(s) => (s.clone(), false),
                        other => (serde_json::to_string(other).unwrap_or_default(), true),
                    };
                    // A name in both files: both rows (the private one wins when sent).
                    rows.push(Row { name: key.clone(), value, private: is_private, json });
                }
            }
            Table { was: Some(name.clone()), name, rows }
        })
        .collect()
}

/// `tables` written into the files' texts as they were (`None`: none): the public one's, the
/// private one's (`None`: nothing private, and there was no such file). A file that isn't
/// JSON: an error, nothing written.
pub fn write(
    tables: &[Table],
    public: Option<&str>,
    private: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let old_public = object(public).map_err(|e| format!("{}: {e}", crate::env::PUBLIC))?;
    let old_private = object(private).map_err(|e| format!("{}: {e}", crate::env::PRIVATE))?;
    // What isn't an environment, as it was.
    let others = |old: &Map<String, Value>| -> Map<String, Value> {
        old.iter().filter(|(_, v)| !v.is_object()).map(|(k, v)| (k.clone(), v.clone())).collect()
    };
    let mut new_public = others(&old_public);
    let mut new_private = others(&old_private);
    for table in tables {
        let keep = |old: &Map<String, Value>, into: &mut Map<String, Value>, private: bool| {
            let mut vars = Map::new();
            let old_name = table.was.as_deref().unwrap_or(&table.name);
            // Its settings first, as they were (under its old name, if renamed).
            if let Some(Value::Object(was)) = old.get(old_name) {
                for key in SETTINGS {
                    if let Some(v) = was.get(*key) {
                        vars.insert((*key).to_owned(), v.clone());
                    }
                }
            }
            for row in
                table.rows.iter().filter(|r| r.private == private && !r.name.trim().is_empty())
            {
                let value = if row.json {
                    serde_json::from_str(&row.value)
                        .unwrap_or_else(|_| Value::String(row.value.clone()))
                } else {
                    Value::String(row.value.clone())
                };
                vars.insert(row.name.trim().to_owned(), value);
            }
            // An environment only private stays out of the public file (unless it was
            // there); a new one with nothing yet goes in it.
            let wanted = !vars.is_empty()
                || old.contains_key(old_name)
                || (!private && !table.rows.iter().any(|r| r.private));
            if wanted {
                into.insert(table.name.clone(), Value::Object(vars));
            }
        };
        keep(&old_public, &mut new_public, false);
        keep(&old_private, &mut new_private, true);
    }
    let text = |map: Map<String, Value>| {
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default() + "\n"
    };
    let private = (!new_private.is_empty() || private.is_some()).then(|| text(new_private));
    Ok((text(new_public), private))
}

#[cfg(test)]
#[path = "../tests/unit/env_edit.rs"]
mod tests;
