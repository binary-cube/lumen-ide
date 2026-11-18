//! A request resolved, made ready to send (plan 77j):
//! - what a URL can't hold as written (a space, a letter outside ASCII, `"`, `<`, `|`…)
//!   percent-encoded, unless the request says `# @no-auto-encoding`;
//! - a URL without its scheme (`example.org/users`) sent over `https://` (the user's choice:
//!   other clients take `http://`); one that's only a path (`/users`) sent to its `Host`
//!   header's host;
//! - `Authorization: Basic user password` (the two written apart) sent encoded, as
//!   `Basic dXNlcjpwYXNzd29yZA==` (one already encoded is sent as it is);
//! - `GRAPHQL url` with a query as its body (and its variables as JSON after a blank line,
//!   or after a line of `{` at its start) sent as a `POST` of `{"query": …, "variables": …}`.

use crate::model::{BodyPart, Header};
use crate::vars::ResolvedRequest;

/// `request` as it's sent.
pub fn prepare(request: ResolvedRequest) -> ResolvedRequest {
    prepare_with(request, true)
}

/// [`prepare`], its URL percent-encoded or (`auto_encoding` off) sent as written.
pub fn prepare_with(mut request: ResolvedRequest, auto_encoding: bool) -> ResolvedRequest {
    request.url = with_scheme(&request.url, &request.headers);
    if auto_encoding {
        request.url = encoded(&request.url);
    }
    for header in &mut request.headers {
        basic(header);
    }
    if request.method.eq_ignore_ascii_case("GRAPHQL") {
        graphql(&mut request);
    }
    request
}

/// `url` with what it can't hold as written percent-encoded (what's encoded already, and the
/// URL's own punctuation, kept).
pub fn encoded(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        let keep = c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c);
        if keep {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// `url` with its scheme (`https://` when it has none), a path alone joined to the `Host`
/// header's host.
fn with_scheme(url: &str, headers: &[Header]) -> String {
    let url = url.trim();
    if url.contains("://") && url.find("://") < url.find(['/', '?', '#']) {
        return url.to_owned();
    }
    if url.starts_with('/')
        && let Some(host) = headers.iter().find(|h| h.name.eq_ignore_ascii_case("host"))
    {
        return format!("https://{}{url}", host.value.trim());
    }
    format!("https://{url}")
}

/// `Basic user password` encoded (`Basic` and one value: as written).
fn basic(header: &mut Header) {
    if !header.name.eq_ignore_ascii_case("authorization") {
        return;
    }
    let value = header.value.trim();
    let Some(rest) = value.strip_prefix("Basic ").or_else(|| value.strip_prefix("basic ")) else {
        return;
    };
    let Some((user, password)) = rest.trim().split_once(char::is_whitespace) else { return };
    let pair = format!("{user}:{}", password.trim());
    header.value = format!("Basic {}", crate::curl::base64(pair.as_bytes()));
}

/// A GraphQL request as the `POST` it is.
fn graphql(request: &mut ResolvedRequest) {
    let text: String = request
        .body
        .iter()
        .filter_map(|p| match p {
            BodyPart::Text(t) => Some(t.as_str()),
            BodyPart::File(_) => None,
        })
        .collect();
    // The query, then its variables: after a blank line, or a line opening with `{` once the
    // query's own braces are closed.
    let (query, variables) = split_graphql(&text);
    let variables = serde_json::from_str::<serde_json::Value>(variables.trim())
        .unwrap_or(serde_json::Value::Null);
    let mut payload = serde_json::json!({ "query": query.trim() });
    if !variables.is_null() {
        payload["variables"] = variables;
    }
    request.method = "POST".to_owned();
    request.body = vec![BodyPart::Text(payload.to_string())];
    if !request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("content-type")) {
        request
            .headers
            .push(Header { name: "Content-Type".into(), value: "application/json".into() });
    }
}

fn split_graphql(text: &str) -> (&str, &str) {
    let mut depth = 0i32;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if depth == 0 && at > 0 && (line.trim().is_empty() || line.starts_with('{')) {
            let rest = &text[at..];
            if rest.trim_start().starts_with('{') {
                return (&text[..at], rest);
            }
        }
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        at += line.len();
    }
    (text, "")
}

#[cfg(test)]
#[path = "../tests/unit/prepare.rs"]
mod tests;
