//! A request as the visual editor holds it (plan 77l), read from its block of a `.http`
//! file and written back into one — in the file's own syntax, so the file stays what both the
//! editor and the user's own writing go by:
//! - its `###` title, `# @name`, its description (`# ` comment lines under the title);
//! - its method and URL, the URL's query as a table of parameters;
//! - its headers, a disabled one (or parameter) a comment `# @disabled Name: value`
//!   (`# @disabled-param name=value`), which other clients leave alone;
//! - its authentication as the header it is (`Authorization: Basic user password`,
//!   `Bearer {{token}}`, `{{$auth.token("id")}}`), an API key as `# @auth api-key Name` over
//!   its header (or `# @auth api-key-query name` over its parameter);
//! - its body by kind (JSON, XML, YAML, text, URL-encoded fields, multipart fields, GraphQL,
//!   a file), its `Content-Type` following it;
//! - its scripts, its other directives (`@no-log`, `@timeout`…) as they were;
//! - what the editor doesn't show ([`Kept`]: the HTTP version, `>> file`, scripts in files,
//!   a multipart body's own text while its fields are unchanged) written back as it was.

use crate::model::{BodyPart, Request, SaveTo, Script};

/// A name and its value, on or off (a header, a parameter, a form's field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

impl Field {
    pub fn on(name: &str, value: &str) -> Self {
        Self { name: name.to_owned(), value: value.to_owned(), enabled: true }
    }
}

/// A multipart field: text, or a file (`< ./path`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub value: String,
    pub file: bool,
}

/// How a request authenticates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Auth {
    #[default]
    None,
    /// A key in a header (or, `in_query`, a parameter).
    ApiKey {
        name: String,
        value: String,
        in_query: bool,
    },
    Basic {
        user: String,
        password: String,
    },
    Digest {
        user: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    /// OAuth 2.0: a configuration of the environment (`Security.Auth.<id>`).
    OAuth2 {
        id: String,
    },
}

/// What a request sends.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Body {
    #[default]
    None,
    Json(String),
    Xml(String),
    Yaml(String),
    Text(String),
    /// Another type (its `Content-Type`), as text.
    Other(String),
    UrlEncoded(Vec<Field>),
    Multipart(Vec<Part>),
    GraphQl {
        query: String,
        variables: String,
    },
    /// A file sent as it is.
    File(String),
}

impl Body {
    /// The `Content-Type` it goes with (`None`: whatever the headers say).
    pub fn content_type(&self) -> Option<&'static str> {
        Some(match self {
            Self::Json(_) | Self::GraphQl { .. } => "application/json",
            Self::Xml(_) => "application/xml",
            Self::Yaml(_) => "application/yaml",
            Self::Text(_) => "text/plain",
            Self::UrlEncoded(_) => "application/x-www-form-urlencoded",
            Self::Multipart(_) => "multipart/form-data; boundary=WebAppBoundary",
            Self::None | Self::Other(_) | Self::File(_) => return None,
        })
    }
}

/// What a request has that the editor doesn't show: written back as it was read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Kept {
    /// `HTTP/2`… after its URL.
    pub version: Option<String>,
    /// `>> file` / `>>! file`.
    pub save_to: Option<SaveTo>,
    /// `< ./pre.js`: a script in a file (an inline one is edited).
    pub pre_file: Option<String>,
    /// `> ./post.js`.
    pub post_file: Option<String>,
    /// A multipart body as read (its fields, its text, its `Content-Type` with its
    /// boundary): its text written again while its fields are those (per part headers,
    /// file names, the boundary kept).
    pub multipart: Option<(Vec<Part>, String, String)>,
}

/// A request being edited.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    pub title: String,
    pub name: Option<String>,
    /// Its description (Markdown).
    pub docs: String,
    pub method: String,
    /// Its URL without its query.
    pub url: String,
    pub params: Vec<Field>,
    pub headers: Vec<Field>,
    pub auth: Auth,
    pub body: Body,
    pub pre_script: Option<String>,
    pub post_script: Option<String>,
    /// Its other directives, as written (`@no-log`, `@timeout 5`…).
    pub directives: Vec<String>,
    pub kept: Kept,
}

/// `url` split at its query: the part before, the parameters (decoded as written: the
/// file's text, its `{{variables}}` kept).
pub fn split_query(url: &str) -> (String, Vec<Field>) {
    let Some((base, query)) = url.split_once('?') else { return (url.to_owned(), Vec::new()) };
    let params = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (name, value) = p.split_once('=').unwrap_or((p, ""));
            Field::on(name, value)
        })
        .collect();
    (base.to_owned(), params)
}

/// The URL with its enabled parameters.
pub fn join_query(url: &str, params: &[Field]) -> String {
    let on: Vec<String> = params
        .iter()
        .filter(|p| p.enabled && !p.name.is_empty())
        .map(
            |p| if p.value.is_empty() { p.name.clone() } else { format!("{}={}", p.name, p.value) },
        )
        .collect();
    if on.is_empty() { url.to_owned() } else { format!("{url}?{}", on.join("&")) }
}

impl Draft {
    /// A new request.
    pub fn new(title: &str) -> Self {
        Self { title: title.to_owned(), method: "GET".into(), ..Self::default() }
    }

    /// `request` as the editor holds it, `block` its text in the file (from its `###` on:
    /// what the parser leaves out — the description, what's disabled — read from it).
    pub fn of(request: &Request, block: &str) -> Self {
        let (url, mut params) = split_query(&request.url);
        let mut draft = Self {
            title: request.title.clone().unwrap_or_default(),
            name: request.name.clone().filter(|n| Some(n) != request.title.as_ref()),
            method: request.method.clone(),
            url,
            headers: request.headers.iter().map(|h| Field::on(&h.name, &h.value)).collect(),
            pre_script: inline(request.pre_script.as_ref()),
            post_script: inline(request.post_script.as_ref()),
            kept: Kept {
                version: request.version.clone(),
                save_to: request.save_to.clone(),
                pre_file: file(request.pre_script.as_ref()),
                post_file: file(request.post_script.as_ref()),
                multipart: None,
            },
            ..Self::default()
        };
        let mut docs = Vec::new();
        let mut api_key: Option<(String, bool)> = None;
        // The comment lines before its request line (or its pre-request script: what's in
        // one is the script's, not the request's).
        for line in block.lines().skip_while(|l| l.trim_start().starts_with("###")) {
            let trimmed = line.trim();
            let Some(text) = trimmed.strip_prefix('#').or_else(|| trimmed.strip_prefix("//"))
            else {
                if trimmed.is_empty() || trimmed.starts_with('@') {
                    continue;
                }
                break;
            };
            let text = text.strip_prefix(' ').unwrap_or(text);
            if let Some(rest) = text.strip_prefix("@disabled-param ") {
                let (name, value) = rest.split_once('=').unwrap_or((rest, ""));
                params.push(Field { enabled: false, ..Field::on(name.trim(), value.trim()) });
            } else if let Some(rest) = text.strip_prefix("@disabled ") {
                let (name, value) = rest.split_once(':').unwrap_or((rest, ""));
                draft
                    .headers
                    .push(Field { enabled: false, ..Field::on(name.trim(), value.trim()) });
            } else if let Some(rest) = text.strip_prefix("@auth ") {
                let mut words = rest.split_whitespace();
                match (words.next(), words.next()) {
                    (Some("api-key"), Some(name)) => api_key = Some((name.to_owned(), false)),
                    (Some("api-key-query"), Some(name)) => api_key = Some((name.to_owned(), true)),
                    _ => {}
                }
            } else if text.starts_with("@name") {
            } else if text.starts_with('@') {
                draft.directives.push(text.to_owned());
            } else {
                docs.push(text.to_owned());
            }
        }
        draft.params = params;
        draft.docs = docs.join("\n");
        draft.auth = take_auth(&mut draft.headers, &mut draft.params, api_key);
        let content_type = draft
            .headers
            .iter()
            .find(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.clone());
        draft.body = body_of(request, &mut draft.headers);
        if let (Body::Multipart(parts), Some(content_type)) = (&draft.body, content_type) {
            draft.kept.multipart = Some((parts.clone(), text_of(request), content_type));
        }
        draft
    }

    /// Its block of text (from its `###` line on, a blank line at its end).
    pub fn to_text(&self) -> String {
        let mut out = format!("### {}\n", one_line(&self.title));
        for line in self.docs.lines() {
            out.push_str(&format!("# {line}\n").replace("# \n", "#\n"));
        }
        if let Some(name) = self.name.as_deref().filter(|n| !n.trim().is_empty()) {
            out.push_str(&format!("# @name {}\n", one_line(name)));
        }
        for directive in &self.directives {
            out.push_str(&format!("# {}\n", one_line(directive)));
        }
        let mut headers = self.headers.clone();
        let mut params = self.params.clone();
        match &self.auth {
            Auth::None => {}
            Auth::ApiKey { name, value, in_query } => {
                if *in_query {
                    out.push_str(&format!("# @auth api-key-query {name}\n"));
                    params.retain(|p| p.name != *name);
                    params.push(Field::on(name, value));
                } else {
                    out.push_str(&format!("# @auth api-key {name}\n"));
                    headers.retain(|h| !h.name.eq_ignore_ascii_case(name));
                    headers.push(Field::on(name, value));
                }
            }
            auth => {
                let value = match auth {
                    Auth::Basic { user, password } => format!("Basic {user} {password}"),
                    Auth::Digest { user, password } => format!("Digest {user} {password}"),
                    Auth::Bearer { token } => format!("Bearer {token}"),
                    Auth::OAuth2 { id } => format!("Bearer {{{{$auth.token(\"{id}\")}}}}"),
                    _ => String::new(),
                };
                headers.retain(|h| !h.name.eq_ignore_ascii_case("authorization"));
                headers.insert(0, Field::on("Authorization", &value));
            }
        }
        // A multipart body unchanged: its own text, its own boundary.
        let kept_multipart = match (&self.body, &self.kept.multipart) {
            (Body::Multipart(parts), Some((was, text, kind))) if parts == was => Some((text, kind)),
            _ => None,
        };
        let content_type =
            kept_multipart.map(|(_, kind)| kind.as_str()).or(self.body.content_type());
        if let Some(kind) = content_type {
            headers.retain(|h| !h.name.eq_ignore_ascii_case("content-type"));
            headers.push(Field::on("Content-Type", kind));
        }
        for param in params.iter().filter(|p| !p.enabled) {
            out.push_str(&format!(
                "# @disabled-param {}={}\n",
                one_line(&param.name),
                one_line(&param.value)
            ));
        }
        for header in headers.iter().filter(|h| !h.enabled) {
            out.push_str(&format!(
                "# @disabled {}: {}\n",
                one_line(&header.name),
                one_line(&header.value)
            ));
        }
        if let Some(script) = &self.pre_script {
            out.push_str(&format!("< {{%\n{}\n%}}\n", script.trim_end()));
        } else if let Some(path) = &self.kept.pre_file {
            out.push_str(&format!("< {}\n", one_line(path)));
        }
        let method = self.sent_method().trim();
        let method = if method.is_empty() { "GET" } else { method };
        let version =
            self.kept.version.as_deref().map(|v| format!(" {}", one_line(v))).unwrap_or_default();
        out.push_str(&format!(
            "{method} {}{version}\n",
            one_line(&join_query(self.url.trim(), &params))
        ));
        for header in headers.iter().filter(|h| h.enabled && !h.name.trim().is_empty()) {
            out.push_str(&format!("{}: {}\n", one_line(&header.name), one_line(&header.value)));
        }
        let body = match &self.body {
            Body::None => None,
            Body::Json(t) | Body::Xml(t) | Body::Yaml(t) | Body::Text(t) | Body::Other(t) => {
                Some(t.trim_end().to_owned())
            }
            Body::UrlEncoded(fields) => Some(
                fields
                    .iter()
                    .filter(|f| f.enabled && !f.name.is_empty())
                    .map(|f| format!("{}={}", f.name, f.value))
                    .collect::<Vec<_>>()
                    .join("&"),
            ),
            Body::Multipart(_) if kept_multipart.is_some() => {
                kept_multipart.map(|(text, _)| text.trim_end().to_owned())
            }
            Body::Multipart(parts) => {
                let mut text = String::new();
                for part in parts {
                    text.push_str("--WebAppBoundary\n");
                    if part.file {
                        let file = part.value.rsplit(['/', '\\']).next().unwrap_or(&part.value);
                        text.push_str(&format!(
                            "Content-Disposition: form-data; name=\"{}\"; filename=\"{file}\"\n\n< {}\n",
                            part.name, part.value
                        ));
                    } else {
                        text.push_str(&format!(
                            "Content-Disposition: form-data; name=\"{}\"\n\n{}\n",
                            part.name, part.value
                        ));
                    }
                }
                text.push_str("--WebAppBoundary--");
                Some(text)
            }
            Body::GraphQl { query, variables } => Some(if variables.trim().is_empty() {
                query.trim_end().to_owned()
            } else {
                format!("{}\n\n{}", query.trim_end(), variables.trim())
            }),
            Body::File(path) => Some(format!("< {path}")),
        };
        if let Some(body) = body.filter(|b| !b.is_empty()) {
            out.push('\n');
            out.push_str(&body);
            out.push('\n');
        }
        if let Some(script) = &self.post_script {
            out.push_str(&format!("\n> {{%\n{}\n%}}\n", script.trim_end()));
        } else if let Some(path) = &self.kept.post_file {
            out.push_str(&format!("\n> {}\n", one_line(path)));
        }
        if let Some(save) = &self.kept.save_to {
            let arrow = if save.replace { ">>!" } else { ">>" };
            out.push_str(&format!("\n{arrow} {}\n", one_line(&save.path)));
        }
        out.push('\n');
        out
    }

    /// The method it's sent with (GraphQL: its own).
    pub fn sent_method(&self) -> &str {
        match self.body {
            Body::GraphQl { .. } => "GRAPHQL",
            _ => &self.method,
        }
    }
}

/// Request `request`'s whole block in `text`: from its `###` line (when it has one) to the
/// next request's — what the editor replaces when it writes it.
pub fn block_of(text: &str, request: &Request) -> std::ops::Range<usize> {
    let start = request.range.start.min(text.len());
    let before = &text[..start];
    // The separator's line: the one just before the block's start.
    let line_start = before.trim_end_matches(['\n', '\r']).rfind('\n').map_or(0, |n| n + 1);
    let separator = before[line_start..].trim_start().starts_with("###");
    let from = if separator { line_start } else { start };
    from..request.range.end.min(text.len())
}

/// A block's fingerprint (its text, its trailing blank lines aside): what a list of
/// requests finds one by again before changing its file.
pub fn block_hash(block: &str) -> u64 {
    // FNV-1a: stable across runs, enough to tell blocks apart.
    block
        .trim_end()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// Which request of `text` has the block whose fingerprint is `hash`.
pub fn find_by_hash(text: &str, hash: u64) -> Option<usize> {
    let parsed = crate::parse(text);
    parsed.requests.iter().position(|r| block_hash(&text[block_of(text, r)]) == hash)
}

/// Request `index`'s block copied as it is to the end of `text`, titled `(copy)` and without
/// its `@name` (two of a name would be ambiguous).
pub fn duplicate_raw(text: &str, index: usize, copy: &str) -> Option<String> {
    let parsed = crate::parse(text);
    let block = &text[block_of(text, parsed.requests.get(index)?)];
    let mut lines: Vec<String> = Vec::new();
    for (i, line) in block.trim_end().lines().enumerate() {
        let trimmed = line.trim_start();
        let named = trimmed
            .strip_prefix('#')
            .or_else(|| trimmed.strip_prefix("//"))
            .is_some_and(|c| c.trim_start().starts_with("@name"));
        if named {
            continue;
        }
        if i == 0
            && let Some(title) = trimmed.strip_prefix("###")
        {
            lines.push(format!("### {} ({copy})", title.trim()));
            continue;
        }
        lines.push(line.to_owned());
    }
    let mut out = text.trim_end().to_owned();
    out.push_str("\n\n");
    if !block.trim_start().starts_with("###") {
        out.push_str(&format!("### ({copy})\n"));
    }
    out.push_str(&lines.join("\n"));
    out.push('\n');
    Some(out)
}

/// Request `index`'s block moved as it is from `source` to the end of `target`: the two
/// texts after.
pub fn move_raw(source: &str, index: usize, target: &str) -> Option<(String, String)> {
    let parsed = crate::parse(source);
    let block = source[block_of(source, parsed.requests.get(index)?)].trim_end().to_owned();
    let mut into = target.trim_end().to_owned();
    if !into.is_empty() {
        into.push_str("\n\n");
    }
    if !block.trim_start().starts_with("###") {
        into.push_str("###\n");
    }
    into.push_str(&block);
    into.push('\n');
    Some((remove_from(source, index), into))
}

/// Which request of `text` has block `block` exactly (what an editor read: found again,
/// wherever it is now; `None`: it changed, or it's gone).
pub fn find_block(text: &str, block: &str) -> Option<usize> {
    let parsed = crate::parse(text);
    let want = block.trim_end();
    parsed.requests.iter().position(|r| text[block_of(text, r)].trim_end() == want)
}

/// `text` with request `index`'s block replaced by `draft` (a new one, past the last:
/// written at the end).
pub fn write_into(text: &str, index: Option<usize>, draft: &Draft) -> String {
    let parsed = crate::parse(text);
    let block = draft.to_text();
    match index.and_then(|i| parsed.requests.get(i)) {
        Some(request) => {
            let span = block_of(text, request);
            let mut out = String::with_capacity(text.len() + block.len());
            out.push_str(&text[..span.start]);
            out.push_str(&block);
            out.push_str(text[span.end..].trim_start_matches(['\n', '\r']));
            out
        }
        None => {
            let mut out = text.trim_end().to_owned();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&block);
            out
        }
    }
}

/// `text` without request `index`'s block (none such: as it is).
pub fn remove_from(text: &str, index: usize) -> String {
    let parsed = crate::parse(text);
    let Some(request) = parsed.requests.get(index) else { return text.to_owned() };
    let span = block_of(text, request);
    let mut out = text[..span.start].to_owned();
    out.push_str(text[span.end..].trim_start_matches(['\n', '\r']));
    out
}

fn inline(script: Option<&Script>) -> Option<String> {
    match script? {
        Script::Inline(code) => Some(code.clone()),
        Script::File(_) => None,
    }
}

fn file(script: Option<&Script>) -> Option<String> {
    match script? {
        Script::File(path) => Some(path.clone()),
        Script::Inline(_) => None,
    }
}

/// A value written on one line of the file: a line break in it (which would add a line of
/// its own — a script, a file sent) made a space.
pub(crate) fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// The authentication its headers (or parameters) carry, taken out of them.
fn take_auth(
    headers: &mut Vec<Field>,
    params: &mut Vec<Field>,
    api_key: Option<(String, bool)>,
) -> Auth {
    if let Some((name, in_query)) = api_key {
        let list: &mut Vec<Field> = if in_query { &mut *params } else { &mut *headers };
        if let Some(at) = list.iter().position(|f| f.enabled && f.name.eq_ignore_ascii_case(&name))
        {
            let field = list.remove(at);
            return Auth::ApiKey { name: field.name, value: field.value, in_query };
        }
    }
    let Some(at) =
        headers.iter().position(|h| h.enabled && h.name.eq_ignore_ascii_case("authorization"))
    else {
        return Auth::None;
    };
    let value = headers[at].value.trim().to_owned();
    let two = |rest: &str| -> Option<(String, String)> {
        let (user, password) = rest.trim().split_once(char::is_whitespace)?;
        Some((user.to_owned(), password.trim().to_owned()))
    };
    let auth = if let Some(rest) = value.strip_prefix("Basic ") {
        two(rest).map(|(user, password)| Auth::Basic { user, password })
    } else if let Some(rest) = value.strip_prefix("Digest ") {
        two(rest).map(|(user, password)| Auth::Digest { user, password })
    } else if let Some(rest) = value.strip_prefix("Bearer ") {
        let rest = rest.trim();
        Some(
            rest.strip_prefix("{{$auth.token(\"")
                .and_then(|r| r.strip_suffix("\")}}"))
                .map_or_else(
                    || Auth::Bearer { token: rest.to_owned() },
                    |id| Auth::OAuth2 { id: id.to_owned() },
                ),
        )
    } else {
        None
    };
    match auth {
        Some(auth) => {
            headers.remove(at);
            auth
        }
        None => Auth::None,
    }
}

/// The body by its kind (its `Content-Type`, taken out of the headers when the kind says
/// it).
fn body_of(request: &Request, headers: &mut Vec<Field>) -> Body {
    let kind = headers
        .iter()
        .find(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_ascii_lowercase())
        .unwrap_or_default();
    if request.method.eq_ignore_ascii_case("GRAPHQL") {
        let text = text_of(request);
        let (query, variables) = text.split_once("\n\n").unwrap_or((&text, ""));
        return Body::GraphQl { query: query.to_owned(), variables: variables.to_owned() };
    }
    if let [BodyPart::File(path)] = request.body.as_slice() {
        return Body::File(path.clone());
    }
    if request.body.is_empty() {
        return Body::None;
    }
    let text = text_of(request);
    let body = if kind.starts_with("application/x-www-form-urlencoded") {
        let fields = text
            .split('&')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (name, value) = p.split_once('=').unwrap_or((p, ""));
                Field::on(name, value)
            })
            .collect();
        Body::UrlEncoded(fields)
    } else if kind.starts_with("multipart/form-data") {
        Body::Multipart(multipart(request))
    } else if kind.contains("json") {
        Body::Json(text)
    } else if kind.contains("xml") {
        Body::Xml(text)
    } else if kind.contains("yaml") {
        Body::Yaml(text)
    } else if kind.starts_with("text/plain") {
        Body::Text(text)
    } else {
        return Body::Other(text);
    };
    // The kind says its type: written again from it.
    headers.retain(|h| !(h.enabled && h.name.eq_ignore_ascii_case("content-type")));
    body
}

fn text_of(request: &Request) -> String {
    request
        .body
        .iter()
        .map(|p| match p {
            BodyPart::Text(t) => t.clone(),
            BodyPart::File(path) => format!("< {path}\n"),
        })
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// A multipart body's fields (its text parts' and files').
fn multipart(request: &Request) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut name = None;
    for piece in &request.body {
        match piece {
            BodyPart::Text(text) => {
                let mut value = Vec::new();
                let mut in_headers = false;
                for line in text.lines() {
                    if line.starts_with("--") {
                        if let Some(n) = name.take() {
                            parts.push(Part { name: n, value: value.join("\n"), file: false });
                        }
                        value.clear();
                        in_headers = true;
                    } else if in_headers {
                        if line.trim().is_empty() {
                            in_headers = false;
                        } else if let Some(at) = line.find("name=\"") {
                            let rest = &line[at + 6..];
                            name = rest.split('"').next().map(str::to_owned);
                        }
                    } else {
                        value.push(line.to_owned());
                    }
                }
                // A part with text of its own ends here; one with nothing yet is a file's
                // (its `< path` next).
                if !value.is_empty()
                    && let Some(n) = name.take()
                {
                    parts.push(Part { name: n, value: value.join("\n"), file: false });
                }
            }
            BodyPart::File(path) => {
                if let Some(n) = name.take() {
                    parts.push(Part { name: n, value: path.clone(), file: true });
                }
            }
        }
    }
    parts
}

#[cfg(test)]
#[path = "../tests/unit/draft.rs"]
mod tests;
