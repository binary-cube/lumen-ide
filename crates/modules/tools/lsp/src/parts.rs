//! The settings' block of the downloaded servers ("Language servers" → "Downloaded
//! servers"): each component of the manifest — Node, PHP, every server — downloaded or not,
//! its version, its size on disk (or to download), Download / Remove (asked first: the
//! servers using it stop) / Stop while it downloads; Download all; the space they take, in
//! which folder. A component a full build bundles is said to be included.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use i18n::{t, tr};
use workbench_model::{
    Notification, OnYes, Question, SettingsBlock, SettingsContribution, SettingsPage, SettingsRow,
    SettingsScope,
};

use crate::downloads::{self, Status};

/// A button's id for component `name` (`download:<name>`…): kept for the process (the
/// settings' rows name their buttons by `&'static str`; a few components, once each).
fn action(kind: &str, name: &str) -> &'static str {
    static IDS: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);
    let key = format!("{kind}:{name}");
    let mut ids = IDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let ids = ids.get_or_insert_with(HashMap::new);
    if let Some(id) = ids.get(&key) {
        return id;
    }
    let id: &'static str = Box::leak(key.clone().into_boxed_str());
    ids.insert(key, id);
    id
}

/// Where component `name` is bundled with the app, if it is: its version.
fn bundled(c: &lsp_client::downloads::Component) -> Option<String> {
    let bundle = crate::catalog::bundle()?;
    let m = &bundle.manifest;
    match c.role {
        lsp_client::downloads::Role::Node => {
            bundle.node().and(m.node.as_ref()).map(|n| n.version.clone())
        }
        lsp_client::downloads::Role::Php => {
            bundle.php().and(m.php.as_ref()).map(|n| n.version.clone())
        }
        lsp_client::downloads::Role::Server => bundle.server(&c.name).map(|(_, v)| v.to_owned()),
    }
}

/// The block.
#[derive(Debug, Default)]
pub struct DownloadedParts {
    /// The downloads' generation last drawn.
    drawn: AtomicU64,
}

impl SettingsContribution for DownloadedParts {
    fn id(&self) -> &'static str {
        "language-servers-parts"
    }

    fn page(&self) -> &'static str {
        "language-servers"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGE_SERVERS)
    }

    fn order(&self) -> u16 {
        10
    }

    fn block(&self, _scope: SettingsScope<'_>) -> SettingsBlock {
        self.drawn.store(crate::install::generation(), Ordering::Relaxed);
        let mut block = SettingsBlock {
            title: t("settings.lsp_parts_title"),
            detail: t("settings.lsp_parts_detail"),
            rows: Vec::new(),
        };
        let Some(root) = downloads::root() else {
            block.rows.push(SettingsRow::Notes(vec![t("settings.lsp_parts_off")]));
            return block;
        };
        let manifest = downloads::manifest();
        let platform = downloads::platform();
        let mut missing = Vec::new();
        for c in &manifest.components {
            let name = c.label();
            let version = c.version.as_str();
            if let Some(v) = bundled(c) {
                rows_status(
                    &mut block.rows,
                    tr!("settings.lsp_part_bundled", name = name.as_str(), version = v.as_str()),
                    None,
                );
                continue;
            }
            if !c.available(platform) {
                rows_status(
                    &mut block.rows,
                    tr!("settings.lsp_part_unavailable", name = name.as_str(), version = version),
                    None,
                );
                continue;
            }
            match downloads::status(&c.name) {
                Status::Downloading(p) => rows_status(
                    &mut block.rows,
                    if p.waiting {
                        tr!("settings.lsp_part_waiting", name = name.as_str(), version = version)
                    } else {
                        tr!(
                            "settings.lsp_part_downloading",
                            name = name.as_str(),
                            version = version,
                            received = i18n::bytes(p.received),
                            total = i18n::bytes(p.total)
                        )
                    },
                    Some((action("stop", &c.name), t("settings.lsp_part_stop"))),
                ),
                Status::Removing => rows_status(
                    &mut block.rows,
                    tr!("settings.lsp_part_removing", name = name.as_str()),
                    None,
                ),
                Status::Installed(found) => {
                    let text = if found.current {
                        tr!(
                            "settings.lsp_part_installed",
                            name = name.as_str(),
                            version = found.version.as_str(),
                            size = i18n::bytes(found.size)
                        )
                    } else {
                        tr!(
                            "settings.lsp_part_outdated",
                            name = name.as_str(),
                            version = found.version.as_str(),
                            pinned = version,
                            size = i18n::bytes(found.size)
                        )
                    };
                    rows_status(
                        &mut block.rows,
                        text,
                        Some((action("remove", &c.name), t("settings.lsp_part_remove"))),
                    );
                }
                Status::Missing { size } => {
                    missing.push(c.name.clone());
                    let text = match downloads::failure(&c.name) {
                        Some(f) if f.offline => tr!(
                            "settings.lsp_part_offline",
                            name = name.as_str(),
                            version = version
                        ),
                        Some(f) => tr!(
                            "settings.lsp_part_failed",
                            name = name.as_str(),
                            version = version,
                            reason = f.reason.as_str()
                        ),
                        None => tr!(
                            "settings.lsp_part_missing",
                            name = name.as_str(),
                            version = version,
                            size = i18n::bytes(size)
                        ),
                    };
                    block.rows.push(SettingsRow::Actions {
                        text,
                        buttons: vec![(
                            action("download", &c.name),
                            t("settings.lsp_part_download"),
                        )],
                    });
                }
            }
        }
        let used = tr!(
            "settings.lsp_parts_used",
            size = i18n::bytes(downloads::used_space()),
            path = root.display().to_string()
        );
        if missing.is_empty() {
            block.rows.push(SettingsRow::Notes(vec![used, t("settings.lsp_parts_note")]));
        } else {
            block.rows.push(SettingsRow::Actions {
                text: used,
                buttons: vec![(
                    "download-all",
                    tr!(
                        "settings.lsp_parts_download_all",
                        size = i18n::bytes(downloads::size_of(&missing))
                    ),
                )],
            });
            block.rows.push(SettingsRow::Notes(vec![t("settings.lsp_parts_note")]));
        }
        block
    }

    fn poll(&self) -> bool {
        crate::install::generation() != self.drawn.load(Ordering::Relaxed)
    }

    fn choose(&self, _scope: SettingsScope<'_>, _key: &'static str, _value: i64) {}

    fn press(&self, scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        let wake = scope.wake.cloned();
        let manifest = downloads::manifest();
        if action == "download-all" {
            let platform = downloads::platform();
            let all: Vec<String> = manifest
                .components
                .iter()
                .filter(|c| c.available(platform) && bundled(c).is_none())
                .filter(|c| matches!(downloads::status(&c.name), Status::Missing { .. }))
                .map(|c| c.name.clone())
                .collect();
            downloads::start(all, wake);
            return None;
        }
        let (kind, name) = action.split_once(':')?;
        let c = manifest.named(name)?.clone();
        match kind {
            "download" => {
                downloads::start(vec![c.name], wake);
                None
            }
            "stop" => {
                downloads::stop(&c.name);
                None
            }
            "remove" => {
                let size = downloads::found(&c.name).map_or(0, |f| f.size);
                let question = Question {
                    title: tr!("settings.lsp_part_remove_title", name = c.label().as_str()),
                    message: tr!(
                        "settings.lsp_part_remove_message",
                        name = c.label().as_str(),
                        size = i18n::bytes(size)
                    ),
                    icon: "trash",
                    yes: t("settings.lsp_part_remove"),
                    danger: true,
                    list: Vec::new(),
                };
                let then: OnYes = Box::new(move |host| {
                    host.spawn(Box::new(move || {
                        let outcome = downloads::remove(&c.name);
                        if let Some(wake) = wake {
                            wake();
                        }
                        Box::new(move |host| {
                            let notification = match outcome {
                                Ok(()) => Notification::success(tr!(
                                    "lsp.part_removed",
                                    name = c.label().as_str()
                                )),
                                Err(why) => Notification::error(tr!(
                                    "lsp.part_remove_failed",
                                    name = c.label().as_str()
                                ))
                                .with_detail(why),
                            };
                            host.notify(notification);
                        })
                    }));
                });
                Some((question, then))
            }
            _ => None,
        }
    }
}

fn rows_status(rows: &mut Vec<SettingsRow>, text: String, button: Option<(&'static str, String)>) {
    rows.push(SettingsRow::Status { text, button });
}

#[cfg(test)]
#[path = "../tests/unit/parts.rs"]
mod tests;
