//! `cargo xtask lsp-manifest [--check]`: the manifest of the language servers the app
//! downloads when they're first needed (`crates/modules/tools/lsp/data/downloads.toml`,
//! inside the app), made from `packaging/lsp.toml` and the lockfiles beside it — the same
//! pins `lsp-bundle` builds the full bundle from:
//!
//! - Node and PHP: each platform's archive (URL, SHA-256), its top folder left out, only
//!   their program and licence kept;
//! - the Node servers: every package of their lockfile (its place in `node_modules`, its
//!   registry URL and SHA-512 integrity; development ones left out), their `prune`, data
//!   files and inlined schemas;
//! - the native servers: each platform's release file, their top folder, `prune`,
//!   `universal`; `strip-debug` can't be done where they're downloaded (no `strip` there):
//!   left out, said in the manifest's notes.
//!
//! Each download's size comes from a HEAD request the first time (curl), then from the
//! manifest itself: `--check` (and the tests) make it again without the network and say
//! whether it's up to date.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use toml::{Table, Value};

/// Where the manifest goes.
pub const OUT: &str = "crates/modules/tools/lsp/data/downloads.toml";

/// The manifest's format (the app's `lsp_client::downloads::FORMAT`).
const FORMAT: i64 = 1;

/// Makes the manifest (`check`: compares it with the one there instead, without the
/// network). Returns what was done.
pub fn generate(root: &Path, check: bool) -> Result<String, String> {
    let text = std::fs::read_to_string(root.join("packaging/lsp.toml"))
        .map_err(|e| format!("packaging/lsp.toml: {e}"))?;
    let packaging: Table = toml::from_str(&text).map_err(|e| format!("packaging/lsp.toml: {e}"))?;
    let out = root.join(OUT);
    let before = std::fs::read_to_string(&out).unwrap_or_default();
    let mut sizes = known_sizes(&before);
    let made = make(root, &packaging, &mut sizes, !check)?;
    if check {
        return if made == before {
            Ok(format!("{OUT} is up to date"))
        } else {
            Err(format!("{OUT} is out of date: run `cargo xtask lsp-manifest`"))
        };
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&out, &made).map_err(|e| format!("{OUT}: {e}"))?;
    Ok(format!("{OUT}: {} components, {} bytes", made.matches("[[component]]").count(), made.len()))
}

/// The sizes the manifest already says, by URL.
fn known_sizes(manifest: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let Ok(table) = toml::from_str::<Table>(manifest) else { return out };
    fn walk(value: &Value, out: &mut BTreeMap<String, u64>) {
        match value {
            Value::Table(t) => {
                if let (Some(Value::String(url)), Some(Value::Integer(size))) =
                    (t.get("url"), t.get("size"))
                {
                    out.insert(url.clone(), *size as u64);
                }
                t.values().for_each(|v| walk(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    walk(&Value::Table(table), &mut out);
    out
}

fn text<'a>(table: &'a Table, key: &str) -> Result<&'a str, String> {
    table.get(key).and_then(Value::as_str).ok_or_else(|| format!("packaging/lsp.toml: no `{key}`"))
}

fn strings(table: &Table, key: &str) -> Vec<Value> {
    table
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|v| v.is_str()).cloned().collect())
        .unwrap_or_default()
}

/// The manifest's text. Sizes not known yet are asked for when `network`, else an error.
fn make(
    root: &Path,
    packaging: &Table,
    sizes: &mut BTreeMap<String, u64>,
    network: bool,
) -> Result<String, String> {
    // Every URL first, their sizes asked for together.
    let mut components: Vec<Table> = Vec::new();
    let node = packaging.get("node").and_then(Value::as_table).ok_or("no [node]")?;
    components.push(runtime(node, "node")?);
    if let Some(php) = packaging.get("php").and_then(Value::as_table) {
        components.push(runtime(php, "php")?);
    }
    for server in packaging.get("servers").and_then(Value::as_array).into_iter().flatten() {
        let server = server.as_table().ok_or("a bad [[servers]]")?;
        components.push(server_component(root, server)?);
    }
    let mut urls = Vec::new();
    collect_urls(&Value::Array(components.iter().cloned().map(Value::Table).collect()), &mut urls);
    let missing: Vec<String> = urls.into_iter().filter(|u| !sizes.contains_key(u)).collect();
    if !missing.is_empty() {
        if !network {
            return Err(format!(
                "{} sizes not known ({}…): run `cargo xtask lsp-manifest`",
                missing.len(),
                missing[0]
            ));
        }
        for (url, size) in head_sizes(&missing)? {
            sizes.insert(url, size);
        }
    }
    let mut list = Vec::new();
    for mut component in components {
        set_sizes(&mut component, sizes);
        list.push(Value::Table(component));
    }
    let mut manifest = Table::new();
    manifest.insert("format".into(), Value::Integer(FORMAT));
    manifest.insert("component".into(), Value::Array(list));
    let body = toml::to_string(&manifest).map_err(|e| e.to_string())?;
    Ok(format!(
        "# Made by `cargo xtask lsp-manifest` from packaging/lsp.toml and the lockfiles beside it:\n\
         # don't edit. What the app downloads when a language's server is first needed (each\n\
         # source pinned: SHA-256, or npm's SHA-512 integrity); `size`: bytes downloaded.\n\
         # Not done where it's downloaded: `strip-debug` (Linux's clangd and ZLS keep their debug\n\
         # information there).\n\n{body}"
    ))
}

fn collect_urls(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Table(t) => {
            if let Some(Value::String(url)) = t.get("url")
                && !out.contains(url)
            {
                out.push(url.clone());
            }
            t.values().for_each(|v| collect_urls(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_urls(v, out)),
        _ => {}
    }
}

fn set_sizes(table: &mut Table, sizes: &BTreeMap<String, u64>) {
    if let Some(Value::String(url)) = table.get("url").cloned()
        && let Some(size) = sizes.get(&url)
    {
        table.insert("size".into(), Value::Integer(*size as i64));
    }
    for (_, value) in table.iter_mut() {
        match value {
            Value::Table(t) => set_sizes(t, sizes),
            Value::Array(items) => {
                for item in items {
                    if let Value::Table(t) = item {
                        set_sizes(t, sizes);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Each URL's size, from a HEAD request (redirects followed: GitHub's releases), eight at a
/// time.
fn head_sizes(urls: &[String]) -> Result<Vec<(String, u64)>, String> {
    println!("lsp-manifest: asking {} sizes", urls.len());
    let mut out = Vec::new();
    for chunk in urls.chunks(8) {
        let found: Vec<Result<(String, u64), String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|url| scope.spawn(move || head_size(url).map(|size| (url.clone(), size))))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Err("a thread".into())))
                .collect()
        });
        for f in found {
            out.push(f?);
        }
    }
    Ok(out)
}

/// The size of the file at `url` (its `Content-Length` after the redirects).
pub(crate) fn head_size(url: &str) -> Result<u64, String> {
    let headers = |args: &[&str]| -> Result<String, String> {
        let output = Command::new("curl")
            .args(["-sSL", "--max-time", "60"])
            .args(args)
            .arg(url)
            .output()
            .map_err(|e| format!("curl: {e}"))?;
        if !output.status.success() {
            return Err(format!("{url}: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    // The last response's (after the redirects) header `name`.
    let last = |text: &str, name: &str| {
        text.lines()
            .filter_map(|l| {
                let (key, value) = l.split_once(':')?;
                key.trim().eq_ignore_ascii_case(name).then(|| value.trim().to_owned())
            })
            .next_back()
    };
    if let Some(length) = last(&headers(&["-I"])?, "content-length").and_then(|v| v.parse().ok()) {
        return Ok(length);
    }
    // A cache that doesn't say it on a HEAD (npm's): its first byte asked for, the whole
    // size in the range answered.
    let ranged =
        headers(&["-r", "0-0", "-D", "-", "-o", if cfg!(windows) { "NUL" } else { "/dev/null" }])?;
    last(&ranged, "content-range")
        .and_then(|r| r.rsplit('/').next().and_then(|t| t.parse().ok()))
        .ok_or_else(|| format!("{url}: no length"))
}

/// The file name at the end of `url`, without its archive's extensions.
fn stem(url: &str) -> &str {
    let file = url.rsplit('/').next().unwrap_or(url);
    for ext in [".tar.xz", ".tar.gz", ".tgz", ".zip"] {
        if let Some(stem) = file.strip_suffix(ext) {
            return stem;
        }
    }
    file
}

/// Node (`node`: `bin/node`, `node.exe` on Windows, under the archive's top folder) or PHP
/// (`php`, `php.exe`, at the archive's top): their program and licence only.
fn runtime(table: &Table, role: &str) -> Result<Table, String> {
    let version = text(table, "version")?;
    let mut c = Table::new();
    c.insert("name".into(), role.into());
    c.insert("role".into(), role.into());
    c.insert("version".into(), version.into());
    c.insert("license".into(), table.get("license").cloned().unwrap_or_else(|| "".into()));
    let mut platforms = Table::new();
    let entries = table.get("platforms").and_then(Value::as_table).ok_or("no platforms")?;
    for (platform, entry) in entries {
        let entry = entry.as_table().ok_or("a bad platform")?;
        let url = text(entry, "url")?;
        let windows = platform.starts_with("windows");
        let mut a = Table::new();
        a.insert("url".into(), url.into());
        a.insert("sha256".into(), text(entry, "sha256")?.into());
        let bin = match (role, windows) {
            ("node", true) => "node.exe",
            ("node", false) => "bin/node",
            (_, true) => "php.exe",
            (_, false) => "php",
        };
        let mut keep = vec![Value::from(bin)];
        if role == "node" {
            // Node's archives: everything under `node-v<version>-<platform>/`.
            a.insert("strip".into(), stem(url).into());
            keep.push("LICENSE".into());
        }
        a.insert("keep".into(), Value::Array(keep));
        a.insert("bin".into(), bin.into());
        platforms.insert(platform.clone(), Value::Table(a));
    }
    c.insert("platforms".into(), Value::Table(platforms));
    if let Some(files) = data_files(table)? {
        c.insert("files".into(), files);
    }
    Ok(c)
}

fn data_files(table: &Table) -> Result<Option<Value>, String> {
    let Some(files) = table.get("files").and_then(Value::as_array) else { return Ok(None) };
    let mut out = Vec::new();
    for file in files {
        let file = file.as_table().ok_or("a bad [[files]]")?;
        let mut f = Table::new();
        for key in ["url", "sha256", "path"] {
            f.insert(key.into(), text(file, key)?.into());
        }
        out.push(Value::Table(f));
    }
    Ok(Some(Value::Array(out)))
}

/// A server: its lockfile's packages, or its release files per platform, or only its files.
fn server_component(root: &Path, server: &Table) -> Result<Table, String> {
    let name = text(server, "name")?;
    let mut c = Table::new();
    c.insert("name".into(), name.into());
    c.insert("role".into(), "server".into());
    c.insert("version".into(), text(server, "version")?.into());
    c.insert("license".into(), server.get("license").cloned().unwrap_or_else(|| "".into()));
    if let Some(entries) = server.get("platforms").and_then(Value::as_table) {
        let mut platforms = Table::new();
        for (platform, entry) in entries {
            let entry = entry.as_table().ok_or("a bad platform")?;
            let mut a = Table::new();
            a.insert("url".into(), text(entry, "url")?.into());
            a.insert("sha256".into(), text(entry, "sha256")?.into());
            if let Some(top) = server.get("strip").and_then(Value::as_str) {
                a.insert("strip".into(), top.into());
            }
            if let Some(program) = entry.get("program").and_then(Value::as_str) {
                a.insert("program".into(), program.into());
            }
            platforms.insert(platform.clone(), Value::Table(a));
        }
        c.insert("platforms".into(), Value::Table(platforms));
    }
    if let Some(packages) = server.get("packages").and_then(Value::as_str) {
        c.insert("packages".into(), Value::Array(lockfile_packages(root, name, packages)?));
    }
    if let Some(files) = data_files(server)? {
        c.insert("files".into(), files);
    }
    for key in ["prune", "universal"] {
        let list = strings(server, key);
        if !list.is_empty() {
            c.insert(key.into(), Value::Array(list));
        }
    }
    if let Some(inline) = server.get("inline") {
        c.insert("inline".into(), inline.clone());
    }
    Ok(c)
}

/// A Node server's packages, from its lockfile: those `npm ci --omit=dev` would install, in
/// their places.
fn lockfile_packages(root: &Path, name: &str, dir: &str) -> Result<Vec<Value>, String> {
    let path = root.join(dir).join("package-lock.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}'s lockfile: {e}"))?;
    let lock: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{name}'s lockfile: {e}"))?;
    let packages = lock
        .get("packages")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("{name}'s lockfile has no packages (lockfile version 2 or 3)"))?;
    let mut out = Vec::new();
    for (place, package) in packages {
        let flag = |key: &str| package.get(key).and_then(serde_json::Value::as_bool) == Some(true);
        if place.is_empty() || flag("dev") || flag("link") || flag("inBundle") {
            continue;
        }
        let field = |key: &str| package.get(key).and_then(serde_json::Value::as_str);
        let (Some(url), Some(integrity)) = (field("resolved"), field("integrity")) else {
            return Err(format!("{name}: {place} has no resolved URL or integrity"));
        };
        if !url.starts_with("https://registry.npmjs.org/") || !integrity.starts_with("sha512-") {
            return Err(format!("{name}: {place} isn't from registry.npmjs.org with a SHA-512"));
        }
        let mut p = Table::new();
        p.insert("path".into(), place.as_str().into());
        p.insert("url".into(), url.into());
        p.insert("integrity".into(), integrity.into());
        for key in ["os", "cpu"] {
            if let Some(list) = package.get(key).and_then(serde_json::Value::as_array) {
                let list: Vec<Value> =
                    list.iter().filter_map(serde_json::Value::as_str).map(Value::from).collect();
                p.insert(key.into(), Value::Array(list));
            }
        }
        out.push(Value::Table(p));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../tests/unit/lsp_manifest.rs"]
mod tests;
