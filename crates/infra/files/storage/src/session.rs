//! `<project dir>/session.toml`: the TOML adapter for [`SessionStore`] (the sizes the user
//! dragged in a `[sizes]` table: `"dock.bottom" = 360.0`).

use std::io;
use std::path::{Path, PathBuf};

use workbench_model::{SavedGroup, SavedTab, SavedToolWindow, SavedView, Session, SessionStore};

use crate::write_atomic;

/// Sessions in `<project>/<project dir>/session.toml`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TomlSessionStore;

fn file(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join("session.toml")
}

impl SessionStore for TomlSessionStore {
    fn save(&self, root: &Path, session: &Session) -> io::Result<()> {
        let s = session;
        let mut table = toml::Table::new();
        table.insert("group".into(), toml::Value::Integer(s.group as i64));
        table.insert("split".into(), toml::Value::String(s.split.clone()));
        table.insert("zoom".into(), toml::Value::Float(f64::from(s.zoom)));
        if let Some(theme) = &s.theme {
            table.insert("theme".into(), toml::Value::String(theme.clone()));
        }
        let groups = s
            .groups
            .iter()
            .map(|g| {
                let mut t = toml::Table::new();
                t.insert("active".into(), toml::Value::Integer(g.active as i64));
                let tabs = g
                    .tabs
                    .iter()
                    .map(|tab| {
                        let mut e = toml::Table::new();
                        e.insert(
                            "path".into(),
                            toml::Value::String(tab.path.to_string_lossy().into_owned()),
                        );
                        if tab.pinned {
                            e.insert("pinned".into(), toml::Value::Boolean(true));
                        }
                        if let Some(split) = tab.split.filter(|r| r.is_finite()) {
                            e.insert("split".into(), toml::Value::Float(f64::from(split)));
                        }
                        if tab.locked {
                            e.insert("locked".into(), toml::Value::Boolean(true));
                        }
                        toml::Value::Table(e)
                    })
                    .collect();
                t.insert("tabs".into(), toml::Value::Array(tabs));
                toml::Value::Table(t)
            })
            .collect();
        table.insert("groups".into(), toml::Value::Array(groups));
        let windows: Vec<toml::Value> = s
            .tool_windows
            .iter()
            .map(|w| {
                let mut t = toml::Table::new();
                t.insert("id".into(), toml::Value::String(w.id.clone()));
                t.insert("active".into(), toml::Value::Integer(w.active as i64));
                let views = w
                    .views
                    .iter()
                    .map(|v| {
                        let mut e = toml::Table::new();
                        e.insert("title".into(), toml::Value::String(v.title.clone()));
                        e.insert("extension".into(), toml::Value::String(v.extension.clone()));
                        if v.pinned {
                            e.insert("pinned".into(), toml::Value::Boolean(true));
                        }
                        if v.group > 0 {
                            e.insert("group".into(), toml::Value::Integer(v.group as i64));
                        }
                        if let Some(key) = v.key.as_ref().filter(|k| **k != v.title) {
                            e.insert("key".into(), toml::Value::String(key.clone()));
                        }
                        if let Some(memento) = &v.memento {
                            e.insert("memento".into(), toml::Value::String(memento.clone()));
                        }
                        toml::Value::Table(e)
                    })
                    .collect();
                t.insert("views".into(), toml::Value::Array(views));
                // Split: the groups' tree, the current one, each one's active view.
                if !w.split.is_empty() {
                    t.insert("split".into(), toml::Value::String(w.split.clone()));
                    t.insert("group".into(), toml::Value::Integer(w.group as i64));
                    let actives =
                        w.actives.iter().map(|a| toml::Value::Integer(*a as i64)).collect();
                    t.insert("actives".into(), toml::Value::Array(actives));
                }
                toml::Value::Table(t)
            })
            .collect();
        table.insert("tool_windows".into(), toml::Value::Array(windows));
        if let Some(bottom) = &s.bottom {
            table.insert("bottom".into(), toml::Value::String(bottom.clone()));
        }
        if !s.panels.is_empty() {
            let panels: toml::Table = s
                .panels
                .iter()
                .map(|(id, memento)| (id.clone(), toml::Value::String(memento.clone())))
                .collect();
            table.insert("panels".into(), toml::Value::Table(panels));
        }
        if !s.sizes.is_empty() {
            let sizes: toml::Table = s
                .sizes
                .iter()
                .map(|(key, size)| (key.clone(), toml::Value::Float(f64::from(*size))))
                .collect();
            table.insert("sizes".into(), toml::Value::Table(sizes));
        }
        let text =
            toml::to_string(&table).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(&file(root), &text)
    }

    fn load(&self, root: &Path) -> Option<Session> {
        let text = std::fs::read_to_string(file(root)).ok()?;
        let table: toml::Table = text.parse().ok()?;
        let int = |v: Option<&toml::Value>| {
            v.and_then(toml::Value::as_integer).unwrap_or(0).max(0) as usize
        };
        let groups = table
            .get("groups")?
            .as_array()?
            .iter()
            .filter_map(|g| {
                let g = g.as_table()?;
                let tabs = g
                    .get("tabs")?
                    .as_array()?
                    .iter()
                    .filter_map(|t| {
                        let t = t.as_table()?;
                        Some(SavedTab {
                            path: PathBuf::from(t.get("path")?.as_str()?),
                            pinned: t.get("pinned").and_then(toml::Value::as_bool).unwrap_or(false),
                            split: t
                                .get("split")
                                .and_then(toml::Value::as_float)
                                .map(|r| r as f32)
                                .filter(|r| r.is_finite()),
                            locked: t.get("locked").and_then(toml::Value::as_bool).unwrap_or(false),
                        })
                    })
                    .collect();
                Some(SavedGroup { tabs, active: int(g.get("active")) })
            })
            .collect();
        let mut tool_windows: Vec<SavedToolWindow> = table
            .get("tool_windows")
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|w| {
                        let w = w.as_table()?;
                        let views = w
                            .get("views")?
                            .as_array()?
                            .iter()
                            .filter_map(|v| {
                                let v = v.as_table()?;
                                Some(SavedView {
                                    title: v.get("title")?.as_str()?.to_owned(),
                                    extension: v.get("extension")?.as_str()?.to_owned(),
                                    pinned: v
                                        .get("pinned")
                                        .and_then(toml::Value::as_bool)
                                        .unwrap_or(false),
                                    group: int(v.get("group")),
                                    key: v
                                        .get("key")
                                        .and_then(toml::Value::as_str)
                                        .map(str::to_owned),
                                    memento: v
                                        .get("memento")
                                        .and_then(toml::Value::as_str)
                                        .map(str::to_owned),
                                })
                            })
                            .collect();
                        Some(SavedToolWindow {
                            id: w.get("id")?.as_str()?.to_owned(),
                            views,
                            active: int(w.get("active")),
                            split: w
                                .get("split")
                                .and_then(toml::Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            group: int(w.get("group")),
                            actives: w
                                .get("actives")
                                .and_then(toml::Value::as_array)
                                .map(|a| a.iter().map(|v| int(Some(v))).collect())
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut bottom = table.get("bottom").and_then(toml::Value::as_str).map(str::to_owned);
        // Saved before the terminal was a tool window like any (an alias: its shells, their
        // names and pins, and whether it was shown, read as its window's views).
        if let Some(t) = table.get("terminal").and_then(toml::Value::as_table)
            && !tool_windows.iter().any(|w| w.id == "terminal")
        {
            let pinned: Vec<bool> = t
                .get("pinned")
                .and_then(toml::Value::as_array)
                .map(|a| a.iter().map(|p| p.as_bool().unwrap_or(false)).collect())
                .unwrap_or_default();
            let views: Vec<SavedView> = t
                .get("tabs")
                .and_then(toml::Value::as_array)
                .map(|a| {
                    a.iter()
                        .enumerate()
                        .map(|(i, n)| SavedView {
                            title: match n.as_str().filter(|s| !s.is_empty()) {
                                Some(name) => name.to_owned(),
                                None if i == 0 => "Local".to_owned(),
                                None => format!("Local ({})", i + 1),
                            },
                            extension: "view-terminal".to_owned(),
                            pinned: pinned.get(i).copied().unwrap_or(false),
                            group: 0,
                            key: None,
                            memento: None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !views.is_empty() {
                if t.get("visible").and_then(toml::Value::as_bool).unwrap_or(false) {
                    bottom.get_or_insert_with(|| "terminal".to_owned());
                }
                tool_windows.push(SavedToolWindow {
                    id: "terminal".to_owned(),
                    views,
                    active: int(t.get("active")),
                    ..SavedToolWindow::default()
                });
            }
        }
        Some(Session {
            groups,
            group: int(table.get("group")),
            split: table.get("split").and_then(toml::Value::as_str).unwrap_or("L0").to_owned(),
            tool_windows,
            bottom,
            zoom: table.get("zoom").and_then(toml::Value::as_float).map_or(1.0, |z| z as f32),
            theme: table.get("theme").and_then(toml::Value::as_str).map(str::to_owned),
            panels: table
                .get("panels")
                .and_then(toml::Value::as_table)
                .map(|t| {
                    let mut panels: Vec<(String, String)> = t
                        .iter()
                        .filter_map(|(id, v)| Some((id.clone(), v.as_str()?.to_owned())))
                        .collect();
                    panels.sort();
                    panels
                })
                .unwrap_or_default(),
            sizes: table
                .get("sizes")
                .and_then(toml::Value::as_table)
                .map(|t| {
                    t.iter()
                        .filter_map(|(key, v)| {
                            let size = v.as_float().or_else(|| v.as_integer().map(|i| i as f64))?;
                            let size = size as f32;
                            size.is_finite().then(|| (key.clone(), size))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/session.rs"]
mod tests;
