//! The data sources' lists: the TOML adapter for [`DataSourceStore`] — a project's in
//! `<project dir>/data-sources.toml`, every project's in the config folder
//! (`config_dir()/data-sources.toml`). Each is written whole, atomically, and never with a
//! secret: no password field exists, and an option named like one (`password`, `token`…)
//! is left out. A file that can't be read as a list is an error, never written over.
//!
//! ```toml
//! [[source]]
//! id = "0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10"
//! name = "shop"
//! driver = "sqlite"
//! file = "data/shop.sqlite"     # relative to the project when inside it
//! secrets = "never"             # never | session | project-vault | keychain
//!
//! [[source]]
//! id = "5d1c0f8e-2b7a-4e3c-8f41-9a6b2c1d0e77"
//! name = "prod"
//! driver = "postgres"
//! host = "db.example.com"
//! port = 5432
//! user = "app"
//! database = "shop"
//! tls = "verify"                # off | verify | trust
//! read_only = true
//! comment = "the shop's live database"
//! schemas = ["public", "billing"]   # those shown (none: all)
//! databases = ["shop", "analytics"]  # a server's databases shown (none: all)
//! options = { sslmode = "require" }
//! tags = [{ name = "prod", color = "#e5484d" }]
//! relations = "both"            # real | inferred | both: what its diagram shows
//! folder = "9e2d4c1a-7b3f-4a5e-8c6d-1f0e2b3a4c5d"   # the folder it's in (none: the top)
//! color = "red"                 # its own (none: its folder's); a name or "#rrggbb"
//! production = true             # its writes confirmed (also when its folder is)
//!
//! # A key the engine doesn't keep, accepted in the relations diagram (and one set aside).
//! [[source.virtual_key]]
//! table = "reviews"             # `schema` / `target_schema` when there are schemas
//! columns = ["product_id"]
//! target = "products"
//! references = ["id"]
//!
//! [[source.ignored_relation]]
//! table = "payments"
//! columns = ["paid_by"]
//! target = "users"
//! references = ["id"]
//!
//! # The list's folders (a source names the one it's in; a folder its parent).
//! [[folder]]
//! id = "9e2d4c1a-7b3f-4a5e-8c6d-1f0e2b3a4c5d"
//! name = "Production"
//! color = "red"                 # red | orange | yellow | green | teal | blue | purple | gray | "#rrggbb"
//! production = true             # every source in it (and its subfolders) is production
//!
//! [[folder]]
//! id = "3a1b5c7d-9e0f-4a2b-8c4d-6e8f0a1b2c3d"
//! name = "EU"
//! parent = "9e2d4c1a-7b3f-4a5e-8c6d-1f0e2b3a4c5d"
//! ```
//!
//! A file from before folders reads as it did (no folder, no color).

use std::path::{Path, PathBuf};

use database::folders::{FolderId, SourceColor, SourceFolder, SourceList};
use database::relations::{RelationKinds, VirtualKey};
use database::{
    DataSource, DataSourceId, DataSourceStore, Scope, SecretPolicy, TableRef, Tag, Tls,
};
use toml::Value;

use crate::write_atomic;

/// The file's name, in either folder.
pub const FILE: &str = "data-sources.toml";

/// The two lists' files (either missing: that list is always empty and never written).
#[derive(Debug, Clone, Default)]
pub struct TomlDataSources {
    project: Option<PathBuf>,
    global: Option<PathBuf>,
}

impl TomlDataSources {
    /// The project at `root`'s list, and every project's in the user's config folder.
    pub fn new(root: &Path) -> Self {
        Self::at(Some(root), crate::config_dir())
    }

    /// The lists of the project at `root` (none: no project) and of the config folder
    /// `config` (the tests give a temporary one).
    pub fn at(root: Option<&Path>, config: Option<PathBuf>) -> Self {
        Self {
            project: root.map(|r| r.join(kernel::brand::PROJECT_DIR).join(FILE)),
            global: config.map(|c| c.join(FILE)),
        }
    }

    /// Where `scope`'s list is kept.
    pub fn file(&self, scope: Scope) -> Option<&Path> {
        match scope {
            Scope::Project => self.project.as_deref(),
            Scope::Global => self.global.as_deref(),
        }
    }
}

fn policy_name(policy: SecretPolicy) -> &'static str {
    match policy {
        SecretPolicy::Never => "never",
        SecretPolicy::Session => "session",
        SecretPolicy::ProjectVault => "project-vault",
        SecretPolicy::Keychain => "keychain",
    }
}

fn policy_of(name: &str) -> Option<SecretPolicy> {
    Some(match name {
        "never" => SecretPolicy::Never,
        "session" => SecretPolicy::Session,
        "project-vault" => SecretPolicy::ProjectVault,
        "keychain" => SecretPolicy::Keychain,
        _ => return None,
    })
}

fn tls_name(tls: Tls) -> &'static str {
    match tls {
        Tls::Off => "off",
        Tls::Verify => "verify",
        Tls::Trust => "trust",
    }
}

fn tls_of(name: &str) -> Option<Tls> {
    Some(match name {
        "off" => Tls::Off,
        "verify" => Tls::Verify,
        "trust" => Tls::Trust,
        _ => return None,
    })
}

/// Entry `i` of the list read back; why not, if it can't be (the whole file is then left
/// alone: dropping an entry would lose it at the next save).
fn source_of(table: &toml::Table, scope: Scope, i: usize) -> Result<DataSource, String> {
    let text = |key: &str| table.get(key).and_then(Value::as_str);
    let id = text("id")
        .and_then(DataSourceId::parse)
        .ok_or_else(|| format!("source {}: no valid `id`", i + 1))?;
    let driver = text("driver")
        .filter(|d| !d.trim().is_empty())
        .ok_or_else(|| format!("source {}: no `driver`", i + 1))?;
    let mut source = DataSource::new(text("name").unwrap_or_default(), driver, scope);
    source.id = id;
    source.read_only = table.get("read_only").and_then(Value::as_bool).unwrap_or(false);
    source.production = table.get("production").and_then(Value::as_bool).unwrap_or(false);
    source.file = text("file").filter(|f| !f.is_empty()).map(PathBuf::from);
    source.comment = text("comment").unwrap_or_default().to_owned();
    if let Some(schemas) = table.get("schemas").and_then(Value::as_array) {
        source.schemas = schemas.iter().filter_map(Value::as_str).map(str::to_owned).collect();
    }
    if let Some(databases) = table.get("databases").and_then(Value::as_array) {
        source.databases = databases.iter().filter_map(Value::as_str).map(str::to_owned).collect();
    }
    if let Some(policy) = text("secrets") {
        source.secrets =
            policy_of(policy).ok_or_else(|| format!("source {}: unknown `secrets`", i + 1))?;
    }
    let target = &mut source.target;
    target.host = text("host").unwrap_or_default().to_owned();
    target.user = text("user").unwrap_or_default().to_owned();
    target.database = text("database").unwrap_or_default().to_owned();
    target.port = table
        .get("port")
        .and_then(Value::as_integer)
        .map(|p| u16::try_from(p).map_err(|_| format!("source {}: invalid `port`", i + 1)))
        .transpose()?;
    if let Some(tls) = text("tls") {
        target.tls = tls_of(tls).ok_or_else(|| format!("source {}: unknown `tls`", i + 1))?;
    }
    if let Some(options) = table.get("options").and_then(Value::as_table) {
        for (key, value) in options {
            if database::source::is_secret_option(key) {
                continue;
            }
            let value = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            target.options.insert(key.clone(), value);
        }
    }
    if let Some(tags) = table.get("tags").and_then(Value::as_array) {
        for tag in tags.iter().filter_map(Value::as_table) {
            let Some(name) = tag.get("name").and_then(Value::as_str) else { continue };
            let color = tag
                .get("color")
                .and_then(Value::as_str)
                .and_then(kernel::Rgba::from_hex)
                .unwrap_or(kernel::Rgba::WHITE);
            source.tags.push(Tag { name: name.to_owned(), color });
        }
    }
    if let Some(kinds) = text("relations") {
        source.relations.show = RelationKinds::parse(kinds)
            .ok_or_else(|| format!("source {}: unknown `relations`", i + 1))?;
    }
    source.relations.virtual_keys = keys_of(table, "virtual_key", i)?;
    source.relations.ignored = keys_of(table, "ignored_relation", i)?;
    if let Some(folder) = text("folder") {
        source.folder = Some(
            FolderId::parse(folder).ok_or_else(|| format!("source {}: invalid `folder`", i + 1))?,
        );
    }
    if let Some(color) = text("color") {
        source.color = Some(
            SourceColor::parse(color)
                .ok_or_else(|| format!("source {}: unknown `color`", i + 1))?,
        );
    }
    Ok(source)
}

/// Folder entry `i` read back; why not, if it can't be.
fn folder_of(table: &toml::Table, scope: Scope, i: usize) -> Result<SourceFolder, String> {
    let text = |key: &str| table.get(key).and_then(Value::as_str);
    let id = text("id")
        .and_then(FolderId::parse)
        .ok_or_else(|| format!("folder {}: no valid `id`", i + 1))?;
    let mut folder = SourceFolder::new(text("name").unwrap_or_default(), scope);
    folder.id = id;
    folder.production = table.get("production").and_then(Value::as_bool).unwrap_or(false);
    if let Some(parent) = text("parent") {
        folder.parent = Some(
            FolderId::parse(parent).ok_or_else(|| format!("folder {}: invalid `parent`", i + 1))?,
        );
    }
    if let Some(color) = text("color") {
        folder.color = Some(
            SourceColor::parse(color)
                .ok_or_else(|| format!("folder {}: unknown `color`", i + 1))?,
        );
    }
    Ok(folder)
}

/// A folder as written.
fn folder_table(folder: &SourceFolder) -> Value {
    let mut t = toml::Table::new();
    t.insert("id".into(), Value::String(folder.id.to_string()));
    t.insert("name".into(), Value::String(folder.name.clone()));
    if let Some(parent) = folder.parent {
        t.insert("parent".into(), Value::String(parent.to_string()));
    }
    if let Some(color) = folder.color {
        t.insert("color".into(), Value::String(color.name()));
    }
    if folder.production {
        t.insert("production".into(), Value::Boolean(true));
    }
    Value::Table(t)
}

/// The keys listed under `name` (none: empty); why not, if one can't be read.
fn keys_of(table: &toml::Table, name: &str, i: usize) -> Result<Vec<VirtualKey>, String> {
    let Some(list) = table.get(name) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| format!("source {}: `{name}` isn't a list", i + 1))?;
    list.iter()
        .map(|entry| {
            let entry = entry
                .as_table()
                .ok_or_else(|| format!("source {}: `{name}` isn't a table", i + 1))?;
            let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
            let names = |key: &str| -> Vec<String> {
                entry
                    .get(key)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_default()
            };
            let table_of = |schema: &str, name: &str| -> Result<TableRef, String> {
                let name = text(name)
                    .filter(|n| !n.is_empty())
                    .ok_or_else(|| format!("source {}: a `{name}` without its table", i + 1))?;
                Ok(TableRef { schema: text(schema), name })
            };
            let key = VirtualKey {
                table: table_of("schema", "table")?,
                columns: names("columns"),
                target: table_of("target_schema", "target")?,
                references: names("references"),
            };
            if key.columns.is_empty() || key.columns.len() != key.references.len() {
                return Err(format!("source {}: a `{name}` whose columns don't match", i + 1));
            }
            Ok(key)
        })
        .collect()
}

/// A key as written.
fn key_table(key: &VirtualKey) -> Value {
    let mut t = toml::Table::new();
    if let Some(schema) = &key.table.schema {
        t.insert("schema".into(), Value::String(schema.clone()));
    }
    t.insert("table".into(), Value::String(key.table.name.clone()));
    let list =
        |names: &[String]| Value::Array(names.iter().map(|n| Value::String(n.clone())).collect());
    t.insert("columns".into(), list(&key.columns));
    if let Some(schema) = &key.target.schema {
        t.insert("target_schema".into(), Value::String(schema.clone()));
    }
    t.insert("target".into(), Value::String(key.target.name.clone()));
    t.insert("references".into(), list(&key.references));
    Value::Table(t)
}

/// A source as written (nothing empty, never a secret).
fn table_of(source: &DataSource) -> toml::Table {
    let mut t = toml::Table::new();
    let mut put = |key: &str, value: &str| {
        if !value.is_empty() {
            t.insert(key.into(), Value::String(value.to_owned()));
        }
    };
    put("id", &source.id.to_string());
    put("name", &source.name);
    put("driver", &source.driver);
    if let Some(file) = &source.file {
        // `/` between folders on every system (the project's file moves with it).
        let parts: Vec<String> =
            file.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let text =
            if file.is_absolute() { file.to_string_lossy().into_owned() } else { parts.join("/") };
        put("file", &text);
    }
    put("host", &source.target.host);
    put("user", &source.target.user);
    put("database", &source.target.database);
    put("secrets", policy_name(source.secrets));
    put("comment", &source.comment);
    if !source.schemas.is_empty() {
        let schemas = source.schemas.iter().map(|s| Value::String(s.clone())).collect();
        t.insert("schemas".into(), Value::Array(schemas));
    }
    if !source.databases.is_empty() {
        let databases = source.databases.iter().map(|s| Value::String(s.clone())).collect();
        t.insert("databases".into(), Value::Array(databases));
    }
    if let Some(port) = source.target.port {
        t.insert("port".into(), Value::Integer(i64::from(port)));
    }
    if source.target.tls != Tls::Off {
        t.insert("tls".into(), Value::String(tls_name(source.target.tls).into()));
    }
    if source.read_only {
        t.insert("read_only".into(), Value::Boolean(true));
    }
    let options: toml::Table = source
        .target
        .options
        .iter()
        .filter(|(k, _)| !database::source::is_secret_option(k))
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    if !options.is_empty() {
        t.insert("options".into(), Value::Table(options));
    }
    if !source.tags.is_empty() {
        let tags = source
            .tags
            .iter()
            .map(|tag| {
                let mut entry = toml::Table::new();
                entry.insert("name".into(), Value::String(tag.name.clone()));
                entry.insert("color".into(), Value::String(tag.color.to_hex()));
                Value::Table(entry)
            })
            .collect();
        t.insert("tags".into(), Value::Array(tags));
    }
    let relations = &source.relations;
    if relations.show != RelationKinds::Both {
        t.insert("relations".into(), Value::String(relations.show.name().into()));
    }
    if !relations.virtual_keys.is_empty() {
        t.insert(
            "virtual_key".into(),
            Value::Array(relations.virtual_keys.iter().map(key_table).collect()),
        );
    }
    if !relations.ignored.is_empty() {
        t.insert(
            "ignored_relation".into(),
            Value::Array(relations.ignored.iter().map(key_table).collect()),
        );
    }
    if let Some(folder) = source.folder {
        t.insert("folder".into(), Value::String(folder.to_string()));
    }
    if let Some(color) = source.color {
        t.insert("color".into(), Value::String(color.name()));
    }
    if source.production {
        t.insert("production".into(), Value::Boolean(true));
    }
    t
}

impl DataSourceStore for TomlDataSources {
    fn load(&self, scope: Scope) -> Result<SourceList, String> {
        let Some(file) = self.file(scope) else { return Ok(SourceList::default()) };
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SourceList::default());
            }
            Err(e) => return Err(e.to_string()),
        };
        let table: toml::Table =
            text.parse().map_err(|e: toml::de::Error| e.message().to_owned())?;
        let entries = |name: &str| -> Result<Vec<&toml::Table>, String> {
            let Some(list) = table.get(name) else { return Ok(Vec::new()) };
            let list = list.as_array().ok_or_else(|| format!("`{name}` isn't a list"))?;
            list.iter()
                .enumerate()
                .map(|(i, e)| e.as_table().ok_or_else(|| format!("{name} {}: not a table", i + 1)))
                .collect()
        };
        let sources = entries("source")?
            .into_iter()
            .enumerate()
            .map(|(i, entry)| source_of(entry, scope, i))
            .collect::<Result<Vec<_>, _>>()?;
        let folders = entries("folder")?
            .into_iter()
            .enumerate()
            .map(|(i, entry)| folder_of(entry, scope, i))
            .collect::<Result<Vec<_>, _>>()?;
        let mut list = SourceList::new(sources, folders);
        // A folder named that isn't there (a file edited by hand): its sources at the top.
        database::folders::repair(&mut list);
        Ok(list)
    }

    fn save(&self, scope: Scope, list: &SourceList) -> Result<(), String> {
        let Some(file) = self.file(scope) else { return Ok(()) };
        let sources = list.sources.iter().map(|s| Value::Table(table_of(s))).collect();
        let mut root = toml::Table::new();
        root.insert("source".into(), Value::Array(sources));
        if !list.folders.is_empty() {
            root.insert(
                "folder".into(),
                Value::Array(list.folders.iter().map(folder_table).collect()),
            );
        }
        write_atomic(file, &root.to_string()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[path = "../tests/unit/data_sources.rs"]
mod tests;
