//! Collections of other HTTP clients turned into `.http` files (plan 77i): Postman's
//! (collection v2.0 / v2.1, its environments), Insomnia's (an export, v4), Bruno's (its `.bru`
//! files), an OpenAPI 3 / Swagger 2 description (JSON or YAML: [`crate::openapi`]). Each collection becomes one file of requests, `###` before each, its folders as the
//! requests' titles (`Users / Log in`), `# @name` from its name; its environments become
//! `http-client.env.json`'s. Their variables are written as ours (`{{name}}`; Insomnia's
//! `{{ _.name }}` too). What has no equivalent (a pre-request script, an auth helper) is
//! said in a comment above the request.

use serde_json::Value;

/// What an import makes: the requests' file's text, and the environments (`None`: none),
/// as `http-client.env.json` holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub name: String,
    pub requests: String,
    pub environments: Option<String>,
    pub count: usize,
    /// Bodies kept in files of their own beside the requests (relative path, text): one the
    /// file would read lines of as its own (a separator, a file sent, a script).
    pub bodies: Vec<(String, String)>,
}

/// Why a file couldn't be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError(pub String);

/// One request as the other clients hold them, before it's written.
#[derive(Debug, Default)]
struct Item {
    path: Vec<String>,
    name: String,
    /// What it's called by (`# @name`), when it says (an OpenAPI `operationId`).
    id: Option<String>,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    notes: Vec<String>,
}

/// `text` (JSON, or YAML: an OpenAPI description often is) read as what [`from_json`] reads.
pub fn from_text(text: &str) -> Result<Imported, ImportError> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return from_json(text);
    }
    let value = crate::openapi::yaml_to_json(text)
        .map_err(|e| ImportError(format!("not JSON nor YAML: {e}")))?;
    from_value(&value)
}

/// `text` (a JSON file) read as a Postman collection or environment, an Insomnia export, or
/// an OpenAPI / Swagger description.
pub fn from_json(text: &str) -> Result<Imported, ImportError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| ImportError(format!("not JSON: {e}")))?;
    from_value(&value)
}

fn from_value(value: &Value) -> Result<Imported, ImportError> {
    let value = value.clone();
    if value.get("openapi").is_some() || value.get("swagger").is_some() {
        let (title, operations, environment) = crate::openapi::read(&value);
        let items: Vec<Item> = operations
            .into_iter()
            .map(|op| Item {
                path: op.path,
                name: op.name,
                id: op.id,
                method: op.method,
                url: op.url,
                headers: op.headers,
                body: op.body,
                notes: op.notes,
            })
            .collect();
        let environments = environments_text(&[("$shared".to_owned(), environment)]);
        return Ok(finish(title, &items, Some(environments)));
    }
    if value.get("resources").is_some() && value.get("__export_format").is_some() {
        return Ok(insomnia(&value));
    }
    if value.get("info").is_some() && value.get("item").is_some() {
        return Ok(postman(&value));
    }
    if value.get("values").is_some_and(Value::is_array) {
        return Ok(postman_environment(&value));
    }
    Err(ImportError("neither a Postman collection or environment nor an Insomnia export".into()))
}

fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn postman(value: &Value) -> Imported {
    let name = text_of(value.pointer("/info/name"));
    let mut items = Vec::new();
    if let Some(list) = value.get("item").and_then(Value::as_array) {
        postman_items(list, &mut Vec::new(), &mut items);
    }
    let environments = value.get("variable").and_then(Value::as_array).map(|vars| {
        let shared: serde_json::Map<String, Value> = vars
            .iter()
            .filter(|v| v.get("disabled") != Some(&Value::Bool(true)))
            .filter_map(|v| {
                Some((v.get("key")?.as_str()?.to_owned(), Value::String(text_of(v.get("value")))))
            })
            .collect();
        environments_text(&[("$shared".to_owned(), shared)])
    });
    finish(name, &items, environments)
}

fn postman_items(list: &[Value], path: &mut Vec<String>, out: &mut Vec<Item>) {
    for entry in list {
        let name = text_of(entry.get("name"));
        if let Some(children) = entry.get("item").and_then(Value::as_array) {
            path.push(name);
            postman_items(children, path, out);
            path.pop();
            continue;
        }
        let Some(request) = entry.get("request") else { continue };
        let mut item = Item { path: path.clone(), name, ..Item::default() };
        // A request may be only its URL.
        if let Value::String(url) = request {
            item.method = "GET".into();
            item.url = url.clone();
            out.push(item);
            continue;
        }
        item.method = text_of(request.get("method")).to_ascii_uppercase();
        item.url = match request.get("url") {
            Some(Value::Object(url)) => text_of(url.get("raw")),
            other => text_of(other),
        };
        for header in request.get("header").and_then(Value::as_array).into_iter().flatten() {
            if header.get("disabled") == Some(&Value::Bool(true)) {
                continue;
            }
            item.headers.push((text_of(header.get("key")), text_of(header.get("value"))));
        }
        if let Some(body) = request.get("body") {
            postman_body(body, &mut item);
        }
        if let Some(auth) = request.get("auth").and_then(|a| a.get("type")).and_then(Value::as_str)
        {
            postman_auth(auth, request.get("auth"), &mut item);
        }
        if entry.get("event").and_then(Value::as_array).is_some_and(|e| !e.is_empty()) {
            item.notes.push("its Postman scripts weren't imported".into());
        }
        out.push(item);
    }
}

fn postman_body(body: &Value, item: &mut Item) {
    let pairs = |key: &str| -> Vec<(String, String)> {
        body.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|p| p.get("disabled") != Some(&Value::Bool(true)))
            .map(|p| (text_of(p.get("key")), text_of(p.get("value"))))
            .collect()
    };
    match body.get("mode").and_then(Value::as_str) {
        Some("raw") => item.body = Some(text_of(body.get("raw"))),
        Some("urlencoded") => {
            let encoded: Vec<String> =
                pairs("urlencoded").into_iter().map(|(k, v)| format!("{k}={v}")).collect();
            ensure_header(item, "Content-Type", "application/x-www-form-urlencoded");
            item.body = Some(encoded.join("&"));
        }
        Some("graphql") => {
            let query = text_of(body.pointer("/graphql/query"));
            let variables = text_of(body.pointer("/graphql/variables"));
            let json = serde_json::json!({
                "query": query,
                "variables": serde_json::from_str::<Value>(&variables).unwrap_or(Value::Null),
            });
            ensure_header(item, "Content-Type", "application/json");
            item.body = Some(serde_json::to_string_pretty(&json).unwrap_or_default());
        }
        Some("formdata") => {
            item.notes.push("its form data was written as URL-encoded fields".into());
            let encoded: Vec<String> =
                pairs("formdata").into_iter().map(|(k, v)| format!("{k}={v}")).collect();
            ensure_header(item, "Content-Type", "application/x-www-form-urlencoded");
            item.body = Some(encoded.join("&"));
        }
        Some("file") => {
            let src = text_of(body.pointer("/file/src"));
            if !src.is_empty() {
                item.body = Some(format!("< {src}"));
            }
        }
        _ => {}
    }
}

fn postman_auth(kind: &str, auth: Option<&Value>, item: &mut Item) {
    let field = |name: &str| -> String {
        auth.and_then(|a| a.get(kind))
            .and_then(Value::as_array)
            .and_then(|fields| {
                fields.iter().find(|f| f.get("key").and_then(Value::as_str) == Some(name))
            })
            .map(|f| text_of(f.get("value")))
            .unwrap_or_default()
    };
    match kind {
        "bearer" => ensure_header(item, "Authorization", &format!("Bearer {}", field("token"))),
        "basic" => {
            let pair = format!("{}:{}", field("username"), field("password"));
            ensure_header(
                item,
                "Authorization",
                &format!("Basic {}", crate::curl::base64(pair.as_bytes())),
            );
        }
        "noauth" | "inherit" => {}
        other => item.notes.push(format!("its {other} authentication wasn't imported")),
    }
}

fn ensure_header(item: &mut Item, name: &str, value: &str) {
    if !item.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
        item.headers.push((name.to_owned(), value.to_owned()));
    }
}

fn postman_environment(value: &Value) -> Imported {
    let name = text_of(value.get("name"));
    let values: serde_json::Map<String, Value> = value
        .get("values")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|v| v.get("enabled") != Some(&Value::Bool(false)))
        .filter_map(|v| {
            Some((v.get("key")?.as_str()?.to_owned(), Value::String(text_of(v.get("value")))))
        })
        .collect();
    let environment = if name.is_empty() { "imported".to_owned() } else { name.clone() };
    Imported {
        name,
        requests: String::new(),
        environments: Some(environments_text(&[(environment, values)])),
        count: 0,
        bodies: Vec::new(),
    }
}

/// Insomnia's `{{ _.name }}` (and `{{_.name}}`) as ours.
fn insomnia_vars(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open..].find("}}").map(|c| open + c) else { break };
        out.push_str(&rest[..open]);
        let inner = rest[open + 2..close].trim();
        let inner = inner.strip_prefix("_.").unwrap_or(inner);
        out.push_str(&format!("{{{{{inner}}}}}"));
        rest = &rest[close + 2..];
    }
    out.push_str(rest);
    out
}

fn insomnia(value: &Value) -> Imported {
    let resources: Vec<&Value> =
        value.get("resources").and_then(Value::as_array).into_iter().flatten().collect();
    let of_type = |kind: &'static str| {
        resources
            .iter()
            .copied()
            .filter(move |r| r.get("_type").and_then(Value::as_str) == Some(kind))
    };
    // By identity, once (not looked for again for each request).
    let by_id: std::collections::HashMap<&str, &Value> =
        resources.iter().filter_map(|r| Some((r.get("_id")?.as_str()?, *r))).collect();
    let kind_of = |id: &str| by_id.get(id).and_then(|r| r.get("_type")).and_then(Value::as_str);
    let name_of = |id: &str| -> Option<(String, String)> {
        let r = by_id.get(id)?;
        Some((text_of(r.get("name")), text_of(r.get("parentId"))))
    };
    let workspace = of_type("workspace").next().map(|w| text_of(w.get("name"))).unwrap_or_default();
    let mut items = Vec::new();
    for request in of_type("request") {
        // Its folders, outermost first.
        let mut path = Vec::new();
        let mut parent = text_of(request.get("parentId"));
        // A loop of groups (a crafted file) ends where it comes back.
        let mut seen = std::collections::HashSet::new();
        while let Some((name, next)) = name_of(&parent) {
            if kind_of(&parent) != Some("request_group") || !seen.insert(parent.clone()) {
                break;
            }
            path.insert(0, name);
            parent = next;
        }
        let mut item = Item {
            path,
            name: text_of(request.get("name")),
            method: text_of(request.get("method")).to_ascii_uppercase(),
            url: insomnia_vars(&text_of(request.get("url"))),
            ..Item::default()
        };
        for header in request.get("headers").and_then(Value::as_array).into_iter().flatten() {
            if header.get("disabled") == Some(&Value::Bool(true)) {
                continue;
            }
            item.headers.push((
                insomnia_vars(&text_of(header.get("name"))),
                insomnia_vars(&text_of(header.get("value"))),
            ));
        }
        if let Some(body) = request.get("body") {
            let mime = text_of(body.get("mimeType"));
            let text = text_of(body.get("text"));
            if !text.is_empty() {
                if !mime.is_empty() {
                    ensure_header(&mut item, "Content-Type", &mime);
                }
                item.body = Some(insomnia_vars(&text));
            } else if let Some(params) = body.get("params").and_then(Value::as_array) {
                let encoded: Vec<String> = params
                    .iter()
                    .map(|p| format!("{}={}", text_of(p.get("name")), text_of(p.get("value"))))
                    .collect();
                ensure_header(&mut item, "Content-Type", "application/x-www-form-urlencoded");
                item.body = Some(insomnia_vars(&encoded.join("&")));
            }
        }
        if let Some(kind) = request.pointer("/authentication/type").and_then(Value::as_str) {
            match kind {
                "bearer" => {
                    let token = insomnia_vars(&text_of(request.pointer("/authentication/token")));
                    ensure_header(&mut item, "Authorization", &format!("Bearer {token}"));
                }
                "basic" => {
                    let pair = format!(
                        "{}:{}",
                        text_of(request.pointer("/authentication/username")),
                        text_of(request.pointer("/authentication/password"))
                    );
                    ensure_header(
                        &mut item,
                        "Authorization",
                        &format!("Basic {}", crate::curl::base64(pair.as_bytes())),
                    );
                }
                "" | "none" => {}
                other => item.notes.push(format!("its {other} authentication wasn't imported")),
            }
        }
        items.push(item);
    }
    // The base environment's data shared; each sub-environment its own.
    let mut sets = Vec::new();
    for environment in of_type("environment") {
        let data = environment.get("data").and_then(Value::as_object).cloned().unwrap_or_default();
        let is_base = kind_of(&text_of(environment.get("parentId"))) == Some("workspace");
        let name = if is_base { "$shared".to_owned() } else { text_of(environment.get("name")) };
        if !data.is_empty() {
            sets.push((name, data));
        }
    }
    let environments = (!sets.is_empty()).then(|| environments_text(&sets));
    finish(workspace, &items, environments)
}

/// Bruno's collection: its `.bru` files (each a request: its name, the file's text), its
/// environments (`environments/<name>.bru`: name, text).
pub fn from_bruno(
    collection: &str,
    requests: &[(String, String)],
    environments: &[(String, String)],
) -> Imported {
    let mut items = Vec::new();
    for (path, text) in requests {
        let blocks = bru_blocks(text);
        let block = |name: &str| blocks.iter().find(|(n, _)| n == name).map(|(_, b)| b.as_str());
        let dict = |body: &str| -> Vec<(String, String)> {
            body.lines()
                .filter_map(|l| {
                    let l = l.trim();
                    if l.starts_with('~') {
                        return None;
                    }
                    let (k, v) = l.split_once(':')?;
                    Some((k.trim().to_owned(), v.trim().to_owned()))
                })
                .collect()
        };
        let meta = dict(block("meta").unwrap_or_default());
        let name =
            meta.iter().find(|(k, _)| k == "name").map(|(_, v)| v.clone()).unwrap_or_default();
        let Some((method, fields)) = ["get", "post", "put", "patch", "delete", "head", "options"]
            .iter()
            .find_map(|m| block(m).map(|b| (m.to_ascii_uppercase(), dict(b))))
        else {
            continue;
        };
        let url =
            fields.iter().find(|(k, _)| k == "url").map(|(_, v)| v.clone()).unwrap_or_default();
        let folders: Vec<String> = path
            .split('/')
            .rev()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(str::to_owned)
            .collect();
        let mut item = Item { path: folders, name, method, url, ..Item::default() };
        item.headers = block("headers").map(dict).unwrap_or_default();
        for (kind, mime) in [
            ("body:json", "application/json"),
            ("body:xml", "application/xml"),
            ("body:text", "text/plain"),
            ("body:graphql", "application/json"),
        ] {
            if let Some(body) = block(kind) {
                ensure_header(&mut item, "Content-Type", mime);
                item.body = Some(unindent(body));
            }
        }
        if let Some(form) = block("body:form-urlencoded") {
            let encoded: Vec<String> =
                dict(form).into_iter().map(|(k, v)| format!("{k}={v}")).collect();
            ensure_header(&mut item, "Content-Type", "application/x-www-form-urlencoded");
            item.body = Some(encoded.join("&"));
        }
        if let Some(bearer) = block("auth:bearer") {
            let token = dict(bearer)
                .into_iter()
                .find(|(k, _)| k == "token")
                .map(|(_, v)| v)
                .unwrap_or_default();
            ensure_header(&mut item, "Authorization", &format!("Bearer {token}"));
        }
        if blocks.iter().any(|(n, _)| n.starts_with("script:") || n == "tests") {
            item.notes.push("its Bruno scripts weren't imported".into());
        }
        items.push(item);
    }
    let sets: Vec<(String, serde_json::Map<String, Value>)> = environments
        .iter()
        .map(|(name, text)| {
            let blocks = bru_blocks(text);
            let vars = blocks
                .iter()
                .find(|(n, _)| n == "vars")
                .map(|(_, body)| {
                    body.lines()
                        .filter_map(|l| {
                            let (k, v) = l.trim().split_once(':')?;
                            (!k.trim().starts_with('~'))
                                .then(|| (k.trim().to_owned(), Value::String(v.trim().to_owned())))
                        })
                        .collect()
                })
                .unwrap_or_default();
            (name.clone(), vars)
        })
        .collect();
    let environments = (!sets.is_empty()).then(|| environments_text(&sets));
    finish(collection.to_owned(), &items, environments)
}

/// A `.bru` file's blocks: `name {` … `}` at the line's start, their bodies.
fn bru_blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        match &mut current {
            None => {
                if let Some(name) = line.trim_end().strip_suffix('{') {
                    current = Some((name.trim().to_owned(), String::new()));
                }
            }
            Some((name, body)) => {
                if line.trim_end() == "}" && !line.starts_with(' ') {
                    out.push((std::mem::take(name), std::mem::take(body)));
                    current = None;
                } else {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
    }
    out
}

/// A block's body with the indentation its lines share taken off.
fn unindent(body: &str) -> String {
    let indent = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    body.lines().map(|l| l.get(indent..).unwrap_or("").trim_end()).collect::<Vec<_>>().join("\n")
}

/// Environments as `http-client.env.json` holds them.
fn environments_text(sets: &[(String, serde_json::Map<String, Value>)]) -> String {
    let object: serde_json::Map<String, Value> =
        sets.iter().map(|(name, vars)| (name.clone(), Value::Object(vars.clone()))).collect();
    serde_json::to_string_pretty(&Value::Object(object)).unwrap_or_default() + "\n"
}

/// A name as `# @name` takes it (letters, digits, `_`).
fn request_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

use crate::draft::one_line;

/// A body the file would read lines of as its own (a separator, a file sent, a script).
fn reads_as_file(body: &str) -> bool {
    body.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("###") || line.starts_with('<') || line.starts_with('>')
    })
}

fn finish(name: String, items: &[Item], environments: Option<String>) -> Imported {
    let mut text = String::new();
    let mut names: Vec<String> = Vec::new();
    let mut bodies = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let mut title: Vec<&str> = item.path.iter().map(String::as_str).collect();
        title.push(&item.name);
        text.push_str(&format!("### {}\n", one_line(&title.join(" / "))));
        let mut id = request_name(item.id.as_deref().unwrap_or(&item.name));
        if !id.is_empty() {
            // The same name twice: the second numbered.
            let base = id.clone();
            let mut n = 2;
            while names.contains(&id) {
                id = format!("{base}_{n}");
                n += 1;
            }
            text.push_str(&format!("# @name {id}\n"));
            names.push(id);
        }
        for note in &item.notes {
            text.push_str(&format!("# Note: {}\n", one_line(note)));
        }
        let method = if item.method.is_empty() { "GET" } else { &item.method };
        text.push_str(&format!("{} {}\n", one_line(method), one_line(&item.url)));
        for (name, value) in &item.headers {
            text.push_str(&format!("{}: {}\n", one_line(name), one_line(value)));
        }
        if let Some(body) = item.body.as_deref().filter(|b| !b.trim().is_empty()) {
            text.push('\n');
            if reads_as_file(body) {
                let path = format!("bodies/{}.txt", index + 1);
                text.push_str(&format!("< ./{path}"));
                bodies.push((path, body.to_owned()));
            } else {
                text.push_str(body.trim_end());
            }
            text.push('\n');
        }
        text.push('\n');
    }
    Imported { name, requests: text, environments, count: items.len(), bodies }
}

#[cfg(test)]
#[path = "../tests/unit/import.rs"]
mod tests;
