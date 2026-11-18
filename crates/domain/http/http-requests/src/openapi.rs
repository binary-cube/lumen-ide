//! An OpenAPI (3.x) or Swagger (2.0) description turned into requests (plan 77i, asked
//! 2026-11-11): one request per operation, titled by its tag and summary, named by its
//! `operationId`; its server as `{{baseUrl}}` in the environment; a path's `{id}` as
//! `{{id}}`; its query and header parameters written with variables; its body from its
//! example, else a sample made from its schema (`$ref`s followed, loops cut); its security
//! as the header it is (Bearer, Basic, an API key).
//!
//! A description is someone else's text: what it names (a parameter, a header, a path's
//! part) becomes a plain variable's name or plain text, never a `{{…}}` of its own (which
//! could read a token, a file or the environment); its examples' `{{` are broken apart; a
//! sample is made of a bounded number of values, and its YAML of a bounded number of nodes
//! (aliases counted as what they copy).

use serde_json::{Map, Value};

/// The most values a body's sample is made of.
const SAMPLE_NODES: usize = 2_000;
/// The most nodes a YAML description may be (its aliases expanded), and the deepest.
const YAML_NODES: usize = 1_000_000;
const YAML_DEPTH: usize = 256;

/// A name from the description as a variable's: letters, digits, `_`, `-`, `.` only.
fn variable(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' })
        .collect();
    if clean.is_empty() { "value".to_owned() } else { clean }
}

/// A name from the description written as text (a parameter's, a header's): without the
/// braces that would make it a variable, nor a line break.
fn literal(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '{' | '}' | '\r' | '\n')).collect()
}

/// A path with its `{id}` parts as variables, the rest as text.
fn path_url(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        out.push_str(&literal(&rest[..open]));
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            rest = after;
            continue;
        };
        out.push_str(&format!("{{{{{}}}}}", variable(&after[..close])));
        rest = &after[close + 1..];
    }
    out.push_str(&literal(rest));
    out
}

/// A text from the description's examples with its `{{` broken apart: shown, never resolved.
fn inert(text: &str) -> String {
    text.replace("{{", "{ {").replace("}}", "} }")
}

/// What a description holds, as [`crate::import`] writes it.
pub(crate) struct Operation {
    pub path: Vec<String>,
    pub name: String,
    pub id: Option<String>,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub notes: Vec<String>,
}

/// The description's title, its operations, its environment (`baseUrl`, the variables its
/// security needs).
pub(crate) fn read(spec: &Value) -> (String, Vec<Operation>, Map<String, Value>) {
    let swagger = spec.get("swagger").is_some();
    let title = spec.pointer("/info/title").and_then(Value::as_str).unwrap_or("API").to_owned();
    let mut environment = Map::new();
    let base = if swagger {
        let scheme = spec
            .get("schemes")
            .and_then(Value::as_array)
            .and_then(|s| s.first())
            .and_then(Value::as_str)
            .unwrap_or("https");
        let host = spec.get("host").and_then(Value::as_str).unwrap_or("localhost");
        let path = spec.get("basePath").and_then(Value::as_str).unwrap_or("");
        format!("{scheme}://{host}{}", path.trim_end_matches('/'))
    } else {
        spec.pointer("/servers/0/url")
            .and_then(Value::as_str)
            .unwrap_or("https://localhost")
            .trim_end_matches('/')
            .to_owned()
    };
    environment.insert("baseUrl".into(), Value::String(base));
    let security = security_headers(spec, swagger, &mut environment);
    let mut operations = Vec::new();
    let Some(paths) = spec.get("paths").and_then(Value::as_object) else {
        return (title, operations, environment);
    };
    for (path, item) in paths {
        let item = resolve(spec, item);
        let shared: Vec<Value> =
            item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
        for method in ["get", "post", "put", "patch", "delete", "head", "options"] {
            let Some(op) = item.get(method) else { continue };
            let mut parameters: Vec<Value> =
                shared.iter().map(|p| resolve(spec, p).clone()).collect();
            for p in op.get("parameters").and_then(Value::as_array).into_iter().flatten() {
                let p = resolve(spec, p).clone();
                let same = |q: &Value| q.get("name") == p.get("name") && q.get("in") == p.get("in");
                parameters.retain(|q| !same(q));
                parameters.push(p);
            }
            operations.push(operation(spec, swagger, path, method, op, &parameters, &security));
        }
    }
    (title, operations, environment)
}

fn operation(
    spec: &Value,
    swagger: bool,
    path: &str,
    method: &str,
    op: &Value,
    parameters: &[Value],
    security: &[(String, String)],
) -> Operation {
    let text = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or_default().to_owned();
    let summary = text(op.get("summary"));
    let id = op.get("operationId").and_then(Value::as_str).map(str::to_owned);
    let name = if summary.is_empty() {
        id.clone().unwrap_or_else(|| format!("{} {path}", method.to_uppercase()))
    } else {
        summary
    };
    let tag =
        op.get("tags").and_then(Value::as_array).and_then(|t| t.first()).and_then(Value::as_str);
    let mut notes = Vec::new();
    if let Some(description) = op.get("description").and_then(Value::as_str) {
        notes.extend(description.lines().take(3).map(str::to_owned));
    }
    if op.get("deprecated") == Some(&Value::Bool(true)) {
        notes.push("deprecated".to_owned());
    }
    // `{id}` as a variable.
    let mut url = format!("{{{{baseUrl}}}}{}", path_url(path));
    let mut query = Vec::new();
    let mut headers: Vec<(String, String)> = security.to_vec();
    let mut form = Vec::new();
    let mut body = None;
    let mut content_type = None;
    for p in parameters {
        let raw = text(p.get("name"));
        let (pname, var) = (literal(&raw), variable(&raw));
        match p.get("in").and_then(Value::as_str) {
            Some("query") => query.push(format!("{pname}={{{{{var}}}}}")),
            Some("header") => headers.push((pname, format!("{{{{{var}}}}}"))),
            Some("body") if swagger => {
                body = Some(inert(&sample_text(spec, p.get("schema"))));
                content_type = Some("application/json".to_owned());
            }
            Some("formData") => form.push(format!("{pname}={{{{{var}}}}}")),
            _ => {}
        }
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    if !form.is_empty() {
        body = Some(form.join("&"));
        content_type = Some("application/x-www-form-urlencoded".to_owned());
    }
    if let Some(request_body) = op.get("requestBody") {
        let request_body = resolve(spec, request_body);
        if let Some(content) = request_body.get("content").and_then(Value::as_object) {
            let pick =
                ["application/json", "application/x-www-form-urlencoded", "multipart/form-data"]
                    .iter()
                    .find_map(|t| content.get(*t).map(|c| ((*t).to_owned(), c)))
                    .or_else(|| content.iter().next().map(|(t, c)| (t.clone(), c)));
            if let Some((kind, media)) = pick {
                let example = media.get("example").cloned().or_else(|| {
                    media
                        .get("examples")
                        .and_then(Value::as_object)
                        .and_then(|e| e.values().next())
                        .and_then(|e| resolve(spec, e).get("value").cloned())
                });
                let value = example.unwrap_or_else(|| {
                    sample(spec, media.get("schema"), 0, &mut Sampling::default())
                });
                body = Some(if kind.contains("x-www-form-urlencoded") {
                    value
                        .as_object()
                        .map(|o| {
                            o.keys()
                                .map(|k| format!("{}={{{{{}}}}}", literal(k), variable(k)))
                                .collect::<Vec<_>>()
                                .join("&")
                        })
                        .unwrap_or_default()
                } else if kind.contains("json") {
                    inert(&serde_json::to_string_pretty(&value).unwrap_or_default())
                } else {
                    notes.push(format!("its {kind} body is to be written"));
                    String::new()
                });
                content_type = Some(if kind.starts_with("multipart") {
                    "multipart/form-data; boundary=WebAppBoundary".to_owned()
                } else {
                    literal(&kind)
                });
            }
        }
    }
    if let Some(kind) = content_type {
        headers.push(("Content-Type".to_owned(), kind));
    }
    Operation {
        path: tag.map(str::to_owned).into_iter().collect(),
        name,
        id,
        method: method.to_uppercase(),
        url,
        headers,
        body: body.filter(|b| !b.is_empty()),
        notes,
    }
}

/// The headers its (first) security scheme asks for, its variables put in the environment.
fn security_headers(
    spec: &Value,
    swagger: bool,
    environment: &mut Map<String, Value>,
) -> Vec<(String, String)> {
    let schemes = if swagger {
        spec.get("securityDefinitions")
    } else {
        spec.pointer("/components/securitySchemes")
    };
    let Some(schemes) = schemes.and_then(Value::as_object) else { return Vec::new() };
    // The one the description asks for everywhere, else the first.
    let wanted = spec
        .get("security")
        .and_then(Value::as_array)
        .and_then(|s| s.first())
        .and_then(Value::as_object)
        .and_then(|o| o.keys().next().cloned());
    let Some((_, scheme)) = wanted
        .as_ref()
        .and_then(|w| schemes.get_key_value(w.as_str()))
        .or_else(|| schemes.iter().next())
    else {
        return Vec::new();
    };
    let scheme = resolve(spec, scheme);
    let kind = scheme.get("type").and_then(Value::as_str).unwrap_or_default();
    let http =
        scheme.get("scheme").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    let mut set = |name: &str| {
        environment.entry(name.to_owned()).or_insert(Value::String(String::new()));
    };
    match (kind, http.as_str()) {
        ("http", "bearer") | ("oauth2", _) | ("openIdConnect", _) => {
            set("token");
            vec![("Authorization".to_owned(), "Bearer {{token}}".to_owned())]
        }
        ("http", "basic") | ("basic", _) => {
            set("username");
            set("password");
            vec![("Authorization".to_owned(), "Basic {{username}} {{password}}".to_owned())]
        }
        ("apiKey", _) if scheme.get("in").and_then(Value::as_str) == Some("header") => {
            let header = literal(scheme.get("name").and_then(Value::as_str).unwrap_or("X-API-Key"));
            set("apiKey");
            vec![(header, "{{apiKey}}".to_owned())]
        }
        _ => Vec::new(),
    }
}

/// `value` with its `$ref` (to this document) followed, as far as they go.
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    let mut at = value;
    for _ in 0..16 {
        let Some(reference) = at.get("$ref").and_then(Value::as_str) else { return at };
        let Some(pointer) = reference.strip_prefix('#') else { return at };
        match spec.pointer(pointer) {
            Some(next) => at = next,
            None => return at,
        }
    }
    at
}

fn sample_text(spec: &Value, schema: Option<&Value>) -> String {
    serde_json::to_string_pretty(&sample(spec, schema, 0, &mut Sampling::default()))
        .unwrap_or_default()
}

/// A sample being made: the references it's inside (a loop cut), the values made so far.
#[derive(Default)]
struct Sampling {
    seen: Vec<String>,
    made: usize,
}

/// A value its schema describes: its example, its default, its first choice, or one made
/// from its type (an object's properties, an array's one item); a loop of references cut,
/// at most [`SAMPLE_NODES`] values made (a wide schema's rest left `null`).
fn sample(spec: &Value, schema: Option<&Value>, depth: usize, seen: &mut Sampling) -> Value {
    let Some(schema) = schema else { return Value::Null };
    if depth > 8 || seen.made >= SAMPLE_NODES {
        return Value::Null;
    }
    seen.made += 1;
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if seen.seen.iter().any(|s| s == reference) {
            return Value::Null;
        }
        seen.seen.push(reference.to_owned());
        let value = sample(spec, Some(resolve(spec, schema)), depth + 1, seen);
        seen.seen.pop();
        return value;
    }
    for key in ["example", "default"] {
        if let Some(v) = schema.get(key) {
            return v.clone();
        }
    }
    if let Some(first) = schema.get("enum").and_then(Value::as_array).and_then(|e| e.first()) {
        return first.clone();
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        if let Some(list) = schema.get(key).and_then(Value::as_array) {
            if key == "allOf" {
                let mut merged = Map::new();
                for part in list {
                    if let Value::Object(o) = sample(spec, Some(part), depth + 1, seen) {
                        merged.extend(o);
                    }
                }
                return Value::Object(merged);
            }
            return sample(spec, list.first(), depth + 1, seen);
        }
    }
    let kind =
        schema.get("type").and_then(|t| t.as_str().or_else(|| t.as_array()?.first()?.as_str()));
    match kind {
        Some("object") | None if schema.get("properties").is_some() || kind == Some("object") => {
            let mut out = Map::new();
            for (name, property) in
                schema.get("properties").and_then(Value::as_object).into_iter().flatten()
            {
                out.insert(name.clone(), sample(spec, Some(property), depth + 1, seen));
            }
            Value::Object(out)
        }
        Some("array") => Value::Array(vec![sample(spec, schema.get("items"), depth + 1, seen)]),
        Some("integer") => Value::from(0),
        Some("number") => Value::from(0.0),
        Some("boolean") => Value::Bool(true),
        Some("string") => Value::String(
            match schema.get("format").and_then(Value::as_str) {
                Some("date-time") => "2026-01-01T00:00:00Z",
                Some("date") => "2026-01-01",
                Some("email") => "user@example.com",
                Some("uuid") => "00000000-0000-4000-8000-000000000000",
                Some("uri" | "url") => "https://example.com",
                _ => "string",
            }
            .to_owned(),
        ),
        _ => Value::Null,
    }
}

/// YAML read as the JSON it describes (a description written in YAML); one too big once
/// its aliases are expanded (or too deep), refused before it's built.
pub fn yaml_to_json(text: &str) -> Result<Value, String> {
    let mut measure = Measure::default();
    yaml_rust2::parser::Parser::new_from_str(text)
        .load(&mut measure, false)
        .map_err(|e| e.to_string())?;
    if measure.too_big {
        return Err(format!(
            "more than {YAML_NODES} values (its aliases expanded) or {YAML_DEPTH} levels"
        ));
    }
    let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    let doc = docs.into_iter().next().ok_or_else(|| "empty".to_owned())?;
    Ok(convert(&doc))
}

/// A YAML document's size as the loader would build it: each anchor's nodes counted once
/// made, an alias as many as it copies.
#[derive(Default)]
struct Measure {
    /// The open collections: their anchor, their nodes so far.
    open: Vec<(usize, usize)>,
    anchors: std::collections::HashMap<usize, usize>,
    total: usize,
    too_big: bool,
}

impl Measure {
    fn add(&mut self, nodes: usize) {
        self.total = self.total.saturating_add(nodes);
        if let Some((_, n)) = self.open.last_mut() {
            *n = n.saturating_add(nodes);
        }
        if self.total > YAML_NODES {
            self.too_big = true;
        }
    }
}

impl yaml_rust2::parser::EventReceiver for Measure {
    fn on_event(&mut self, event: yaml_rust2::parser::Event) {
        use yaml_rust2::parser::Event;
        if self.too_big {
            return;
        }
        match event {
            Event::Scalar(_, _, anchor, _) => {
                if anchor > 0 {
                    self.anchors.insert(anchor, 1);
                }
                self.add(1);
            }
            Event::Alias(anchor) => {
                let nodes = self.anchors.get(&anchor).copied().unwrap_or(1);
                self.add(nodes);
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                self.open.push((anchor, 1));
                if self.open.len() > YAML_DEPTH {
                    self.too_big = true;
                }
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((anchor, nodes)) = self.open.pop() {
                    if anchor > 0 {
                        self.anchors.insert(anchor, nodes);
                    }
                    // Its nodes were counted in the total as they came; its parent gets
                    // them now.
                    self.total = self.total.saturating_sub(nodes.saturating_sub(1));
                    self.add(nodes);
                }
            }
            _ => {}
        }
    }
}

fn convert(yaml: &yaml_rust2::Yaml) -> Value {
    use yaml_rust2::Yaml;
    match yaml {
        Yaml::Real(r) => r.parse::<f64>().map_or_else(|_| Value::String(r.clone()), Value::from),
        Yaml::Integer(i) => Value::from(*i),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(items) => Value::Array(items.iter().map(convert).collect()),
        Yaml::Hash(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Boolean(b) => b.to_string(),
                        Yaml::Real(r) => r.clone(),
                        _ => String::new(),
                    };
                    (key, convert(v))
                })
                .collect(),
        ),
        Yaml::Alias(_) | Yaml::Null | Yaml::BadValue => Value::Null,
    }
}

#[cfg(test)]
#[path = "../tests/unit/openapi.rs"]
mod tests;
