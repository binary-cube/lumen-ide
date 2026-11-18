//! What a `{{` typed in the editor's fields offers (plan 77l): the environment's variables
//! (an object's members by their path, their value beside — a private one hidden), the
//! file's own (`@name = …`), the dynamic ones (`$uuid`, `$random.*`, `$env.NAME`…), the
//! file's named requests, and — once `name.response` is typed — their newest response's
//! parts and its JSON body's paths (JSONPath, each value beside), as the language's
//! completion does in the file. The environments are read on a worker; the rest is in
//! memory.

use std::sync::Arc;

use http_requests::env::Environments;

/// A suggestion: what it shows, what it puts after `{{` (the name, closed by `}}`), a word
/// about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub insert: String,
    pub detail: String,
}

/// The dynamic variables offered (those of the `.http` format).
const DYNAMIC: &[(&str, &str)] = &[
    ("$uuid", "UUID v4"),
    ("$timestamp", "Unix time"),
    ("$isoTimestamp", "ISO-8601"),
    ("$randomInt", "0–1000"),
    ("$random.integer(0, 100)", "integer"),
    ("$random.float(0, 1)", "float"),
    ("$random.alphabetic(8)", "letters"),
    ("$random.alphanumeric(8)", "letters, digits"),
    ("$random.hexadecimal(8)", "hex"),
    ("$random.email", "e-mail"),
    ("$random.name.fullName", "Faker"),
    ("$random.name.firstName", "Faker"),
    ("$random.internet.emailAddress", "Faker"),
    ("$random.address.city", "Faker"),
    ("$random.phoneNumber.cellPhone", "Faker"),
    ("$random.lorem.sentence", "Faker"),
    ("$auth.token(\"\")", "OAuth 2.0"),
    ("$auth.idToken(\"\")", "OAuth 2.0"),
    ("$env.", "system environment"),
    ("$projectRoot", "folder"),
    ("$historyFolder", "folder"),
];

/// What's typed after the last `{{` before `cursor` on its line of `text` (none open:
/// `None`), and where it starts. Only the caret's line is looked at (a variable never spans
/// lines; a body of megabytes isn't scanned at each key).
pub fn typed(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = text.get(..cursor)?;
    let line = before.rfind('\n').map_or(0, |at| at + 1);
    let open = line + before[line..].rfind("{{")?;
    if before[open..].contains("}}") {
        return None;
    }
    Some((open + 2, &before[open + 2..]))
}

/// What a `.http` file itself offers after `{{`: its variables (`@name = value`) and its
/// named requests. Read once per text of the file (not at each key typed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileNames {
    variables: Vec<(String, String)>,
    requests: Vec<String>,
}

impl FileNames {
    pub fn of(file_text: &str) -> Self {
        let parsed = http_requests::parse(file_text);
        Self {
            variables: parsed.variables.into_iter().map(|v| (v.name, v.value)).collect(),
            requests: parsed.requests.into_iter().filter_map(|r| r.name).collect(),
        }
    }
}

/// The suggestions for `prefix` (what's typed after `{{`).
pub fn suggestions(
    prefix: &str,
    environments: &Environments,
    environment: Option<&str>,
    file: &FileNames,
    responses: &dyn Fn(&str) -> crate::assist::Answer,
) -> Vec<Suggestion> {
    let prefix_trim = prefix.trim_start();
    // A response's parts and its body's paths.
    if prefix_trim.contains(".response") {
        let line = format!("{{{{{prefix_trim}");
        let Some((_, offers)) = crate::assist::complete(&line, line.len(), responses) else {
            return Vec::new();
        };
        let start = prefix_trim.rfind(['.', '[']).map_or(0, |at| at + 1);
        let head = &prefix_trim[..start.min(prefix_trim.len())];
        return offers
            .into_iter()
            .map(|(label, insert, detail)| {
                let insert = if insert.starts_with('[') || insert.starts_with('.') {
                    format!("{}{insert}", head.trim_end_matches(['.', '[']))
                } else {
                    format!("{head}{insert}")
                };
                Suggestion { label, insert, detail }
            })
            .collect();
    }
    let lower = prefix_trim.to_lowercase();
    let matches = |name: &str| lower.is_empty() || name.to_lowercase().contains(&lower);
    let mut out = Vec::new();
    for (name, variable) in environments.variables(environment) {
        if matches(name) {
            let detail = if variable.source.is_secret() {
                "••••••".to_owned()
            } else {
                short(&variable.value)
            };
            out.push(Suggestion { label: name.to_owned(), insert: name.to_owned(), detail });
        }
    }
    for (name, value) in &file.variables {
        if matches(name) && !out.iter().any(|s| s.label == *name) {
            out.push(Suggestion {
                label: name.clone(),
                insert: name.clone(),
                detail: short(value),
            });
        }
    }
    for request in &file.requests {
        let reference = format!("{request}.response.body.$");
        if matches(&reference) {
            out.push(Suggestion {
                label: reference.clone(),
                insert: reference,
                detail: i18n::t("http.suggest_response"),
            });
        }
    }
    for (name, detail) in DYNAMIC {
        if matches(name) {
            out.push(Suggestion {
                label: (*name).to_owned(),
                insert: (*name).to_owned(),
                detail: (*detail).to_owned(),
            });
        }
    }
    out.truncate(60);
    out
}

/// A value shortened to be shown beside its name.
fn short(value: &str) -> String {
    let line = value.lines().next().unwrap_or_default();
    let mut out: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        out.push('…');
    }
    out
}

/// `text` with what's typed after `{{` (from `start` to `cursor`) replaced by `insert` and
/// closed (`}}`, unless it is already): the new text, where the caret goes. A response's
/// path still being written (`….body.$`) is left open.
pub fn apply(text: &str, start: usize, cursor: usize, insert: &str) -> (String, usize) {
    let after = &text[cursor..];
    let open_path = insert.ends_with(".$") || insert.ends_with('.');
    let close = if after.starts_with("}}") || open_path { "" } else { "}}" };
    let new = format!("{}{insert}{close}{after}", &text[..start]);
    let caret = start + insert.len() + close.len();
    (new, caret)
}

/// The environments read for completion, shared by the editor's fields.
pub type Read = Arc<Environments>;

#[cfg(test)]
#[path = "../../tests/unit/editor/suggest.rs"]
mod tests;
