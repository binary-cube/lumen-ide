//! The bar's menus (Run's ▾, the transaction's, the mode's, the settings', the source's,
//! the database and schema it runs on) and the setting or command a row gives.

use super::*;

impl ConsoleBar {
    /// A menu under `tool`'s control.
    pub(super) fn menu(&self, tool: Tool, entries: Vec<ViewMenuEntry>) -> Option<ToolbarAction> {
        let (rect, _) = self.hits.iter().find(|(_, t)| *t == tool)?;
        Some(ToolbarAction::Menu { anchor: *rect, entries })
    }

    pub(super) fn checked(entry: ViewMenuEntry, on: bool) -> ViewMenuEntry {
        if on { entry.with_icon("check") } else { entry }
    }

    pub(super) fn run_menu(&self) -> Vec<ViewMenuEntry> {
        let keys = |id: &str| ui_components::shortcuts::label(id);
        let item = |id: &str, key: &str, icon: &'static str, command: &str| {
            let entry = ViewMenuEntry::item(id, i18n::t(key)).with_icon(icon);
            match keys(command) {
                Some(k) => entry.with_shortcut(k),
                None => entry,
            }
        };
        let caps = self.capabilities();
        let mut entries = vec![
            item("run", "console.menu_execute", "play", crate::RUN.0),
            item("run-script", "console.menu_execute_script", "play", crate::RUN_SCRIPT.0),
        ];
        if caps.explain.plan || caps.explain.analyze {
            entries.push(ViewMenuEntry::Separator);
        }
        if caps.explain.plan {
            entries.push(item("explain", "console.menu_explain", "plan", crate::EXPLAIN.0));
        }
        if caps.explain.analyze {
            entries.push(item(
                "explain-analyze",
                "console.menu_explain_analyze",
                "plan",
                crate::EXPLAIN_ANALYZE.0,
            ));
        }
        entries
    }

    pub(super) fn tx_menu(&self) -> Vec<ViewMenuEntry> {
        let mut entries = vec![
            Self::checked(
                ViewMenuEntry::item("tx:auto", i18n::t("console.tx_auto_long")),
                self.spec.tx == TxMode::Auto,
            ),
            Self::checked(
                ViewMenuEntry::item("tx:manual", i18n::t("console.tx_manual_long")),
                self.spec.tx == TxMode::Manual,
            ),
        ];
        let levels: Vec<Isolation> = Isolation::ALL
            .into_iter()
            .filter(|i| self.capabilities().isolation.contains(*i))
            .collect();
        if !levels.is_empty() {
            entries.push(ViewMenuEntry::Separator);
            let mut rows = vec![Self::checked(
                ViewMenuEntry::item("isolation:default", i18n::t("console.isolation_default")),
                self.spec.isolation.is_none(),
            )];
            rows.extend(levels.into_iter().map(|level| {
                Self::checked(
                    ViewMenuEntry::item(
                        format!("isolation:{}", level.id()),
                        i18n::t(&format!("console.isolation_{}", level.id())),
                    ),
                    self.spec.isolation == Some(level),
                )
            }));
            entries.push(ViewMenuEntry::Submenu {
                label: i18n::t("console.isolation"),
                icon: None,
                entries: rows,
            });
        }
        entries
    }

    pub(super) fn mode_menu(&self) -> Vec<ViewMenuEntry> {
        vec![
            Self::checked(
                ViewMenuEntry::item("mode:playground", i18n::t("console.mode_playground_long")),
                self.spec.mode == RunMode::Playground,
            ),
            Self::checked(
                ViewMenuEntry::item("mode:script", i18n::t("console.mode_script_long")),
                self.spec.mode == RunMode::Script,
            ),
        ]
    }

    pub(super) fn settings_menu(&self) -> Vec<ViewMenuEntry> {
        let limit = crate::settings::fetch_limit();
        let rows = crate::settings::FETCH_CHOICES
            .iter()
            .map(|&n| {
                Self::checked(
                    ViewMenuEntry::item(format!("fetch:{n}"), n.to_string()),
                    i64::from(limit) == n,
                )
            })
            .collect();
        vec![
            ViewMenuEntry::Submenu {
                label: i18n::t("console.menu_fetch_limit"),
                icon: Some("table"),
                entries: rows,
            },
            Self::checked(
                ViewMenuEntry::item("stop-on-error", i18n::t("console.menu_stop_on_error")),
                crate::settings::stop_on_error(),
            ),
            ViewMenuEntry::Submenu {
                label: i18n::t("console.menu_profiling"),
                icon: Some("plan"),
                entries: crate::settings::ProfileSwitch::ALL
                    .into_iter()
                    .map(|switch| {
                        Self::checked(
                            ViewMenuEntry::item(
                                format!("profile-option:{}", switch.id()),
                                i18n::t(&format!("console.menu_profile_{}", switch.id())),
                            ),
                            crate::settings::profile_switch(switch),
                        )
                    })
                    .collect(),
            },
        ]
    }

    /// An attached file's source pill: the sources speaking SQL (the one in use checked),
    /// then Detach.
    pub(super) fn source_menu(&self) -> Vec<ViewMenuEntry> {
        let sources = self.consoles.access().map(|a| a.sources()).unwrap_or_default();
        let mut entries: Vec<ViewMenuEntry> = sources
            .iter()
            .filter(|s| s.dialect)
            .map(|s| {
                let entry = ViewMenuEntry::item(format!("source:{}", s.id), s.name.clone());
                if s.id == self.source { entry.with_icon("check") } else { entry.with_icon(s.icon) }
            })
            .collect();
        if !entries.is_empty() {
            entries.push(ViewMenuEntry::Separator);
        }
        entries.push(ViewMenuEntry::item("detach", i18n::t("console.detach")).with_icon("x"));
        entries
    }

    /// The switcher's menu: the schemas (the one in use checked), the databases beside.
    pub(super) fn place_menu_action(&self) -> Option<ToolbarAction> {
        let places = self.snapshot.places.clone().unwrap_or_default();
        let current = self.spec.schema.clone().or_else(|| self.snapshot.place.schema.clone());
        let mut entries: Vec<ViewMenuEntry> = places
            .schemas
            .iter()
            .map(|s| {
                Self::checked(
                    ViewMenuEntry::item(format!("schema:{s}"), s.clone()),
                    current.as_deref() == Some(s.as_str()),
                )
            })
            .collect();
        let caps = self.capabilities();
        if caps.databases && places.databases.len() > 1 {
            let database =
                self.spec.database.clone().or_else(|| self.snapshot.place.database.clone());
            let rows = places
                .databases
                .iter()
                .map(|d| {
                    Self::checked(
                        ViewMenuEntry::item(format!("db:{d}"), d.clone()),
                        database.as_deref() == Some(d.as_str()),
                    )
                })
                .collect();
            if !entries.is_empty() {
                entries.push(ViewMenuEntry::Separator);
            }
            entries.push(ViewMenuEntry::Submenu {
                label: i18n::t("console.databases"),
                icon: Some("database"),
                entries: rows,
            });
        }
        if entries.is_empty() {
            entries.push(ViewMenuEntry::item("none", i18n::t("console.no_schemas")));
        }
        if caps.switch_schema == SchemaSwitch::NamesOnly {
            entries.push(ViewMenuEntry::Separator);
            entries.push(ViewMenuEntry::item("none", i18n::t("console.names_only")));
        }
        self.menu(Tool::Place, entries)
    }

    /// The switcher pressed: its menu, once its lists are read (connecting if needed).
    pub(super) fn open_place_menu(&mut self) -> Option<ToolbarAction> {
        if self.snapshot.places.is_some() {
            return self.place_menu_action();
        }
        if self.snapshot.busy {
            self.notifications.push(Notification::info(i18n::t("console.busy")));
            return None;
        }
        let access = self.consoles.access()?;
        let overrides = Overrides { database: self.spec.database.clone() };
        match access.connector(self.source, &overrides) {
            Ok(connector) => {
                let name = self.brief.as_ref().map(|b| b.name.clone()).unwrap_or_default();
                if self.runner.list_places(connector, name) {
                    self.place_menu = true;
                }
            }
            Err(e) => self.notifications.push(Notification::error(crate::runner::message(&e))),
        }
        None
    }
}
