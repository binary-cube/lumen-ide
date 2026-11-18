//! The "Database" window's tree, as a model (no drawing, no I/O): the sources, then what
//! each holds as it's read — source › schema › tables / views › table › columns /
//! indexes / foreign keys — with the counts beside the nodes. A server with several
//! databases (PostgreSQL's) has them between: source › database › schema › …, each
//! database read through a session of its own ([`TreeDatabase::id`]: the source's own for
//! the database it opens, another identity for each other one), all those the user may
//! connect to listed (or those the source shows), the source's own marked. A node's children are read
//! only when it's opened: [`SourceTree::expand`] says what to ask for ([`Need`]), the
//! answers come back through [`SourceTree::set_tables`] / [`SourceTree::set_structure`];
//! until then a node shows a loading row, after a failure an error row.
//!
//! Its rows ([`SourceTree::rows`]) are what's open, or — with a filter — every node read so
//! far that matches it, with the nodes on the way to it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use database::folders::{self, FolderId, SourceColor, SourceFolder};
use database::{DataSourceId, Scope, Structure, TableInfo, TableKind, TableRef};
use ui_kit::ListFilter;

/// A source as the tree shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSource {
    pub id: DataSourceId,
    pub name: String,
    /// The driver's id, and its name (shown beside it).
    pub driver: String,
    pub driver_name: String,
    pub icon: &'static str,
    /// Where it's listed; `None`: a file shown by itself, not configured (only while open).
    pub scope: Option<Scope>,
    /// A file-based source's file (absolute).
    pub file: Option<PathBuf>,
    /// It says where to go (nothing is connected until it does).
    pub configured: bool,
    pub read_only: bool,
    /// The structure changes its driver makes (none for a read-only source).
    pub schema_changes: database::SchemaChanges,
    /// The schemas shown (none: all).
    pub schemas: Vec<String>,
    /// Its server has several databases, shown under it (source › database › schema).
    pub databases: bool,
    /// The databases shown (none: all).
    pub shown_databases: Vec<String>,
    /// A server's name and version, once connected (`PostgreSQL 16.4`).
    pub server: Option<String>,
    /// The folder it's in (of its list's folders); `None`: at its list's top.
    pub folder: Option<FolderId>,
    /// The color it's shown in (its own, else its folder's).
    pub color: Option<SourceColor>,
    /// Production (marked so, or its folder is).
    pub production: bool,
    /// A console of it has a transaction open (`TransactionChanged`).
    pub transaction: bool,
}

/// A group under a schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Tables,
    Views,
}

/// A part of a table's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Columns,
    Indexes,
    Keys,
}

/// A node, by what it is (stable while the tree is read again: what's open and selected
/// stays so).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeKey {
    /// The lists' own notice (a list that can't be read): not a source's.
    Notice(Scope),
    /// A folder of sources.
    Folder(FolderId),
    Source(DataSourceId),
    /// A database of a source's server, by its own identity ([`TreeDatabase::id`]: the
    /// source's for the one it opens).
    Database(DataSourceId),
    Schema(DataSourceId, String),
    Group(DataSourceId, Option<String>, Group),
    Table(DataSourceId, TableRef),
    Part(DataSourceId, TableRef, Part),
    Column(DataSourceId, TableRef, String),
    Index(DataSourceId, TableRef, String),
    Key(DataSourceId, TableRef, usize),
    /// A loading, error or hint row under `parent`.
    Status(Box<NodeKey>),
}

impl NodeKey {
    /// The source it's under.
    pub fn source(&self) -> Option<DataSourceId> {
        match self {
            Self::Notice(_) | Self::Folder(_) => None,
            Self::Source(id)
            | Self::Database(id)
            | Self::Schema(id, _)
            | Self::Group(id, ..)
            | Self::Table(id, _)
            | Self::Part(id, ..)
            | Self::Column(id, ..)
            | Self::Index(id, ..)
            | Self::Key(id, ..) => Some(*id),
            Self::Status(parent) => parent.source(),
        }
    }

    /// The table it's in (or is).
    pub fn table(&self) -> Option<&TableRef> {
        match self {
            Self::Table(_, t)
            | Self::Part(_, t, _)
            | Self::Column(_, t, _)
            | Self::Index(_, t, _)
            | Self::Key(_, t, _) => Some(t),
            Self::Status(parent) => parent.table(),
            _ => None,
        }
    }
}

/// What a row is (its icon, its menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    Folder,
    Source,
    Database,
    Schema,
    Group(Group),
    Table(TableKind),
    Part(Part),
    Column {
        key: bool,
        foreign: bool,
    },
    Index {
        unique: bool,
    },
    Key,
    Loading,
    Error,
    /// A note (a source not configured yet, nothing in a list).
    Hint,
}

/// A row of the tree as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: NodeKey,
    pub depth: u16,
    pub kind: RowKind,
    pub label: String,
    /// After the label, dimmed (a column's type, a driver's name, a table's rows).
    pub detail: String,
    /// The badge at the end (tables in a schema, columns of a table).
    pub count: Option<usize>,
    pub expandable: bool,
    pub expanded: bool,
    /// A folder's or a source's color (its mark in the row).
    pub color: Option<SourceColor>,
    /// What the row says of how carefully it's used (its badges after the name).
    pub marks: RowMarks,
}

/// A source's or a folder's badges: production, a console's transaction open on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowMarks {
    pub production: bool,
    pub transaction: bool,
}

/// Something read, or being read.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Pending,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// What the tree needs read (the window asks the source's worker).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Need {
    /// A server's databases (source `id`'s).
    Databases(DataSourceId),
    Tables(DataSourceId),
    Structure(DataSourceId, TableRef),
}

/// A database of a source's server, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDatabase {
    pub name: String,
    /// What reads it: the source's own identity for the database it opens, another for
    /// each other one ([`database::DataSourceId::of_database`]).
    pub id: DataSourceId,
    /// The one the source opens (its own).
    pub default: bool,
}

/// What's known of a source (or of a database of its server).
#[derive(Debug, Default)]
struct Known {
    tables: Load<Vec<TableInfo>>,
    structures: HashMap<TableRef, Load<Structure>>,
}

/// The texts the rows show that aren't data (translated by the window).
#[derive(Debug, Clone)]
pub struct Words {
    pub loading: String,
    /// Beside the database a source opens.
    pub default_database: String,
    pub no_databases: String,
    pub not_configured: String,
    pub no_tables: String,
    pub tables: String,
    pub views: String,
    pub columns: String,
    pub indexes: String,
    pub keys: String,
    pub unique: String,
    pub global: String,
    pub read_only: String,
    pub open_file: String,
    /// `{count}` rows (an estimate).
    pub rows: fn(u64) -> String,
}

/// The tree (see the module's doc).
#[derive(Debug, Default)]
pub struct SourceTree {
    sources: Vec<TreeSource>,
    known: HashMap<DataSourceId, Known>,
    expanded: HashSet<NodeKey>,
    /// The lists that can't be read: why (their notice at the top).
    notices: Vec<(Scope, String)>,
    /// The lists' folders (the sources are shown in them).
    folders: Vec<SourceFolder>,
    /// The folders closed (open unless closed: what's in them is seen at once).
    closed: HashSet<FolderId>,
    /// Each server's databases, as read (by the source's identity).
    databases: HashMap<DataSourceId, Load<Vec<TreeDatabase>>>,
    /// The databases' own sources (another database than its source's: its identity, the
    /// source with its name), by their identity.
    subs: Vec<TreeSource>,
}

impl SourceTree {
    pub fn sources(&self) -> &[TreeSource] {
        &self.sources
    }

    /// Source `id`, or a database of a source's server as its own source (its identity,
    /// its name with the database's).
    pub fn source(&self, id: DataSourceId) -> Option<&TreeSource> {
        self.sources.iter().find(|s| s.id == id).or_else(|| self.subs.iter().find(|s| s.id == id))
    }

    /// The source whose server database `id` is (itself for a source).
    pub fn base_of(&self, id: DataSourceId) -> DataSourceId {
        if self.sources.iter().any(|s| s.id == id) {
            return id;
        }
        self.databases
            .iter()
            .find(|(_, load)| load.ready().is_some_and(|l| l.iter().any(|d| d.id == id)))
            .map_or(id, |(base, _)| *base)
    }

    /// Source `id`'s databases, as read.
    pub fn databases(&self, id: DataSourceId) -> Option<&[TreeDatabase]> {
        self.databases.get(&id)?.ready().map(Vec::as_slice)
    }

    /// The identities of source `id` and of its server's databases.
    fn family(&self, id: DataSourceId) -> Vec<DataSourceId> {
        let mut ids = vec![id];
        if let Some(list) = self.databases(id) {
            ids.extend(list.iter().map(|d| d.id).filter(|d| *d != id));
        }
        ids
    }

    /// The databases' own sources, made again from their sources (their names, flags).
    fn make_subs(&mut self) {
        let mut subs = Vec::new();
        for source in &self.sources {
            let Some(list) = self.databases.get(&source.id).and_then(Load::ready) else {
                continue;
            };
            for d in list.iter().filter(|d| d.id != source.id) {
                subs.push(TreeSource {
                    id: d.id,
                    name: crate::catalog::database_name(&source.name, &d.name),
                    databases: false,
                    ..source.clone()
                });
            }
        }
        self.subs = subs;
    }

    /// Source `id`'s databases, as read (or why not): each with the identity it's read by.
    pub fn set_databases(
        &mut self,
        id: DataSourceId,
        databases: Result<Vec<TreeDatabase>, String>,
    ) {
        let load = match databases {
            Ok(mut list) => {
                list.sort_by_key(|d| d.name.to_lowercase());
                // What's known of those no longer there goes.
                let gone: Vec<DataSourceId> = self
                    .databases(id)
                    .map(|old| {
                        old.iter()
                            .filter(|o| o.id != id && !list.iter().any(|d| d.id == o.id))
                            .map(|o| o.id)
                            .collect()
                    })
                    .unwrap_or_default();
                for d in gone {
                    self.known.remove(&d);
                }
                Load::Ready(list)
            }
            Err(why) => Load::Failed(why),
        };
        self.databases.insert(id, load);
        self.make_subs();
    }

    /// The sources shown now: what's known of those still there kept (open, read), of the
    /// others forgotten.
    pub fn set_sources(&mut self, sources: Vec<TreeSource>) {
        let mut ids: HashSet<DataSourceId> = sources.iter().map(|s| s.id).collect();
        self.databases.retain(|id, _| ids.contains(id));
        // A database's own identity stays while its source does.
        for list in self.databases.values().filter_map(Load::ready) {
            ids.extend(list.iter().map(|d| d.id));
        }
        self.known.retain(|id, _| ids.contains(id));
        self.expanded.retain(|k| k.source().is_none_or(|id| ids.contains(&id)));
        // A source whose settings changed (another file, configured now) is read again.
        // (Its server's version, known once connected, isn't a setting.)
        // Nor where it's shown (its folder, its color).
        let bare = |s: &TreeSource| TreeSource {
            server: None,
            folder: None,
            color: None,
            production: false,
            transaction: false,
            ..s.clone()
        };
        for source in &sources {
            if self.sources.iter().any(|s| s.id == source.id && bare(s) != bare(source)) {
                for id in self.family(source.id) {
                    self.known.remove(&id);
                }
                self.databases.remove(&source.id);
            }
        }
        self.sources = sources;
        self.make_subs();
    }

    /// Source `id`'s server, as its session said once connected.
    pub fn set_server(&mut self, id: DataSourceId, server: Option<String>) {
        if let Some(source) = self.sources.iter_mut().find(|s| s.id == id) {
            source.server = server;
        }
    }

    /// The lists' folders (both lists').
    pub fn set_folders(&mut self, folders: Vec<SourceFolder>) {
        self.closed.retain(|id| folders.iter().any(|f| f.id == *id));
        self.folders = folders;
    }

    pub fn folders(&self) -> &[SourceFolder] {
        &self.folders
    }

    /// The notices of the lists that can't be read.
    pub fn set_notices(&mut self, notices: Vec<(Scope, String)>) {
        self.notices = notices;
    }

    pub fn is_expanded(&self, key: &NodeKey) -> bool {
        match key {
            NodeKey::Folder(id) => !self.closed.contains(id),
            _ => self.expanded.contains(key),
        }
    }

    /// The tables of source `id`, as read.
    pub fn tables(&self, id: DataSourceId) -> Option<&[TableInfo]> {
        self.known.get(&id)?.tables.ready().map(Vec::as_slice)
    }

    /// Table `table`'s structure, as read.
    pub fn structure(&self, id: DataSourceId, table: &TableRef) -> Option<&Structure> {
        self.known.get(&id)?.structures.get(table)?.ready()
    }

    /// `key` opened; what to read for it, if it isn't read (or being read).
    pub fn expand(&mut self, key: &NodeKey) -> Option<Need> {
        match key {
            NodeKey::Folder(id) => {
                self.closed.remove(id);
                return None;
            }
            // A source shown: the folders it's in opened too.
            NodeKey::Source(id) => {
                if let Some(folder) = self.source(*id).and_then(|s| s.folder) {
                    for f in folders::folder_path(&self.folders, folder) {
                        self.closed.remove(&f.id);
                    }
                }
            }
            _ => {}
        }
        self.expanded.insert(key.clone());
        self.need_of(key)
    }

    pub fn collapse(&mut self, key: &NodeKey) {
        match key {
            NodeKey::Folder(id) => {
                self.closed.insert(*id);
            }
            _ => {
                self.expanded.remove(key);
            }
        }
    }

    /// Opened if it was closed (what to read then), closed otherwise.
    pub fn toggle(&mut self, key: &NodeKey) -> Option<Need> {
        if self.is_expanded(key) {
            self.collapse(key);
            None
        } else {
            self.expand(key)
        }
    }

    /// Every node closed.
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.closed = self.folders.iter().map(|f| f.id).collect();
    }

    /// What opening `key` needs read (marked as being read).
    fn need_of(&mut self, key: &NodeKey) -> Option<Need> {
        let id = key.source()?;
        let configured = self.source(id).is_some_and(|s| s.configured);
        let has_databases = self.source(id).is_some_and(|s| s.databases);
        if let NodeKey::Source(_) = key
            && configured
            && has_databases
        {
            let load = self.databases.entry(id).or_default();
            return matches!(load, Load::Idle | Load::Failed(_)).then(|| {
                *load = Load::Pending;
                Need::Databases(id)
            });
        }
        let known = self.known.entry(id).or_default();
        match key {
            NodeKey::Source(_) | NodeKey::Database(_) if configured => {
                matches!(known.tables, Load::Idle | Load::Failed(_)).then(|| {
                    known.tables = Load::Pending;
                    Need::Tables(id)
                })
            }
            NodeKey::Table(_, table) | NodeKey::Part(_, table, _) => {
                let slot = known.structures.entry(table.clone()).or_default();
                matches!(slot, Load::Idle | Load::Failed(_)).then(|| {
                    *slot = Load::Pending;
                    Need::Structure(id, table.clone())
                })
            }
            _ => None,
        }
    }

    /// Source `id`'s tables, as read (or why not).
    pub fn set_tables(&mut self, id: DataSourceId, tables: Result<Vec<TableInfo>, String>) {
        let known = self.known.entry(id).or_default();
        known.tables = match tables {
            Ok(mut tables) => {
                // Each name lowercased once (thousands of tables, on the UI thread).
                tables.sort_by_cached_key(|t| {
                    (t.kind == TableKind::View, t.table.name.to_lowercase())
                });
                // The structures of tables no longer there go.
                let there: HashSet<&TableRef> = tables.iter().map(|t| &t.table).collect();
                known.structures.retain(|t, _| there.contains(t));
                Load::Ready(tables)
            }
            Err(why) => Load::Failed(why),
        };
    }

    /// Table `table`'s structure, as read (or why not).
    pub fn set_structure(
        &mut self,
        id: DataSourceId,
        table: TableRef,
        structure: Result<Structure, String>,
    ) {
        let known = self.known.entry(id).or_default();
        let load = match structure {
            Ok(s) => Load::Ready(s),
            Err(why) => Load::Failed(why),
        };
        known.structures.insert(table, load);
    }

    /// A read asked for was given up (the window hidden): asked again when next needed.
    pub fn forget_pending(&mut self) {
        for load in self.databases.values_mut() {
            if *load == Load::Pending {
                *load = Load::Idle;
            }
        }
        for known in self.known.values_mut() {
            if known.tables == Load::Pending {
                known.tables = Load::Idle;
            }
            known.structures.retain(|_, s| *s != Load::Pending);
        }
    }

    /// Source `id` read again: what's open in it asked for again (its databases, then
    /// their tables, first) — a source's, its server's databases too; a database's, only
    /// its own.
    pub fn refresh(&mut self, id: DataSourceId) -> Vec<Need> {
        let ids = self.family(id);
        for id in &ids {
            self.known.remove(id);
        }
        if self.databases.contains_key(&id) {
            self.databases.insert(id, Load::Idle);
        }
        let open: Vec<NodeKey> = self
            .expanded
            .iter()
            .filter(|k| {
                k.source().is_some_and(|s| ids.contains(&s))
                    && matches!(k, NodeKey::Source(_) | NodeKey::Database(_) | NodeKey::Table(..))
            })
            .cloned()
            .collect();
        let mut needs: Vec<Need> = open.iter().filter_map(|k| self.need_of(k)).collect();
        needs.sort_by_key(need_order);
        needs
    }

    /// What the open nodes need read that isn't (after a read was given up, a purge).
    pub fn open_needs(&mut self) -> Vec<Need> {
        let open: Vec<NodeKey> = self
            .expanded
            .iter()
            .filter(|k| matches!(k, NodeKey::Source(_) | NodeKey::Database(_) | NodeKey::Table(..)))
            .cloned()
            .collect();
        let idle = |tree: &Self, key: &NodeKey| {
            let Some(id) = key.source() else { return true };
            if let NodeKey::Source(_) = key
                && tree.source(id).is_some_and(|s| s.databases)
            {
                return tree.databases.get(&id).is_none_or(|l| *l == Load::Idle);
            }
            // A database no longer listed asks for nothing.
            if let NodeKey::Database(_) = key
                && tree.source(id).is_none()
            {
                return false;
            }
            let Some(known) = tree.known.get(&id) else { return true };
            match key {
                NodeKey::Table(_, t) => known.structures.get(t).is_none_or(|l| *l == Load::Idle),
                _ => known.tables == Load::Idle,
            }
        };
        let idle: Vec<NodeKey> = open.into_iter().filter(|k| idle(self, k)).collect();
        let mut needs: Vec<Need> = idle.iter().filter_map(|k| self.need_of(k)).collect();
        needs.sort_by_key(need_order);
        needs
    }

    /// What's read is forgotten ("Clear caches"): read again when opened.
    pub fn purge(&mut self) {
        self.known.clear();
        self.databases.clear();
        self.subs.clear();
    }

    /// The rows shown: what's open, or with `filter`, what matches among what's read.
    pub fn rows(&self, filter: &ListFilter, words: &Words) -> Vec<Row> {
        let mut out = Vec::new();
        for (scope, why) in &self.notices {
            out.push(Row {
                key: NodeKey::Notice(*scope),
                depth: 0,
                kind: RowKind::Error,
                label: why.clone(),
                detail: String::new(),
                count: None,
                expandable: false,
                expanded: false,
                color: None,
                marks: RowMarks::default(),
            });
        }
        for scope in [Scope::Project, Scope::Global] {
            self.level_rows((Some(scope), None, 0), filter, false, words, &mut out);
        }
        // The files shown by themselves (in no list), last.
        self.level_rows((None, None, 0), filter, false, words, &mut out);
        out
    }

    /// The folder `source` is shown in: its own, when it's one of its list's.
    fn folder_of(&self, source: &TreeSource) -> Option<FolderId> {
        let id = source.folder?;
        folders::folder(&self.folders, id)
            .is_some_and(|f| Some(f.scope) == source.scope)
            .then_some(id)
    }

    /// A level of `scope`'s list (`parent`: the folder it's in, `None`: the top) at
    /// `depth`: its folders (by name), then its sources. With a filter, a folder named like
    /// it shows all it holds (`all`); another, only what matches inside it.
    fn level_rows(
        &self,
        (scope, parent, depth): (Option<Scope>, Option<FolderId>, u16),
        filter: &ListFilter,
        all: bool,
        words: &Words,
        out: &mut Vec<Row>,
    ) {
        let filtering = !filter.is_empty();
        if let Some(scope) = scope {
            for f in folders::children(&self.folders, scope, parent) {
                let named = all || (filtering && filter.matches(&f.name));
                let key = NodeKey::Folder(f.id);
                let open = filtering || !self.closed.contains(&f.id);
                let mut inside = Vec::new();
                if open {
                    self.level_rows(
                        (Some(scope), Some(f.id), depth + 1),
                        filter,
                        named,
                        words,
                        &mut inside,
                    );
                }
                if filtering && !named && inside.is_empty() {
                    continue;
                }
                let count = folders::descendants(&self.folders, f.id)
                    .into_iter()
                    .chain([f.id])
                    .map(|id| self.sources.iter().filter(|s| s.folder == Some(id)).count())
                    .sum();
                out.push(Row {
                    key,
                    depth,
                    kind: RowKind::Folder,
                    label: f.name.clone(),
                    detail: String::new(),
                    count: Some(count),
                    expandable: true,
                    expanded: open,
                    color: folders::folder_color(&self.folders, f.id),
                    marks: RowMarks {
                        production: folders::folder_production(&self.folders, f.id),
                        transaction: false,
                    },
                });
                out.extend(inside);
            }
        }
        let none = ListFilter::new("");
        for source in self.sources.iter().filter(|s| s.scope == scope) {
            if self.folder_of(source) != parent {
                continue;
            }
            let at = out.len();
            self.source_rows(source, if all { &none } else { filter }, words, out);
            for row in &mut out[at..] {
                row.depth += depth;
            }
        }
    }

    fn source_rows(
        &self,
        source: &TreeSource,
        filter: &ListFilter,
        words: &Words,
        out: &mut Vec<Row>,
    ) {
        let key = NodeKey::Source(source.id);
        // Named like the filter: shown as without one (what it holds, opened by the user),
        // as a folder named like it.
        let none = ListFilter::new("");
        let filter =
            if !filter.is_empty() && filter.matches(&source.name) { &none } else { filter };
        let filtering = !filter.is_empty();
        let mut children = Vec::new();
        let known = self.known.get(&source.id);
        let tables = known.and_then(|k| k.tables.ready());
        let open = filtering || self.expanded.contains(&key);
        let databases = self.databases.get(&source.id);
        if open {
            match known.map(|k| &k.tables) {
                _ if !source.configured => {
                    children.push(status(&key, 1, RowKind::Hint, &words.not_configured));
                }
                _ if source.databases => match databases {
                    Some(Load::Ready(list)) => {
                        self.database_rows(source, list, filter, words, &mut children)
                    }
                    Some(Load::Failed(why)) => children.push(status(&key, 1, RowKind::Error, why)),
                    _ => children.push(status(&key, 1, RowKind::Loading, &words.loading)),
                },
                Some(Load::Ready(tables)) => {
                    self.schema_rows(source, &key, tables, filter, words, &mut children)
                }
                Some(Load::Failed(why)) => children.push(status(&key, 1, RowKind::Error, why)),
                _ => children.push(status(&key, 1, RowKind::Loading, &words.loading)),
            }
        }
        if filtering {
            // Its own states only: one of a database opened by the user (named like the
            // filter) stays.
            children.retain(|r| {
                r.depth != 1 || !matches!(r.kind, RowKind::Loading | RowKind::Error | RowKind::Hint)
            });
            if children.is_empty() && !filter.matches(&source.name) {
                return;
            }
        }
        let mut detail = source.server.clone().unwrap_or_else(|| source.driver_name.clone());
        match source.scope {
            Some(Scope::Global) => push_word(&mut detail, &words.global),
            None => push_word(&mut detail, &words.open_file),
            Some(Scope::Project) => {}
        }
        if source.read_only {
            push_word(&mut detail, &words.read_only);
        }
        out.push(Row {
            key,
            depth: 0,
            kind: RowKind::Source,
            label: source.name.clone(),
            detail,
            count: if source.databases {
                databases.and_then(Load::ready).map(|l| self.shown_databases(source, l).count())
            } else {
                tables.map(Vec::len)
            },
            expandable: true,
            expanded: open,
            color: source.color,
            marks: RowMarks { production: source.production, transaction: source.transaction },
        });
        out.extend(children);
    }

    /// The databases `source` shows of `list` (none chosen: all).
    fn shown_databases<'a>(
        &self,
        source: &'a TreeSource,
        list: &'a [TreeDatabase],
    ) -> impl Iterator<Item = &'a TreeDatabase> {
        list.iter().filter(|d| {
            source.shown_databases.is_empty() || source.shown_databases.contains(&d.name)
        })
    }

    /// A server's databases under its source, each with what's read of it (its schemas,
    /// as a source's).
    fn database_rows(
        &self,
        source: &TreeSource,
        list: &[TreeDatabase],
        filter: &ListFilter,
        words: &Words,
        out: &mut Vec<Row>,
    ) {
        let filtering = !filter.is_empty();
        let shown: Vec<&TreeDatabase> = self.shown_databases(source, list).collect();
        if shown.is_empty() {
            let key = NodeKey::Source(source.id);
            out.push(status(&key, 1, RowKind::Hint, &words.no_databases));
            return;
        }
        let none = ListFilter::new("");
        for database in shown {
            let key = NodeKey::Database(database.id);
            // Named like the filter: its schemas and tables as without one (opening it reads
            // them: the filter finds only what's read).
            let named = filtering && filter.matches(&database.name);
            let (filter, filtering) = if named { (&none, false) } else { (filter, filtering) };
            let open = filtering || self.expanded.contains(&key);
            let known = self.known.get(&database.id);
            let tables = known.and_then(|k| k.tables.ready());
            let mut children = Vec::new();
            if open {
                let own = self.source(database.id).unwrap_or(source);
                match known.map(|k| &k.tables) {
                    Some(Load::Ready(tables)) => {
                        let at = children.len();
                        self.schema_rows(own, &key, tables, filter, words, &mut children);
                        for row in &mut children[at..] {
                            row.depth += 1;
                        }
                    }
                    Some(Load::Failed(why)) => children.push(status(&key, 2, RowKind::Error, why)),
                    _ => children.push(status(&key, 2, RowKind::Loading, &words.loading)),
                }
            }
            if filtering {
                children.retain(|r| {
                    r.depth != 2
                        || !matches!(r.kind, RowKind::Loading | RowKind::Error | RowKind::Hint)
                });
                if children.is_empty() && !filter.matches(&database.name) {
                    continue;
                }
            }
            out.push(Row {
                key,
                depth: 1,
                kind: RowKind::Database,
                label: database.name.clone(),
                detail: if database.default {
                    words.default_database.clone()
                } else {
                    String::new()
                },
                count: tables.map(Vec::len),
                expandable: true,
                expanded: open,
                color: None,
                marks: RowMarks::default(),
            });
            out.extend(children);
        }
    }

    /// A source's tables, by schema (a source without schemas: its groups right under it),
    /// under `parent` (the source's row, or its database's).
    fn schema_rows(
        &self,
        source: &TreeSource,
        parent: &NodeKey,
        tables: &[TableInfo],
        filter: &ListFilter,
        words: &Words,
        out: &mut Vec<Row>,
    ) {
        let id = source.id;
        let filtering = !filter.is_empty();
        if tables.is_empty() {
            out.push(status(parent, 1, RowKind::Hint, &words.no_tables));
            return;
        }
        // The tables by schema, in the order the schemas first come, grouped in one pass
        // (thousands of tables in tens of schemas: not a pass over all of them per schema).
        let mut at: HashMap<Option<&str>, usize> = HashMap::new();
        let mut schemas: Vec<(Option<String>, Vec<&TableInfo>)> = Vec::new();
        for info in tables {
            let i = *at.entry(info.table.schema.as_deref()).or_insert_with(|| {
                schemas.push((info.table.schema.clone(), Vec::new()));
                schemas.len() - 1
            });
            schemas[i].1.push(info);
        }
        // Only those the source shows (none named: all).
        schemas.retain(|(s, _)| {
            source.schemas.is_empty() || s.as_ref().is_none_or(|s| source.schemas.contains(s))
        });
        let none = ListFilter::new("");
        for (schema, mine) in schemas {
            let mut children = Vec::new();
            // Named like the filter: its tables as without one.
            let named = filtering && schema.as_ref().is_some_and(|n| filter.matches(n));
            let (filter, filtering) = if named { (&none, false) } else { (filter, filtering) };
            let (depth, open) = match &schema {
                Some(name) => {
                    let key = NodeKey::Schema(id, name.clone());
                    (2, filtering || self.expanded.contains(&key))
                }
                None => (1, true),
            };
            if open {
                for (group, kind, label) in [
                    (Group::Tables, TableKind::Table, &words.tables),
                    (Group::Views, TableKind::View, &words.views),
                ] {
                    let listed: Vec<&TableInfo> =
                        mine.iter().copied().filter(|t| t.kind == kind).collect();
                    if listed.is_empty() {
                        continue;
                    }
                    let key = NodeKey::Group(id, schema.clone(), group);
                    let group_open = filtering || self.expanded.contains(&key);
                    let mut rows = Vec::new();
                    if group_open {
                        for info in &listed {
                            self.table_rows(id, info, depth + 1, filter, words, &mut rows);
                        }
                    }
                    if filtering && rows.is_empty() && !filter.matches(label) {
                        continue;
                    }
                    children.push(Row {
                        key,
                        depth,
                        kind: RowKind::Group(group),
                        label: label.clone(),
                        detail: String::new(),
                        count: Some(listed.len()),
                        expandable: true,
                        expanded: group_open,
                        color: None,
                        marks: RowMarks::default(),
                    });
                    children.extend(rows);
                }
            }
            match schema {
                Some(name) => {
                    if filtering && children.is_empty() && !filter.matches(&name) {
                        continue;
                    }
                    out.push(Row {
                        key: NodeKey::Schema(id, name.clone()),
                        depth: 1,
                        kind: RowKind::Schema,
                        label: name,
                        detail: String::new(),
                        count: Some(mine.len()),
                        expandable: true,
                        expanded: open,
                        color: None,
                        marks: RowMarks::default(),
                    });
                    out.extend(children);
                }
                None => out.extend(children),
            }
        }
    }

    /// A table and, opened, its structure's parts.
    fn table_rows(
        &self,
        id: DataSourceId,
        info: &TableInfo,
        depth: u16,
        filter: &ListFilter,
        words: &Words,
        out: &mut Vec<Row>,
    ) {
        // Named like the filter: its parts as without one.
        let none = ListFilter::new("");
        let filter =
            if !filter.is_empty() && filter.matches(&info.table.name) { &none } else { filter };
        let filtering = !filter.is_empty();
        let key = NodeKey::Table(id, info.table.clone());
        let load = self.known.get(&id).and_then(|k| k.structures.get(&info.table));
        let structure = load.and_then(Load::ready);
        let open = self.expanded.contains(&key) || (filtering && structure.is_some());
        let mut children = Vec::new();
        if open {
            match load {
                Some(Load::Ready(s)) => {
                    self.part_rows(id, info, s, depth + 1, (filter, words), &mut children)
                }
                Some(Load::Failed(why)) => {
                    children.push(status(&key, depth + 1, RowKind::Error, why))
                }
                _ => children.push(status(&key, depth + 1, RowKind::Loading, &words.loading)),
            }
        }
        if filtering {
            children.retain(|r| !matches!(r.kind, RowKind::Loading | RowKind::Error));
            if children.is_empty() && !filter.matches(&info.table.name) {
                return;
            }
        }
        out.push(Row {
            key,
            depth,
            kind: RowKind::Table(info.kind),
            label: info.table.name.clone(),
            detail: info.rows.map(words.rows).unwrap_or_default(),
            count: structure.map(|s| s.columns.len()),
            expandable: true,
            expanded: open,
            color: None,
            marks: RowMarks::default(),
        });
        out.extend(children);
    }

    /// A table's columns, indexes and foreign keys (each part only when it has some).
    fn part_rows(
        &self,
        id: DataSourceId,
        info: &TableInfo,
        s: &Structure,
        depth: u16,
        (filter, words): (&ListFilter, &Words),
        out: &mut Vec<Row>,
    ) {
        let filtering = !filter.is_empty();
        let table = &info.table;
        let foreign: HashSet<&str> =
            s.foreign_keys.iter().flat_map(|k| k.columns.iter().map(String::as_str)).collect();
        let columns: Vec<Row> = s
            .columns
            .iter()
            .map(|c| {
                let mut detail = c.declared_type.clone();
                if !c.nullable && !c.in_key {
                    push_word(&mut detail, "NOT NULL");
                }
                Row {
                    key: NodeKey::Column(id, table.clone(), c.name.clone()),
                    depth: depth + 1,
                    kind: RowKind::Column {
                        key: c.in_key,
                        foreign: foreign.contains(c.name.as_str()),
                    },
                    label: c.name.clone(),
                    detail,
                    count: None,
                    expandable: false,
                    expanded: false,
                    color: None,
                    marks: RowMarks::default(),
                }
            })
            .collect();
        let indexes: Vec<Row> = s
            .indexes
            .iter()
            .map(|i| {
                let mut detail = format!("({})", i.columns.join(", "));
                if i.unique {
                    push_word(&mut detail, &words.unique);
                }
                Row {
                    key: NodeKey::Index(id, table.clone(), i.name.clone()),
                    depth: depth + 1,
                    kind: RowKind::Index { unique: i.unique },
                    label: i.name.clone(),
                    detail,
                    count: None,
                    expandable: false,
                    expanded: false,
                    color: None,
                    marks: RowMarks::default(),
                }
            })
            .collect();
        let keys: Vec<Row> = s
            .foreign_keys
            .iter()
            .enumerate()
            .map(|(n, k)| Row {
                key: NodeKey::Key(id, table.clone(), n),
                depth: depth + 1,
                kind: RowKind::Key,
                label: k.name.clone().unwrap_or_else(|| k.columns.join(", ")),
                detail: format!("→ {} ({})", k.table.name, k.references.join(", ")),
                count: None,
                expandable: false,
                expanded: false,
                color: None,
                marks: RowMarks::default(),
            })
            .collect();
        for (part, label, rows) in [
            (Part::Columns, &words.columns, columns),
            (Part::Indexes, &words.indexes, indexes),
            (Part::Keys, &words.keys, keys),
        ] {
            if rows.is_empty() {
                continue;
            }
            let key = NodeKey::Part(id, table.clone(), part);
            let total = rows.len();
            let shown: Vec<Row> = if filtering {
                rows.into_iter().filter(|r| filter.matches(&r.label)).collect()
            } else {
                rows
            };
            if filtering && shown.is_empty() {
                continue;
            }
            let open = filtering || self.expanded.contains(&key);
            out.push(Row {
                key,
                depth,
                kind: RowKind::Part(part),
                label: label.clone(),
                detail: String::new(),
                count: Some(total),
                expandable: true,
                expanded: open,
                color: None,
                marks: RowMarks::default(),
            });
            if open {
                out.extend(shown);
            }
        }
    }
}

/// The order needs are asked in: a server's databases, then tables, then structures.
fn need_order(need: &Need) -> u8 {
    match need {
        Need::Databases(_) => 0,
        Need::Tables(_) => 1,
        Need::Structure(..) => 2,
    }
}

/// `word` after `text`, a `·` between them.
fn push_word(text: &mut String, word: &str) {
    if word.is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push_str(" · ");
    }
    text.push_str(word);
}

/// A loading, error or hint row under `parent`.
fn status(parent: &NodeKey, depth: u16, kind: RowKind, label: &str) -> Row {
    Row {
        key: NodeKey::Status(Box::new(parent.clone())),
        depth,
        kind,
        label: label.to_owned(),
        detail: String::new(),
        count: None,
        expandable: false,
        expanded: false,
        color: None,
        marks: RowMarks::default(),
    }
}

/// The row of `key` among `rows`, else the nearest row on the way to it (its table, its
/// source): where the selection goes when its own row is gone (closed, filtered out).
pub fn nearest(rows: &[Row], key: &NodeKey) -> Option<usize> {
    if let Some(at) = rows.iter().position(|r| r.key == *key) {
        return Some(at);
    }
    let table = key.table().cloned();
    let source = key.source();
    table
        .and_then(|t| {
            rows.iter().position(
                |r| matches!(&r.key, NodeKey::Table(id, x) if Some(*id) == source && *x == t),
            )
        })
        .or_else(|| {
            // Its database's row (a server's), else its source's.
            source.and_then(|id| {
                rows.iter()
                    .position(|r| matches!(&r.key, NodeKey::Database(d) if *d == id))
                    .or_else(|| rows.iter().position(|r| r.key == NodeKey::Source(id)))
            })
        })
}

#[cfg(test)]
#[path = "../tests/unit/tree.rs"]
mod tests;
