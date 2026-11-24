//! The rows shown: the lists' folders and sources (all, or what matches the filter), a
//! server's databases, loading and error rows, the row nearest to one that's gone.

use database::folders::FolderId;
use database::{Scope, folders};
use ui_kit::ListFilter;

use super::{
    Link, Load, NodeKey, Row, RowKind, RowMarks, SourceTree, TreeDatabase, TreeSource, Words,
};

impl SourceTree {
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
                        link: Link::Closed,
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
            marks: RowMarks {
                production: source.production,
                transaction: source.transaction,
                link: source.link,
            },
        });
        out.extend(children);
    }

    /// The databases `source` shows of `list` (none chosen: all).
    pub(super) fn shown_databases<'a>(
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
}

/// `word` after `text`, a `·` between them.
pub(super) fn push_word(text: &mut String, word: &str) {
    if word.is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push_str(" · ");
    }
    text.push_str(word);
}

/// A loading, error or hint row under `parent`.
pub(super) fn status(parent: &NodeKey, depth: u16, kind: RowKind, label: &str) -> Row {
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
