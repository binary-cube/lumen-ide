//! What's open and what it needs read: nodes opened and closed, what a node asks of the
//! source's worker (in order: databases, tables, structures), the answers kept, a source
//! read again, what's read forgotten.

use std::collections::HashSet;

use database::{DataSourceId, Structure, TableInfo, TableKind, TableRef, folders};

use super::{Load, Need, NodeKey, SourceTree};

impl SourceTree {
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
}

/// The order needs are asked in: a server's databases, then tables, then structures.
fn need_order(need: &Need) -> u8 {
    match need {
        Need::Databases(_) => 0,
        Need::Tables(_) => 1,
        Need::Structure(..) => 2,
    }
}
