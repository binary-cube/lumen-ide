//! `cargo xtask lsp-update [--check] [--major]`: the language servers' pinned versions
//! brought up to date (plan 46b), every month by `.github/workflows/language-servers.yml`.
//!
//! For each component of `packaging/lsp.toml` the newest release is looked up where it's
//! published: Node's newest **LTS** (nodejs.org's `index.json`, never a "Current" line),
//! static PHP's newest build of every platform's file (dl.static-php.dev's listing), the
//! Node servers' packages (registry.npmjs.org: up to the `latest` tag), the native servers
//! and phpactor (their GitHub releases: no drafts, no pre-releases, every file the pin uses
//! published). The servers a user installs (intelephense: `[language-servers.install]` in
//! the languages' descriptions) the same way, from the registry.
//!
//! Semver: a patch or minor release is taken; a major one (the first number, the second
//! below 1.0, a new Node LTS line; calendar versions like phpactor's never) only with
//! `--major`. What's written: the block's version and URLs (the old version replaced by the
//! new one), each changed URL's SHA-256 (GitHub's digest of the file, Node's
//! `SHASUMS256.txt`, otherwise the file downloaded and hashed), the Node servers'
//! `package.json` and their lockfile made again by npm (`--package-lock-only`, no scripts:
//! the registry's SHA-512 of every package pinned there), the installable servers'
//! version, integrity and size; then `lsp-manifest` makes the app's download manifest
//! again. `--check` says what would change and writes nothing.
//!
//! The choices are pure (tested with made-up listings); the network is behind [`Net`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// What the network answers (the real one: curl; the tests': made-up listings).
pub trait Net {
    /// The text at `url`.
    fn text(&self, url: &str) -> Result<String, String>;
    /// The SHA-256 (hex) of the file at `url`.
    fn sha256(&self, url: &str) -> Result<String, String>;
    /// The size of the file at `url`, in bytes.
    fn size(&self, url: &str) -> Result<u64, String>;
}

/// A version as numbers (`v26.13.0` → `[26, 13, 0]`); `None`: a pre-release, a build or
/// not a version.
pub fn parse(version: &str) -> Option<Vec<u64>> {
    let v = version.trim().trim_start_matches('v');
    if v.is_empty() || v.contains(['-', '+']) {
        return None;
    }
    v.split('.').map(|p| p.parse().ok()).collect()
}

/// Whether going from `old` to `new` is a major update: the first number changes (the
/// second below 1.0); a calendar version (a year first) never is.
pub fn is_major(old: &str, new: &str) -> bool {
    let (Some(a), Some(b)) = (parse(old), parse(new)) else { return true };
    let first = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    if first(&a, 0) >= 1000 {
        return false;
    }
    if first(&a, 0) == 0 && first(&b, 0) == 0 {
        return first(&a, 1) != first(&b, 1);
    }
    first(&a, 0) != first(&b, 0)
}

/// What `candidates` offer over `current`: the newest one allowed (`major`: any; else not a
/// major update), and the newest of all when it's a major one held back.
pub fn pick(current: &str, candidates: &[String], major: bool) -> (Option<String>, Option<String>) {
    pick_by(current, candidates, major, is_major)
}

/// Whether going from PHP `old` to `new` is a major update: a new minor line too (8.4 →
/// 8.5: what the PHP servers run on changes).
pub fn is_major_line(old: &str, new: &str) -> bool {
    let (Some(a), Some(b)) = (parse(old), parse(new)) else { return true };
    a.get(..2) != b.get(..2)
}

/// [`pick`] with `major` telling a major update.
pub fn pick_by(
    current: &str,
    candidates: &[String],
    major: bool,
    is_major: fn(&str, &str) -> bool,
) -> (Option<String>, Option<String>) {
    let Some(now) = parse(current) else { return (None, None) };
    let mut newer: Vec<(Vec<u64>, &String)> =
        candidates.iter().filter_map(|c| Some((parse(c)?, c))).filter(|(v, _)| *v > now).collect();
    newer.sort();
    let newest = newer.last().map(|(_, c)| (*c).clone());
    let allowed =
        newer.iter().rev().find(|(_, c)| major || !is_major(current, c)).map(|(_, c)| (*c).clone());
    let held = newest.filter(|n| Some(n) != allowed.as_ref());
    (allowed, held)
}

/// Node's LTS releases in nodejs.org's `index.json`.
pub fn node_versions(index: &Value) -> Vec<String> {
    index
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r.get("lts").is_some_and(|l| l.is_string() || l.as_bool() == Some(true)))
        .filter_map(|r| Some(r.get("version")?.as_str()?.trim_start_matches('v').to_owned()))
        .collect()
}

/// A package's stable versions in its registry document, up to its `latest` tag, not
/// deprecated.
pub fn npm_versions(packument: &Value) -> Vec<String> {
    let latest = packument.pointer("/dist-tags/latest").and_then(Value::as_str).and_then(parse);
    let Some(versions) = packument.get("versions").and_then(Value::as_object) else {
        return Vec::new();
    };
    versions
        .iter()
        .filter(|(_, meta)| meta.get("deprecated").is_none())
        .filter(|(v, _)| parse(v).is_some_and(|p| latest.as_ref().is_none_or(|l| p <= *l)))
        .map(|(v, _)| v.clone())
        .collect()
}

/// A release on GitHub: its version (the tag, without a leading `v`) and its files with
/// their SHA-256 when GitHub says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub tag: String,
    pub assets: BTreeMap<String, Option<String>>,
}

/// The published releases of a repository's `releases` list (drafts and pre-releases left).
pub fn github_releases(list: &Value) -> Vec<Release> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            r.get("draft").and_then(Value::as_bool) != Some(true)
                && r.get("prerelease").and_then(Value::as_bool) != Some(true)
        })
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?.to_owned();
            let assets = r
                .get("assets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|a| {
                    let name = a.get("name")?.as_str()?.to_owned();
                    let digest = a
                        .get("digest")
                        .and_then(Value::as_str)
                        .and_then(|d| d.strip_prefix("sha256:"))
                        .map(str::to_owned);
                    Some((name, digest))
                })
                .collect();
            Some(Release { version: tag.trim_start_matches('v').to_owned(), tag, assets })
        })
        .collect()
}

/// The file names a directory listing mentions (dl.static-php.dev's JSON or HTML): every
/// `php-<version>-cli-…` name.
pub fn listing_names(listing: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = listing;
    while let Some(at) = rest.find("php-") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
            .unwrap_or(tail.len());
        let name = &tail[..end];
        if name.contains("-cli-") {
            out.insert(name.to_owned());
        }
        rest = &tail[4..];
    }
    out
}

/// The versions every one of `files` (each a directory's file named with `current`) is
/// published at, as `names` (by directory) list them.
pub fn php_versions(
    files: &[(String, String)],
    current: &str,
    names: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<String> {
    let Some((dir, first)) = files.first() else { return Vec::new() };
    let Some((before, after)) = first.split_once(current) else { return Vec::new() };
    let found = names.get(dir).into_iter().flatten().filter_map(|n| {
        let v = n.strip_prefix(before)?.strip_suffix(after)?;
        parse(v).map(|_| v.to_owned())
    });
    found
        .filter(|v| {
            files.iter().all(|(dir, file)| {
                names.get(dir).is_some_and(|n| n.contains(&file.replace(current, v)))
            })
        })
        .collect()
}

/// A component's block of `packaging/lsp.toml`: from its header (`[node]`, `[php]`,
/// `[[servers]]`) to the next one, the comments before that one excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub range: std::ops::Range<usize>,
    /// `node`, `php`, or the server's name.
    pub name: String,
}

/// The components' blocks of `text`.
pub fn blocks(text: &str) -> Vec<Block> {
    let mut starts = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let l = line.trim();
        if l == "[node]" || l == "[php]" || l == "[[servers]]" {
            starts.push((at, l.to_owned()));
        }
        at += line.len();
    }
    let mut out = Vec::new();
    for (i, (start, header)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map_or(text.len(), |(s, _)| *s);
        // The comment lines just before the next header are that one's.
        let body = &text[*start..end];
        let mut cut = body.len();
        for line in body.split_inclusive('\n').rev() {
            let l = line.trim();
            if l.starts_with('#') || l.is_empty() {
                cut -= line.len();
            } else {
                break;
            }
        }
        let name = match header.as_str() {
            "[node]" => "node".to_owned(),
            "[php]" => "php".to_owned(),
            _ => value_of(body, "name").unwrap_or_default(),
        };
        out.push(Block { range: *start..start + cut, name });
    }
    out
}

/// The string value of the first `key = "…"` line of `block`.
pub fn value_of(block: &str, key: &str) -> Option<String> {
    block.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// `block` with `old` replaced by `new` on every line but the comments.
pub fn retarget(block: &str, old: &str, new: &str) -> String {
    block
        .split_inclusive('\n')
        .map(|line| {
            if line.trim_start().starts_with('#') {
                line.to_owned()
            } else {
                line.replace(old, new)
            }
        })
        .collect()
}

/// The `url = "…"` of `block` each with the `sha256 = "…"` line after it (before the next
/// URL), as (url, the sha256 line's byte range in the block).
pub fn pinned_urls(block: &str) -> Vec<(String, Option<std::ops::Range<usize>>)> {
    let mut out: Vec<(String, Option<std::ops::Range<usize>>)> = Vec::new();
    let mut at = 0;
    for line in block.split_inclusive('\n') {
        let l = line.trim();
        if let Some(url) = l.strip_prefix("url = ") {
            out.push((url.trim_matches('"').to_owned(), None));
        } else if l.starts_with("sha256 = ")
            && let Some(last) = out.last_mut()
            && last.1.is_none()
        {
            last.1 = Some(at..at + line.trim_end_matches('\n').len());
        }
        at += line.len();
    }
    out
}

/// What the update found for one component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub current: String,
    /// The version taken (`None`: none newer allowed).
    pub update: Option<String>,
    /// A newer major version left for `--major`.
    pub held: Option<String>,
}

impl Found {
    fn line(&self) -> String {
        let mut s = match &self.update {
            Some(new) => format!("{}: {} → {new}", self.name, self.current),
            None => format!("{}: {} (up to date)", self.name, self.current),
        };
        if let Some(held) = &self.held {
            s.push_str(&format!(", {held} is a major update: --major"));
        }
        s
    }
}

/// The GitHub repository (`owner/repo`) and tag of a release file's URL.
pub fn github_place(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("https://raw.githubusercontent.com/"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    let repo = format!("{}/{}", parts.first()?, parts.get(1)?);
    if parts.get(2) == Some(&"releases") && parts.get(3) == Some(&"download") {
        return Some((repo, (*parts.get(4)?).to_owned()));
    }
    None
}

/// The updated text of one component's block (`None`: nothing newer allowed), what was
/// found, and the npm packages to pin (`package.json` changes: name → version).
pub type BlockUpdate = (Option<String>, Found, BTreeMap<String, String>);

/// The update of `block` (`name` its component), the network asked through `net`.
pub fn update_block(
    root: &Path,
    name: &str,
    block: &str,
    net: &dyn Net,
    major: bool,
    check: bool,
) -> Result<BlockUpdate, String> {
    let current = value_of(block, "version").ok_or_else(|| format!("{name}: no version"))?;
    let urls = pinned_urls(block);
    let mut digests: BTreeMap<String, String> = BTreeMap::new();
    let mut packages = BTreeMap::new();
    let mut found =
        Found { name: name.to_owned(), current: current.clone(), update: None, held: None };
    let candidates: Vec<String> = if name == "node" {
        let index = net.text("https://nodejs.org/dist/index.json")?;
        node_versions(&serde_json::from_str(&index).map_err(|e| format!("node: {e}"))?)
    } else if name == "php" {
        let files: Vec<(String, String)> = urls
            .iter()
            .filter(|(u, _)| u.starts_with("https://dl.static-php.dev/"))
            .filter_map(|(u, _)| {
                let (dir, file) = u.rsplit_once('/')?;
                Some((format!("{dir}/"), file.to_owned()))
            })
            .collect();
        let mut names = BTreeMap::new();
        for dir in files.iter().map(|(d, _)| d).collect::<BTreeSet<_>>() {
            names.insert(dir.clone(), listing_names(&net.text(&format!("{dir}?format=json"))?));
        }
        php_versions(&files, &current, &names)
    } else if let Some(dir) = value_of(block, "packages") {
        // A Node server: its packages, each to its own newest allowed version; the
        // server's version is its main package's (named after it, or its scope).
        let manifest = std::fs::read_to_string(root.join(&dir).join("package.json"))
            .map_err(|e| format!("{dir}/package.json: {e}"))?;
        let manifest: Value =
            serde_json::from_str(&manifest).map_err(|e| format!("{dir}/package.json: {e}"))?;
        let deps = manifest.get("dependencies").and_then(Value::as_object).cloned();
        let mut main = None;
        for (package, pinned) in deps.into_iter().flatten() {
            let pinned = pinned.as_str().unwrap_or_default().to_owned();
            let document = net.text(&format!("https://registry.npmjs.org/{package}"))?;
            let document: Value =
                serde_json::from_str(&document).map_err(|e| format!("{package}: {e}"))?;
            let (allowed, held) = pick(&pinned, &npm_versions(&document), major);
            let is_main = package == name
                || package.strip_prefix('@').is_some_and(|p| p.starts_with(&format!("{name}/")));
            if is_main {
                main = Some((allowed.clone(), held.clone()));
            } else if let Some(held) = &held {
                println!("lsp-update: {name}: {package} {held} is a major update: --major");
            }
            if let Some(new) = allowed {
                if !is_main {
                    println!("lsp-update: {name}: {package} {pinned} → {new}");
                }
                packages.insert(package.clone(), new);
            }
        }
        let (update, held) = main.unwrap_or((None, None));
        found.held = held;
        if update.is_none() && packages.is_empty() {
            return Ok((None, found, packages));
        }
        found.update = update.clone();
        let text = match &update {
            Some(new) => retarget(block, &current, new),
            None => block.to_owned(),
        };
        return Ok((Some(text), found, packages));
    } else if let Some((repo, tag)) = urls.iter().find_map(|(u, _)| github_place(u)) {
        let list =
            net.text(&format!("https://api.github.com/repos/{repo}/releases?per_page=30"))?;
        let releases =
            github_releases(&serde_json::from_str(&list).map_err(|e| format!("{name}: {e}"))?);
        // Every file the pin takes from the release, published in the new one.
        let files: Vec<String> = urls
            .iter()
            .filter(|(u, _)| github_place(u).is_some_and(|(r, t)| r == repo && t == tag))
            .filter_map(|(u, _)| u.rsplit('/').next().map(str::to_owned))
            .collect();
        let mut usable = Vec::new();
        for r in releases {
            let all = files.iter().all(|f| r.assets.contains_key(&f.replace(&current, &r.version)));
            if all {
                for (file, digest) in &r.assets {
                    if let Some(d) = digest {
                        digests.insert(
                            format!("https://github.com/{repo}/releases/download/{}/{file}", r.tag),
                            d.clone(),
                        );
                    }
                }
                usable.push(r.version);
            }
        }
        usable
    } else {
        return Ok((None, found, packages));
    };
    let rule = if name == "php" { is_major_line } else { is_major };
    let (update, held) = pick_by(&current, &candidates, major, rule);
    found.held = held;
    let Some(new) = update else { return Ok((None, found, packages)) };
    found.update = Some(new.clone());
    let mut text = retarget(block, &current, &new);
    if check {
        // What would change is known: nothing downloaded to hash.
        return Ok((Some(text), found, packages));
    }
    // Node's checksums, published beside its files.
    let node_sums = if name == "node" {
        let sums = net.text(&format!("https://nodejs.org/dist/v{new}/SHASUMS256.txt"))?;
        sums.lines()
            .filter_map(|l| {
                let (sum, file) = l.split_once("  ")?;
                Some((file.trim().to_owned(), sum.trim().to_owned()))
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    // Each changed URL's SHA-256, from the last line backwards (the ranges stay right).
    let old_urls: BTreeSet<String> = urls.iter().map(|(u, _)| u.clone()).collect();
    let mut pins = pinned_urls(&text);
    pins.reverse();
    for (url, line) in pins {
        let Some(line) = line else { continue };
        if old_urls.contains(&url) {
            continue;
        }
        let file = url.rsplit('/').next().unwrap_or_default();
        let sum = match (digests.get(&url), node_sums.get(file)) {
            (Some(d), _) | (None, Some(d)) => d.clone(),
            (None, None) => net.sha256(&url)?,
        };
        text.replace_range(line, &format!("sha256 = \"{sum}\""));
    }
    Ok((Some(text), found, packages))
}

/// The `[language-servers.install]` npm pins of a description (`text`): its updated text
/// (`None`: nothing newer allowed) and what was found per package.
pub fn update_installs(
    text: &str,
    net: &dyn Net,
    major: bool,
    check: bool,
) -> Result<(Option<String>, Vec<Found>), String> {
    let mut out = text.to_owned();
    let mut found = Vec::new();
    let mut changed = false;
    let mut at = 0;
    while let Some(i) = out[at..].find("[language-servers.install]") {
        let start = at + i;
        let end = out[start + 1..].find("\n[").map_or(out.len(), |e| start + 1 + e);
        let block = out[start..end].to_owned();
        at = end;
        let (Some(package), Some(current)) = (value_of(&block, "npm"), value_of(&block, "version"))
        else {
            continue;
        };
        let document = net.text(&format!("https://registry.npmjs.org/{package}"))?;
        let document: Value =
            serde_json::from_str(&document).map_err(|e| format!("{package}: {e}"))?;
        let (update, held) = pick(&current, &npm_versions(&document), major);
        found.push(Found {
            name: package.clone(),
            current: current.clone(),
            update: update.clone(),
            held,
        });
        let Some(new) = update else { continue };
        let meta = document
            .pointer(&format!("/versions/{}/dist", new.replace('~', "~0").replace('/', "~1")));
        let integrity = meta
            .and_then(|m| m.get("integrity"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{package} {new}: no integrity"))?;
        let tarball = meta
            .and_then(|m| m.get("tarball"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{package} {new}: no tarball"))?;
        let size = if check { 0 } else { net.size(tarball)? };
        let mut lines: Vec<String> = Vec::new();
        for line in block.split_inclusive('\n') {
            let key = line.split_once('=').map(|(k, _)| k.trim());
            let nl = if line.ends_with('\n') { "\n" } else { "" };
            lines.push(match key {
                Some("version") => format!("version = \"{new}\"{nl}"),
                Some("integrity") => format!("integrity = \"{integrity}\"{nl}"),
                Some("size") => format!("size = {size}{nl}"),
                _ => line.to_owned(),
            });
        }
        let replaced = lines.concat();
        at = start + replaced.len();
        out.replace_range(start..end, &replaced);
        changed = true;
    }
    Ok((changed.then_some(out), found))
}

/// `package.json`'s text with the dependencies of `packages` pinned at their versions.
pub fn pin_packages(manifest: &str, packages: &BTreeMap<String, String>) -> String {
    let mut out = manifest.to_owned();
    for (package, version) in packages {
        let key = format!("\"{package}\": \"");
        if let Some(i) = out.find(&key) {
            let from = i + key.len();
            if let Some(len) = out[from..].find('"') {
                out.replace_range(from..from + len, version);
            }
        }
    }
    out
}

/// The update: what's found (and, unless `check`, written).
pub fn update(root: &Path, check: bool, major: bool) -> Result<String, String> {
    update_with(root, &Curl, check, major)
}

/// The update through `net` (the tests' made-up network: never written when `check`).
pub fn update_with(root: &Path, net: &dyn Net, check: bool, major: bool) -> Result<String, String> {
    let path = root.join("packaging/lsp.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("packaging/lsp.toml: {e}"))?;
    let mut out = text.clone();
    let mut report = Vec::new();
    let mut failed = Vec::new();
    let mut npm_dirs = Vec::new();
    // From the last block backwards: the earlier ranges stay right.
    for block in blocks(&text).into_iter().rev() {
        let body = &text[block.range.clone()];
        match update_block(root, &block.name, body, net, major, check) {
            Ok((new, found, packages)) => {
                report.push(found.line());
                if let Some(new) = new {
                    out.replace_range(block.range.clone(), &new);
                }
                if !packages.is_empty()
                    && let Some(dir) = value_of(body, "packages")
                {
                    npm_dirs.push((dir, packages));
                }
            }
            Err(e) => failed.push(format!("{}: {e}", block.name)),
        }
    }
    report.reverse();
    // The servers the user installs, in the languages' descriptions.
    let mut descriptions = Vec::new();
    let languages = root.join("crates/modules/languages");
    let mut dirs: Vec<_> = std::fs::read_dir(&languages)
        .map_err(|e| format!("{}: {e}", languages.display()))?
        .flatten()
        .map(|e| e.path().join("data/language-servers.toml"))
        .filter(|p| p.is_file())
        .collect();
    dirs.sort();
    for file in dirs {
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        if !text.contains("[language-servers.install]") {
            continue;
        }
        match update_installs(&text, net, major, check) {
            Ok((new, found)) => {
                report
                    .extend(found.iter().map(|f| format!("{} (installed by the user)", f.line())));
                if let Some(new) = new {
                    descriptions.push((file, new));
                }
            }
            Err(e) => failed.push(format!("{}: {e}", file.display())),
        }
    }
    let changed = out != text || !npm_dirs.is_empty() || !descriptions.is_empty();
    if !check && changed {
        std::fs::write(&path, &out).map_err(|e| format!("packaging/lsp.toml: {e}"))?;
        for (file, new) in &descriptions {
            std::fs::write(file, new).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        for (dir, packages) in &npm_dirs {
            let manifest = root.join(dir).join("package.json");
            let before = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
            std::fs::write(&manifest, pin_packages(&before, packages))
                .map_err(|e| e.to_string())?;
            lockfile(&root.join(dir))?;
        }
        report.push(crate::lsp_manifest::generate(root, false)?);
    }
    for f in &failed {
        report.push(format!("not updated: {f}"));
    }
    if check {
        report.push(if changed {
            "updates available (nothing written: --check)".into()
        } else {
            "everything up to date".into()
        });
    }
    Ok(report.join("\n"))
}

/// The lockfile of the Node server in `dir` made again from its `package.json`: every
/// package's registry URL and SHA-512 pinned, no script run.
fn lockfile(dir: &Path) -> Result<(), String> {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm)
        .args(["install", "--package-lock-only", "--ignore-scripts", "--no-audit", "--no-fund"])
        .current_dir(dir)
        .status()
        .map_err(|e| format!("npm (needed to make the lockfiles again): {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("npm failed in {}", dir.display())) }
}

/// The network, through curl (a GitHub token from `GITHUB_TOKEN` when there is one).
struct Curl;

impl Curl {
    fn get(url: &str) -> Result<Vec<u8>, String> {
        let mut command = Command::new("curl");
        command.args(["-sSfL", "--max-time", "600"]);
        if url.starts_with("https://api.github.com/") {
            command.args(["-H", "Accept: application/vnd.github+json"]);
            if let Ok(token) = std::env::var("GITHUB_TOKEN")
                && !token.is_empty()
            {
                command.args(["-H", &format!("Authorization: Bearer {token}")]);
            }
        }
        let output = command.arg(url).output().map_err(|e| format!("curl: {e}"))?;
        if !output.status.success() {
            return Err(format!("{url}: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(output.stdout)
    }
}

impl Net for Curl {
    fn text(&self, url: &str) -> Result<String, String> {
        String::from_utf8(Self::get(url)?).map_err(|e| format!("{url}: {e}"))
    }

    fn sha256(&self, url: &str) -> Result<String, String> {
        println!("lsp-update: hashing {url}");
        let bytes = Self::get(url)?;
        Ok(pinned_downloads::digest::sha256_hex(&bytes))
    }

    fn size(&self, url: &str) -> Result<u64, String> {
        crate::lsp_manifest::head_size(url)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lsp_update.rs"]
mod tests;
