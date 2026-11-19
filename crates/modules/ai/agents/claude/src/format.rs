//! What Claude Code's session files say, read from their first and last bytes only (pure: the
//! bytes are read elsewhere, on a worker).
//!
//! A session is a JSONL file (`~/.claude/projects/<folder>/<id>.jsonl`), one record a line:
//! its rows carry `sessionId`, `cwd` and an ISO `timestamp`; its name is written again and
//! again as it goes — the one the user gave (`custom-title`), the one Claude gave
//! (`ai-title`) — so the file's end has the latest; its last request (`last-prompt`) too.
//! The files grow to tens of MB: never read whole.

use serde_json::Value;

/// How many bytes are read from a file's start and from its end.
pub const HEAD: usize = 64 * 1024;
pub const TAIL: usize = 64 * 1024;

/// The most working folders kept of a session.
pub const CWDS_MAX: usize = 16;

/// A title's longest (characters), longer ones cut with "…".
const TITLE_MAX: usize = 80;

/// The folder Claude Code keeps a project's sessions in, under `projects/`: the project's
/// path with every character that isn't a letter or a digit written `-`
/// (`/Users/a/api.shop.ro` → `-Users-a-api-shop-ro`). Several paths can give the same name:
/// the sessions' own `cwd` tells them apart.
pub fn folder_name(root: &str) -> String {
    root.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// What a session's first and last bytes say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub id: Option<String>,
    pub title: Option<String>,
    /// The folders it worked in (distinct, in the order met, at most [`CWDS_MAX`]): from
    /// its start and its end — a session resumed after its project moved works in the new
    /// folder from then on, its start still naming the old one.
    pub cwds: Vec<String>,
    /// When it started (seconds since the epoch).
    pub started: Option<i64>,
}

/// The records of `bytes`, whole lines only: a slice that starts or ends mid-line (`cut`
/// says which) skips that part.
fn records(bytes: &[u8], cut_start: bool, cut_end: bool) -> impl Iterator<Item = Value> + '_ {
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if cut_end {
        lines.pop();
    }
    let skip = usize::from(cut_start);
    lines.into_iter().skip(skip).filter_map(|line| serde_json::from_slice(line).ok())
}

/// A session's id as it may be used (named on a command line: `--resume <id>`, through
/// `cmd.exe` for an npm launcher on Windows): letters, digits, `-` and `_`, at most 64 —
/// anything else read from a file is refused.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The record Claude Code's `/rename` writes into session `id`'s file: the title the user
/// gave it (the newest one read wins), one JSON line.
pub fn title_record(id: &str, title: &str) -> String {
    let record = serde_json::json!({"type": "custom-title", "customTitle": title, "sessionId": id});
    format!("{record}\n")
}

/// `text` as a title: on one line, its spaces collapsed, cut to [`TITLE_MAX`] characters.
fn tidy(text: &str) -> Option<String> {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return None;
    }
    if one.chars().count() <= TITLE_MAX {
        return Some(one);
    }
    let cut: String = one.chars().take(TITLE_MAX - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// A user's message as typed (not a command's wrapper, nor a tool's result).
fn typed(record: &Value) -> Option<&str> {
    if record.get("type")?.as_str()? != "user" || record.get("isMeta") == Some(&Value::Bool(true)) {
        return None;
    }
    let content = record.get("message")?.get("content")?;
    let text = match content {
        Value::String(s) => s.as_str(),
        Value::Array(parts) => parts
            .iter()
            .find(|p| p.get("type").and_then(Value::as_str) == Some("text"))?
            .get("text")?
            .as_str()?,
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty() && !text.starts_with('<')).then_some(text)
}

/// An ISO 8601 moment in seconds since the epoch.
fn seconds(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|t| t.timestamp())
}

/// What the session says, from its `head` (the file's first bytes: the whole file when it's
/// short) and its `tail` (its last bytes; empty when the head is the whole file). `whole`:
/// the head ends where the file does.
pub fn facts(head: &[u8], tail: &[u8], whole: bool) -> Facts {
    let mut facts = Facts::default();
    let mut first_typed = None;
    // The names, the latest of each kind: the start's, then the end's over them.
    let (mut custom, mut ai, mut last) = (None, None, None);
    let mut cwds: Vec<String> = Vec::new();
    let mut take = |record: &Value| {
        if let Some(cwd) = record.get("cwd").and_then(Value::as_str)
            && cwds.len() < CWDS_MAX
            && !cwds.iter().any(|c| c == cwd)
        {
            cwds.push(cwd.to_owned());
        }
        let text = |key: &str| record.get(key).and_then(Value::as_str).and_then(tidy);
        match record.get("type").and_then(Value::as_str) {
            Some("custom-title") => custom = text("customTitle").or(custom.take()),
            Some("ai-title") => ai = text("aiTitle").or(ai.take()),
            Some("last-prompt") => last = text("lastPrompt").or(last.take()),
            _ => {}
        }
    };
    // The start read once: its facts and its names.
    for record in records(head, false, !whole) {
        let field = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_owned);
        facts.id = facts.id.or_else(|| field("sessionId").filter(|id| valid_id(id)));
        if facts.started.is_none() {
            facts.started = record.get("timestamp").and_then(Value::as_str).and_then(seconds);
        }
        if first_typed.is_none() {
            first_typed = typed(&record).map(str::to_owned);
        }
        take(&record);
    }
    if !whole {
        records(tail, true, false).for_each(|r| take(&r));
    }
    facts.cwds = cwds;
    facts.title = custom.or(ai).or(last).or_else(|| first_typed.as_deref().and_then(tidy));
    facts
}

#[cfg(test)]
#[path = "../tests/unit/format.rs"]
mod tests;
