//! Requests chained by their responses: a variable naming another request's response
//! (`{{login.response.body.$.token}}`, `{{login.response.headers.Location}}`,
//! `{{login.response.status}}`), what a request depends on, the order a chain runs in (each
//! request after the ones it uses), a loop said.

use crate::model::{BodyPart, HttpFile, Request};

/// What of a response a variable takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponsePart {
    /// Its body at a path (JSONPath; XPath for XML when it starts with `/`; `*`: the whole
    /// body).
    Body(String),
    /// Its header (any case).
    Header(String),
    Status,
}

/// A variable naming a request's response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseRef {
    pub request: String,
    pub part: ResponsePart,
}

impl ResponseRef {
    /// `name` read as one (`None`: not a response's).
    pub fn parse(name: &str) -> Option<Self> {
        let (request, rest) = name.split_once(".response")?;
        if request.is_empty() || request.contains(char::is_whitespace) {
            return None;
        }
        let rest = rest.strip_prefix('.').unwrap_or(rest);
        let part = if let Some(path) = rest.strip_prefix("body") {
            let path = path.strip_prefix('.').unwrap_or(path).trim();
            ResponsePart::Body(if path.is_empty() { "*".to_owned() } else { path.to_owned() })
        } else if let Some(header) = rest.strip_prefix("headers.") {
            ResponsePart::Header(header.trim().to_owned())
        } else if rest == "status" {
            ResponsePart::Status
        } else {
            return None;
        };
        Some(Self { request: request.to_owned(), part })
    }
}

/// The responses `request` uses: each `{{…}}` naming one, as written and read, wherever
/// it is (the URL, a header's name or value, the body, a file's path), in the order met.
fn response_refs(request: &Request) -> impl Iterator<Item = (&str, ResponseRef)> {
    let headers = request.headers.iter().flat_map(|h| [h.name.as_str(), h.value.as_str()]);
    let body = request.body.iter().map(|part| match part {
        BodyPart::Text(text) | BodyPart::File(text) => text.as_str(),
    });
    std::iter::once(request.url.as_str())
        .chain(headers)
        .chain(body)
        .flat_map(crate::vars::references)
        .filter_map(|(_, name)| Some((name, ResponseRef::parse(name)?)))
}

/// The requests `request` uses the responses of (each once, in the order met).
pub fn dependencies(request: &Request) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, reference) in response_refs(request) {
        if !out.contains(&reference.request) {
            out.push(reference.request);
        }
    }
    out
}

/// Why a chain can't run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    /// A request uses one the file doesn't name.
    Unknown(String),
    /// Requests using each other (the loop, in order).
    Loop(Vec<String>),
}

/// The requests to run for request `at` (an index of `file.requests`), in order: each one
/// it uses (theirs first), then itself; `fresh(name)`: whether a response of `name` is kept
/// already (then it isn't run again).
pub fn order(
    file: &HttpFile,
    at: usize,
    fresh: &dyn Fn(&str) -> bool,
) -> Result<Vec<usize>, ChainError> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    visit(file, at, fresh, &mut path, &mut out)?;
    Ok(out)
}

/// Every request of `file` in an order where each runs after the ones it uses (the order
/// written, but for that): what a graph's Play runs.
pub fn order_all(file: &HttpFile) -> Result<Vec<usize>, ChainError> {
    let mut out = Vec::new();
    for at in 0..file.requests.len() {
        let mut path = Vec::new();
        visit(file, at, &|_| false, &mut path, &mut out)?;
    }
    Ok(out)
}

fn visit(
    file: &HttpFile,
    at: usize,
    fresh: &dyn Fn(&str) -> bool,
    path: &mut Vec<usize>,
    out: &mut Vec<usize>,
) -> Result<(), ChainError> {
    if out.contains(&at) {
        return Ok(());
    }
    if let Some(start) = path.iter().position(|p| *p == at) {
        let names = path[start..]
            .iter()
            .chain(std::iter::once(&at))
            .map(|i| file.requests[*i].label())
            .collect();
        return Err(ChainError::Loop(names));
    }
    path.push(at);
    for name in dependencies(&file.requests[at]) {
        let Some(used) = file.index_of(&name) else {
            return Err(ChainError::Unknown(name));
        };
        // Its response kept: used as it is (the first request of the chain runs again).
        if !fresh(&name) || path.contains(&used) {
            visit(file, used, fresh, path, out)?;
        }
    }
    path.pop();
    out.push(at);
    Ok(())
}

/// The file's chain as a graph: each request's index, and the edges (from the one whose
/// response is used, to the one using it, with the variable it goes by).
pub fn edges(file: &HttpFile) -> Vec<(usize, usize, String)> {
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    for (to, request) in file.requests.iter().enumerate() {
        for (name, reference) in response_refs(request) {
            let Some(from) = file.index_of(&reference.request) else { continue };
            if !out.iter().any(|(f, t, n)| *f == from && *t == to && n == name) {
                out.push((from, to, name.to_owned()));
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/chain.rs"]
mod tests;
