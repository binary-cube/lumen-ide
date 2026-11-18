//! Completion of a response's parts in a `.http` file, from the responses this module keeps:
//! after `{{login.response.` its `body`, `headers`, `status`; after `body.` the keys and the
//! indexes at the path typed, in the newest response of `login` (each with its value
//! beside); after `headers.` the headers it came with. The rest of a `.http` file's
//! completion is the language's.

use std::ops::Range;
use std::sync::Arc;

use code_intelligence::{
    AssistContext, CompletionItem, CompletionKind, DocumentAssist, FrameworkCompletion,
};

use crate::runs::Runs;

/// What's typed inside `{{` before `offset` (from its start): its start and the text.
fn typed(text: &str, offset: usize) -> Option<(usize, &str)> {
    let line_start = text[..offset].rfind('\n').map_or(0, |n| n + 1);
    let before = &text[line_start..offset];
    let open = before.rfind("{{")?;
    if before[open..].contains("}}") {
        return None;
    }
    let start = line_start + open + 2;
    let typed = &text[start..offset];
    let trimmed = typed.trim_start();
    Some((start + (typed.len() - trimmed.len()), trimmed))
}

/// A request's newest response, as completion reads it: its headers' names, its body as
/// JSON (when it is).
pub type Answer = Option<(Vec<String>, Option<Arc<serde_json::Value>>)>;

/// An offer: its label, what it inserts, a word about it.
pub type Offer = (String, String, String);

/// What a response's completion offers at `offset`: the text it replaces, and the items
/// (label, insert, detail) — `answer(request)` gives the newest response of a request (its
/// headers' names, its body as JSON).
pub fn complete(
    text: &str,
    offset: usize,
    answer: &dyn Fn(&str) -> Answer,
) -> Option<(Range<usize>, Vec<Offer>)> {
    let (start, typed) = typed(text, offset)?;
    let (request, rest) = typed.split_once(".response")?;
    let after = start + request.len() + ".response".len();
    let rest_trimmed = rest.strip_prefix('.').unwrap_or(rest);
    // `login.response` / `login.response.` / `login.response.bo`: its parts.
    if !rest_trimmed.contains('.') && !["body", "headers"].contains(&rest_trimmed)
        || rest_trimmed.is_empty()
    {
        let parts = [
            ("body", ".body.$", "the body (JSONPath after it)"),
            ("headers", ".headers.", "a header"),
            ("status", ".status", "the status code"),
        ];
        let items = parts
            .iter()
            .map(|(l, i, d)| ((*l).to_owned(), (*i).to_owned(), (*d).to_owned()))
            .collect();
        return Some((after..offset, items));
    }
    let (headers, body) = answer(request)?;
    if let Some(partial) = rest_trimmed.strip_prefix("headers.") {
        let from = offset - partial.len();
        let items = headers.into_iter().map(|h| (h.clone(), h, String::new())).collect();
        return Some((from..offset, items));
    }
    let path = rest_trimmed.strip_prefix("body.").or_else(|| rest_trimmed.strip_prefix("body"))?;
    let body = body?;
    // The path up to its last step, and the step being typed.
    // `$` alone: what comes after it (nothing typed of the next step yet).
    let boundary = path.rfind(['.', '[']).unwrap_or(if path == "$" { 1 } else { 0 });
    let (parent, partial) = path.split_at(boundary);
    let parent = if parent.is_empty() || parent == "$" { "$" } else { parent };
    let from = offset - partial.len();
    let items = http_requests::jsonpath::children(&body, parent)
        .into_iter()
        .map(|(step, preview)| (step.trim_start_matches('.').to_owned(), step, preview))
        .collect();
    Some((from..offset, items))
}

#[derive(Debug, Default)]
pub struct ResponseCompletion;

impl DocumentAssist for ResponseCompletion {
    fn id(&self) -> &'static str {
        "http-responses"
    }

    fn languages(&self) -> &'static [&'static str] {
        &["http"]
    }

    fn triggers(&self) -> &'static str {
        ".["
    }

    fn complete(&self, at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        let runs = at.project.and_then(Runs::held)?;
        let answer = |request: &str| {
            let outcome = runs.latest(at.path, request)?;
            let headers = outcome.response.headers.iter().map(|h| h.name.clone()).collect();
            // As read on its run's worker: nothing read nor parsed here, on each key.
            Some((headers, outcome.json.clone()))
        };
        let (replace, items) = complete(at.text, at.offset, &answer)?;
        let items = items
            .into_iter()
            .map(|(label, insert, detail)| CompletionItem {
                label,
                kind: CompletionKind::Property,
                detail: (!detail.is_empty()).then_some(detail),
                insert,
                ..CompletionItem::default()
            })
            .collect::<Vec<_>>();
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }

    fn generation(&self, project: Option<kernel::ProjectId>) -> u64 {
        project.and_then(Runs::held).map_or(0, |r| r.generation())
    }
}

#[cfg(test)]
#[path = "../tests/unit/assist.rs"]
mod tests;
