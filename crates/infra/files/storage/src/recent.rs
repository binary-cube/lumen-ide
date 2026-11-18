//! `recent.toml`: the TOML adapter for [`RecentStore`]. The projects stay a plain list
//! (`projects = [...]`, as older files have it); the groups (`[[groups]]`) and the group
//! of each grouped project (`[placed]`, path → group) come after; a group in the order the
//! user chose has `order = "manual"` and that order (`projects`). A file without them has
//! every project in the recent group.

use std::io;
use std::path::PathBuf;

use workspace::RecentStore;
use workspace::recent::{Group, GroupId, Order, RecentProjects};

use crate::write_atomic;

/// The recent projects in one TOML file.
#[derive(Debug, Clone)]
pub struct TomlRecentStore {
    path: Option<PathBuf>,
}

impl TomlRecentStore {
    /// `path`: the file (`None`: nothing is kept).
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
}

fn id_text(id: GroupId) -> String {
    format!("{:032x}", id.0)
}

fn parse_id(value: &toml::Value) -> Option<GroupId> {
    u128::from_str_radix(value.as_str()?, 16).ok().map(GroupId)
}

fn paths(value: Option<&toml::Value>) -> Vec<PathBuf> {
    value
        .and_then(toml::Value::as_array)
        .map(|list| list.iter().filter_map(toml::Value::as_str).map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn parse(table: &toml::Table) -> RecentProjects {
    let projects = paths(table.get("projects"));
    let groups = table
        .get("groups")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .filter_map(|g| {
            Some(Group {
                id: parse_id(g.get("id")?)?,
                name: g.get("name")?.as_str()?.to_owned(),
                parent: g.get("parent").and_then(parse_id),
                collapsed: g.get("collapsed").and_then(toml::Value::as_bool).unwrap_or(false),
                order: match g.get("order").and_then(toml::Value::as_str) {
                    Some("manual") => Order::Manual,
                    _ => Order::Recent,
                },
                manual: paths(g.get("projects")),
            })
        })
        .collect();
    let placed = table
        .get("placed")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(path, group)| Some((PathBuf::from(path), parse_id(group)?)))
        .collect();
    RecentProjects { projects, groups, placed }.repaired()
}

fn render(list: &RecentProjects) -> toml::Table {
    let text = |p: &PathBuf| p.to_str().map(str::to_owned);
    let projects = list.projects.iter().filter_map(text).map(toml::Value::String).collect();
    let mut table = toml::Table::new();
    table.insert("projects".into(), toml::Value::Array(projects));
    if !list.groups.is_empty() {
        let groups = list
            .groups
            .iter()
            .map(|g| {
                let mut row = toml::Table::new();
                row.insert("id".into(), id_text(g.id).into());
                row.insert("name".into(), g.name.clone().into());
                if let Some(parent) = g.parent {
                    row.insert("parent".into(), id_text(parent).into());
                }
                if g.collapsed {
                    row.insert("collapsed".into(), true.into());
                }
                if g.order == Order::Manual {
                    row.insert("order".into(), "manual".into());
                    let order = g.manual.iter().filter_map(text).map(toml::Value::String);
                    row.insert("projects".into(), toml::Value::Array(order.collect()));
                }
                toml::Value::Table(row)
            })
            .collect();
        table.insert("groups".into(), toml::Value::Array(groups));
    }
    if !list.placed.is_empty() {
        // In the list's order, so the file doesn't reshuffle on every save.
        let placed = list
            .projects
            .iter()
            .filter_map(|p| Some((text(p)?, id_text(*list.placed.get(p)?).into())))
            .collect();
        table.insert("placed".into(), toml::Value::Table(placed));
    }
    table
}

impl RecentStore for TomlRecentStore {
    fn load(&self) -> RecentProjects {
        let Some(text) = self.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return RecentProjects::default();
        };
        text.parse::<toml::Table>().map(|t| parse(&t)).unwrap_or_default()
    }

    fn save(&self, list: &RecentProjects) -> io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let text = toml::to_string(&render(list))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(path, &text)
    }
}

#[cfg(test)]
#[path = "../tests/unit/recent.rs"]
mod tests;
